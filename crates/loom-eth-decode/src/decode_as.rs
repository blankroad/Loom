//! Read-time "Decode As" rules (operator-upgrade B3b, finding ETH-06): decode a non-standard port as
//! DoIP or SOME/IP, like Wireshark's Decode As. Rules are an interpretation of stored bytes chosen by
//! the reader (UI/CLI/agent) per request — never written into captured records. Every packet decoded
//! through a rule carries a `decode_as` warning so the interpretation is visible.

use std::fmt;

/// Rules per request.
pub const MAX_DECODE_AS_RULES: usize = 8;

/// Transport a rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// TCP (stream reassembly uses the rule's application framing).
    Tcp,
    /// UDP (each datagram).
    Udp,
}

/// Application protocol a port is decoded as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppProto {
    /// ISO 13400 DoIP.
    Doip,
    /// SOME/IP (incl. SD).
    SomeIp,
}

/// One rule: `transport` `port` (either direction) → `proto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecodeAsRule {
    pub transport: Transport,
    pub port: u16,
    pub proto: AppProto,
}

/// Validated rule set (≤ [`MAX_DECODE_AS_RULES`], no port 0, no conflicting rules). Empty = defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodeAs {
    rules: Vec<DecodeAsRule>,
}

/// Why a rule set was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeAsError {
    /// More than [`MAX_DECODE_AS_RULES`] distinct rules.
    TooMany { max: usize },
    /// Port 0.
    BadPort,
    /// Same transport + port mapped to two protocols.
    Conflict { transport: Transport, port: u16 },
}

impl Transport {
    /// `"tcp"` | `"udp"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Tcp => "tcp",
            Transport::Udp => "udp",
        }
    }
}

impl AppProto {
    /// `"doip"` | `"someip"`.
    pub fn as_str(self) -> &'static str {
        match self {
            AppProto::Doip => "doip",
            AppProto::SomeIp => "someip",
        }
    }
}

impl DecodeAs {
    /// Validates and de-duplicates `rules` (FUNCTIONS.md F5.2).
    pub fn new(rules: &[DecodeAsRule]) -> Result<DecodeAs, DecodeAsError> {
        let mut accepted: Vec<DecodeAsRule> = Vec::new();
        for r in rules {
            if r.port == 0 {
                return Err(DecodeAsError::BadPort);
            }
            if accepted.contains(r) {
                continue;
            }
            if accepted
                .iter()
                .any(|a| a.transport == r.transport && a.port == r.port)
            {
                return Err(DecodeAsError::Conflict {
                    transport: r.transport,
                    port: r.port,
                });
            }
            accepted.push(*r);
        }
        if accepted.len() > MAX_DECODE_AS_RULES {
            return Err(DecodeAsError::TooMany {
                max: MAX_DECODE_AS_RULES,
            });
        }
        Ok(DecodeAs { rules: accepted })
    }
    /// The rules, in input order (duplicates removed).
    pub fn rules(&self) -> &[DecodeAsRule] {
        &self.rules
    }
    /// No rule.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
    /// Rule for a packet of `transport` between `src_port` and `dst_port`: the destination port's
    /// rule first, else the source port's (FUNCTIONS.md F5.3).
    pub fn lookup(&self, transport: Transport, src_port: u16, dst_port: u16) -> Option<DecodeAsRule> {
        self.rules
            .iter()
            .find(|r| r.transport == transport && r.port == dst_port)
            .or_else(|| {
                self.rules
                    .iter()
                    .find(|r| r.transport == transport && r.port == src_port)
            })
            .copied()
    }
}

impl fmt::Display for DecodeAsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeAsError::BadPort => write!(f, "decode_as port must be 1..65535"),
            DecodeAsError::TooMany { max } => write!(f, "decode_as allows at most {max} rules"),
            DecodeAsError::Conflict { transport, port } => {
                write!(
                    f,
                    "decode_as maps {} port {port} to two protocols",
                    transport.as_str()
                )
            }
        }
    }
}

impl std::error::Error for DecodeAsError {}
