//! CAN / CAN FD frame types shared by the Loom CAN worker, ISO-TP engine and DBC codec.
//!
//! Design rules (CAN plan v2.0 §3.3.1, §9.4):
//! - A frame is a fixed-size value (no heap) so it can live in fixed ring slots.
//! - Invalid frames cannot be constructed: id range, payload length and FD/RTR
//!   combinations are checked once, at the edge.
//! - Driver time and host ingest time are kept apart; this crate never
//!   converts one into the other.

use std::fmt;

/// Largest standard (11-bit) identifier.
pub const MAX_STANDARD_ID: u32 = 0x7FF;
/// Largest extended (29-bit) identifier.
pub const MAX_EXTENDED_ID: u32 = 0x1FFF_FFFF;
/// Largest Classic CAN payload.
pub const CLASSIC_MAX_LEN: usize = 8;
/// Largest CAN FD payload.
pub const FD_MAX_LEN: usize = 64;
/// Valid CAN FD payload lengths above 8 bytes (DLC 9..=15).
pub const FD_LENGTHS_ABOVE_8: [usize; 7] = [12, 16, 20, 24, 32, 48, 64];

/// Errors raised when a frame or identifier is not valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// Identifier does not fit the selected format.
    IdOutOfRange { raw: u32, extended: bool },
    /// Payload length is not allowed for the frame kind.
    InvalidLength { len: usize, fd: bool },
    /// DLC code above 15 (or above 8 for Classic).
    InvalidDlc { dlc: u8, fd: bool },
    /// Remote frames exist only in Classic CAN.
    RemoteInFd,
    /// BRS/ESI flags exist only in CAN FD.
    FdFlagOnClassic,
    /// Text form could not be parsed.
    Parse(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::IdOutOfRange { raw, extended } => {
                let max = if *extended {
                    MAX_EXTENDED_ID
                } else {
                    MAX_STANDARD_ID
                };
                write!(f, "CAN id 0x{raw:X} out of range (max 0x{max:X})")
            }
            FrameError::InvalidLength { len, fd } => {
                if *fd {
                    write!(
                        f,
                        "invalid CAN FD length {len} (0..=8, 12, 16, 20, 24, 32, 48, 64)"
                    )
                } else {
                    write!(f, "invalid Classic CAN length {len} (0..=8)")
                }
            }
            FrameError::InvalidDlc { dlc, fd } => {
                write!(
                    f,
                    "invalid DLC {dlc} for {}",
                    if *fd { "CAN FD" } else { "Classic CAN" }
                )
            }
            FrameError::RemoteInFd => write!(f, "remote frames are not allowed in CAN FD"),
            FrameError::FdFlagOnClassic => write!(f, "BRS/ESI flags require a CAN FD frame"),
            FrameError::Parse(msg) => write!(f, "cannot parse frame: {msg}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// CAN identifier: 11-bit standard or 29-bit extended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CanId {
    raw: u32,
    extended: bool,
}

impl CanId {
    /// Standard 11-bit identifier.
    pub fn standard(raw: u32) -> Result<Self, FrameError> {
        if raw > MAX_STANDARD_ID {
            return Err(FrameError::IdOutOfRange { raw, extended: false });
        }
        Ok(Self { raw, extended: false })
    }

    /// Extended 29-bit identifier.
    pub fn extended(raw: u32) -> Result<Self, FrameError> {
        if raw > MAX_EXTENDED_ID {
            return Err(FrameError::IdOutOfRange { raw, extended: true });
        }
        Ok(Self { raw, extended: true })
    }

    /// Picks the format from the value: above 0x7FF means extended.
    pub fn auto(raw: u32) -> Result<Self, FrameError> {
        if raw > MAX_STANDARD_ID {
            Self::extended(raw)
        } else {
            Self::standard(raw)
        }
    }

    pub fn raw(self) -> u32 {
        self.raw
    }

    pub fn is_extended(self) -> bool {
        self.extended
    }
}

impl fmt::Display for CanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.extended {
            write!(f, "0x{:08X}", self.raw)
        } else {
            write!(f, "0x{:03X}", self.raw)
        }
    }
}

/// Classic CAN or CAN FD, with the flags that belong to each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameKind {
    Classic { remote: bool },
    Fd { brs: bool, esi: bool },
}

