//! On-disk segment format (contract 06 §6.2, master E5). All integers are little-endian.
//!
//! ```text
//! header: magic "LOOMJ1\0\0" | u16 format_ver=1 | u16 codec | u16 checksum_alg=1 (crc32c)
//!         | producer_id (u16 len + UTF-8) | u64 producer_epoch | u64 created wall_ns
//!         | clock_id (u16 len + UTF-8)
//! frame:  u32 len | u32 crc32c(payload) | u8 rec_type | payload[len]
//! ```
//!
//! The CRC covers the payload only (as the contract says); `rec_type` is protected only by the
//! "known value" check.

use std::io::{self, Read};

use loom_proto::num::U64;
use serde::Deserialize;

/// Segment magic.
pub const SEGMENT_MAGIC: [u8; 8] = *b"LOOMJ1\0\0";
/// Segment format version.
pub const FORMAT_VERSION: u16 = 1;
/// `checksum_alg` id of CRC32C (Castagnoli) — the only one defined.
pub const CHECKSUM_CRC32C: u16 = 1;
/// Bytes before the payload in a frame (`len`, `crc`, `rec_type`).
pub const FRAME_HEADER_LEN: u64 = 9;
/// Largest payload accepted by writer and reader. A larger `len` on disk is treated as damage.
pub const MAX_FRAME_PAYLOAD: usize = 64 * 1024 * 1024;
/// Upper bound for `producer_id` / `clock_id` in the header.
pub const MAX_HEADER_TEXT: usize = 1024;
/// Payload length of an [`IndexHint`] frame.
pub const HINT_PAYLOAD_LEN: usize = 17;
/// Full frame length of an [`IndexHint`] frame.
pub const HINT_FRAME_LEN: u64 = FRAME_HEADER_LEN + HINT_PAYLOAD_LEN as u64;

/// Record codec registry (header `codec` id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    /// `json/1` — one frame = one `SourceRecord` JSON object (M1 default).
    Json,
    /// `msgpack/1` — reserved (M2). Treated as opaque bytes by this crate.
    Msgpack,
    /// `pcap-epb/1` — one frame = pcapng EPB bytes (eth). Opaque to this crate.
    PcapEpb,
}

impl Codec {
    /// Header id.
    pub const fn id(self) -> u16 {
        match self {
            Codec::Json => 1,
            Codec::Msgpack => 2,
            Codec::PcapEpb => 3,
        }
    }

    /// Codec of a header id.
    pub const fn from_id(id: u16) -> Option<Self> {
        match id {
            1 => Some(Codec::Json),
            2 => Some(Codec::Msgpack),
            3 => Some(Codec::PcapEpb),
            _ => None,
        }
    }

    /// Registry name (`json/1` …).
    pub const fn name(self) -> &'static str {
        match self {
            Codec::Json => "json/1",
            Codec::Msgpack => "msgpack/1",
            Codec::PcapEpb => "pcap-epb/1",
        }
    }

    /// True if this crate does not parse the payload. Such segments carry `(source_seq,
    /// mono_ns)` in [`IndexHint`] frames instead (see [`crate::writer`]).
    pub const fn is_opaque(self) -> bool {
        !matches!(self, Codec::Json)
    }
}

/// Frame record type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecType {
    /// 1 — one event (`SourceRecord` JSON, or opaque bytes for opaque codecs).
    Event,
    /// 2 — `(source_seq, mono_ns, flags)` of the next event (opaque codecs only).
    IndexHint,
    /// 3 — journal marker (JSON; e.g. a recorded loss).
    Marker,
    /// 4 — stream mapping entry (JSON, gateway mapping log).
    Mapping,
}

impl RecType {
    /// On-disk id.
    pub const fn id(self) -> u8 {
        match self {
            RecType::Event => 1,
            RecType::IndexHint => 2,
            RecType::Marker => 3,
            RecType::Mapping => 4,
        }
    }

    /// Record type of an on-disk id.
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(RecType::Event),
            2 => Some(RecType::IndexHint),
            3 => Some(RecType::Marker),
            4 => Some(RecType::Mapping),
            _ => None,
        }
    }

    /// True for record types that carry an index key (`source_seq` or `seq_first`).
    pub const fn is_keyed(self) -> bool {
        matches!(self, RecType::Event | RecType::Mapping)
    }
}

/// Decoded segment header (format version and checksum algorithm are fixed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentHeader {
    /// Record codec.
    pub codec: Codec,
    /// Producer id.
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: u64,
    /// Wall clock at creation (ns since Unix epoch).
    pub created_wall_ns: u64,
    /// Monotonic clock domain id.
    pub clock_id: String,
}

