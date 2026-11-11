//! Event records (contract v1 §6.1, master D3·A13·A14·E1·E8).
//!
//! Two record types:
//! * [`SourceRecord`] — what a worker writes to its journal and sends to the gateway. It has
//!   the source identity `(producer_id, producer_epoch, source_seq)` but **no** stream seq.
//! * [`PublishedRecord`] — the same fields plus `seq`/`epoch` (stream_seq / stream_epoch)
//!   assigned by the hub. This is what subscribers and the CLI see.
//!
//! Both are flat JSON objects. They are generated from one field list by `define_record!` so the
//! two can never drift; `#[serde(flatten)]` is deliberately **not** used because serde's
//! buffered content forgets `is_human_readable()` and would break [`U64`]'s binary form.
//!
//! `payload` is a kind-specific structured object (E1). Raw bytes live in `payload.bytes`
//! ([`PayloadBytes`]) and the write state of a write row in `payload.write_state` (E8).
//! There is no top-level `write_state` or `data` field.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::bytes::{decode_base64, decode_hex};
use crate::error::ProtoError;
use crate::num::U64;

/// Timestamp of an event (D3). All integers are nanoseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Ts {
    /// Host monotonic clock (same-host comparisons, latency gates).
    pub mono_ns: U64,
    /// Wall clock (UTC, ns since Unix epoch).
    pub wall_ns: U64,
    /// Identifies the monotonic clock domain (changes on reboot).
    pub clock_id: String,
    /// Where the timestamp was taken.
    pub source: TsSource,
    /// Free-form quality note (e.g. `usb_latency_ms`), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
}

/// Origin of a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TsSource {
    /// Host read call completed.
    HostReadComplete,
    /// Driver-provided timestamp.
    Driver,
    /// Hardware timestamp from the interface.
    Hardware,
    /// Timestamp reported by the device itself.
    Device,
    /// Assigned by the gateway.
    Gateway,
    /// Synthetic (simulation, replay).
    Synthetic,
    /// pcap host timestamp (eth; `ts.source = "pcap_host"`).
    PcapHost,
    /// pcap adapter (NIC hardware) timestamp (eth).
    PcapAdapter,
    /// Re-emitted from a recording (eth replay).
    Replay,
    /// Produced by a simulation backend (eth sim).
    Sim,
}

/// Direction relative to Loom; absent for state/control events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    /// Received from the device.
    Rx,
    /// Sent to the device.
    Tx,
}

/// Who caused the event (fixed 4 values, D3). Decided by connection credentials, never by
/// request fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// AI agent via CLI / agent connection.
    Agent,
    /// Human via a viewer connection.
    User,
    /// The device (or external tool on the bus, see `external_actor`).
    Device,
    /// Loom runner / test session.
    Script,
}

/// Risk class (D6). Under F1 every class runs automatically by default; the class is still
/// shown and recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub enum Risk {
    /// Observe only.
    R0,
    /// Reversible, low risk write.
    R1,
    /// State-changing write.
    R2,
    /// Dangerous (security access, actuators, periodic TX …).
    R3,
    /// Irreversible / high risk.
    R4,
}

/// Environment badge (B4). Does not change risk handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Env {
    /// Simulation backends.
    Sim,
    /// Bench hardware.
    Bench,
    /// Real vehicle.
    Vehicle,
}

/// Write state of a write row (A14), carried as `payload.write_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WriteState {
    /// Received by the gateway.
    Requested,
    /// Passed policy (and approval if enabled).
    Accepted,
    /// Handed to driver/socket (not a wire confirmation).
    Sent,
    /// Backend confirmed transmission.
    Confirmed,
    /// Effect verified (e.g. read-back).
    EffectiveVerified,
    /// Effect could not be verified.
    EffectUnverified,
    /// Definitely failed.
    Failed,
    /// May or may not have executed; never auto-retry.
    OutcomeUnknown,
}

/// Source identity of an event: who produced it and its producer sequence (D3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SourceRef {
    /// Producer (worker instance or `gateway`).
    pub producer_id: String,
    /// Increments on every producer (worker) start.
    pub producer_epoch: U64,
    /// Monotonic per producer epoch.
    pub source_seq: U64,
}

