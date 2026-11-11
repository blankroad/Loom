//! Risk classification of outgoing agent/script text (UART plan §10.1–10.2).
//!
//! String rules are a guard against mistakes on the intended path, not a security boundary
//! (D6 threat model). Under F1 every class runs automatically by default; the class is still
//! shown on screen and in the audit log, and the user can turn on confirm/approve/deny per rule
//! in `loom.toml`.
//!
use loom_proto::event::Risk;

mod engine;

pub use engine::{
    BASE_RULES, MAX_CLASSIFY_BYTES, MAX_LINE_BYTES, MAX_SCRATCH_BYTES, PROFILES, REGEX_DFA_SIZE_LIMIT,
    REGEX_NEST_LIMIT, REGEX_SIZE_LIMIT, classify_text, classify_text_with, warm,
};

/// Result of classifying one payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Risk class.
    pub risk: Risk,
    /// Rule id that decided it (`uboot.saveenv`), `None` = default class.
    pub rule: Option<&'static str>,
    /// Human reason.
    pub reason: String,
}

/// One string rule. Patterns match one reassembled line (no line terminator).
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    /// Stable id shown to the user and used as the policy selector.
    pub id: &'static str,
    /// Class when matched.
    pub risk: Risk,
    /// Regex over the line bytes.
    pub pattern: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_r1_and_rules_raise() {
        assert_eq!(classify_text(b"ls -l\r").risk, Risk::R1);
        assert_eq!(classify_text(b"reboot\r").risk, Risk::R2);
        assert_eq!(classify_text(b"saveenv\r").rule, Some("uboot.saveenv"));
        assert_eq!(classify_text(b"echo hi\nsf erase 0 1000\n").risk, Risk::R4);
        assert_eq!(classify_text(b"\x03").risk, Risk::R1);
    }
}
