//! Policy files (contract 04 §4.8, F1): project `loom.toml` + user `config/policy.toml`, merged
//! to the **stricter** side. M1 keys:
//!
//! ```toml
//! [policy]
//! deny = ["uds.*", "can.periodic.*"]
//! [policy.actions]
//! "echo.send" = "auto"        # auto | deny | confirm | approve
//! [record]
//! max_age_days = 30
//! max_bytes = 21474836480
//! [env]
//! env = "bench"               # sim | bench | vehicle
//! ```
//!
//! `confirm`/`approve` are parsed but not implemented in M1: they are enforced as **deny** with a
//! warning, so a safety setting the user turned on is never silently weakened.
//! Changes are detected before each write (mtime/len, then content hash) — no watcher thread.
//! A file that fails to parse keeps the previous effective policy.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use loom_proto::event::Env;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

/// Per-method action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Run automatically (F1 default).
    Auto,
    /// Confirmation (M2) — enforced as deny in M1.
    Confirm,
    /// Viewer approval (M2) — enforced as deny in M1.
    Approve,
    /// Hard block.
    Deny,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePolicy {
    #[serde(default)]
    deny: Vec<String>,
    #[serde(default)]
    actions: std::collections::BTreeMap<String, Action>,
}

#[derive(Debug, Default, Deserialize)]
struct FileRecord {
    max_age_days: Option<u64>,
    max_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct FileEnv {
    env: Option<Env>,
}

/// Where device workers run (`[workers] mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkersMode {
    /// Separate processes over `loom-worker/1` (default).
    Ipc,
    /// Inside `loomd` (tests, benches, simulated domains).
    Inproc,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileWorkers {
    mode: Option<WorkersMode>,
}

#[derive(Debug, Default, Deserialize)]
struct File {
    #[serde(default)]
    policy: FilePolicy,
    #[serde(default)]
    record: FileRecord,
    #[serde(default)]
    env: FileEnv,
    #[serde(default)]
    workers: FileWorkers,
}

/// Effective policy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// Denied method patterns (union of all files).
    pub deny: Vec<String>,
    /// `(pattern, action)`, strictest action per pattern.
    pub actions: Vec<(String, Action)>,
    /// Retention age (days).
    pub max_age_days: Option<u64>,
    /// Retention quota (bytes).
    pub max_bytes: Option<u64>,
    /// Environment badge.
    pub env: Option<Env>,
    /// Worker placement (`[workers] mode`; read at start only).
    pub workers: Option<WorkersMode>,
    /// Domain sections (`[uart]`, `[psu]` …) as JSON, merged project-over-user per key
    /// (`CallCtx::config`).
    pub sections: std::collections::BTreeMap<String, serde_json::Value>,
    /// Hash of the sources (`sha256` hex of the file contents in order).
    pub hash: String,
    /// Warnings (confirm/approve treated as deny …).
    pub warnings: Vec<String>,
}

/// Why a write is blocked by policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    /// Pattern that matched.
    pub pattern: String,
    /// Configured action (`deny`, `confirm`, `approve`).
    pub action: Action,
}

/// `*` glob over method names.
pub fn glob(p: &str, s: &str) -> bool {
    match p.split_once('*') {
        None => p == s,
        Some((a, rest)) => {
            s.len() >= a.len() && s.starts_with(a) && {
                let tail = &s[a.len()..];
                if rest.is_empty() {
                    true
                } else {
                    (0..=tail.len())
                        .filter(|i| tail.is_char_boundary(*i))
                        .any(|i| glob(rest, &tail[i..]))
                }
            }
        }
    }
}