/// Numeric sample attached to an event (CR-X2); viz plots these without domain code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Metric {
    /// Metric name, e.g. `current`.
    pub name: String,
    /// Value.
    pub value: f64,
    /// Unit, e.g. `A`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

macro_rules! define_record {
    (
        $(#[$meta:meta])*
        $name:ident { $( $(#[$fmeta:meta])* $field:ident : $ty:ty ),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
        pub struct $name {
            /// Record schema version (currently [`crate::SCHEMA_VERSION`]).
            pub schema_version: u16,
            /// Recording session id.
            pub session_id: String,
            /// Domain (`uart`, `can`, `uds`, `core`, `display`, `record`, `source`, `run`, `test`, `ui` …).
            pub domain: String,
            /// Domain channel alias (`dut`, `ch0`, `ecu1`); absent for core events.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub channel: Option<String>,
            /// Event kind `<domain>.<event>` (see [`validate_event_kind`]).
            pub kind: String,
            $( $(#[$fmeta])* pub $field: $ty, )*
            /// Producer id (worker instance or `gateway`).
            pub producer_id: String,
            /// Producer epoch.
            pub producer_epoch: U64,
            /// Producer sequence.
            pub source_seq: U64,
            /// Timestamps.
            pub ts: Ts,
            /// Direction (`rx`/`tx`), absent for state/control.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub dir: Option<Dir>,
            /// Origin decided by connection credentials.
            pub origin: Origin,
            /// Client id (`LOOM_CLIENT` + pid, `web:tab-3`).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub client_id: Option<String>,
            /// Runner/test actor (`run:<id>`, `test:<id>`).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub actor_id: Option<String>,
            /// External tool on the bus (`canoe`, `host`, `unknown`) when origin = device.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub external_actor: Option<String>,
            /// Original origin of a replayed event.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub original_origin: Option<Origin>,
            /// Request id; write rows of one request share it (A2).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub request_id: Option<String>,
            /// Job id (periodic TX, run, export …).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub job_id: Option<String>,
            /// Device/link generation (UART gen, UDS link epoch, CAN reopen) — CR-C5.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub generation: Option<u32>,
            /// Purpose (`agent_send`, `keepalive`, `run`, `replay`, `sim` …).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub purpose: Option<String>,
            /// Risk class of a write.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub risk: Option<Risk>,
            /// Test session this client was bound to (auto-tagged by the hub, TC-C1).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub test_session: Option<String>,
            /// Test step at the time of the event.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub test_step: Option<String>,
            /// Environment badge; required on write events (B4).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub env: Option<Env>,
            /// Kind-specific structured object (E1). Raw bytes in `payload.bytes`,
            /// write state in `payload.write_state` (E8).
            #[serde(default = "empty_object")]
            pub payload: Value,
            /// Numeric samples (omitted when empty).
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub metrics: Vec<Metric>,
            /// Quality flags (`stale`, `decimated:N`, `simulated` …), omitted when empty.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub quality: Vec<String>,
        }

        impl $name {
            /// Source identity of this record.
            pub fn source_ref(&self) -> SourceRef {
                SourceRef {
                    producer_id: self.producer_id.clone(),
                    producer_epoch: self.producer_epoch,
                    source_seq: self.source_seq,
                }
            }

            /// `payload.write_state`, if this is a write row.
            pub fn write_state(&self) -> Result<Option<WriteState>, ProtoError> {
                payload_write_state(&self.payload)
            }
        }
    };
}

define_record! {
    /// Original record as produced by a worker (journal form). No stream seq/epoch.
    SourceRecord {}
}

define_record! {
    /// Record as published by the hub to subscribers and queries (stream form).
    PublishedRecord {
        /// stream_seq assigned by the hub.
        seq: U64,
        /// stream_epoch (increments on every gateway start) — cursors bind to it.
        epoch: U64,
    }
}

/// Copy the shared fields of `$from` into a struct literal of type `$to`, plus extra fields.
macro_rules! convert_record {
    ($from:expr, $to:ident { $( $extra:ident : $val:expr ),* }) => {{
        let r = $from;
        $to {
            schema_version: r.schema_version,
            session_id: r.session_id,
            domain: r.domain,
            channel: r.channel,
            kind: r.kind,
            $( $extra: $val, )*
            producer_id: r.producer_id,
            producer_epoch: r.producer_epoch,
            source_seq: r.source_seq,
            ts: r.ts,
            dir: r.dir,
            origin: r.origin,
            client_id: r.client_id,
            actor_id: r.actor_id,
            external_actor: r.external_actor,
            original_origin: r.original_origin,
            request_id: r.request_id,
            job_id: r.job_id,
            generation: r.generation,
            purpose: r.purpose,
            risk: r.risk,
            test_session: r.test_session,
            test_step: r.test_step,
            env: r.env,
            payload: r.payload,
            metrics: r.metrics,
            quality: r.quality,
        }
    }};
}

impl SourceRecord {
    /// Attach the hub's stream position.
    pub fn publish(self, seq: U64, epoch: U64) -> PublishedRecord {
        convert_record!(
            self,
            PublishedRecord {
                seq: seq,
                epoch: epoch
            }
        )
    }
}

impl PublishedRecord {
    /// Drop the stream position (back to journal form).
    pub fn into_source(self) -> SourceRecord {
        convert_record!(self, SourceRecord {})
    }
}

/// Encoding of `payload.bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// Lowercase hex in `data`.
    Hex,
    /// Standard base64 in `data`.
    Base64,
    /// Out-of-line reference in `ref`.
    RawRef,
}

/// Out-of-line large data reference (fetched with HTTP GET, plan §4.8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawRef {
    /// Server-owned opaque artifact id (never a file path).
    pub id: String,
    /// SHA-256 of the content, lowercase hex.
    pub sha256: String,
    /// Length in bytes.
    pub length: U64,
    /// Content format (`raw`, `csv`, `sigrok` …).
    pub format: String,
}

/// Shape of `payload.bytes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PayloadBytes {
    /// Encoding of the bytes.
    pub encoding: Encoding,
    /// Encoded bytes for `hex` / `base64`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Reference for `raw_ref`.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<RawRef>,
}

impl PayloadBytes {
    /// Inline bytes as base64 (the default for event payloads).
    pub fn base64(bytes: &[u8]) -> Self {
        use base64::Engine as _;
        PayloadBytes {
            encoding: Encoding::Base64,
            data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            r#ref: None,
        }
    }

    /// Inline bytes as hex.
    pub fn hex(bytes: &[u8]) -> Self {
        PayloadBytes {
            encoding: Encoding::Hex,
            data: Some(hex::encode(bytes)),
            r#ref: None,
        }
    }

    /// Decode inline bytes (size-limited, fallible). `raw_ref` → [`ProtoError::NotInline`].
    pub fn decode(&self) -> Result<Vec<u8>, ProtoError> {
        let data = || {
            self.data
                .as_deref()
                .ok_or_else(|| ProtoError::InvalidPayload("bytes.data missing".into()))
        };
        match self.encoding {
            Encoding::Hex => decode_hex(data()?),
            Encoding::Base64 => decode_base64(data()?),
            Encoding::RawRef => Err(ProtoError::NotInline),
        }
    }
}

/// Read `payload.write_state`. `Ok(None)` if absent, `Err` if present but not a known state.
pub fn payload_write_state(payload: &Value) -> Result<Option<WriteState>, ProtoError> {
    match payload.get("write_state") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| ProtoError::InvalidPayload(format!("write_state: {e}"))),
    }
}

/// Read `payload.bytes` as a spec (`Ok(None)` if absent).
pub fn payload_bytes_spec(payload: &Value) -> Result<Option<PayloadBytes>, ProtoError> {
    match payload.get("bytes") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| ProtoError::InvalidPayload(format!("bytes: {e}"))),
    }
}