impl FrameKind {
    pub fn is_fd(self) -> bool {
        matches!(self, FrameKind::Fd { .. })
    }
}

/// Converts a payload length to its DLC code. Errors if the length is not exact.
pub fn len_to_dlc(len: usize, fd: bool) -> Result<u8, FrameError> {
    if len <= CLASSIC_MAX_LEN {
        return Ok(len as u8);
    }
    if fd && let Some(pos) = FD_LENGTHS_ABOVE_8.iter().position(|&l| l == len) {
        return Ok(9 + pos as u8);
    }
    Err(FrameError::InvalidLength { len, fd })
}

/// Converts a DLC code to the payload length.
pub fn dlc_to_len(dlc: u8, fd: bool) -> Result<usize, FrameError> {
    match dlc {
        0..=8 => Ok(dlc as usize),
        9..=15 if fd => Ok(FD_LENGTHS_ABOVE_8[(dlc - 9) as usize]),
        _ => Err(FrameError::InvalidDlc { dlc, fd }),
    }
}

/// Smallest valid CAN FD length that holds `len` bytes (for padding). None if > 64.
pub fn fd_padded_len(len: usize) -> Option<usize> {
    if len <= CLASSIC_MAX_LEN {
        return Some(len);
    }
    FD_LENGTHS_ABOVE_8.iter().copied().find(|&l| l >= len)
}

/// One CAN or CAN FD frame. Fixed size, no heap.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CanFrame {
    id: CanId,
    kind: FrameKind,
    len: u8,
    data: [u8; FD_MAX_LEN],
}

impl CanFrame {
    /// Classic data frame (0..=8 bytes).
    pub fn classic(id: CanId, data: &[u8]) -> Result<Self, FrameError> {
        Self::new(id, FrameKind::Classic { remote: false }, data)
    }

    /// Classic remote frame. `len` is the requested length (0..=8), no data is carried.
    pub fn remote(id: CanId, len: usize) -> Result<Self, FrameError> {
        if len > CLASSIC_MAX_LEN {
            return Err(FrameError::InvalidLength { len, fd: false });
        }
        Ok(Self {
            id,
            kind: FrameKind::Classic { remote: true },
            len: len as u8,
            data: [0; FD_MAX_LEN],
        })
    }

    /// CAN FD frame. Length must be one of the valid FD lengths.
    pub fn fd(id: CanId, data: &[u8], brs: bool, esi: bool) -> Result<Self, FrameError> {
        Self::new(id, FrameKind::Fd { brs, esi }, data)
    }

    fn new(id: CanId, kind: FrameKind, data: &[u8]) -> Result<Self, FrameError> {
        let fd = kind.is_fd();
        if let FrameKind::Classic { remote: true } = kind {
            return Self::remote(id, data.len());
        }
        len_to_dlc(data.len(), fd)?;
        let mut buf = [0u8; FD_MAX_LEN];
        buf[..data.len()].copy_from_slice(data);
        Ok(Self {
            id,
            kind,
            len: data.len() as u8,
            data: buf,
        })
    }

    pub fn id(&self) -> CanId {
        self.id
    }

    pub fn kind(&self) -> FrameKind {
        self.kind
    }

    pub fn is_fd(&self) -> bool {
        self.kind.is_fd()
    }

    pub fn is_remote(&self) -> bool {
        matches!(self.kind, FrameKind::Classic { remote: true })
    }

    /// Payload length in bytes (for remote frames: the requested length).
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// DLC code (0..=15).
    pub fn dlc(&self) -> u8 {
        len_to_dlc(self.len as usize, self.is_fd()).expect("length validated at construction")
    }

    /// Payload bytes. Empty for remote frames.
    pub fn data(&self) -> &[u8] {
        if self.is_remote() {
            &[]
        } else {
            &self.data[..self.len as usize]
        }
    }

    /// Payload as uppercase hex without separators.
    pub fn data_hex(&self) -> String {
        self.data().iter().map(|b| format!("{b:02X}")).collect()
    }
}

impl fmt::Debug for CanFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CanFrame({self})")
    }
}