/// Header decode failure.
#[derive(Debug)]
pub enum HeaderError {
    /// File ends inside the header (e.g. crash right after create).
    Incomplete,
    /// Not a valid header.
    Invalid(String),
    /// Read error.
    Io(io::Error),
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeaderError::Incomplete => f.write_str("incomplete header"),
            HeaderError::Invalid(r) => write!(f, "invalid header: {r}"),
            HeaderError::Io(e) => write!(f, "read error: {e}"),
        }
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) -> Result<(), HeaderError> {
    if s.len() > MAX_HEADER_TEXT {
        return Err(HeaderError::Invalid(format!(
            "header text longer than {MAX_HEADER_TEXT} bytes"
        )));
    }
    let len = u16::try_from(s.len()).map_err(|_| HeaderError::Invalid("text too long".into()))?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn read_exact_hdr<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<(), HeaderError> {
    r.read_exact(buf).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            HeaderError::Incomplete
        } else {
            HeaderError::Io(e)
        }
    })
}

fn read_u16<R: Read>(r: &mut R) -> Result<u16, HeaderError> {
    let mut b = [0u8; 2];
    read_exact_hdr(r, &mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_u64<R: Read>(r: &mut R) -> Result<u64, HeaderError> {
    let mut b = [0u8; 8];
    read_exact_hdr(r, &mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_str<R: Read>(r: &mut R) -> Result<String, HeaderError> {
    let len = usize::from(read_u16(r)?);
    if len > MAX_HEADER_TEXT {
        return Err(HeaderError::Invalid(format!("text length {len}")));
    }
    let mut b = vec![0u8; len];
    read_exact_hdr(r, &mut b)?;
    String::from_utf8(b).map_err(|_| HeaderError::Invalid("text is not UTF-8".into()))
}

impl SegmentHeader {
    /// Encode the header.
    pub fn encode(&self) -> Result<Vec<u8>, HeaderError> {
        let mut out = Vec::with_capacity(48 + self.producer_id.len() + self.clock_id.len());
        out.extend_from_slice(&SEGMENT_MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.codec.id().to_le_bytes());
        out.extend_from_slice(&CHECKSUM_CRC32C.to_le_bytes());
        put_str(&mut out, &self.producer_id)?;
        out.extend_from_slice(&self.producer_epoch.to_le_bytes());
        out.extend_from_slice(&self.created_wall_ns.to_le_bytes());
        put_str(&mut out, &self.clock_id)?;
        Ok(out)
    }

    /// Read a header; returns it and its encoded length.
    pub fn read_from<R: Read>(r: &mut R) -> Result<(Self, u64), HeaderError> {
        let mut magic = [0u8; 8];
        read_exact_hdr(r, &mut magic)?;
        if magic != SEGMENT_MAGIC {
            return Err(HeaderError::Invalid("bad magic".into()));
        }
        let ver = read_u16(r)?;
        if ver != FORMAT_VERSION {
            return Err(HeaderError::Invalid(format!("format_ver {ver}")));
        }
        let codec_id = read_u16(r)?;
        let codec =
            Codec::from_id(codec_id).ok_or_else(|| HeaderError::Invalid(format!("codec {codec_id}")))?;
        let alg = read_u16(r)?;
        if alg != CHECKSUM_CRC32C {
            return Err(HeaderError::Invalid(format!("checksum_alg {alg}")));
        }
        let producer_id = read_str(r)?;
        let producer_epoch = read_u64(r)?;
        let created_wall_ns = read_u64(r)?;
        let clock_id = read_str(r)?;
        let len = 8 + 2 + 2 + 2 + 2 + producer_id.len() + 8 + 8 + 2 + clock_id.len();
        Ok((
            SegmentHeader {
                codec,
                producer_id,
                producer_epoch,
                created_wall_ns,
                clock_id,
            },
            len as u64,
        ))
    }
}

/// Append one frame to `out`. The caller keeps `payload.len() <= MAX_FRAME_PAYLOAD`.
pub fn encode_frame(rec_type: RecType, payload: &[u8], out: &mut Vec<u8>) {
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&crc32c::crc32c(payload).to_le_bytes());
    out.push(rec_type.id());
    out.extend_from_slice(payload);
}

/// Encoded length of a frame with a `payload_len`-byte payload.
pub const fn frame_len(payload_len: usize) -> u64 {
    FRAME_HEADER_LEN + payload_len as u64
}

/// Why a frame could not be read.
#[derive(Debug)]
pub enum FrameError {
    /// File ends inside the frame.
    Incomplete,
    /// CRC32C mismatch.
    BadCrc,
    /// Unknown `rec_type`.
    BadType(u8),
    /// `len` above [`MAX_FRAME_PAYLOAD`].
    TooLong(u32),
    /// Read error.
    Io(io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Incomplete => f.write_str("incomplete frame"),
            FrameError::BadCrc => f.write_str("crc32c mismatch"),
            FrameError::BadType(t) => write!(f, "unknown rec_type {t}"),
            FrameError::TooLong(l) => write!(f, "frame length {l} above limit"),
            FrameError::Io(e) => write!(f, "read error: {e}"),
        }
    }
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Read one frame into `payload`. `Ok(None)` = clean end of file.
pub fn read_frame<R: Read>(r: &mut R, payload: &mut Vec<u8>) -> Result<Option<RecType>, FrameError> {
    let mut hdr = [0u8; FRAME_HEADER_LEN as usize];
    let n = read_full(r, &mut hdr).map_err(FrameError::Io)?;
    if n == 0 {
        return Ok(None);
    }
    if n < hdr.len() {
        return Err(FrameError::Incomplete);
    }
    let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let crc = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
    let rec_type = RecType::from_id(hdr[8]).ok_or(FrameError::BadType(hdr[8]))?;
    let len_usize = usize::try_from(len).map_err(|_| FrameError::TooLong(len))?;
    if len_usize > MAX_FRAME_PAYLOAD {
        return Err(FrameError::TooLong(len));
    }
    payload.clear();
    payload.resize(len_usize, 0);
    let got = read_full(r, payload).map_err(FrameError::Io)?;
    if got < len_usize {
        return Err(FrameError::Incomplete);
    }
    if crc32c::crc32c(payload) != crc {
        return Err(FrameError::BadCrc);
    }
    Ok(Some(rec_type))
}

/// Payload of a `rec_type = 2` frame: identity of the next event in an opaque-codec segment.
///
/// The writer emits one before the first event of a segment, before any event whose
/// `source_seq` is not `previous + 1`, and at every sparse-index point (`index_point = true`).
/// Other events derive `source_seq = previous + 1`; their exact `mono_ns` is not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexHint {
    /// `source_seq` of the next event.
    pub source_seq: u64,
    /// `mono_ns` of the next event.
    pub mono_ns: u64,
    /// The writer put a sparse-index entry here (lets `rebuild_index` reproduce it).
    pub index_point: bool,
}

impl IndexHint {
    /// Encode to [`HINT_PAYLOAD_LEN`] bytes.
    pub fn encode(&self) -> [u8; HINT_PAYLOAD_LEN] {
        let mut b = [0u8; HINT_PAYLOAD_LEN];
        b[0..8].copy_from_slice(&self.source_seq.to_le_bytes());
        b[8..16].copy_from_slice(&self.mono_ns.to_le_bytes());
        b[16] = u8::from(self.index_point);
        b
    }

    /// Decode; `None` if the length or flags are wrong.
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != HINT_PAYLOAD_LEN || b[16] > 1 {
            return None;
        }
        let mut s = [0u8; 8];
        s.copy_from_slice(&b[0..8]);
        let mut m = [0u8; 8];
        m.copy_from_slice(&b[8..16]);
        Some(IndexHint {
            source_seq: u64::from_le_bytes(s),
            mono_ns: u64::from_le_bytes(m),
            index_point: b[16] == 1,
        })
    }
}

#[derive(Deserialize)]
struct EventMeta {
    source_seq: U64,
    ts: TsMeta,
}

#[derive(Deserialize)]
struct TsMeta {
    mono_ns: U64,
}

#[derive(Deserialize)]
struct MappingMeta {
    seq_first: U64,
    mono_first: U64,
}

/// `(source_seq, ts.mono_ns)` of a `json/1` event payload.
pub(crate) fn json_event_key(payload: &[u8]) -> Option<(u64, u64)> {
    serde_json::from_slice::<EventMeta>(payload)
        .ok()
        .map(|m| (m.source_seq.0, m.ts.mono_ns.0))
}

/// `(seq_first, mono_first)` of a mapping payload.
pub(crate) fn json_mapping_key(payload: &[u8]) -> Option<(u64, u64)> {
    serde_json::from_slice::<MappingMeta>(payload)
        .ok()
        .map(|m| (m.seq_first.0, m.mono_first.0))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = SegmentHeader {
            codec: Codec::PcapEpb,
            producer_id: "eth-0".into(),
            producer_epoch: 7,
            created_wall_ns: 123,
            clock_id: "boot-1".into(),
        };
        let b = h.encode().unwrap();
        let (d, len) = SegmentHeader::read_from(&mut b.as_slice()).unwrap();
        assert_eq!(d, h);
        assert_eq!(len, b.len() as u64);
        assert!(matches!(
            SegmentHeader::read_from(&mut &b[..b.len() - 1]),
            Err(HeaderError::Incomplete)
        ));
    }

    #[test]
    fn frame_roundtrip_and_damage() {
        let mut out = Vec::new();
        encode_frame(RecType::Event, b"hello", &mut out);
        let mut buf = Vec::new();
        assert_eq!(
            read_frame(&mut out.as_slice(), &mut buf).unwrap(),
            Some(RecType::Event)
        );
        assert_eq!(buf, b"hello");
        assert!(matches!(
            read_frame(&mut &out[..out.len() - 1], &mut buf),
            Err(FrameError::Incomplete)
        ));
        let mut bad = out.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(matches!(
            read_frame(&mut bad.as_slice(), &mut buf),
            Err(FrameError::BadCrc)
        ));
        assert_eq!(read_frame(&mut &out[..0], &mut buf).unwrap(), None);
    }

    #[test]
    fn hint_roundtrip() {
        let h = IndexHint {
            source_seq: u64::MAX,
            mono_ns: 5,
            index_point: true,
        };
        assert_eq!(IndexHint::decode(&h.encode()), Some(h));
        assert_eq!(IndexHint::decode(&[0u8; 3]), None);
    }
}
