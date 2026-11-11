//! JSON Schema export for the contract types (C3: Rust types -> JSON Schema -> TS/Python).
//!
//! The committed files under `schema/v1/` are the input for UI (TypeScript) and Python adapter
//! code generation. `check` fails when they drift from `loom-proto`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Default output directory relative to the workspace root.
pub const SCHEMA_DIR: &str = "schema/v1";

/// Renders every exported schema to pretty JSON text, keyed by file name (`<Name>.schema.json`).
///
/// # Errors
/// Returns an error if a schema cannot be serialized (should not happen for valid schemars output).
pub fn render_all() -> Result<BTreeMap<String, String>, serde_json::Error> {
    let mut out = BTreeMap::new();
    for (name, schema) in loom_proto::schema::export_all() {
        let mut text = serde_json::to_string_pretty(&schema)?;
        text.push('\n');
        out.insert(format!("{name}.schema.json"), text);
    }
    Ok(out)
}

/// Path of the schema directory for a workspace root.
#[must_use]
pub fn schema_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(SCHEMA_DIR)
}

/// Result of comparing rendered schemas with files on disk.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Drift {
    /// Files that should exist but are missing.
    pub missing: Vec<String>,
    /// Files whose content differs from the rendered schema.
    pub changed: Vec<String>,
    /// `*.schema.json` files on disk that no longer correspond to an exported type.
    pub stale: Vec<String>,
}

impl Drift {
    /// True when disk matches the rendered schemas exactly.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty() && self.changed.is_empty() && self.stale.is_empty()
    }
}

fn existing_schema_files(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    if !dir.exists() {
        return Ok(names);
    }
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".schema.json") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Compares rendered schemas with the files in `dir`.
///
/// # Errors
/// I/O errors while reading `dir`, or serialization errors.
pub fn check(dir: &Path) -> io::Result<Drift> {
    let rendered = render_all().map_err(io::Error::other)?;
    let mut drift = Drift::default();
    for (name, text) in &rendered {
        match fs::read_to_string(dir.join(name)) {
            Ok(disk) if disk == *text => {}
            Ok(_) => drift.changed.push(name.clone()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => drift.missing.push(name.clone()),
            Err(e) => return Err(e),
        }
    }
    for name in existing_schema_files(dir)? {
        if !rendered.contains_key(&name) {
            drift.stale.push(name);
        }
    }
    Ok(drift)
}

/// Writes all rendered schemas into `dir` and removes stale `*.schema.json` files.
///
/// # Errors
/// I/O errors while writing.
pub fn export(dir: &Path) -> io::Result<usize> {
    let rendered = render_all().map_err(io::Error::other)?;
    fs::create_dir_all(dir)?;
    for name in existing_schema_files(dir)? {
        if !rendered.contains_key(&name) {
            fs::remove_file(dir.join(&name))?;
        }
    }
    for (name, text) in &rendered {
        fs::write(dir.join(name), text)?;
    }
    Ok(rendered.len())
}
