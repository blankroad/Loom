//! `[uds]` section of `loom.toml` (schema only; the core loads and enforces policy).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One configured target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PolicyTarget {
    pub name: String,
    /// Virtual ECU behind a loopback DoIP entity.
    #[serde(default)]
    pub sim: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Target logical address (`0x1001`).
    pub la: Option<String>,
    pub tester_la: Option<String>,
    /// Routing activation type, numeric only (`0x00`, `0xE0`, OEM values).
    pub activation: Option<u8>,
}

/// Per-service policy action. F1: everything is `auto` unless the user sets otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyValue {
    Auto,
    Confirm,
    Approve,
    Deny,
}

/// `[uds]` in `loom.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PolicySection {
    /// Reconnect after link loss (generation + 1, unfinished writes `outcome_unknown`, no resend).
    pub auto_reconnect: Option<bool>,
    #[serde(default)]
    pub targets: Vec<PolicyTarget>,
    /// Keys `"0xSID:*"` or `"0xSID:0xSUB"` (e.g. `"0x31:*" = "deny"`, `"0x10:0x02" = "approve"`).
    #[serde(default)]
    pub policy: BTreeMap<String, PolicyValue>,
}
