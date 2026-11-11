//! Policy / approval types (contract v1 §10, master D6·E4·E9·**F1**). Types only — no decision
//! logic lives here.
//!
//! **F1 (user decision, highest priority): every risk class R0–R4 runs automatically by
//! default.** Confirm tokens, viewer approval and presence checks are off unless the user turns
//! them on in `loom.toml`. Risk classification, live display and audit stay on.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::Risk;
use crate::num::U64;

/// What the policy does with a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    /// Run immediately (F1 default for every class).
    #[default]
    Auto,
    /// R2-style confirmation token (opt-in).
    Confirm,
    /// Viewer approval (opt-in).
    Approve,
    /// Refuse.
    Deny,
}

/// Default action per risk class. [`Default`] = all [`PolicyAction::Auto`] (F1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct RiskDefaults {
    /// R0.
    #[serde(default)]
    pub r0: PolicyAction,
    /// R1.
    #[serde(default)]
    pub r1: PolicyAction,
    /// R2.
    #[serde(default)]
    pub r2: PolicyAction,
    /// R3.
    #[serde(default)]
    pub r3: PolicyAction,
    /// R4.
    #[serde(default)]
    pub r4: PolicyAction,
}

impl RiskDefaults {
    /// Action for a class.
    pub const fn for_risk(&self, risk: Risk) -> PolicyAction {
        match risk {
            Risk::R0 => self.r0,
            Risk::R1 => self.r1,
            Risk::R2 => self.r2,
            Risk::R3 => self.r3,
            Risk::R4 => self.r4,
        }
    }
}

/// Block layers (A11 as narrowed by F1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockClass {
    /// Blocked until the user unlocks it. **Empty by default under F1.**
    DefaultBlocked,
    /// User-configured hard deny list (empty by default); approval cannot bypass it.
    HardDeny,
    /// Services Loom does not implement (e.g. UDS 0x2A/0x86) — cannot be enabled.
    ImmutableDeny,
}

/// Scope of an approval (only used when approvals are enabled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// One use.
    Once,
    /// Several uses until a deadline.
    Session {
        /// Maximum uses.
        max_uses: u32,
        /// Host monotonic deadline (ns).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until_mono_ns: Option<U64>,
    },
    /// Bound to a test session (E4 bundle approval).
    TestSession {
        /// Session id.
        session_id: String,
    },
    /// Bound to a run (E4).
    Run {
        /// Run id.
        run_id: String,
    },
}

/// How a human presence check was performed (E9; only when turned on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresenceMethod {
    /// Short code shown only to viewers (mis-click guard, not proof of a human).
    Code,
    /// WebAuthn assertion with a registered credential.
    Webauthn,
    /// Out-of-band code.
    Oob,
}

/// Presence proof attached to an approval response (E9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Presence {
    /// Method.
    pub method: PresenceMethod,
    /// Proof (code text or WebAuthn assertion, base64url) — viewer serialization only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
}

/// What is being approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// A single write.
    Write,
    /// Start of a job.
    JobStart,
    /// Relaxing a policy setting.
    PolicyRelax,
    /// Unlocking a default-blocked item.
    Unlock,
    /// Test plan bundle (E4).
    TestPlan,
    /// Run plan bundle (E4).
    RunPlan,
}

/// Write class (A1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WriteClass {
    /// One request through the full pipeline.
    Request,
    /// Job: only start/update/renew/stop are gated.
    Job,
    /// Interactive input stream (lease checked once).
    Interactive,
}