impl Policy {
    /// Parse one file's text.
    pub fn parse(text: &str) -> Result<Policy, String> {
        let f: File = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut p = Policy {
            deny: f.policy.deny,
            actions: f.policy.actions.into_iter().collect(),
            max_age_days: f.record.max_age_days,
            max_bytes: f.record.max_bytes,
            env: f.env.env,
            workers: f.workers.mode,
            sections: domain_sections(text)?,
            ..Policy::default()
        };
        for (pat, a) in &p.actions {
            if matches!(a, Action::Confirm | Action::Approve) {
                p.warnings.push(format!(
                    "{pat} = {a:?}: confirm/approve are M2 — enforced as deny"
                ));
            }
        }
        Ok(p)
    }

    /// Merge to the stricter side (deny union, strictest action, smallest limits). Env: the first
    /// file that sets it (project before user).
    pub fn merge(mut self, other: Policy) -> Policy {
        for d in other.deny {
            if !self.deny.contains(&d) {
                self.deny.push(d);
            }
        }
        for (pat, a) in other.actions {
            match self.actions.iter_mut().find(|(p, _)| *p == pat) {
                Some((_, cur)) => *cur = (*cur).max(a),
                None => self.actions.push((pat, a)),
            }
        }
        self.max_age_days = min_opt(self.max_age_days, other.max_age_days);
        self.max_bytes = min_opt(self.max_bytes, other.max_bytes);
        self.env = self.env.or(other.env);
        self.workers = self.workers.or(other.workers);
        for (k, v) in other.sections {
            match (self.sections.get_mut(&k), v) {
                (Some(serde_json::Value::Object(mine)), serde_json::Value::Object(theirs)) => {
                    for (kk, vv) in theirs {
                        mine.entry(kk).or_insert(vv);
                    }
                }
                (Some(_), _) => {}
                (None, v) => {
                    self.sections.insert(k, v);
                }
            }
        }
        self.warnings.extend(other.warnings);
        self
    }

    /// Blocking rule for `method`, if any (deny list first, then the strictest matching action).
    pub fn blocked(&self, method: &str) -> Option<Blocked> {
        if let Some(p) = self.deny.iter().find(|p| glob(p, method)) {
            return Some(Blocked {
                pattern: p.clone(),
                action: Action::Deny,
            });
        }
        self.actions
            .iter()
            .filter(|(p, _)| glob(p, method))
            .max_by_key(|(_, a)| *a)
            .filter(|(_, a)| *a != Action::Auto)
            .map(|(p, a)| Blocked {
                pattern: p.clone(),
                action: *a,
            })
    }

    /// Retention policy for the journal.
    pub fn retention(&self) -> loom_journal::RetainPolicy {
        let mut r = loom_journal::RetainPolicy::default();
        if let Some(d) = self.max_age_days {
            r.max_age = Duration::from_secs(d.saturating_mul(86_400));
        }
        if let Some(b) = self.max_bytes {
            r.max_bytes = b;
        }
        r
    }
}

/// Top-level tables other than the core ones.
fn domain_sections(text: &str) -> Result<std::collections::BTreeMap<String, serde_json::Value>, String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(t.into_iter()
        .filter(|(k, v)| v.is_table() && !matches!(k.as_str(), "policy" | "record" | "env" | "workers"))
        .filter_map(|(k, v)| serde_json::to_value(v).ok().map(|j| (k, j)))
        .collect())
}

fn min_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    }
}

/// Fingerprint of one source file (missing = `None`).
type Stamp = Option<(SystemTime, u64)>;

fn stamp(p: &Path) -> Stamp {
    fs::metadata(p)
        .ok()
        .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

/// Outcome of a reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reload {
    /// Nothing changed.
    Same,
    /// New effective policy.
    Changed,
    /// A file failed to parse; the previous policy stays.
    Failed(String),
}

/// Policy sources and the current effective policy.
#[derive(Debug)]
pub struct PolicySet {
    /// Files in merge order (project first, then user).
    pub files: Vec<PathBuf>,
    stamps: Vec<Stamp>,
    /// Effective policy.
    pub current: Policy,
}

