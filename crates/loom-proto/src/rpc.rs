//! External IPC base types: JSON-RPC 2.0, hello/role, subscription (contract v1 §3.4, §4.7,
//! master D2·E1·E9).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::Viewers;
use crate::error::ProtoError;
use crate::event::{Env, GapSummary, Origin, PublishedRecord, is_token};
use crate::num::U64;

/// The literal `"2.0"`.
pub const JSONRPC_VERSION: &str = "2.0";
/// JSON-RPC parse error.
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC invalid request.
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC method not found.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC invalid params.
pub const INVALID_PARAMS: i64 = -32602;
/// JSON-RPC internal error.
pub const INTERNAL_ERROR: i64 = -32603;
/// Maximum client→server message and `core.events` batch size (D2·A5).
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// Domains reserved for CORE (CR-C7).
pub const RESERVED_DOMAINS: [&str; 8] = [
    "core", "record", "display", "source", "run", "test", "ui", "adapter",
];

/// JSON-RPC request id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Id {
    /// Integer id.
    Number(i64),
    /// String id.
    String(String),
}

/// JSON-RPC 2.0 request or notification (no `id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Request {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Id; absent for notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// Method name (see [`validate_method`]).
    pub method: String,
    /// Parameters (object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    /// New request with an id.
    pub fn new(id: Id, method: impl Into<String>, params: Value) -> Self {
        Request {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(id),
            method: method.into(),
            params: Some(params),
        }
    }

    /// Check version and method name.
    pub fn validate(&self) -> Result<(), ProtoError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(ProtoError::InvalidName(format!(
                "jsonrpc must be \"2.0\": {:?}",
                self.jsonrpc
            )));
        }
        validate_method(&self.method)
    }
}

/// JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcError {
    /// Code (spec range or Loom application code).
    pub code: i64,
    /// Message.
    pub message: String,
    /// Extra data (Loom puts the `ErrorBody` here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 response: exactly one of `result` / `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Response {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Echoed id (`null` if the request id could not be read).
    pub id: Option<Id>,
    /// Result on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    /// Success response.
    pub fn ok(id: Option<Id>, result: Value) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Error response.
    pub fn err(id: Option<Id>, error: RpcError) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Exactly one of `result`/`error` and version `"2.0"`.
    pub fn validate(&self) -> Result<(), ProtoError> {
        if self.jsonrpc != JSONRPC_VERSION || self.result.is_some() == self.error.is_some() {
            return Err(ProtoError::InvalidName(
                "response must have jsonrpc 2.0 and exactly one of result/error".into(),
            ));
        }
        Ok(())
    }
}

/// Validate an RPC method: `<domain>.<verb>` or `<domain>.<noun>.<verb>`, tokens `[a-z][a-z0-9_]*`.
pub fn validate_method(method: &str) -> Result<(), ProtoError> {
    let n = method.split('.').count();
    if (2..=3).contains(&n) && method.split('.').all(is_token) {
        Ok(())
    } else {
        Err(ProtoError::InvalidName(format!(
            "method must be <domain>.<verb>[.<verb>] snake tokens: {method:?}"
        )))
    }
}

/// True if `domain` is reserved for CORE.
pub fn is_reserved_domain(domain: &str) -> bool {
    RESERVED_DOMAINS.contains(&domain)
}

/// Connection role, assigned by the server (D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// `daemon.json` token holder (CLI). Writes carry origin = agent.
    Agent,
    /// Web viewer (bootstrap cookie). Writes carry origin = user.
    Viewer,
    /// Internal runner / test actor. Origin = script.
    Script,
}

impl Role {
    /// Origin of writes made on a connection with this role.
    pub const fn origin(self) -> Origin {
        match self {
            Role::Agent => Origin::Agent,
            Role::Viewer => Origin::User,
            Role::Script => Origin::Script,
        }
    }
}

/// Viewer rights level (E9). Ordered: watch < operate < approve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rights {
    /// See everything, change nothing.
    Watch,
    /// Input, STOP release, worker restart, run/test control.
    Operate,
    /// Approvals, policy relaxations, verdict overrides.
    Approve,
}

