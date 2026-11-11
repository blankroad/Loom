//! Diagnostic DB file: schema, loader, identity hash, DID lookup.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CodecError;
use crate::did::{DidDef, MAX_NAME_LEN, standard_did};

/// Largest DB file accepted, checked before parsing.
pub const MAX_DB_FILE_BYTES: usize = 1024 * 1024;
/// Most DIDs in one DB file.
pub const MAX_DB_DIDS: usize = 4096;
/// Current DB schema version (`loom_diag` field).
pub const DB_SCHEMA_VERSION: u32 = 1;

/// Diagnostic DB for one ECU.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagDb {
    /// Schema version, must be [`DB_SCHEMA_VERSION`].
    pub loom_diag: u32,
    #[serde(default)]
    pub ecu: Option<String>,
    #[serde(default)]
    pub dids: Vec<DidDef>,
}

impl DiagDb {
    /// JSON Schema of the DB file (for editors, docs and `loom uds help --json`).
    pub fn json_schema() -> schemars::Schema {
        schemars::schema_for!(DiagDb)
    }
}

/// Input file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    /// Reserved: needs the `serde-saphyr` workspace dependency (requested from CORE).
    Yaml,
}

/// A validated DB plus its identity hash and an index by DID.
#[derive(Debug, Clone)]
pub struct LoadedDb {
    pub db: DiagDb,
    /// FNV-1a 64 over the normalized JSON form (field order fixed, whitespace removed).
    pub hash: u64,
    index: BTreeMap<u16, usize>,
}

impl LoadedDb {
    /// Hash as 16 hex digits, for records and approval digests.
    pub fn hash_hex(&self) -> String {
        format!("{:016x}", self.hash)
    }

    pub fn get(&self, did: u16) -> Option<&DidDef> {
        self.index.get(&did).map(|&i| &self.db.dids[i])
    }
}

/// FNV-1a 64-bit hash (deterministic across platforms and runs).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
}

/// Parse, check limits and validate a DB file.
pub fn load_db(bytes: &[u8], format: Format) -> Result<LoadedDb, CodecError> {
    if bytes.len() > MAX_DB_FILE_BYTES {
        return Err(CodecError::Db(format!(
            "file is {} bytes, limit {MAX_DB_FILE_BYTES}",
            bytes.len()
        )));
    }
    let db: DiagDb = match format {
        Format::Json => serde_json::from_slice(bytes).map_err(|e| CodecError::Db(e.to_string()))?,
        Format::Yaml => return Err(CodecError::Db("YAML is not yet supported (use JSON)".into())),
    };
    if db.loom_diag != DB_SCHEMA_VERSION {
        return Err(CodecError::Db(format!(
            "loom_diag {} unsupported (expected {DB_SCHEMA_VERSION})",
            db.loom_diag
        )));
    }
    if db.ecu.as_ref().is_some_and(|e| e.len() > MAX_NAME_LEN) {
        return Err(CodecError::Db("ecu name too long".into()));
    }
    if db.dids.len() > MAX_DB_DIDS {
        return Err(CodecError::Db(format!(
            "{} DIDs, limit {MAX_DB_DIDS}",
            db.dids.len()
        )));
    }
    let mut index = BTreeMap::new();
    for (i, def) in db.dids.iter().enumerate() {
        def.validate()?;
        if index.insert(def.id, i).is_some() {
            return Err(CodecError::Db(format!("DID 0x{:04X} defined twice", def.id)));
        }
    }
    let normalized = serde_json::to_vec(&db).map_err(|e| CodecError::Db(e.to_string()))?;
    Ok(LoadedDb {
        hash: fnv1a64(&normalized),
        db,
        index,
    })
}

/// Where DID definitions come from (user DB first, then the standard table).
pub trait DidLookup {
    fn did_def(&self, did: u16) -> Option<DidDef>;

    fn did_len(&self, did: u16) -> Option<usize> {
        self.did_def(did).and_then(|d| d.length).map(usize::from)
    }
}

/// Only the built-in ISO F180–F19E table.
#[derive(Debug, Clone, Copy, Default)]
pub struct StandardDids;

impl DidLookup for StandardDids {
    fn did_def(&self, did: u16) -> Option<DidDef> {
        standard_did(did)
    }
}

impl DidLookup for LoadedDb {
    fn did_def(&self, did: u16) -> Option<DidDef> {
        self.get(did).cloned().or_else(|| standard_did(did))
    }
}