impl PolicySet {
    /// Load; a parse error at start is an error (the daemon must not run with an unknown policy).
    pub fn load(files: Vec<PathBuf>) -> Result<PolicySet, String> {
        let mut s = PolicySet {
            stamps: vec![None; files.len()],
            files,
            current: Policy::default(),
        };
        match s.reload() {
            Reload::Failed(e) => Err(e),
            _ => Ok(s),
        }
    }

    /// Re-read if any file's mtime/len changed.
    pub fn check(&mut self) -> Reload {
        let now: Vec<Stamp> = self.files.iter().map(|f| stamp(f)).collect();
        if now == self.stamps {
            return Reload::Same;
        }
        self.reload()
    }

    fn reload(&mut self) -> Reload {
        let stamps: Vec<Stamp> = self.files.iter().map(|f| stamp(f)).collect();
        let mut merged: Option<Policy> = None;
        let mut h = Sha256::new();
        for f in &self.files {
            let text = match fs::read_to_string(f) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    self.stamps = stamps;
                    return Reload::Failed(format!("{}: {e}", f.display()));
                }
            };
            h.update(f.display().to_string().as_bytes());
            h.update([0]);
            h.update(text.as_bytes());
            match Policy::parse(&text) {
                Ok(p) => {
                    merged = Some(match merged {
                        Some(m) => m.merge(p),
                        None => p,
                    })
                }
                Err(e) => {
                    self.stamps = stamps;
                    return Reload::Failed(format!("{}: {e}", f.display()));
                }
            }
        }
        self.stamps = stamps;
        let mut p = merged.unwrap_or_default();
        p.hash = hex::encode(h.finalize());
        if p == self.current {
            return Reload::Same;
        }
        self.current = p;
        Reload::Changed
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_merge_and_block() {
        let a = Policy::parse(
            "[policy]\ndeny = [\"uds.*\"]\n[policy.actions]\n\"echo.send\" = \"auto\"\n\"can.*\" = \"approve\"\n[record]\nmax_age_days = 7\n[env]\nenv = \"vehicle\"\n",
        )
        .unwrap();
        let b = Policy::parse("[policy]\ndeny = [\"x.y\"]\n[policy.actions]\n\"echo.send\" = \"deny\"\n[record]\nmax_age_days = 3\nmax_bytes = 1000\n").unwrap();
        assert_eq!(a.warnings.len(), 1, "approve → deny warning");
        let m = a.merge(b);
        assert_eq!(m.deny, vec!["uds.*", "x.y"]);
        assert_eq!(m.blocked("uds.request").unwrap().action, Action::Deny);
        assert_eq!(
            m.blocked("echo.send").unwrap().action,
            Action::Deny,
            "stricter wins"
        );
        assert_eq!(m.blocked("can.send").unwrap().action, Action::Approve);
        assert!(m.blocked("echo.read").is_none());
        assert_eq!(
            (m.max_age_days, m.max_bytes, m.env),
            (Some(3), Some(1000), Some(Env::Vehicle))
        );
        assert!(Policy::parse("[policy]\ndeny = 3").is_err());
        assert!(Policy::parse("[policy]\nunknown = 1").is_err());
        assert!(glob("a.*.c", "a.b.c") && glob("*", "x.y") && !glob("a.*", "b.c"));
    }

    #[test]
    fn reload_keeps_previous_on_error() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("loom.toml");
        fs::write(&f, "[policy]\ndeny = [\"a.b\"]\n").unwrap();
        let mut s = PolicySet::load(vec![f.clone(), d.path().join("missing.toml")]).unwrap();
        assert_eq!(s.current.deny, vec!["a.b"]);
        assert_eq!(s.check(), Reload::Same);
        fs::write(&f, "[policy\nbroken").unwrap();
        assert!(matches!(s.check(), Reload::Failed(_)));
        assert_eq!(s.current.deny, vec!["a.b"], "previous kept");
        fs::write(&f, "[policy]\ndeny = [\"c.d\", \"e.f\"]\n").unwrap();
        assert_eq!(s.check(), Reload::Changed);
        assert_eq!(s.current.deny, vec!["c.d", "e.f"]);
    }
}
