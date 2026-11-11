//! E2E protection *suspicion* (plan §10.2 after B4).
//!
//! This only explains why a message looks E2E protected (CRC / alive counter). Blocking
//! or asking for approval is the policy layer's decision. The `loom.toml e2e_protected`
//! list (hard block) is also handled there, not here.

use crate::model::Message;

/// Name fragments checked case-insensitively, most specific first.
const NAME_PATTERNS: &[&str] = &["checksum", "chksum", "crc", "chk", "counter", "alive"];

/// Why a message is suspected to be E2E protected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum E2eReason {
    MessageName {
        pattern: &'static str,
    },
    SignalName {
        signal: String,
        pattern: &'static str,
    },
    /// A message or signal attribute whose name hints at E2E (`*E2E*`, `*CRC*`, `*DataID*` …).
    Attribute {
        name: String,
    },
}

fn match_name(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    NAME_PATTERNS.iter().copied().find(|p| lower.contains(p))
}

/// Reasons this message looks E2E protected, or `None`. A guess, not a verdict.
pub fn is_e2e_suspect(message: &Message) -> Option<Vec<E2eReason>> {
    let mut reasons = Vec::new();
    if let Some(pattern) = match_name(message.name()) {
        reasons.push(E2eReason::MessageName { pattern });
    }
    for s in message.signals() {
        if let Some(pattern) = match_name(s.name()) {
            reasons.push(E2eReason::SignalName {
                signal: s.name().to_string(),
                pattern,
            });
        }
    }
    for a in message.e2e_attributes() {
        reasons.push(E2eReason::Attribute { name: a.clone() });
    }
    (!reasons.is_empty()).then_some(reasons)
}