impl fmt::Display for CanFrame {
    /// Same syntax that [`parse_frame`] reads back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let id = if self.id.extended {
            format!("{:08X}", self.id.raw)
        } else {
            format!("{:03X}", self.id.raw)
        };
        match self.kind {
            FrameKind::Classic { remote: true } => {
                if self.len == 0 {
                    write!(f, "{id}#R")
                } else {
                    write!(f, "{id}#R{}", self.len)
                }
            }
            FrameKind::Classic { remote: false } => write!(f, "{id}#{}", self.data_hex()),
            FrameKind::Fd { brs, esi } => {
                let flags = (brs as u8) | ((esi as u8) << 1);
                write!(f, "{id}##{flags:X}{}", self.data_hex())
            }
        }
    }
}

/// Parses the cansend-style text form used by `loom can send`.
///
/// - `300#0102`         Classic data frame (standard id because it has ≤ 3 hex digits)
/// - `18DAF110#01`      extended id (more than 3 hex digits, or value above 0x7FF)
/// - `0x300#01`         optional `0x` prefix on the id
/// - `300#R` / `300#R4` Classic remote frame (optional requested length)
/// - `123##1DEADBEEF`   CAN FD: first nibble after `##` = flags (bit0 BRS, bit1 ESI)
/// - `300#01.02.03`     dots between bytes are allowed
///
/// `force_extended` makes a short id extended (CLI `--ext`).
pub fn parse_frame(text: &str, force_extended: bool) -> Result<CanFrame, FrameError> {
    let text = text.trim();
    let (id_part, rest) = text
        .split_once('#')
        .ok_or_else(|| FrameError::Parse(format!("missing '#' in {text:?}")))?;
    let id_hex = id_part
        .strip_prefix("0x")
        .or_else(|| id_part.strip_prefix("0X"))
        .unwrap_or(id_part);
    if id_hex.is_empty() || id_hex.len() > 8 {
        return Err(FrameError::Parse(format!("bad id {id_part:?}")));
    }
    let raw =
        u32::from_str_radix(id_hex, 16).map_err(|_| FrameError::Parse(format!("bad id {id_part:?}")))?;
    let id = if force_extended || id_hex.len() > 3 {
        CanId::extended(raw)?
    } else {
        CanId::auto(raw)?
    };

    if let Some(fd_rest) = rest.strip_prefix('#') {
        let mut chars = fd_rest.chars();
        let flag = chars
            .next()
            .and_then(|c| c.to_digit(16))
            .ok_or_else(|| FrameError::Parse("CAN FD frame needs a flag nibble after '##'".into()))?;
        if flag > 3 {
            return Err(FrameError::Parse(format!("unknown CAN FD flags {flag:X}")));
        }
        let data = parse_hex_bytes(chars.as_str())?;
        return CanFrame::fd(id, &data, flag & 1 != 0, flag & 2 != 0);
    }
    if let Some(r) = rest.strip_prefix('R').or_else(|| rest.strip_prefix('r')) {
        let len = if r.is_empty() {
            0
        } else {
            r.parse::<usize>()
                .map_err(|_| FrameError::Parse(format!("bad remote length {r:?}")))?
        };
        return CanFrame::remote(id, len);
    }
    let data = parse_hex_bytes(rest)?;
    CanFrame::classic(id, &data)
}

fn parse_hex_bytes(s: &str) -> Result<Vec<u8>, FrameError> {
    let clean: String = s.chars().filter(|c| *c != '.' && *c != ' ').collect();
    if !clean.len().is_multiple_of(2) {
        return Err(FrameError::Parse(format!("odd number of hex digits in {s:?}")));
    }
    if clean.len() / 2 > FD_MAX_LEN {
        return Err(FrameError::Parse(format!("more than {FD_MAX_LEN} data bytes")));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|_| FrameError::Parse(format!("bad hex in {s:?}")))
        })
        .collect()
}

/// Rx or Tx as seen by the owning worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Rx,
    Tx,
}

/// Where a device timestamp came from. Never upgrade quality by guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimestampSource {
    /// Interface hardware clock (e.g. XL, PCAN).
    Hardware,
    /// Kernel software timestamp (e.g. SocketCAN `SO_TIMESTAMPNS`).
    KernelSoftware,
    /// Taken by the host process.
    Host,
}

/// Timestamp as delivered by the driver, in its own clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceTimestamp {
    pub value_ns: u64,
    pub source: TimestampSource,
    /// Identifies the clock domain (e.g. one XL port). Timestamps with different
    /// clock ids must not be subtracted from each other.
    pub clock_id: u32,
}

