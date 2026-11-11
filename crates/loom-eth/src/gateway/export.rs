//! `eth.pkt` record → pcapng EPB for `eth.export`, keeping Loom metadata and comments.
//!
//! - Own packets (dir known and `attributed_origin` set) get the X10 comment
//!   `loom1 seq=<stream seq> dir=… org=… [req=…]` — the seq is the REF coordinate.
//!   Device packets get none: X10 has no `org=device`, and absence already means "observed".
//! - `bytes.comments` (user comments from a replayed file) are written back verbatim.
//! - `bytes.loom_comment` (the X10 string of the original file) becomes
//!   `loom1-orig seq=… dir=… org=… [req=…]` — original coordinates, never read back as
//!   attribution (X10 readers only accept the `loom1 ` prefix).
//! - Limits of the pcapng crate (`MAX_COMMENTS`, `MAX_COMMENT_BYTES`) are respected; comments
//!   that do not fit are dropped and counted.

use loom_eth_pcapng::{
    Direction, LoomMeta, MAX_COMMENT_BYTES, MAX_COMMENTS, MAX_REQUEST_BYTES, Origin as POrigin, Packet,
};
use loom_proto::event::PublishedRecord;
use serde_json::Value;

/// Prefix of a preserved original X10 comment.
pub const ORIG_PREFIX: &str = "loom1-orig ";

/// X10 meta of an own packet (`None` for device packets or unknown direction).
pub fn loom_meta(rec: &PublishedRecord) -> Option<LoomMeta> {
    let p = &rec.payload;
    let dir = match p.get("dir").and_then(Value::as_str)? {
        "tx" => Direction::Tx,
        "rx" => Direction::Rx,
        _ => return None,
    };
    let org = match p.get("attributed_origin").and_then(Value::as_str)? {
        "agent" => POrigin::Agent,
        "user" => POrigin::User,
        "script" => POrigin::Script,
        _ => return None,
    };
    // X10 request ids are nonempty and whitespace-free; anything else is left out, not mangled.
    let req = p
        .pointer("/link/request_id")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty() && r.len() <= MAX_REQUEST_BYTES && !r.chars().any(char::is_whitespace))
        .map(str::to_owned);
    Some(LoomMeta {
        seq: rec.seq.get(),
        dir,
        req,
        org,
    })
}

/// `loom1 …` → `loom1-orig …` (`None` if it is not an X10 string).
pub fn orig_comment(x10: &str) -> Option<String> {
    LoomMeta::from_comment(x10)?;
    let c = format!("{ORIG_PREFIX}{}", x10.strip_prefix("loom1 ")?);
    (c.len() <= MAX_COMMENT_BYTES).then_some(c)
}

/// EPB comments of a record: user comments verbatim, then the original X10 as `loom1-orig`.
/// Returns the comments and how many were dropped (too long / too many).
pub fn comments(rec: &PublishedRecord, has_meta: bool) -> (Vec<String>, usize) {
    let bytes = rec.payload.get("bytes");
    let user: Vec<&str> = bytes
        .and_then(|b| b.get("comments"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let orig = bytes.and_then(|b| b.get("loom_comment")).and_then(Value::as_str);
    let room = MAX_COMMENTS - usize::from(has_meta);
    let mut out = Vec::new();
    let mut dropped = 0;
    let mut push = |c: String, out: &mut Vec<String>| {
        if c.len() <= MAX_COMMENT_BYTES && out.len() < room {
            out.push(c);
        } else {
            dropped += 1;
        }
    };
    // The original X10 goes first so it survives when user comments are many.
    if let Some(o) = orig {
        match orig_comment(o) {
            Some(c) => push(c, &mut out),
            None => push(o.to_owned(), &mut out), // not X10 after all: keep as an ordinary comment
        }
    }
    for c in user {
        // A user comment that parses as X10 would be read back as attribution: keep it as orig.
        match orig_comment(c) {
            Some(o) => push(o, &mut out),
            None => push(c.to_owned(), &mut out),
        }
    }
    (out, dropped)
}

/// The EPB for one record. Returns the packet and the number of dropped comments.
pub fn to_packet(rec: &PublishedRecord, interface_id: u32, data: Vec<u8>) -> (Packet, usize) {
    let caplen = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let origlen = rec
        .payload
        .get("len")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(caplen)
        .max(caplen);
    let meta = loom_meta(rec);
    let (comments, dropped) = comments(rec, meta.is_some());
    let pkt = Packet {
        interface_id,
        section_index: 0,
        ts_ns: u128::from(rec.ts.wall_ns.get()),
        caplen,
        origlen,
        data,
        comments,
        meta,
    };
    (pkt, dropped)
}