/// Decode `payload.bytes` inline (`Ok(None)` if absent, `Err(NotInline)` for `raw_ref`).
pub fn payload_bytes(payload: &Value) -> Result<Option<Vec<u8>>, ProtoError> {
    payload_bytes_spec(payload)?.map(|b| b.decode()).transpose()
}

/// Inclusive stream-seq range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SeqRange {
    /// First seq (inclusive).
    pub from: U64,
    /// Last seq (inclusive).
    pub to: U64,
}

/// Inclusive source-seq range of one producer epoch.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ProducerRange {
    /// Producer id.
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: U64,
    /// First source seq (inclusive).
    pub from: U64,
    /// Last source seq (inclusive).
    pub to: U64,
}

/// Loss count: exact or unknown. JSON: `"123"` or `"unknown"`; binary: integer or `"unknown"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GapCount {
    /// Exactly this many events were lost.
    Exact(U64),
    /// Loss happened but the amount is unknown.
    Unknown,
}

const UNKNOWN: &str = "unknown";

impl Serialize for GapCount {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            GapCount::Exact(n) => n.serialize(s),
            GapCount::Unknown => s.serialize_str(UNKNOWN),
        }
    }
}

struct GapCountVisitor {
    human: bool,
}

impl serde::de::Visitor<'_> for GapCountVisitor {
    type Value = GapCount;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a u64 count or \"unknown\"")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<GapCount, E> {
        if v == UNKNOWN {
            return Ok(GapCount::Unknown);
        }
        if !self.human {
            return Err(E::invalid_value(serde::de::Unexpected::Str(v), &self));
        }
        crate::num::parse_u64_canonical(v)
            .map(|n| GapCount::Exact(U64(n)))
            .map_err(E::custom)
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<GapCount, E> {
        if self.human {
            return Err(E::invalid_type(serde::de::Unexpected::Unsigned(v), &self));
        }
        Ok(GapCount::Exact(U64(v)))
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<GapCount, E> {
        if self.human {
            return Err(E::invalid_type(serde::de::Unexpected::Signed(v), &self));
        }
        u64::try_from(v)
            .map(|n| GapCount::Exact(U64(n)))
            .map_err(|_| E::invalid_value(serde::de::Unexpected::Signed(v), &self))
    }
}

impl<'de> Deserialize<'de> for GapCount {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let human = d.is_human_readable();
        if human {
            d.deserialize_str(GapCountVisitor { human })
        } else {
            d.deserialize_any(GapCountVisitor { human })
        }
    }
}