/// A frame plus the facts the worker knows when it sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedFrame {
    pub frame: CanFrame,
    pub dir: Direction,
    /// Host monotonic time when the worker ingested the frame (definition fixed by D3).
    pub ingest_mono_ns: u64,
    pub device_ts: Option<DeviceTimestamp>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn id_ranges() {
        assert!(CanId::standard(0x7FF).is_ok());
        assert_eq!(
            CanId::standard(0x800),
            Err(FrameError::IdOutOfRange {
                raw: 0x800,
                extended: false
            })
        );
        assert!(CanId::extended(0x1FFF_FFFF).is_ok());
        assert!(CanId::extended(0x2000_0000).is_err());
        assert!(CanId::auto(0x800).unwrap().is_extended());
        assert!(!CanId::auto(0x7FF).unwrap().is_extended());
    }

    #[test]
    fn dlc_ladder_round_trip() {
        for dlc in 0..=15u8 {
            let len = dlc_to_len(dlc, true).unwrap();
            assert_eq!(len_to_dlc(len, true).unwrap(), dlc);
        }
        for dlc in 0..=8u8 {
            assert_eq!(dlc_to_len(dlc, false).unwrap(), dlc as usize);
        }
        assert!(dlc_to_len(9, false).is_err());
        assert!(dlc_to_len(16, true).is_err());
        assert!(len_to_dlc(9, true).is_err());
        assert!(len_to_dlc(12, false).is_err());
    }

    #[test]
    fn fd_padding() {
        assert_eq!(fd_padded_len(5), Some(5));
        assert_eq!(fd_padded_len(9), Some(12));
        assert_eq!(fd_padded_len(33), Some(48));
        assert_eq!(fd_padded_len(64), Some(64));
        assert_eq!(fd_padded_len(65), None);
    }

    #[test]
    fn construction_rules() {
        let id = CanId::standard(0x123).unwrap();
        assert!(CanFrame::classic(id, &[0; 9]).is_err());
        assert!(CanFrame::fd(id, &[0; 9], false, false).is_err());
        let f = CanFrame::fd(id, &[0xAA; 12], true, false).unwrap();
        assert_eq!(f.dlc(), 9);
        assert_eq!(f.len(), 12);
        let r = CanFrame::remote(id, 4).unwrap();
        assert!(r.is_remote());
        assert!(r.data().is_empty());
        assert_eq!(r.dlc(), 4);
    }

    #[test]
    fn parse_and_display_round_trip() {
        for text in [
            "300#0102",
            "18DAF110#01",
            "123#R",
            "123#R4",
            "123##1DEADBEEF",
            "7E0##3",
            "000#",
        ] {
            let f = parse_frame(text, false).unwrap();
            assert_eq!(f.to_string(), text, "round trip of {text}");
        }
    }

    #[test]
    fn parse_variants() {
        let f = parse_frame("0x300#01.02", false).unwrap();
        assert_eq!(f.id(), CanId::standard(0x300).unwrap());
        assert_eq!(f.data(), &[1, 2]);
        assert!(parse_frame("300#01", true).unwrap().id().is_extended());
        assert!(parse_frame("800#01", false).unwrap().id().is_extended());
        let fd = parse_frame("123##3AABB", false).unwrap();
        assert_eq!(fd.kind(), FrameKind::Fd { brs: true, esi: true });
        assert_eq!(fd.data(), &[0xAA, 0xBB]);
    }

    #[test]
    fn parse_errors() {
        for bad in [
            "300",
            "#01",
            "300#0",
            "300#ZZ",
            "123456789#00",
            "300#010203040506070809",
            "123##4",
            "123##",
            "300#Rx",
        ] {
            assert!(parse_frame(bad, false).is_err(), "{bad} should fail");
        }
        assert!(parse_frame("123##1".to_string().as_str(), false).is_ok());
        let long = format!("123##0{}", "00".repeat(13));
        assert!(matches!(
            parse_frame(&long, false),
            Err(FrameError::InvalidLength { len: 13, fd: true })
        ));
    }

    #[test]
    fn frame_is_fixed_size() {
        // Fits a 128-byte ring slot together with timing metadata (plan §9.4).
        assert!(std::mem::size_of::<CanFrame>() <= 80);
        assert!(std::mem::size_of::<TimedFrame>() <= 128);
    }
}
