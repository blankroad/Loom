//! Seed→key provider for `uds.security_unlock` (U2): configuration lookup/validation, output
//! parsing and the bounded child process. Provider input (seed) and output (stdout/stderr) are
//! untrusted: never interpreted as anything but hex, never echoed beyond [`MAX_ERROR_SNIPPET`].

use loom_proto::error::ErrorCode;
use serde_json::Value;

use crate::policy::KeyProviderConfig;

pub const DEFAULT_PROVIDER_TIMEOUT_MS: u32 = 2_000;
pub const MAX_PROVIDER_TIMEOUT_MS: u32 = 10_000;
/// stdout above this is refused (the child is killed); stderr is read up to this and discarded.
pub const MAX_PROVIDER_OUTPUT_BYTES: usize = 4096;
pub const MAX_KEY_HEX_CHARS: usize = 1024;
pub const MAX_PROVIDER_ARGS: usize = 16;
pub const MAX_PROVIDER_ARG_LEN: usize = 256;
pub const MAX_PROVIDER_COMMAND_LEN: usize = 4096;
/// Longest piece of provider stderr quoted in an error.
pub const MAX_ERROR_SNIPPET: usize = 200;

/// Why no key was produced. Nothing is ever retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// No `[[uds.key_providers]]` entry with this name.
    NotConfigured(String),
    /// The entry (or the section) is invalid.
    BadConfig(String),
    LevelNotAllowed {
        level: u8,
        allowed: Vec<u8>,
    },
    Spawn(String),
    Timeout {
        ms: u32,
    },
    Exit {
        code: Option<i32>,
        stderr: String,
    },
    OutputTooLarge,
    BadKey(String),
}

impl ProviderError {
    /// B3-K1: contract error code.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Timeout { .. } => ErrorCode::Timeout,
            Self::OutputTooLarge => ErrorCode::Limit,
            _ => ErrorCode::Config,
        }
    }

    /// B3-K1: one-line message (no raw provider bytes beyond the snippet).
    pub fn message(&self) -> String {
        match self {
            Self::NotConfigured(n) => {
                format!("key provider \"{n}\" is not configured in loom.toml [[uds.key_providers]]")
            }
            Self::BadConfig(m) => format!("key provider config: {m}"),
            Self::LevelNotAllowed { level, allowed } => {
                let list = allowed
                    .iter()
                    .map(|l| format!("{l:02X}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("key provider does not allow level 0x{level:02X} (allowed: {list})")
            }
            Self::Spawn(m) => format!("key provider could not start: {m}"),
            Self::Timeout { ms } => format!("key provider timed out after {ms} ms (killed; no key sent)"),
            Self::Exit { code, stderr } => {
                let how = match code {
                    Some(c) => format!("code {c}"),
                    None => "a signal".to_string(),
                };
                if stderr.is_empty() {
                    format!("key provider exited with {how}")
                } else {
                    format!("key provider exited with {how}: {stderr}")
                }
            }
            Self::OutputTooLarge => {
                format!(
                    "key provider output exceeded {MAX_PROVIDER_OUTPUT_BYTES} bytes (killed; no key sent)"
                )
            }
            Self::BadKey(m) => format!("key provider output is not a key: {m}"),
        }
    }
}

/// B3-K2: validate one entry.
pub fn validate_provider(cfg: &KeyProviderConfig) -> Result<(), ProviderError> {
    let bad = |m: &str| Err(ProviderError::BadConfig(m.to_string()));
    let n = &cfg.name;
    if n.is_empty()
        || n.len() > 64
        || !n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return bad("name must be 1-64 characters of A-Z a-z 0-9 _ . -");
    }
    let c = cfg.command.as_str();
    if !std::path::Path::new(c).is_absolute() || c.len() > MAX_PROVIDER_COMMAND_LEN || c.contains('\0') {
        return bad("command must be an absolute path (no PATH lookup, no NUL, at most 4096 bytes)");
    }
    if std::path::Path::new(c)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
    {
        return bad("command must be an executable, not a batch script (.bat/.cmd run through cmd.exe)");
    }
    if cfg.args.len() > MAX_PROVIDER_ARGS
        || cfg
            .args
            .iter()
            .any(|a| a.len() > MAX_PROVIDER_ARG_LEN || a.contains('\0'))
    {
        return bad("args: at most 16 arguments of at most 256 bytes, no NUL");
    }
    if let Some(ms) = cfg.timeout_ms
        && (ms == 0 || ms > MAX_PROVIDER_TIMEOUT_MS)
    {
        return bad("timeout_ms must be 1-10000");
    }
    if cfg.levels.is_empty() {
        return bad("levels must not be empty");
    }
    if cfg.levels.iter().any(|l| l % 2 == 0 || *l > 0x7D) {
        return bad("levels must be odd requestSeed sub-functions 0x01–0x7D");
    }
    Ok(())
}

