//! Pure bounded ISO-TP CLI argument parsing.
/// Maximum ISO-TP receive/send timeout.
pub const MAX_TIMEOUT_MS: u32 = 60_000;

/// Parse a nonempty bounded hex PDU.
pub fn parse_pdu(s: &str) -> Result<Vec<u8>, String> {
    let clean: String = s.chars().filter(|c| !matches!(c, ' ' | '.' | ':')).collect();
    if !clean.is_ascii()
        || clean.is_empty()
        || !clean.len().is_multiple_of(2)
        || clean.len() > 2 * loom_can_isotp::MAX_PDU
    {
        return Err(format!(
            "pdu must be 1..={} bytes of hex",
            loom_can_isotp::MAX_PDU
        ));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|_| format!("bad hex in pdu: {s:?}")))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn multibyte_non_hex_is_rejected_without_slicing_a_character() {
        assert!(super::parse_pdu("ああ").is_err());
    }
}