impl JsonSchema for GapCount {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "GapCount".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^(unknown|0|[1-9][0-9]{0,19})$",
            "description": "Exact loss count as canonical decimal string, or \"unknown\"."
        })
    }
}

/// Payload of the three loss events `display.gap` / `record.gap` / `source.gap` (A13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GapData {
    /// Stream range when known (display gaps after the hub).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_range: Option<SeqRange>,
    /// Source range (losses before the hub never invent stream seqs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_range: Option<ProducerRange>,
    /// Number of lost events.
    pub count: GapCount,
    /// Cause (`slow_client`, `ipc_backpressure`, `disk_full`, `driver_overrun`, `gateway_restart` …).
    pub cause: String,
    /// Lost bytes when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<U64>,
    /// Extra detail (domain summary, e.g. top CAN ids).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

/// Short gap summary carried in envelopes and event batches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GapSummary {
    /// Total lost events.
    pub count: GapCount,
    /// Covered stream range when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_range: Option<SeqRange>,
    /// Distinct causes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<String>,
}

/// Event kind name constants owned by CORE (§6.1, §3.4). Domain kinds live in domain crates.
pub mod kinds {
    /// STOP latch state change (payload: on, by, origin, jobs_stopped, … persisted).
    pub const CORE_LATCH: &str = "core.latch";
    /// Approval created (only when approvals are enabled, F1).
    pub const CORE_APPROVAL_REQUESTED: &str = "core.approval_requested";
    /// Approval granted / denied / expired.
    pub const CORE_APPROVAL_RESOLVED: &str = "core.approval_resolved";
    /// R2 confirmation token issued (when enabled).
    pub const CORE_CONFIRM_ISSUED: &str = "core.confirm_issued";
    /// Write denied by policy.
    pub const CORE_POLICY_DENIED: &str = "core.policy_denied";
    /// Effective policy changed.
    pub const CORE_POLICY_CHANGED: &str = "core.policy_changed";
    /// Worker lifecycle (starting, ready, stalled, crashed, quarantined …).
    pub const CORE_WORKER_STATE: &str = "core.worker_state";
    /// Job lifecycle.
    pub const CORE_JOB_STATE: &str = "core.job_state";
    /// Job progress (capture, export …).
    pub const CORE_JOB_PROGRESS: &str = "core.job_progress";
    /// Aggregated agent activity incl. read ranges (I4, CR-UI-11).
    pub const CORE_ACTIVITY: &str = "core.activity";
    /// Point where an agent received user interventions (CR-UI-11).
    pub const CORE_AGENT_CURSOR: &str = "core.agent_cursor";
    /// User/agent mark on all timelines.
    pub const CORE_MARK: &str = "core.mark";
    /// Gateway restarted with a new stream epoch.
    pub const CORE_EPOCH_CHANGED: &str = "core.epoch_changed";
    /// Unfinished write recovered as outcome_unknown after restart.
    pub const CORE_WRITE_RECOVERED: &str = "core.write_recovered";
    /// Recording session closed.
    pub const CORE_SESSION_CLOSED: &str = "core.session_closed";
    /// Environment badge changed.
    pub const CORE_ENV: &str = "core.env";
    /// Heartbeat (not recorded).
    pub const CORE_HEARTBEAT: &str = "core.heartbeat";
    /// Screen-only drop.
    pub const DISPLAY_GAP: &str = "display.gap";
    /// Loom recording loss.
    pub const RECORD_GAP: &str = "record.gap";
    /// Recording health change (ok, degraded, failed, disabled).
    pub const RECORD_HEALTH: &str = "record.health";
    /// Loss before Loom received the data.
    pub const SOURCE_GAP: &str = "source.gap";
    /// Agent asks the UI to show a panel (advisory).
    pub const UI_FOCUS_REQUEST: &str = "ui.focus_request";
    /// Test session started.
    pub const TEST_SESSION_STARTED: &str = "test.session_started";
    /// Test session ended.
    pub const TEST_SESSION_ENDED: &str = "test.session_ended";
    /// Test step started (marker on all timelines).
    pub const TEST_STEP_STARTED: &str = "test.step_started";
    /// Test step ended.
    pub const TEST_STEP_ENDED: &str = "test.step_ended";
    /// Evidence citation checked.
    pub const TEST_EVIDENCE_CHECKED: &str = "test.evidence_checked";
    /// Step verdict (proposed / effective / override).
    pub const TEST_VERDICT: &str = "test.verdict";
    /// Run started.
    pub const RUN_STARTED: &str = "run.started";
    /// Run step state.
    pub const RUN_STEP: &str = "run.step";
    /// Run ended.
    pub const RUN_ENDED: &str = "run.ended";

