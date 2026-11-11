//! Request text parser (UI console line, `loom uds raw`) and typed-argument encoding.
//!
//! M1 grammar (ui-uds answer #4):
//! * hex: `22 F1 90`, `22F190`, `0x22 0xF190`
//! * `read <DID> [<DID> …]`
//! * `write <DID> <hex>`
//! * `session <default|ext|extended|prog|programming|safety|hex>`
//! * `tp` (TesterPresent `3E 00`)

use loom_uds_codec::request;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Longest accepted request text (checked before parsing).
pub const MAX_TEXT_LEN: usize = 8192;

/// Where and why parsing failed (`at` = byte offset into the text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ParseError {
    pub at: usize,
    pub message: String,
}

impl ParseError {
    fn new(at: usize, message: impl Into<String>) -> Self {
        Self {
            at,
            message: message.into(),
        }
    }
}

/// Whitespace-separated tokens with their byte offsets.
fn tokens(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(s)) => {
                out.push((s, &text[s..i]));
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &text[s..]));
    }
    out
}

fn strip_0x(t: &str) -> &str {
    t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t)
}

fn hex_tokens(toks: &[(usize, &str)]) -> Result<Vec<u8>, ParseError> {
    let mut digits = String::new();
    for (at, t) in toks {
        let body = strip_0x(t);
        if let Some(pos) = body.find(|c: char| !c.is_ascii_hexdigit()) {
            return Err(ParseError::new(
                at + (t.len() - body.len()) + pos,
                "not a hex digit",
            ));
        }
        digits.push_str(body);
    }
    if digits.is_empty() {
        return Err(ParseError::new(0, "empty request"));
    }
    if !digits.len().is_multiple_of(2) {
        let end = toks.last().map_or(0, |(at, t)| at + t.len());
        return Err(ParseError::new(end, "odd number of hex digits"));
    }
    if digits.len() / 2 > loom_uds_codec::MAX_PDU_LEN {
        return Err(ParseError::new(
            0,
            format!("request longer than {} bytes", loom_uds_codec::MAX_PDU_LEN),
        ));
    }
    hex::decode(&digits).map_err(|e| ParseError::new(0, e.to_string()))
}

/// Parse a DID written as `F190` or `0xF190`.
pub fn parse_did(t: &str) -> Option<u16> {
    let body = strip_0x(t);
    if body.is_empty() || body.len() > 4 {
        return None;
    }
    u16::from_str_radix(body, 16).ok()
}

/// Session name or hex byte → session id.
pub fn parse_session(t: &str) -> Option<u8> {
    match t.to_ascii_lowercase().as_str() {
        "default" | "def" => Some(0x01),
        "programming" | "prog" => Some(0x02),
        "extended" | "ext" => Some(0x03),
        "safety" => Some(0x04),
        other => {
            let body = strip_0x(other);
            if body.is_empty() || body.len() > 2 {
                return None;
            }
            u8::from_str_radix(body, 16).ok().filter(|v| *v != 0)
        }
    }
}

