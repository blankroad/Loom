//! Per-target diagnostic DB (U5): the existing `loom_diag` v1 schema from `loom-uds-codec`, attached
//! inline through `uds.db.set` (the daemon never reads a path). Names/units from a DB are untrusted
//! display text. A DB never changes risk (risk.rs classifies bytes only).

use loom_uds_codec::{DidLookup, LoadedDb};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::payload::DecodedDid;

/// Largest serialized DB accepted inline (one RPC message is at most 64 KiB).
pub const MAX_INLINE_DB_BYTES: usize = 48 * 1024;
/// Longest error text returned for an invalid DB.
pub const MAX_DB_ERROR_LEN: usize = 300;

/// What `uds.state.db` / `uds.db.set` report about an attached DB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DbInfo {
    /// `LoadedDb::hash_hex()` (FNV-1a 64 over the normalized form).
    pub hash: String,
    pub ecu: Option<String>,
    pub dids: u32,
}

/// A DB attached to one target (`uds.db.set`): the lookup plus its precomputed identity, taken as
/// one snapshot per request so a row's `db_hash` always names the DB that decoded it.
#[derive(Debug)]
pub struct AttachedDb {
    pub db: LoadedDb,
    pub info: DbInfo,
}

impl AttachedDb {
    /// B3-D5: `info = db_info(&db)`.
    pub fn new(db: LoadedDb) -> Self {
        Self {
            info: db_info(&db),
            db,
        }
    }
}

/// B3-D1: validate an inline DB value.
pub fn parse_inline_db(v: &Value) -> Result<LoadedDb, String> {
    let bytes = serde_json::to_vec(v).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_INLINE_DB_BYTES {
        return Err(format!(
            "diagnostic DB is {} bytes; the inline limit is {MAX_INLINE_DB_BYTES}",
            bytes.len()
        ));
    }
    loom_uds_codec::load_db(&bytes, loom_uds_codec::Format::Json).map_err(|e| {
        format!("invalid loom_diag DB: {e}")
            .chars()
            .take(MAX_DB_ERROR_LEN)
            .collect()
    })
}

/// B3-D2.
pub fn db_info(db: &LoadedDb) -> DbInfo {
    DbInfo {
        hash: db.hash_hex(),
        ecu: db.db.ecu.clone(),
        dids: u32::try_from(db.db.dids.len()).unwrap_or(u32::MAX),
    }
}

/// B3-D3: non-fatal remarks shown to the user before/after attaching.
pub fn db_warnings(db: &LoadedDb) -> Vec<String> {
    let mut out: Vec<String> = db
        .db
        .dids
        .iter()
        .filter_map(|def| {
            loom_uds_codec::did::standard_did(def.id)
                .map(|std| format!("DID {:04X} overrides the ISO table entry {}", def.id, std.name))
        })
        .collect();
    if db.db.dids.iter().any(|d| d.writable) {
        out.push("writable flags are informational: write risk is decided from the bytes and never lowered by a DB".to_string());
    }
    out
}

/// B3-D4: decoded values of a positive `0x62` response with `lookup` (DB first,
/// then the ISO table when `lookup` is a `LoadedDb`). `None` when the response does not parse.
pub fn decode_dids_with(lookup: &dyn DidLookup, pdu: &[u8], resp: &[u8]) -> Option<Vec<DecodedDid>> {
    let recs = loom_uds_codec::response::parse_read_did_response(pdu, resp, lookup).ok()?;
    Some(
        recs.iter()
            .map(|r| {
                match lookup
                    .did_def(r.did)
                    .and_then(|d| d.decode(&r.data).ok().map(|v| (d.name, v)))
                {
                    Some((name, v)) => DecodedDid::new(r.did, Some(name), &v),
                    None => DecodedDid {
                        did: format!("{:04X}", r.did),
                        name: None,
                        value: serde_json::json!(hex::encode_upper(&r.data)),
                        unit: None,
                    },
                }
            })
            .collect(),
    )
}
