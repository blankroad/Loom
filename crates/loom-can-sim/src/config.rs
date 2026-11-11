//! SimNode configuration (M1: raw rules). Built in code or read from TOML.
//!
//! ```toml
//! [[node]]
//! name = "BCM"
//! channel = "sim1"
//! seed = 7
//!
//! [[node.on]]               # reply rule
//! id = 0x300
//! data = "01"               # optional match bytes (hex) …
//! mask = "FF"               # … and mask (default: all bits of `data`)
//! reply_id = 0x301
//! reply = "copy"            # or fixed hex bytes, e.g. "0102"
//! delay_ms = 5.0
//!
//! [[node.periodic]]
//! id = 0x310
//! period_ms = 100.0
//! data = "0100000000000000"
//! follow = [{ source_id = 0x300, source_byte = 0, byte = 0 }]
//!
//! [node.faults]             # sim-only fault injection, all off by default
//! drop_ids = [0x310]
//! extra_delay_ms = 0.0
//! jitter_ms = 0.0
//! crash_after_ms = 1000.0
//! ```
//! Ids above 0x7FF are extended; `ext = true` on a rule/periodic makes small ids extended.
//! `fd = true` sends CAN FD frames.

use loom_can_frame::{CanFrame, CanId, fd_padded_len};
use serde::Deserialize;

use crate::error::ConfigError;

/// Limits for one node config (C2).
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
pub const MAX_NODES_PER_FILE: usize = 16;
pub const MAX_RULES: usize = 64;
pub const MAX_PERIODIC: usize = 64;
pub const MAX_FOLLOWS: usize = 8;
pub const MAX_DROP_IDS: usize = 64;
pub const MAX_NAME_BYTES: usize = 64;
/// Shortest allowed period (bounds the frame rate a node can generate).
pub const MIN_PERIOD_NS: u64 = 100_000;
/// Longest reply delay / crash time accepted.
pub const MAX_DELAY_NS: u64 = 3_600 * 1_000_000_000;

/// What a rule replies with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyData {
    /// The trigger frame's payload.
    Copy,
    Fixed(Vec<u8>),
}

/// "On frame `id` (matching `data` under `mask`), send `reply_id` after `delay_ns`."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: CanId,
    pub data: Vec<u8>,
    pub mask: Vec<u8>,
    pub reply_id: CanId,
    pub reply: ReplyData,
    pub delay_ns: u64,
    pub fd: bool,
}

/// Copy byte `source_byte` of the last frame `source_id` into byte `byte`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Follow {
    pub source_id: CanId,
    pub source_byte: u8,
    pub byte: u8,
}

/// A frame sent every `period_ns`, first at node start + `offset_ns`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Periodic {
    pub id: CanId,
    pub period_ns: u64,
    pub offset_ns: u64,
    pub data: Vec<u8>,
    pub fd: bool,
    pub follow: Vec<Follow>,
}

/// Sim-only faults. Default: none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeFaults {
    /// The node does not send these ids (frames lost).
    pub drop_ids: Vec<CanId>,
    /// Added to every reply delay.
    pub extra_delay_ns: u64,
    /// Random extra reply delay in `0..=jitter_ns` (from the node seed).
    pub jitter_ns: u64,
    /// The node crashes (stops, pending frames lost) this long after start.
    pub crash_after_ns: Option<u64>,
}

/// One fake ECU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeConfig {
    pub name: String,
    /// Bus name; nodes and endpoints on the same name share a bus.
    pub channel: String,
    /// Seed for fault jitter. Fixed by default so runs repeat.
    pub seed: u64,
    pub rules: Vec<Rule>,
    pub periodic: Vec<Periodic>,
    pub faults: NodeFaults,
}

impl NodeConfig {
    pub fn new(name: &str, channel: &str) -> Self {
        Self {
            name: name.to_string(),
            channel: channel.to_string(),
            seed: 1,
            rules: Vec::new(),
            periodic: Vec::new(),
            faults: NodeFaults::default(),
        }
    }

