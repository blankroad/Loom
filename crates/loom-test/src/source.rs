use crate::*;
use std::{fs::File, io::Read};
/// Explicit {text:…}/{path:…}; single-line *.tc.md/*.plan.yaml strings are also paths.
pub(crate) fn load(v: &Value, suffix: &str) -> Result<String, loom_domain_api::DomainError> {
    let (value, path) = if let Some(s) = v.as_str() {
        (s, !s.contains('\n') && s.ends_with(suffix))
    } else {
        let m = v
            .as_object()
            .ok_or_else(|| usage("source must be text, path or {text|path}"))?;
        if m.len() != 1 {
            return Err(usage("source object needs exactly one text or path"));
        }
        if let Some(s) = m.get("text").and_then(Value::as_str) {
            (s, false)
        } else if let Some(s) = m.get("path").and_then(Value::as_str) {
            (s, true)
        } else {
            return Err(usage("invalid source object"));
        }
    };
    if !path {
        if value.len() > loom_test_format::MAX_FILE_BYTES {
            return Err(usage("source exceeds 1 MiB"));
        }
        return Ok(value.into());
    }
    if value.len() > 4096 {
        return Err(usage("source path exceeds 4096 bytes"));
    }
    let metadata =
        std::fs::metadata(value).map_err(|e| err(loom_proto::error::ErrorCode::NotFound, e.to_string()))?;
    if !metadata.is_file() {
        return Err(usage("source must be a regular file"));
    }
    let file = File::open(value).map_err(|e| err(loom_proto::error::ErrorCode::NotFound, e.to_string()))?;
    if !file.metadata().map_err(|e| usage(e.to_string()))?.is_file() {
        return Err(usage("source must remain a regular file"));
    }
    let mut bytes = Vec::new();
    file.take((loom_test_format::MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| usage(e.to_string()))?;
    if bytes.len() > loom_test_format::MAX_FILE_BYTES {
        return Err(usage("source exceeds 1 MiB"));
    }
    String::from_utf8(bytes).map_err(|_| usage("source must be UTF-8"))
}
pub(crate) fn format_error(e: loom_test_format::FormatError) -> loom_domain_api::DomainError {
    usage(e.to_string()).with_detail(serde_json::to_value(e).unwrap_or(Value::Null))
}
