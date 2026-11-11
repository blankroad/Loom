//! UART write hooks (contract 08 §8.2, plan §10.2). Synchronous, ≤ 1 ms, no I/O.

use loom_domain_api::{Binding, Classification, Grant, PresendError, WriteHooks, WriteRequest};
use loom_proto::event::{Origin, Risk};
use loom_proto::policy::WriteClass;
use serde_json::{Value, json};

use crate::classify::{Verdict, classify_text_with};
use crate::mirror::Mirror;

/// Longest payload the hooks decode for classification (one `uart.send` RPC ≤ 64 KiB, hex).
const MAX_CLASSIFY_BYTES: usize = 32 * 1024;

/// Hooks with read access to the channel mirror.
#[derive(Debug, Clone, Default)]
pub struct UartHooks {
    mirror: Mirror,
}

impl UartHooks {
    /// Hooks sharing `mirror` with the reducer.
    pub fn new(mirror: Mirror) -> Self {
        UartHooks { mirror }
    }
}

/// Outgoing bytes of a text write: `req.bytes` when the core decoded them, else `params.data` (hex).
fn payload_bytes(req: &WriteRequest<'_>) -> Option<Vec<u8>> {
    if let Some(b) = req.bytes {
        return Some(b[..b.len().min(MAX_CLASSIFY_BYTES)].to_vec());
    }
    let hex_str = req.args.get("data").and_then(Value::as_str)?;
    let cut = hex_str.len().min(MAX_CLASSIFY_BYTES * 2) & !1;
    hex::decode(&hex_str[..cut]).ok()
}

/// Class of one request (exported for `--dry-run` and tests).
pub fn classify_request(req: &WriteRequest<'_>) -> Verdict {
    classify_request_with(req, "generic")
}

/// Rule set for a channel profile name (`uart.config.profile`, plan §4.6 profile names).
pub fn rule_profile(channel_profile: &str) -> &'static str {
    match channel_profile {
        "linux" | "linux-shell" => "linux",
        "uboot" => "uboot",
        "esp32" | "esp-idf" => "esp-idf",
        "zephyr" | "zephyr-shell" => "zephyr",
        _ => "generic",
    }
}

/// Class of one request using the rule set of `profile` (a [`rule_profile`] name).
pub fn classify_request_with(req: &WriteRequest<'_>, profile: &str) -> Verdict {
    let fixed = |risk: Risk, reason: &str| Verdict {
        risk,
        rule: None,
        reason: reason.into(),
    };
    // User keystrokes (write class ③) are never string-classified (plan §3.2, A1).
    if req.write_class == WriteClass::Interactive || req.origin == Origin::User && req.method == "uart.input"
    {
        return fixed(Risk::R1, "interactive input");
    }
    match req.method {
        "uart.send" | "uart.run" => match payload_bytes(req) {
            Some(b) => classify_text_with(profile, &[], &b),
            None => fixed(Risk::R1, "uart text (undecoded)"),
        },
        "uart.signal" => fixed(Risk::R2, "line signal (break/DTR/RTS)"),
        "uart.configure" => fixed(Risk::R2, "line settings change"),
        "uart.release" | "uart.reclaim" => fixed(Risk::R2, "port release/reclaim"),
        "uart.open" => fixed(Risk::R1, "open port (may toggle DTR/RTS)"),
        "uart.close" => fixed(Risk::R1, "close port (may toggle DTR/RTS)"),
        _ => fixed(Risk::R1, "uart write"),
    }
}

impl UartHooks {
    /// Rule set of the request's channel: mirrored `uart.config.profile`, else `generic`.
    /// (The worker's unfinished-line state is not mirrored yet: `line_state` is empty — P1 gap.)
    fn profile_of(&self, req: &WriteRequest<'_>) -> &'static str {
        let ch = req.target.channel.as_deref().unwrap_or_default();
        self.mirror
            .get(ch)
            .and_then(|v| {
                v.config
                    .get("profile")
                    .and_then(serde_json::Value::as_str)
                    .map(rule_profile)
            })
            .unwrap_or("generic")
    }
}

impl WriteHooks for UartHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        let v = classify_request_with(req, self.profile_of(req));
        Classification {
            risk: v.risk,
            reason: v.reason,
            block: None,
        }
    }

    fn approval_binding(&self, req: &WriteRequest<'_>) -> Binding {
        let v = classify_request_with(req, self.profile_of(req));
        let channel = req.target.channel.clone().unwrap_or_default();
        let mut b = Binding::default();
        b.digest_fields.insert("channel".into(), json!(channel));
        b.digest_fields
            .insert("generation".into(), json!(self.mirror.generation(&channel)));
        b.digest_fields.insert("rule".into(), json!(v.rule));
        if let Some(data) = req.args.get("data") {
            b.digest_fields.insert("data".into(), data.clone());
        }
        b.display = json!({"channel": channel, "rule": v.rule, "reason": v.reason, "method": req.method});
        b
    }

    fn presend_check(&self, req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        let Some(channel) = req.target.channel.as_deref() else {
            return Ok(());
        };
        match (req.target.generation, self.mirror.generation(channel)) {
            (Some(want), Some(now)) if want != now => Err(PresendError::EpochChanged(format!(
                "uart channel {channel} generation {want} -> {now}"
            ))),
            _ => Ok(()),
        }
    }
}
