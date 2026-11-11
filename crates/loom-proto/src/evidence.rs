//! Evidence DTO for wait/check results and test citations (contract v1 §4.3, §10.9,
//! `_xteam/test-domains.md` TC-C5/C7).
//!
//! Domains put `data.evidence[]` ([`Evidence`]) in results; agents copy them into verdicts
//! instead of inventing seqs. The human REF string is
//! `<domain>:<channel>@e<epoch>:seq:<a>[..<b>][#<json-pointer>]`; the epoch-less form
//! `<domain>:<channel>@seq:<a>[..<b>]` is accepted on input (epoch then comes from the session).

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ProtoError;
use crate::event::{SeqRange, SourceRef, is_token};
use crate::num::{U64, parse_u64_canonical};

/// Machine reference to a stream range plus its source identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceRef {
    /// Stream epoch the seq range belongs to.
    pub stream_epoch: U64,
    /// Inclusive stream range.
    pub seq_range: SeqRange,
    /// Source identities (journal truth) of the cited events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

/// One evidence item in `data.evidence[]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    /// Where the evidence is.
    #[serde(rename = "ref")]
    pub r#ref: EvidenceRef,
    /// Evidence kind (`uart.match`, `can.signal`, `uds.rx`, `uart.timeout` …).
    pub kind: String,
    /// RFC 6901 pointer into the published record JSON (`/payload/state`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// Plan expectation id this evidence answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate_id: Option<String>,
    /// Observed value summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
    /// Expected value summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    /// Request id linking request and response rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// True when the checked window had full coverage (no gap / retention / no-record loss).
    pub complete: bool,
}

/// Validate an RFC 6901 JSON Pointer (`""` or `/a/b~1c~0d`).
pub fn validate_json_pointer(p: &str) -> Result<(), ProtoError> {
    if p.is_empty() {
        return Ok(());
    }
    if !p.starts_with('/') {
        return Err(ProtoError::InvalidPointer(format!("must start with '/': {p:?}")));
    }
    let b = p.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'~' && !matches!(b.get(i + 1), Some(b'0' | b'1')) {
            return Err(ProtoError::InvalidPointer(format!(
                "'~' must be followed by 0 or 1: {p:?}"
            )));
        }
    }
    Ok(())
}

/// Human evidence REF string (parse/format).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefString {
    /// Domain token.
    pub domain: String,
    /// Channel name (`[A-Za-z0-9_.-]+`).
    pub channel: String,
    /// Stream epoch; `None` for the epoch-less input form.
    pub epoch: Option<u64>,
    /// First seq (inclusive).
    pub from: u64,
    /// Last seq (inclusive); equals `from` for a single event.
    pub to: u64,
    /// Optional JSON Pointer fragment.
    pub pointer: Option<String>,
}

fn bad(s: &str, why: &str) -> ProtoError {
    let shown: String = s.chars().take(96).collect();
    ProtoError::InvalidRef(format!("{why}: {shown:?}"))
}

fn valid_channel(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

impl FromStr for RefString {
    type Err = ProtoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() > 512 {
            return Err(bad(s, "too long"));
        }
        let (body, pointer) = match s.split_once('#') {
            Some((b, p)) => {
                validate_json_pointer(p)?;
                (b, Some(p.to_owned()))
            }
            None => (s, None),
        };
        let (target, pos) = body.split_once('@').ok_or_else(|| bad(s, "missing '@'"))?;
        let (domain, channel) = target
            .split_once(':')
            .ok_or_else(|| bad(s, "expected <domain>:<channel>"))?;
        if !is_token(domain) {
            return Err(bad(s, "bad domain"));
        }
        if !valid_channel(channel) {
            return Err(bad(s, "bad channel"));
        }
        let (epoch, range) = if let Some(rest) = pos.strip_prefix("seq:") {
            (None, rest)
        } else {
            let (e, rest) = pos
                .split_once(':')
                .ok_or_else(|| bad(s, "expected e<epoch>:seq:"))?;
            let e = e.strip_prefix('e').ok_or_else(|| bad(s, "expected e<epoch>"))?;
            let e = parse_u64_canonical(e).map_err(|_| bad(s, "bad epoch"))?;
            let rest = rest.strip_prefix("seq:").ok_or_else(|| bad(s, "expected seq:"))?;
            (Some(e), rest)
        };
        let (a, b) = match range.split_once("..") {
            Some((a, b)) => (a, b),
            None => (range, range),
        };
        let from = parse_u64_canonical(a).map_err(|_| bad(s, "bad seq"))?;
        let to = parse_u64_canonical(b).map_err(|_| bad(s, "bad seq"))?;
        if from > to {
            return Err(bad(s, "range start > end"));
        }
        Ok(RefString {
            domain: domain.to_owned(),
            channel: channel.to_owned(),
            epoch,
            from,
            to,
            pointer,
        })
    }
}

impl fmt::Display for RefString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}@", self.domain, self.channel)?;
        if let Some(e) = self.epoch {
            write!(f, "e{e}:")?;
        }
        write!(f, "seq:{}", self.from)?;
        if self.to != self.from {
            write!(f, "..{}", self.to)?;
        }
        if let Some(p) = &self.pointer {
            write!(f, "#{p}")?;
        }
        Ok(())
    }
}

impl RefString {
    /// Inclusive range as a [`SeqRange`].
    pub fn seq_range(&self) -> SeqRange {
        SeqRange {
            from: U64(self.from),
            to: U64(self.to),
        }
    }
}