/// Client self-description in `core.hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClientInfo {
    /// `cli`, `web`, `tui`, `sdk` …
    pub kind: String,
    /// Client version.
    pub version: String,
    /// stdin and stdout are terminals (informational only, never an authority).
    #[serde(default)]
    pub tty: bool,
}

/// `core.hello` params — first message on every connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Hello {
    /// Protocol, e.g. `"loom/1.0"`.
    pub proto: String,
    /// `daemon.json` token (agent); viewers authenticate with the cookie instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Stable client id (`LOOM_CLIENT` + pid).
    pub client_id: String,
    /// Requested role; the server decides.
    pub want_role: Role,
    /// Client description.
    pub client: ClientInfo,
}

/// Server identity in the hello result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ServerInfo {
    /// Daemon version.
    pub version: String,
    /// Build id.
    pub build: String,
    /// Schema hash of `loom-proto` (drift detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_hash: Option<String>,
}

/// `core.hello` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HelloResult {
    /// Granted role.
    pub role: Role,
    /// Rights for viewer connections (`None` for agent/script).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<Rights>,
    /// Connection id.
    pub conn_id: String,
    /// Current stream epoch.
    pub stream_epoch: U64,
    /// Server identity.
    pub server: ServerInfo,
    /// Environment badge.
    pub env: Env,
    /// STOP latch state.
    pub latch: bool,
    /// Viewer counts.
    pub viewers: Viewers,
    /// Capabilities, e.g. `core/1`, `uart/1`.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Event filter of a subscription (empty list = all).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SubFilter {
    /// Domains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    /// Channels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
    /// Kinds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    /// Origins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<Origin>,
    /// Domain extension filter pushed down to the domain (e.g. eth display filter, F2). Opaque here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<Value>,
}

/// Snapshot backlog limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Backlog {
    /// Max events.
    pub max_events: u32,
    /// Max encoded bytes.
    pub max_bytes: u32,
    /// Max age in ms.
    pub max_age_ms: u32,
}

/// Wire encoding of event batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BatchEncoding {
    /// JSON text frames (M1 default).
    #[default]
    Json,
    /// Negotiated binary column frames (`loom.columns.v1`, M2).
    Binary,
}

fn default_max_batch_ms() -> u32 {
    40
}

/// `core.subscribe` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Subscribe {
    /// Filter.
    #[serde(default)]
    pub filter: SubFilter,
    /// Resume after this stream seq (same epoch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<U64>,
    /// Stream epoch `after_seq` belongs to (reconnect). A different epoch → `core.epoch_changed` + snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_epoch: Option<U64>,
    /// Request an atomic snapshot first.
    #[serde(default)]
    pub snapshot: bool,
    /// Snapshot backlog limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlog: Option<Backlog>,
    /// Batch flush upper bound (terminal 10, default 40).
    #[serde(default = "default_max_batch_ms")]
    pub max_batch_ms: u32,
    /// Batch encoding.
    #[serde(default)]
    pub encoding: BatchEncoding,
    /// Screen-only coalescing key (`id`), counts reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesce_by: Option<String>,
}

/// `core.subscribe` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubscribeResult {
    /// Subscription id.
    pub sub_id: String,
    /// Stream epoch.
    pub stream_epoch: U64,
}

/// `core.events` notification params (one batch, ≤ 64 KiB).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventsBatch {
    /// Subscription id.
    pub sub: String,
    /// First stream seq in the batch.
    pub first: U64,
    /// Last stream seq in the batch.
    pub last: U64,
    /// Events (filtered; seq may be discontinuous).
    pub events: Vec<PublishedRecord>,
    /// Screen drops since the previous batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_gap: Option<GapSummary>,
    /// Age of the oldest event at send time (ms).
    pub lag_ms: u32,
    /// Hub monotonic time when the batch was published (ns) — UI latency reference.
    #[serde(default)]
    pub hub_publish_mono_ns: U64,
    /// Recording health at publish time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_health: Option<crate::envelope::RecordHealth>,
}