    /// Checks limits and that every frame can be built.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let p = |s: &str| format!("node {}{s}", self.name);
        if self.name.is_empty() || self.name.len() > MAX_NAME_BYTES {
            return Err(ConfigError::new(p(".name"), "must be 1..=64 bytes"));
        }
        if self.channel.is_empty() || self.channel.len() > MAX_NAME_BYTES {
            return Err(ConfigError::new(p(".channel"), "must be 1..=64 bytes"));
        }
        if self.rules.len() > MAX_RULES {
            return Err(ConfigError::new(p(".on"), format!("more than {MAX_RULES} rules")));
        }
        if self.periodic.len() > MAX_PERIODIC {
            return Err(ConfigError::new(
                p(".periodic"),
                format!("more than {MAX_PERIODIC} periodic frames"),
            ));
        }
        if self.faults.drop_ids.len() > MAX_DROP_IDS {
            return Err(ConfigError::new(
                p(".faults.drop_ids"),
                format!("more than {MAX_DROP_IDS} ids"),
            ));
        }
        for (i, r) in self.rules.iter().enumerate() {
            let at = |f: &str| p(&format!(".on[{i}].{f}"));
            if r.mask.len() != r.data.len() {
                return Err(ConfigError::new(at("mask"), "must have the same length as data"));
            }
            if r.delay_ns > MAX_DELAY_NS {
                return Err(ConfigError::new(at("delay_ms"), "longer than 1 h"));
            }
            if let ReplyData::Fixed(bytes) = &r.reply {
                check_payload(bytes, r.fd).map_err(|m| ConfigError::new(at("reply"), m))?;
            }
        }
        for (i, per) in self.periodic.iter().enumerate() {
            let at = |f: &str| p(&format!(".periodic[{i}].{f}"));
            if per.period_ns < MIN_PERIOD_NS || per.period_ns > MAX_DELAY_NS {
                return Err(ConfigError::new(at("period_ms"), "must be 0.1 ms ..= 1 h"));
            }
            if per.offset_ns > MAX_DELAY_NS {
                return Err(ConfigError::new(at("offset_ms"), "longer than 1 h"));
            }
            check_payload(&per.data, per.fd).map_err(|m| ConfigError::new(at("data"), m))?;
            if per.follow.len() > MAX_FOLLOWS {
                return Err(ConfigError::new(at("follow"), format!("more than {MAX_FOLLOWS}")));
            }
            if per.follow.iter().any(|f| f.byte as usize >= per.data.len()) {
                return Err(ConfigError::new(at("follow"), "byte outside data"));
            }
        }
        if self.faults.extra_delay_ns > MAX_DELAY_NS || self.faults.jitter_ns > MAX_DELAY_NS {
            return Err(ConfigError::new(p(".faults"), "delay longer than 1 h"));
        }
        if self.faults.crash_after_ns.is_some_and(|c| c > MAX_DELAY_NS) {
            return Err(ConfigError::new(p(".faults.crash_after_ms"), "longer than 1 h"));
        }
        Ok(())
    }
}

fn check_payload(bytes: &[u8], fd: bool) -> Result<(), String> {
    let id = CanId::standard(0).expect("0 is valid");
    let ok = if fd {
        fd_padded_len(bytes.len()) == Some(bytes.len()) && CanFrame::fd(id, bytes, false, false).is_ok()
    } else {
        CanFrame::classic(id, bytes).is_ok()
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{} bytes is not a valid {} length",
            bytes.len(),
            if fd { "CAN FD" } else { "Classic CAN" }
        ))
    }
}

// ---- TOML ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRaw {
    #[serde(default)]
    node: Vec<NodeRaw>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeRaw {
    name: String,
    channel: String,
    #[serde(default = "one")]
    seed: u64,
    #[serde(default)]
    on: Vec<RuleRaw>,
    #[serde(default)]
    periodic: Vec<PeriodicRaw>,
    #[serde(default)]
    faults: FaultsRaw,
}

fn one() -> u64 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleRaw {
    id: u32,
    #[serde(default)]
    ext: bool,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    mask: Option<String>,
    reply_id: u32,
    reply: String,
    #[serde(default)]
    delay_ms: f64,
    #[serde(default)]
    fd: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeriodicRaw {
    id: u32,
    #[serde(default)]
    ext: bool,
    period_ms: f64,
    #[serde(default)]
    offset_ms: f64,
    data: String,
    #[serde(default)]
    fd: bool,
    #[serde(default)]
    follow: Vec<FollowRaw>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FollowRaw {
    source_id: u32,
    #[serde(default)]
    source_ext: bool,
    source_byte: u8,
    byte: u8,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FaultsRaw {
    #[serde(default)]
    drop_ids: Vec<u32>,
    #[serde(default)]
    extra_delay_ms: f64,
    #[serde(default)]
    jitter_ms: f64,
    #[serde(default)]
    crash_after_ms: Option<f64>,
}

/// Parses `[[node]]` tables from TOML text. Every node is validated.
pub fn parse_nodes(text: &str) -> Result<Vec<NodeConfig>, ConfigError> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::new(
            "",
            format!("file larger than {MAX_CONFIG_BYTES} bytes"),
        ));
    }
    let raw: FileRaw = toml::from_str(text).map_err(|e| ConfigError::new("", e.to_string()))?;
    if raw.node.len() > MAX_NODES_PER_FILE {
        return Err(ConfigError::new(
            "node",
            format!("more than {MAX_NODES_PER_FILE} nodes"),
        ));
    }
    raw.node
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            let cfg = convert(i, n)?;
            cfg.validate()?;
            Ok(cfg)
        })
        .collect()
}

