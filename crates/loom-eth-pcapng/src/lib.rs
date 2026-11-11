//! Bounded, streaming pcapng/pcap IO and the canonical nanosecond `pcap-epb/1` codec.
//! Only `Read`/`Write` are required. Offsets are absolute byte offsets; `last_good_offset`
//! is the end of the last completely validated block (the recoverable file prefix).
mod reader;
mod wire;
mod writer;
pub use reader::PcapNgReader;
use serde::{Deserialize, Serialize};
use std::{fmt, io};
pub use writer::{PcapNgWriter, decode_record, encode_record, export, try_encode_record};

/// Hard/default block ceiling; reject before allocating a block. Callers may lower it.
pub const MAX_BLOCK_BYTES: usize = 1024 * 1024;
/// Per-section IDB ceiling; reject additional interfaces.
pub const MAX_INTERFACES: usize = 256;
/// Packet byte ceiling; reject rather than truncate implicitly.
pub const MAX_PACKET_BYTES: usize = MAX_BLOCK_BYTES - 32;
/// Per-block comment ceiling; reject additional comments.
pub const MAX_COMMENTS: usize = 64;
/// Per-comment UTF-8 byte ceiling; reject before copying.
pub const MAX_COMMENT_BYTES: usize = 4096;
/// Interface name byte ceiling; reject before copying.
pub const MAX_NAME_BYTES: usize = 255;
/// Request-id byte ceiling; reject rather than truncate metadata.
pub const MAX_REQUEST_BYTES: usize = 512;
/// Bounded option visitor count; reject oversized option lists including unknown options.
pub const MAX_OPTIONS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimestampResolution {
    Decimal(u8),
    Binary(u8),
}
impl TimestampResolution {
    pub const MICRO: Self = Self::Decimal(6);
    pub const NANO: Self = Self::Decimal(9);
    pub(crate) fn byte(self) -> Result<u8> {
        match self {
            Self::Decimal(n) if n < 128 => Ok(n),
            Self::Binary(n) if n < 128 => Ok(128 | n),
            _ => Err(Error::invalid("timestamp resolution exponent")),
        }
    }
    pub(crate) fn from_byte(n: u8) -> Self {
        if n & 128 == 0 {
            Self::Decimal(n)
        } else {
            Self::Binary(n & 127)
        }
    }
    fn denominator(self) -> Option<u128> {
        match self {
            Self::Decimal(n) => 10u128.checked_pow(n.into()),
            Self::Binary(n) => 2u128.checked_pow(n.into()),
        }
    }
    /// Interpret pcapng ticks with floor-to-nanosecond rounding for finer resolutions.
    /// u128 preserves even the full u64 microsecond timestamp domain without overflow.
    pub fn to_ns(self, ticks: u64) -> u128 {
        self.denominator()
            .map_or(0, |den| u128::from(ticks) * 1_000_000_000 / den)
    }
    pub(crate) fn ticks(self, ns: u128) -> Result<u64> {
        self.byte()?;
        let den = self
            .denominator()
            .ok_or_else(|| Error::invalid("timestamp resolution too fine for writing"))?;
        let whole = (ns / 1_000_000_000).checked_mul(den);
        let frac = (ns % 1_000_000_000).checked_mul(den);
        let (whole, frac) = whole
            .zip(frac)
            .ok_or_else(|| Error::invalid("timestamp overflow"))?;
        if frac % 1_000_000_000 != 0 {
            return Err(Error::invalid("timestamp loses precision"));
        }
        u64::try_from(
            whole
                .checked_add(frac / 1_000_000_000)
                .ok_or_else(|| Error::invalid("timestamp overflow"))?,
        )
        .map_err(|_| Error::invalid("timestamp exceeds u64 ticks"))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interface {
    pub linktype: u16,
    pub snaplen: u32,
    pub name: String,
    pub tsresol: TimestampResolution,
}
impl Interface {
    pub fn validate(&self) -> Result<()> {
        if self.name.len() > MAX_NAME_BYTES {
            return Err(Error::limit("interface name"));
        }
        self.tsresol.byte()?;
        Ok(())
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SessionHeader {
    pub interfaces: Vec<Interface>,
}
impl SessionHeader {
    pub fn validate(&self) -> Result<()> {
        if self.interfaces.len() > MAX_INTERFACES {
            return Err(Error::limit("interfaces"));
        }
        for i in &self.interfaces {
            i.validate()?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Rx,
    Tx,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Agent,
    User,
    Script,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoomMeta {
    pub seq: u64,
    pub dir: Direction,
    pub req: Option<String>,
    pub org: Origin,
}
impl LoomMeta {
    pub fn to_comment(&self) -> Result<String> {
        if let Some(req) = &self.req
            && (req.is_empty() || req.len() > MAX_REQUEST_BYTES || req.chars().any(char::is_whitespace))
        {
            return Err(Error::invalid("request id must be nonempty and whitespace-free"));
        }
        let dir = match self.dir {
            Direction::Rx => "rx",
            Direction::Tx => "tx",
        };
        let org = match self.org {
            Origin::Agent => "agent",
            Origin::User => "user",
            Origin::Script => "script",
        };
        let mut out = format!("loom1 seq={} dir={dir} org={org}", self.seq);
        if let Some(req) = &self.req {
            out.push_str(" req=");
            out.push_str(req);
        }
        Ok(out)
    }
    /// Malformed/unknown-version comments remain ordinary user comments, never attribution.
    pub fn from_comment(s: &str) -> Option<Self> {
        if s.len() > MAX_COMMENT_BYTES {
            return None;
        }
        let rest = s.strip_prefix("loom1 ")?;
        let (mut seq, mut dir, mut req, mut org) = (None, None, None, None);
        for part in rest.split(' ') {
            let (k, v) = part.split_once('=')?;
            match k {
                "seq" if seq.is_none() => seq = Some(v.parse().ok()?),
                "dir" if dir.is_none() => {
                    dir = Some(match v {
                        "rx" => Direction::Rx,
                        "tx" => Direction::Tx,
                        _ => return None,
                    })
                }
                "org" if org.is_none() => {
                    org = Some(match v {
                        "agent" => Origin::Agent,
                        "user" => Origin::User,
                        "script" => Origin::Script,
                        _ => return None,
                    })
                }
                "req"
                    if req.is_none()
                        && !v.is_empty()
                        && v.len() <= MAX_REQUEST_BYTES
                        && !v.chars().any(char::is_whitespace) =>
                {
                    req = Some(v.to_owned())
                }
                _ => return None,
            }
        }
        Some(Self {
            seq: seq?,
            dir: dir?,
            req,
            org: org?,
        })
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Packet {
    pub interface_id: u32,
    pub section_index: u64,
    pub ts_ns: u128,
    pub caplen: u32,
    pub origlen: u32,
    pub data: Vec<u8>,
    /// User comments; successfully parsed loom1 metadata is held separately in `meta`.
    pub comments: Vec<String>,
    pub meta: Option<LoomMeta>,
}
impl Packet {
    pub fn validate(&self) -> Result<()> {
        if self.data.len() > MAX_PACKET_BYTES {
            return Err(Error::limit("packet bytes"));
        }
        if self.caplen as usize != self.data.len() || self.caplen > self.origlen {
            return Err(Error::invalid("caplen/data/origlen mismatch"));
        }
        if self.comments.len() + usize::from(self.meta.is_some()) > MAX_COMMENTS {
            return Err(Error::limit("comments"));
        }
        for c in &self.comments {
            if c.len() > MAX_COMMENT_BYTES {
                return Err(Error::limit("comment bytes"));
            }
            if LoomMeta::from_comment(c).is_some() {
                return Err(Error::invalid("reserved loom1 comment; use meta"));
            }
        }
        if let Some(m) = &self.meta {
            m.to_comment()?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceStatistics {
    pub interface_id: u32,
    pub ts_ns: u128,
    pub if_drop: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Section { index: u64 },
    Interface { id: u32, interface: Interface },
    Packet(Packet),
    Statistics(InterfaceStatistics),
    Unknown { block_type: u32 },
}
#[derive(Debug)]
pub enum ErrorKind {
    Io(io::Error),
    Truncated,
    Invalid(&'static str),
    Limit(&'static str),
}
#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: u64,
    pub last_good_offset: u64,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub(crate) fn invalid(why: &'static str) -> Self {
        Self {
            kind: ErrorKind::Invalid(why),
            offset: 0,
            last_good_offset: 0,
        }
    }
    pub(crate) fn limit(why: &'static str) -> Self {
        Self {
            kind: ErrorKind::Limit(why),
            offset: 0,
            last_good_offset: 0,
        }
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self {
            kind: ErrorKind::Io(e),
            offset: 0,
            last_good_offset: 0,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} at {}, valid prefix {}",
            self.kind, self.offset, self.last_good_offset
        )
    }
}
impl std::error::Error for Error {}
