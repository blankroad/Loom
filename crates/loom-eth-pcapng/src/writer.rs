use crate::{wire::*, *};
use std::io::{Read, Write};
#[derive(Debug)]
pub struct PcapNgWriter<W: Write> {
    writer: W,
    interfaces: Vec<Interface>,
}
impl<W: Write> PcapNgWriter<W> {
    pub fn new(writer: W) -> Result<Self> {
        let mut s = Self {
            writer,
            interfaces: Vec::new(),
        };
        s.start_section()?;
        Ok(s)
    }
    pub fn start_section(&mut self) -> Result<()> {
        let mut body = Vec::with_capacity(16);
        body.extend(0x1a2b3c4du32.to_le_bytes());
        body.extend(1u16.to_le_bytes());
        body.extend(0u16.to_le_bytes());
        body.extend((-1i64).to_le_bytes());
        self.writer.write_all(&block_bytes(SHB, &body)?)?;
        self.interfaces.clear();
        Ok(())
    }
    pub fn write_interface(&mut self, interface: &Interface) -> Result<u32> {
        interface.validate()?;
        if self.interfaces.len() >= MAX_INTERFACES {
            return Err(Error::limit("interfaces"));
        }
        let mut body = Vec::with_capacity(24 + MAX_NAME_BYTES);
        body.extend(interface.linktype.to_le_bytes());
        body.extend(0u16.to_le_bytes());
        body.extend(interface.snaplen.to_le_bytes());
        if !interface.name.is_empty() {
            append_option(&mut body, 2, interface.name.as_bytes())?;
        }
        append_option(&mut body, 9, &[interface.tsresol.byte()?])?;
        append_option(&mut body, 0, &[])?;
        self.writer.write_all(&block_bytes(IDB, &body)?)?;
        let id = self.interfaces.len() as u32;
        self.interfaces.push(interface.clone());
        Ok(id)
    }
    pub fn write_packet(&mut self, packet: &Packet) -> Result<()> {
        let interface = self
            .interfaces
            .get(packet.interface_id as usize)
            .ok_or_else(|| Error::invalid("EPB interface id"))?;
        if interface.snaplen != 0 && packet.caplen > interface.snaplen {
            return Err(Error::invalid("caplen exceeds snaplen"));
        }
        self.writer.write_all(&epb_bytes(packet, interface.tsresol)?)?;
        Ok(())
    }
    pub fn write_statistics(&mut self, stats: &InterfaceStatistics) -> Result<()> {
        let i = self
            .interfaces
            .get(stats.interface_id as usize)
            .ok_or_else(|| Error::invalid("ISB interface id"))?;
        let t = i.tsresol.ticks(stats.ts_ns)?;
        let mut body = Vec::with_capacity(28);
        for n in [stats.interface_id, (t >> 32) as u32, t as u32] {
            body.extend(n.to_le_bytes());
        }
        if let Some(n) = stats.if_drop {
            append_option(&mut body, 5, &n.to_le_bytes())?;
        }
        append_option(&mut body, 0, &[])?;
        self.writer.write_all(&block_bytes(ISB, &body)?)?;
        Ok(())
    }
    pub fn into_inner(self) -> W {
        self.writer
    }
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}
/// Canonical EPB record: little endian, timestamps are always u64 nanoseconds.
/// Fallible so invalid packets/metadata/oversize timestamps are never silently written.
pub fn encode_record(packet: &Packet) -> Result<Vec<u8>> {
    epb_bytes(packet, TimestampResolution::NANO)
}
pub fn try_encode_record(packet: &Packet) -> Result<Vec<u8>> {
    encode_record(packet)
}
pub fn decode_record(bytes: &[u8]) -> Result<Packet> {
    validate_record(bytes)?;
    parse_epb(
        &bytes[8..bytes.len() - 4],
        Endian::Le,
        TimestampResolution::NANO,
        0,
    )
}
fn validate_record(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_BLOCK_BYTES {
        return Err(Error::limit("record bytes"));
    }
    if bytes.len() < 32 || !bytes.len().is_multiple_of(4) {
        return Err(Error::invalid("record size"));
    }
    if Endian::Le.u32(bytes)? != EPB
        || Endian::Le.u32(&bytes[4..])? as usize != bytes.len()
        || Endian::Le.u32(&bytes[bytes.len() - 4..])? as usize != bytes.len()
    {
        return Err(Error::invalid("record envelope"));
    }
    Ok(())
}
/// Export one canonical capture session. IDBs are normalized to nanosecond resolution;
/// validated EPB bytes are copied verbatim, without collecting records or re-encoding.
/// A failure can leave a valid prefix in `writer`; callers should use transactional files.
pub fn export<'a>(
    header: &SessionHeader,
    records: impl Iterator<Item = &'a [u8]>,
    writer: impl Write,
) -> Result<()> {
    header.validate()?;
    let mut out = PcapNgWriter::new(writer)?;
    for i in &header.interfaces {
        let mut i = i.clone();
        i.tsresol = TimestampResolution::NANO;
        out.write_interface(&i)?;
    }
    for bytes in records {
        validate_record(bytes)?;
        let id = Endian::Le.u32(&bytes[8..])? as usize;
        let i = header
            .interfaces
            .get(id)
            .ok_or_else(|| Error::invalid("record interface id"))?;
        let caplen = Endian::Le.u32(&bytes[20..])?;
        if i.snaplen != 0 && caplen > i.snaplen {
            return Err(Error::invalid("record exceeds snaplen"));
        }
        // Validate internal structure using borrowed bytes; the packet data is not copied.
        let body = &bytes[8..bytes.len() - 4];
        let n = caplen as usize;
        if n > MAX_PACKET_BYTES || n > Endian::Le.u32(&bytes[24..])? as usize || 20 + padded(n) > body.len() {
            return Err(Error::invalid("record packet length"));
        }
        let mut comments = 0;
        let mut metadata_seen = false;
        options(&body[20 + padded(n)..], Endian::Le, |code, data| {
            if code == 1 {
                comments += 1;
                if comments > MAX_COMMENTS || data.len() > MAX_COMMENT_BYTES {
                    return Err(Error::limit("record comments"));
                }
                let text = std::str::from_utf8(data).map_err(|_| Error::invalid("comment UTF-8"))?;
                if LoomMeta::from_comment(text).is_some() {
                    if metadata_seen {
                        return Err(Error::invalid("duplicate Loom metadata"));
                    }
                    metadata_seen = true;
                }
            }
            Ok(())
        })?;
        out.writer.write_all(bytes)?;
    }
    out.flush()
}
// Ensure the public IO API does not require Seek, filesystem access, or a native capture library.
fn _read_only_bound<R: Read>(_reader: R) {}