fn can_id(raw: u32, ext: bool, path: &str) -> Result<CanId, ConfigError> {
    let r = if ext {
        CanId::extended(raw)
    } else {
        CanId::auto(raw)
    };
    r.map_err(|e| ConfigError::new(path, e.to_string()))
}

fn hex(s: &str, path: &str) -> Result<Vec<u8>, ConfigError> {
    let clean: String = s.chars().filter(|c| !matches!(c, ' ' | '.' | '_')).collect();
    if !clean.len().is_multiple_of(2) || clean.len() > 128 {
        return Err(ConfigError::new(
            path,
            "hex must be an even number of digits, ≤ 64 bytes",
        ));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|_| ConfigError::new(path, format!("bad hex {s:?}")))
        })
        .collect()
}

fn ms_to_ns(ms: f64, path: &str) -> Result<u64, ConfigError> {
    if !ms.is_finite() || ms < 0.0 || ms > (MAX_DELAY_NS / 1_000_000) as f64 {
        return Err(ConfigError::new(path, "must be 0 ..= 3,600,000 ms"));
    }
    Ok((ms * 1_000_000.0).round() as u64)
}

fn convert(i: usize, n: NodeRaw) -> Result<NodeConfig, ConfigError> {
    let mut rules = Vec::with_capacity(n.on.len().min(MAX_RULES + 1));
    for (j, r) in n.on.into_iter().enumerate().take(MAX_RULES + 1) {
        let at = |f: &str| format!("node[{i}].on[{j}].{f}");
        let data = r
            .data
            .as_deref()
            .map(|s| hex(s, &at("data")))
            .transpose()?
            .unwrap_or_default();
        let mask = match r.mask {
            Some(m) => hex(&m, &at("mask"))?,
            None => vec![0xFF; data.len()],
        };
        let reply = if r.reply.eq_ignore_ascii_case("copy") {
            ReplyData::Copy
        } else {
            ReplyData::Fixed(hex(&r.reply, &at("reply"))?)
        };
        rules.push(Rule {
            id: can_id(r.id, r.ext, &at("id"))?,
            data,
            mask,
            reply_id: can_id(r.reply_id, r.ext, &at("reply_id"))?,
            reply,
            delay_ns: ms_to_ns(r.delay_ms, &at("delay_ms"))?,
            fd: r.fd,
        });
    }
    let mut periodic = Vec::with_capacity(n.periodic.len().min(MAX_PERIODIC + 1));
    for (j, p) in n.periodic.into_iter().enumerate().take(MAX_PERIODIC + 1) {
        let at = |f: &str| format!("node[{i}].periodic[{j}].{f}");
        let mut follow = Vec::new();
        for f in p.follow.into_iter().take(MAX_FOLLOWS + 1) {
            follow.push(Follow {
                source_id: can_id(f.source_id, f.source_ext, &at("follow.source_id"))?,
                source_byte: f.source_byte,
                byte: f.byte,
            });
        }
        periodic.push(Periodic {
            id: can_id(p.id, p.ext, &at("id"))?,
            period_ns: ms_to_ns(p.period_ms, &at("period_ms"))?,
            offset_ns: ms_to_ns(p.offset_ms, &at("offset_ms"))?,
            data: hex(&p.data, &at("data"))?,
            fd: p.fd,
            follow,
        });
    }
    let at = |f: &str| format!("node[{i}].faults.{f}");
    let mut drop_ids = Vec::new();
    for raw in n.faults.drop_ids.into_iter().take(MAX_DROP_IDS + 1) {
        drop_ids.push(can_id(raw, false, &at("drop_ids"))?);
    }
    Ok(NodeConfig {
        name: n.name,
        channel: n.channel,
        seed: n.seed,
        rules,
        periodic,
        faults: NodeFaults {
            drop_ids,
            extra_delay_ns: ms_to_ns(n.faults.extra_delay_ms, &at("extra_delay_ms"))?,
            jitter_ns: ms_to_ns(n.faults.jitter_ms, &at("jitter_ms"))?,
            crash_after_ns: n
                .faults
                .crash_after_ms
                .map(|m| ms_to_ns(m, &at("crash_after_ms")))
                .transpose()?,
        },
    })
}