/// B3-K3: find `name` in the `[uds]` section (`ctx.config()`), validate it and check `level`.
pub fn provider_from_config(
    config: &Value,
    name: &str,
    level: u8,
) -> Result<KeyProviderConfig, ProviderError> {
    let list = match config.get("key_providers") {
        None | Some(Value::Null) => return Err(ProviderError::NotConfigured(name.to_string())),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(ProviderError::BadConfig("key_providers must be an array".into())),
    };
    let mut all = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        let c = serde_json::from_value::<KeyProviderConfig>(v.clone())
            .map_err(|e| ProviderError::BadConfig(format!("key_providers[{i}]: {e}")))?;
        all.push(c);
    }
    let cfg = all
        .into_iter()
        .find(|c| c.name == name)
        .ok_or_else(|| ProviderError::NotConfigured(name.to_string()))?;
    validate_provider(&cfg)?;
    if !cfg.levels.contains(&level) {
        return Err(ProviderError::LevelNotAllowed {
            level,
            allowed: cfg.levels.clone(),
        });
    }
    Ok(cfg)
}

/// B3-K4: stdout → key bytes.
pub fn parse_key_output(stdout: &[u8]) -> Result<Vec<u8>, ProviderError> {
    if stdout.len() > MAX_PROVIDER_OUTPUT_BYTES {
        return Err(ProviderError::OutputTooLarge);
    }
    let s = std::str::from_utf8(stdout).map_err(|_| ProviderError::BadKey("not UTF-8".into()))?;
    let t = s.trim();
    if t.is_empty() {
        return Err(ProviderError::BadKey("empty output".into()));
    }
    if t.chars().any(char::is_whitespace) {
        return Err(ProviderError::BadKey("expected one hex line".into()));
    }
    if t.len() % 2 != 0 || t.len() > MAX_KEY_HEX_CHARS {
        return Err(ProviderError::BadKey("key must be 2–1024 hex digits".into()));
    }
    hex::decode(t).map_err(|_| ProviderError::BadKey("key must be 2–1024 hex digits".into()))
}

/// B3-K5: printable, bounded rendering of untrusted provider bytes.
pub fn sanitize_snippet(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end()
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(MAX_ERROR_SNIPPET)
        .collect()
}

/// B3-K6: run the provider once for `seed` (bounded, killed on timeout or drop).
#[cfg(feature = "runtime")]
pub async fn run_provider(cfg: &KeyProviderConfig, level: u8, seed: &[u8]) -> Result<Vec<u8>, ProviderError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    validate_provider(cfg)?;
    if !cfg.levels.contains(&level) {
        return Err(ProviderError::LevelNotAllowed {
            level,
            allowed: cfg.levels.clone(),
        });
    }
    let ms = cfg.timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS);
    let mut cmd = tokio::process::Command::new(&cfg.command);
    cmd.args(&cfg.args)
        .env_clear()
        .env("LOOM_UDS_LEVEL", format!("{level:02X}"));
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ProviderError::Spawn(sanitize_snippet(e.to_string().as_bytes())))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let line = format!("{}\n", hex::encode_upper(seed));

    let work = async {
        let write = async move {
            if let Some(mut si) = stdin {
                let _ = si.write_all(line.as_bytes()).await;
                let _ = si.shutdown().await;
            }
        };
        let read_out = async move {
            let mut buf = Vec::new();
            if let Some(so) = stdout {
                let _ = so
                    .take(MAX_PROVIDER_OUTPUT_BYTES as u64 + 1)
                    .read_to_end(&mut buf)
                    .await;
            }
            buf
        };
        let read_err = async move {
            let mut buf = Vec::new();
            if let Some(se) = stderr {
                let _ = se
                    .take(MAX_PROVIDER_OUTPUT_BYTES as u64)
                    .read_to_end(&mut buf)
                    .await;
            }
            buf
        };
        let ((), out, err) = tokio::join!(write, read_out, read_err);
        if out.len() > MAX_PROVIDER_OUTPUT_BYTES {
            let _ = child.start_kill();
            return Err(ProviderError::OutputTooLarge);
        }
        let status = child
            .wait()
            .await
            .map_err(|e| ProviderError::Spawn(sanitize_snippet(e.to_string().as_bytes())))?;
        Ok((status, out, err))
    };
    let res = tokio::time::timeout(std::time::Duration::from_millis(u64::from(ms)), work).await;
    match res {
        Err(_) => {
            let _ = child.start_kill();
            Err(ProviderError::Timeout { ms })
        }
        Ok(Err(e)) => Err(e),
        Ok(Ok((status, out, err))) => {
            if !status.success() {
                return Err(ProviderError::Exit {
                    code: status.code(),
                    stderr: sanitize_snippet(&err),
                });
            }
            parse_key_output(&out)
        }
    }
}
