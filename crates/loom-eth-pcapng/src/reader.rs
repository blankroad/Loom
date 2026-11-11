use crate::{wire::*, *};
use std::io::{self, Read};
#[derive(Debug, Clone, Copy)]
enum Format {
    Ng(Option<Endian>),
    Pcap(Endian, TimestampResolution),
}
#[derive(Debug)]
pub struct PcapNgReader<R: Read> {
    reader: R,
    format: Option<Format>,
    interfaces: Vec<Interface>,
    section: u64,
    seen_section: bool,
    offset: u64,
    last_good: u64,
    limit: usize,
    done: bool,
}
impl<R: Read> PcapNgReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            format: None,
            interfaces: Vec::new(),
            section: 0,
            seen_section: false,
            offset: 0,
            last_good: 0,
            limit: MAX_BLOCK_BYTES,
            done: false,
        }
    }
    pub fn with_block_limit(reader: R, limit: usize) -> Result<Self> {
        if !(28..=MAX_BLOCK_BYTES).contains(&limit) {
            return Err(Error::limit("block limit must be 28..=1 MiB"));
        }
        Ok(Self {
            limit,
            ..Self::new(reader)
        })
    }
    pub fn interfaces(&self) -> &[Interface] {
        &self.interfaces
    }
    pub fn section_index(&self) -> u64 {
        self.section
    }
    pub fn last_good_offset(&self) -> u64 {
        self.last_good
    }
    pub fn into_inner(self) -> R {
        self.reader
    }
    fn read_exact_or_eof(&mut self, buf: &mut [u8], allow_eof: bool) -> Result<bool> {
        let mut n = 0;
        while n < buf.len() {
            match self.reader.read(&mut buf[n..]) {
                Ok(0) if n == 0 && allow_eof => return Ok(false),
                Ok(0) => {
                    return Err(Error {
                        kind: ErrorKind::Truncated,
                        offset: self.offset,
                        last_good_offset: self.last_good,
                    });
                }
                Ok(k) => {
                    n += k;
                    self.offset = self
                        .offset
                        .checked_add(k as u64)
                        .ok_or_else(|| Error::invalid("file offset overflow"))?;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(true)
    }
    pub fn next_block(&mut self) -> Result<Option<Block>> {
        if self.done {
            return Ok(None);
        }
        match self.next_inner() {
            Ok(Some(b)) => {
                self.last_good = self.offset;
                Ok(Some(b))
            }
            Ok(None) => {
                self.done = true;
                Ok(None)
            }
            Err(mut e) => {
                e.offset = self.offset;
                e.last_good_offset = self.last_good;
                self.done = true;
                Err(e)
            }
        }
    }
    pub fn next_packet(&mut self) -> Result<Option<Packet>> {
        while let Some(b) = self.next_block()? {
            if let Block::Packet(p) = b {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }
    fn next_inner(&mut self) -> Result<Option<Block>> {
        if self.format.is_none() {
            let mut magic = [0; 4];
            self.read_exact_or_eof(&mut magic, false)?;
            if magic == SHB.to_le_bytes() {
                self.format = Some(Format::Ng(None));
                return self.read_ng(Some(magic));
            }
            let (e, res) = match magic {
                [0xd4, 0xc3, 0xb2, 0xa1] => (Endian::Le, TimestampResolution::MICRO),
                [0xa1, 0xb2, 0xc3, 0xd4] => (Endian::Be, TimestampResolution::MICRO),
                [0x4d, 0x3c, 0xb2, 0xa1] => (Endian::Le, TimestampResolution::NANO),
                [0xa1, 0xb2, 0x3c, 0x4d] => (Endian::Be, TimestampResolution::NANO),
                _ => return Err(Error::invalid("capture magic")),
            };
            let mut h = [0; 20];
            self.read_exact_or_eof(&mut h, false)?;
            if e.u16(&h)? != 2 || e.u16(&h[2..])? != 4 {
                return Err(Error::invalid("pcap version"));
            }
            let snaplen = e.u32(&h[12..])?;
            if snaplen == 0 {
                return Err(Error::invalid("zero pcap snaplen"));
            }
            let interface = Interface {
                linktype: (e.u32(&h[16..])? & 0xffff) as u16,
                snaplen,
                name: String::new(),
                tsresol: res,
            };
            self.interfaces.push(interface.clone());
            self.format = Some(Format::Pcap(e, res));
            return Ok(Some(Block::Interface { id: 0, interface }));
        }
        match self.format {
            Some(Format::Pcap(e, res)) => self.read_pcap(e, res),
            _ => self.read_ng(None),
        }
    }
    fn read_pcap(&mut self, e: Endian, res: TimestampResolution) -> Result<Option<Block>> {
        let mut h = [0; 16];
        if !self.read_exact_or_eof(&mut h, true)? {
            return Ok(None);
        }
        let sec = e.u32(&h)?;
        let sub = e.u32(&h[4..])?;
        let scale = if res == TimestampResolution::NANO {
            1_000_000_000
        } else {
            1_000_000
        };
        if sub >= scale {
            return Err(Error::invalid("pcap fractional timestamp"));
        }
        let n = e.u32(&h[8..])?;
        let orig = e.u32(&h[12..])?;
        if n as usize > MAX_PACKET_BYTES || n as usize + 16 > self.limit {
            return Err(Error::limit("pcap packet bytes"));
        }
        if n > orig || n > self.interfaces[0].snaplen {
            return Err(Error::invalid("pcap caplen"));
        }
        let mut data = vec![0; n as usize];
        self.read_exact_or_eof(&mut data, false)?;
        Ok(Some(Block::Packet(Packet {
            ts_ns: u128::from(sec) * 1_000_000_000 + res.to_ns(sub.into()),
            caplen: n,
            origlen: orig,
            data,
            ..Packet::default()
        })))
    }
    fn read_ng(&mut self, first: Option<[u8; 4]>) -> Result<Option<Block>> {
        let mut kind = [0; 4];
        if let Some(first) = first {
            kind = first;
        } else if !self.read_exact_or_eof(&mut kind, true)? {
            return Ok(None);
        }
        let mut len = [0; 4];
        self.read_exact_or_eof(&mut len, false)?;
        let shb = kind == SHB.to_le_bytes();
        let mut bom = [0; 4];
        let e = if shb {
            self.read_exact_or_eof(&mut bom, false)?;
            match bom {
                [0x4d, 0x3c, 0x2b, 0x1a] => Endian::Le,
                [0x1a, 0x2b, 0x3c, 0x4d] => Endian::Be,
                _ => return Err(Error::invalid("section byte order magic")),
            }
        } else {
            match self.format {
                Some(Format::Ng(Some(e))) => e,
                _ => return Err(Error::invalid("missing section")),
            }
        };
        let n = e.u32(&len)? as usize;
        if n > self.limit {
            return Err(Error::limit("block bytes"));
        }
        if n < if shb { 28 } else { 12 } || !n.is_multiple_of(4) {
            return Err(Error::invalid("block length"));
        }
        let prefix = if shb { 12 } else { 8 };
        let mut bytes = vec![0; n - prefix];
        self.read_exact_or_eof(&mut bytes, false)?;
        if e.u32(&bytes[bytes.len() - 4..])? as usize != n {
            return Err(Error::invalid("block trailer length"));
        }
        let body = &bytes[..bytes.len() - 4];
        if shb {
            if e.u16(body)? != 1 || e.u16(&body[2..])? != 0 {
                return Err(Error::invalid("pcapng version"));
            }
            // body starts after BOM: version(4), section_length(8), options.
            if body.len() < 12 {
                return Err(Error::invalid("short section"));
            }
            options(&body[12..], e, |_, _| Ok(()))?;
            if self.seen_section {
                self.section = self
                    .section
                    .checked_add(1)
                    .ok_or_else(|| Error::invalid("section count overflow"))?;
            }
            self.seen_section = true;
            self.interfaces.clear();
            self.format = Some(Format::Ng(Some(e)));
            return Ok(Some(Block::Section { index: self.section }));
        }
        let kind = e.u32(&kind)?;
        Ok(Some(match kind {
            IDB => self.parse_interface(body, e)?,
            EPB => {
                let id = e.u32(body)?;
                let interface = self
                    .interfaces
                    .get(id as usize)
                    .ok_or_else(|| Error::invalid("EPB interface id"))?;
                let p = parse_epb(body, e, interface.tsresol, self.section)?;
                if interface.snaplen != 0 && p.caplen > interface.snaplen {
                    return Err(Error::invalid("caplen exceeds snaplen"));
                }
                Block::Packet(p)
            }
            ISB => {
                if body.len() < 12 {
                    return Err(Error::invalid("short ISB"));
                }
                let id = e.u32(body)?;
                let i = self
                    .interfaces
                    .get(id as usize)
                    .ok_or_else(|| Error::invalid("ISB interface id"))?;
                let ticks = (u64::from(e.u32(&body[4..])?) << 32) | u64::from(e.u32(&body[8..])?);
                let mut if_drop = None;
                options(&body[12..], e, |code, data| {
                    if code == 5 {
                        if data.len() != 8 {
                            return Err(Error::invalid("isb_ifdrop length"));
                        }
                        if_drop = Some(e.u64(data)?);
                    }
                    Ok(())
                })?;
                Block::Statistics(InterfaceStatistics {
                    interface_id: id,
                    ts_ns: i.tsresol.to_ns(ticks),
                    if_drop,
                })
            }
            _ => Block::Unknown { block_type: kind },
        }))
    }
    fn parse_interface(&mut self, body: &[u8], e: Endian) -> Result<Block> {
        if self.interfaces.len() >= MAX_INTERFACES {
            return Err(Error::limit("interfaces"));
        }
        if body.len() < 8 || e.u16(&body[2..])? != 0 {
            return Err(Error::invalid("IDB header"));
        }
        let mut interface = Interface {
            linktype: e.u16(body)?,
            snaplen: e.u32(&body[4..])?,
            name: String::new(),
            tsresol: TimestampResolution::MICRO,
        };
        let (mut name, mut res) = (false, false);
        options(&body[8..], e, |code, data| {
            match code {
                2 if !name => {
                    interface.name = text(data, MAX_NAME_BYTES)?;
                    name = true;
                }
                9 if !res && data.len() == 1 => {
                    interface.tsresol = TimestampResolution::from_byte(data[0]);
                    res = true;
                }
                2 | 9 => return Err(Error::invalid("duplicate/invalid IDB option")),
                // Offset would change timestamps; reject instead of silently interpreting incorrectly.
                14 => return Err(Error::invalid("if_tsoffset is unsupported")),
                _ => (),
            }
            Ok(())
        })?;
        let id = self.interfaces.len() as u32;
        self.interfaces.push(interface.clone());
        Ok(Block::Interface { id, interface })
    }
}