    /// All constants above (for tests and registries).
    pub const ALL: &[&str] = &[
        CORE_LATCH,
        CORE_APPROVAL_REQUESTED,
        CORE_APPROVAL_RESOLVED,
        CORE_CONFIRM_ISSUED,
        CORE_POLICY_DENIED,
        CORE_POLICY_CHANGED,
        CORE_WORKER_STATE,
        CORE_JOB_STATE,
        CORE_JOB_PROGRESS,
        CORE_ACTIVITY,
        CORE_AGENT_CURSOR,
        CORE_MARK,
        CORE_EPOCH_CHANGED,
        CORE_WRITE_RECOVERED,
        CORE_SESSION_CLOSED,
        CORE_ENV,
        CORE_HEARTBEAT,
        DISPLAY_GAP,
        RECORD_GAP,
        RECORD_HEALTH,
        SOURCE_GAP,
        UI_FOCUS_REQUEST,
        TEST_SESSION_STARTED,
        TEST_SESSION_ENDED,
        TEST_STEP_STARTED,
        TEST_STEP_ENDED,
        TEST_EVIDENCE_CHECKED,
        TEST_VERDICT,
        RUN_STARTED,
        RUN_STEP,
        RUN_ENDED,
    ];
}

/// True if `s` is a name token `[a-z][a-z0-9_]*`.
pub(crate) fn is_token(s: &str) -> bool {
    let mut it = s.bytes();
    matches!(it.next(), Some(b'a'..=b'z'))
        && it.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// Validate an event kind: exactly two tokens `<domain>.<event>` (§4.7).
pub fn validate_event_kind(kind: &str) -> Result<(), ProtoError> {
    let mut parts = kind.split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(d), Some(e), None) if is_token(d) && is_token(e) => Ok(()),
        _ => Err(ProtoError::InvalidName(format!(
            "event kind must be <domain>.<event>: {kind:?}"
        ))),
    }
}