/// Parse a request line into the bytes that would be sent.
pub fn parse_text(text: &str) -> Result<Vec<u8>, ParseError> {
    if text.len() > MAX_TEXT_LEN {
        return Err(ParseError::new(
            MAX_TEXT_LEN,
            format!("text longer than {MAX_TEXT_LEN} bytes"),
        ));
    }
    let toks = tokens(text);
    let Some(&(at0, verb)) = toks.first() else {
        return Err(ParseError::new(0, "empty request"));
    };
    let codec = |at: usize| move |e: loom_uds_codec::CodecError| ParseError::new(at, e.to_string());
    match verb.to_ascii_lowercase().as_str() {
        "read" => {
            if toks.len() < 2 {
                return Err(ParseError::new(at0 + verb.len(), "read needs a DID"));
            }
            let mut dids = Vec::with_capacity(toks.len() - 1);
            for (at, t) in &toks[1..] {
                dids.push(parse_did(t).ok_or_else(|| ParseError::new(*at, "DID must be 1–4 hex digits"))?);
            }
            request::read_dids(&dids).map_err(codec(at0))
        }
        "write" => {
            let Some(&(at_did, did_t)) = toks.get(1) else {
                return Err(ParseError::new(
                    at0 + verb.len(),
                    "write needs a DID and hex data",
                ));
            };
            let did =
                parse_did(did_t).ok_or_else(|| ParseError::new(at_did, "DID must be 1–4 hex digits"))?;
            if toks.len() < 3 {
                return Err(ParseError::new(at_did + did_t.len(), "write needs hex data"));
            }
            let data = hex_tokens(&toks[2..])?;
            request::write_did(did, &data).map_err(codec(at0))
        }
        "session" => {
            let Some(&(at, s)) = toks.get(1) else {
                return Err(ParseError::new(
                    at0 + verb.len(),
                    "session needs a name or hex id",
                ));
            };
            if toks.len() > 2 {
                return Err(ParseError::new(toks[2].0, "unexpected argument"));
            }
            let id = parse_session(s).ok_or_else(|| ParseError::new(at, "unknown session"))?;
            request::session_control(id, false).map_err(codec(at))
        }
        "tp" => {
            if toks.len() > 1 {
                return Err(ParseError::new(toks[1].0, "tp takes no arguments"));
            }
            Ok(request::tester_present(false))
        }
        _ => hex_tokens(&toks),
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn did_arg(v: &Value) -> Option<u16> {
    match v {
        Value::String(s) => parse_did(s),
        Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
        _ => None,
    }
}

/// Bytes a typed RPC would send, from its params. Used by the `classify` hook when the core has
/// not supplied `bytes`, and by dry-run.
pub fn encode_typed(method: &str, args: &Value) -> Result<Vec<u8>, String> {
    match method {
        "uds.request" => {
            if let Some(h) = arg_str(args, "bytes_hex") {
                return hex_tokens(&tokens(h)).map_err(|e| e.message);
            }
            let text = arg_str(args, "text").ok_or("uds.request needs text or bytes_hex")?;
            parse_text(text).map_err(|e| format!("at {}: {}", e.at, e.message))
        }
        "uds.read_did" => {
            let list = args
                .get("dids")
                .and_then(Value::as_array)
                .ok_or("read_did needs dids")?;
            let dids = list
                .iter()
                .map(did_arg)
                .collect::<Option<Vec<_>>>()
                .ok_or("bad DID")?;
            request::read_dids(&dids).map_err(|e| e.to_string())
        }
        "uds.write_did" => {
            let did = args.get("did").and_then(did_arg).ok_or("write_did needs did")?;
            let data = arg_str(args, "data_hex").ok_or("write_did needs data_hex")?;
            let data = hex_tokens(&tokens(data)).map_err(|e| e.message)?;
            request::write_did(did, &data).map_err(|e| e.to_string())
        }
        "uds.session" => {
            let s = match args.get("session") {
                Some(Value::String(s)) => parse_session(s),
                Some(Value::Number(n)) => n.as_u64().and_then(|n| u8::try_from(n).ok()),
                _ => None,
            }
            .ok_or("session needs a name or id")?;
            request::session_control(s, false).map_err(|e| e.to_string())
        }
        "uds.dtc.list" => {
            let mask = match args.get("mask") {
                None | Some(Value::Null) => 0xFF,
                Some(v) => v
                    .as_u64()
                    .and_then(|m| u8::try_from(m).ok())
                    .ok_or("mask must be 0..=255")?,
            };
            Ok(request::read_dtc_by_status_mask(mask))
        }
        other => Err(format!("{other} sends no UDS request")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar() {
        assert_eq!(parse_text("22 F1 90"), Ok(vec![0x22, 0xF1, 0x90]));
        assert_eq!(parse_text("  22F190 "), Ok(vec![0x22, 0xF1, 0x90]));
        assert_eq!(parse_text("0x22 0xF190"), Ok(vec![0x22, 0xF1, 0x90]));
        assert_eq!(
            parse_text("read F190 f18c"),
            Ok(vec![0x22, 0xF1, 0x90, 0xF1, 0x8C])
        );
        assert_eq!(parse_text("write F1A0 02"), Ok(vec![0x2E, 0xF1, 0xA0, 0x02]));
        assert_eq!(parse_text("session ext"), Ok(vec![0x10, 0x03]));
        assert_eq!(parse_text("tp"), Ok(vec![0x3E, 0x00]));
        assert_eq!(
            parse_text("22 F1 9"),
            Err(ParseError::new(7, "odd number of hex digits"))
        );
        assert_eq!(parse_text("22 G1").map_err(|e| e.at), Err(3));
        assert_eq!(parse_text("read").map_err(|e| e.at), Err(4));
        assert_eq!(parse_text("write F1A0").map_err(|e| e.at), Err(10));
        assert_eq!(parse_text("session warp").map_err(|e| e.at), Err(8));
        assert!(parse_text("").is_err());
        assert!(parse_text(&"0".repeat(MAX_TEXT_LEN + 2)).is_err());
    }
}
