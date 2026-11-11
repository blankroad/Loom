//! Worker protocol `loom-worker/1` message types and frame header (contract v1 §3.3).
//!
//! The byte codec (MessagePack map via rmp-serde) is chosen by `loom-worker-sdk`; this module only
//! defines serde types and the pure frame-header functions.
//!
//! Frame: `u32 LE payload_len | u8 msg_type | u8 flags | u16 reserved(=0) | payload`.
//! A whole frame (header + payload) is at most [`MAX_FRAME`] bytes. Lengths are validated before
//! any allocation. v1 defines no flag bits, so `flags` must be 0.
//!
//! [`WorkerMsg`] is externally tagged (`{"hello": {...}}`). Internally tagged / untagged enums
//! are avoided because serde's buffered content would lose `is_human_readable()` for [`U64`].

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bytes::Bytes;
use crate::envelope::RecordHealth;
use crate::error::{ErrorCode, ProtoError};
use crate::event::{Origin, SourceRecord, SourceRef};
use crate::num::U64;
use crate::policy::WriteClass;

/// Protocol name.
pub const WORKER_PROTO: &str = "loom-worker";
/// Protocol major version.
pub const WORKER_PROTO_MAJOR: u16 = 1;
/// Protocol minor version.
pub const WORKER_PROTO_MINOR: u16 = 2;
/// Maximum frame size including the header (64 KiB).
pub const MAX_FRAME: usize = 64 * 1024;
/// Header size in bytes.
pub const HEADER_LEN: usize = 8;
/// Maximum payload size.
pub const MAX_PAYLOAD: usize = MAX_FRAME - HEADER_LEN;

/// Which of the two physical connections a message travels on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Requests, replies, write states, latch, input, health, priority events.
    Ctl,
    /// Bulk RX batches and credits.
    Data,
}

/// Major/minor version pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProtoVersion {
    /// Major (mismatch = reject).
    pub major: u16,
    /// Minor (intersection is used).
    pub minor: u16,
}

/// W→G first message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkerHello {
    /// Protocol version.
    pub proto: ProtoVersion,
    /// Worker kind (`uart`, `can`, `seedkey` …).
    pub kind: String,
    /// Instance (`uart-dut`, `can-hw`).
    pub instance: String,
    /// Producer id used in events.
    pub producer_id: String,
    /// Producer epoch (increments per start).
    pub producer_epoch: U64,
    /// OS process id.
    pub pid: u32,
    /// Build id.
    pub build: String,
    /// `loom-proto` schema hash.
    pub schema_hash: String,
    /// Supported codecs (`msgpack-map`).
    pub codecs: Vec<String>,
    /// Domains served.
    pub domains: Vec<String>,
    /// Capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Journal directory used by this worker.
    pub journal_dir: String,
    /// Re-attach nonce (M2), absent on fresh start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_nonce: Option<String>,
}

/// Memory and credit budgets sent to a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkerBudgets {
    /// Soft memory budget (bytes).
    pub mem_soft_bytes: U64,
    /// Hard memory budget (bytes; Windows = commit limit).
    pub mem_hard_bytes: U64,
    /// Initial data-lane credit window (bytes).
    pub credit_bytes: U64,
}

/// G→W accept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Welcome {
    /// Accepted minor version.
    pub accepted_minor: u16,
    /// Current stream epoch.
    pub stream_epoch: U64,
    /// Domain configuration.
    #[serde(default)]
    pub config: Value,
    /// Budgets.
    pub budgets: WorkerBudgets,
    /// Current STOP latch.
    pub latch: bool,
}

/// G→W refuse (e.g. major mismatch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Reject {
    /// Reason.
    pub reason: String,
}

/// G→W generic call (open, close, query …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Call {
    /// 호출을 소유한 연결. 이전 프로토콜에서는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conn_id: Option<String>,
    /// Call id (echoed in [`Reply`]).
    pub call_id: U64,
    /// Method.
    pub method: String,
    /// Arguments.
    #[serde(default)]
    pub args: Value,
    /// Origin of the caller.
    pub origin: Origin,
    /// Client id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Actor id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// Request id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Absolute deadline (host monotonic ns).
    pub deadline_mono_ns: U64,
    /// Approval reference when one was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<String>,
}

/// G→W write that passed the pipeline (request class).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Write {
    /// Call id.
    pub call_id: U64,
    /// Request id (write rows).
    pub request_id: String,
    /// Write class.
    pub write_class: WriteClass,
    /// Domain operation.
    pub op: Value,
    /// Origin.
    pub origin: Origin,
    /// Client id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Plan digest when the write belongs to an approved plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_digest: Option<String>,
    /// Approval reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<String>,
    /// Absolute deadline (host monotonic ns).
    pub deadline_mono_ns: U64,
}

/// Job start (job class, A1-②).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobStart {
    /// 선택적 응답 식별자. 없으면 v1.0 소켓 요청 순서 번호를 사용한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    /// Job id.
    pub job_id: String,
    /// Approved plan (ids, ranges, period).
    pub plan: Value,
    /// Lease in ms.
    pub lease_ms: u32,
    /// Approval reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<String>,
}

/// Job plan update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobUpdate {
    /// 선택적 응답 식별자. 없으면 v1.0 소켓 요청 순서 번호를 사용한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    /// Job id.
    pub job_id: String,
    /// New plan.
    pub plan: Value,
}

/// Job lease renewal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JobRenew {
    /// 선택적 응답 식별자. 없으면 v1.0 소켓 요청 순서 번호를 사용한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    /// Job id.
    pub job_id: String,
    /// New lease in ms.
    pub lease_ms: u32,
}

/// Job stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JobStop {
    /// 선택적 응답 식별자. 없으면 v1.0 소켓 요청 순서 번호를 사용한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    /// Job id.
    pub job_id: String,
    /// Reason (`user`, `stop_latch`, `expired` …).
    pub reason: String,
}

/// Interactive input lease grant (A1-③).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputLease {
    /// 입력 lease를 소유한 연결.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conn_id: Option<String>,
    /// Channel.
    pub channel: String,
    /// Lease id.
    pub lease_id: String,
    /// Owner origin.
    pub owner: Origin,
    /// Owner client id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Interactive input bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Input {
    /// 입력을 보낸 연결.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conn_id: Option<String>,
    /// 입력 대상 채널.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// 연결 내 입력 순서 번호.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_seq: Option<U64>,
    /// 선택적 응답 식별자. 없으면 v1.0 소켓 요청 순서 번호를 사용한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    /// Lease id.
    pub lease_id: String,
    /// Bytes to send.
    pub bytes: Bytes,
    /// Priority (Ctrl-C etc.) — handled before queued input.
    #[serde(default)]
    pub priority: bool,
}

/// 입력 lease가 끝난 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputLeaseEndCause {
    /// 연결이 종료되었다.
    ConnClosed,
    /// 사용자가 lease를 반납했다.
    Released,
}

/// G→W 입력 lease 종료 통지. 응답을 요구하지 않으며 소켓 요청 순번을 소비하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputLeaseEnd {
    /// 종료한 lease.
    pub lease_id: String,
    /// 소유 연결. 알 수 없으면 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conn_id: Option<String>,
    /// 종료 이유.
    pub cause: InputLeaseEndCause,
}

/// STOP latch change (ack required).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Latch {
    /// Latch on/off.
    pub on: bool,
    /// Scope (`all` in M1).
    pub scope: String,
    /// Reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Latch sequence (echoed in [`LatchAck`]).
    pub latch_seq: U64,
}

/// Data-lane credit grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Credit {
    /// Additional bytes allowed.
    pub bytes: U64,
}

/// Error reported in a [`Reply`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkerError {
    /// Error code.
    pub code: ErrorCode,
    /// Message.
    pub message: String,
    /// Detail (e.g. `write_state: outcome_unknown`, `written_bytes`).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

/// Result of a call or write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplyResult {
    /// Success value.
    Ok(Value),
    /// Error.
    Err(WorkerError),
}

/// W→G reply. `covers` = last source event emitted before this reply (publish-before-reply
/// watermark, E7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Reply {
    /// Echoed call id.
    pub call_id: U64,
    /// Result.
    pub result: ReplyResult,
    /// Watermark the gateway waits for before answering the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covers: Option<SourceRef>,
}

/// W→G priority events on the control lane (write states, responses, state, source gaps).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PriorityEvents {
    /// Records in source order.
    pub records: Vec<SourceRecord>,
}

/// One bounded queue's statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QueueStat {
    /// Current depth (items).
    pub depth: u32,
    /// Current bytes.
    pub bytes: U64,
    /// High-water depth.
    pub max_depth: u32,
    /// Items dropped so far.
    pub drops: U64,
}

/// Journal state reported in [`Health`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JournalHealth {
    /// Highest source seq handed to the OS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written_through: Option<U64>,
    /// Highest source seq synced to disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_through: Option<U64>,
    /// Recording health.
    pub health: RecordHealth,
}

/// W→G heartbeat (1 s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Health {
    /// Worker monotonic time (ns).
    pub mono_ns: U64,
    /// Resident set size (bytes).
    pub rss_bytes: U64,
    /// Committed bytes (Windows) when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_bytes: Option<U64>,
    /// Per-queue statistics by queue name.
    #[serde(default)]
    pub queues: BTreeMap<String, QueueStat>,
    /// Journal state.
    pub journal: JournalHealth,
    /// Thread count.
    pub threads: u32,
}

/// W→G latch acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LatchAck {
    /// Echoed latch sequence.
    pub latch_seq: U64,
}

/// W→G sealed journal segment report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Segment {
    /// File name relative to the journal dir.
    pub file: String,
    /// Producer id.
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: U64,
    /// First source seq.
    pub first_source_seq: U64,
    /// Last source seq.
    pub last_source_seq: U64,
    /// First monotonic time (ns).
    pub mono_first: U64,
    /// Last monotonic time (ns).
    pub mono_last: U64,
    /// Size in bytes.
    pub bytes: U64,
    /// CRC32C of the file.
    pub crc32c: u32,
}

/// W→G bulk RX batch (data lane).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Events {
    /// Source seq of the first record.
    pub first_source_seq: U64,
    /// Number of records.
    pub count: u32,
    /// Records.
    pub records: Vec<SourceRecord>,
}

macro_rules! define_worker_msgs {
    ($( $(#[$doc:meta])* $variant:ident($ty:ty) = $code:literal, $lane:ident; )+) => {
        /// Every `loom-worker/1` message (externally tagged, snake_case).
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum WorkerMsg {
            $( $(#[$doc])* $variant($ty), )+
        }

        impl WorkerMsg {
            /// Frame `msg_type` byte.
            pub const fn msg_type(&self) -> u8 {
                match self { $( WorkerMsg::$variant(_) => $code, )+ }
            }
            /// Lane this message must travel on.
            pub const fn lane(&self) -> Lane {
                match self { $( WorkerMsg::$variant(_) => Lane::$lane, )+ }
            }
        }

        /// All known `msg_type` bytes.
        pub const KNOWN_MSG_TYPES: &[u8] = &[ $( $code, )+ ];
    };
}

define_worker_msgs! {
    /// W→G hello.
    Hello(WorkerHello) = 1, Ctl;
    /// G→W welcome.
    Welcome(Welcome) = 2, Ctl;
    /// G→W reject.
    Reject(Reject) = 3, Ctl;
    /// G→W call.
    Call(Call) = 10, Ctl;
    /// G→W write.
    Write(Write) = 11, Ctl;
    /// G→W job start.
    JobStart(JobStart) = 12, Ctl;
    /// G→W job update.
    JobUpdate(JobUpdate) = 13, Ctl;
    /// G→W job renew.
    JobRenew(JobRenew) = 14, Ctl;
    /// G→W job stop.
    JobStop(JobStop) = 15, Ctl;
    /// G→W input lease.
    InputLease(InputLease) = 16, Ctl;
    /// G→W input bytes.
    Input(Input) = 17, Ctl;
    /// G→W STOP latch.
    Latch(Latch) = 18, Ctl;
    /// G→W data credit.
    Credit(Credit) = 19, Data;
    /// G→W 입력 lease 종료 통지(v1.2).
    InputLeaseEnd(InputLeaseEnd) = 20, Ctl;
    /// W→G reply.
    Reply(Reply) = 30, Ctl;
    /// W→G priority events.
    PriorityEvents(PriorityEvents) = 31, Ctl;
    /// W→G health.
    Health(Health) = 32, Ctl;
    /// W→G latch ack.
    LatchAck(LatchAck) = 33, Ctl;
    /// W→G sealed segment.
    Segment(Segment) = 34, Ctl;
    /// W→G bulk events.
    Events(Events) = 40, Data;
}

/// Decoded frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Payload length (excluding the 8-byte header).
    pub len: u32,
    /// Message type (see [`KNOWN_MSG_TYPES`]).
    pub msg_type: u8,
    /// Flags (must be 0 in v1).
    pub flags: u8,
}

impl FrameHeader {
    /// Build a header, validating the length and type.
    pub fn new(len: usize, msg_type: u8) -> Result<Self, ProtoError> {
        if len > MAX_PAYLOAD {
            return Err(ProtoError::InvalidFrame(format!("payload {len} > {MAX_PAYLOAD}")));
        }
        if !KNOWN_MSG_TYPES.contains(&msg_type) {
            return Err(ProtoError::InvalidFrame(format!("unknown msg_type {msg_type}")));
        }
        let len = u32::try_from(len).map_err(|_| ProtoError::InvalidFrame("length overflow".into()))?;
        Ok(FrameHeader {
            len,
            msg_type,
            flags: 0,
        })
    }

    /// Encode to 8 bytes.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let l = self.len.to_le_bytes();
        [l[0], l[1], l[2], l[3], self.msg_type, self.flags, 0, 0]
    }

    /// Decode and validate a header from the start of `buf`. Never allocates.
    pub fn decode(buf: &[u8]) -> Result<Self, ProtoError> {
        let Some(h) = buf.get(..HEADER_LEN) else {
            return Err(ProtoError::Incomplete {
                need: HEADER_LEN - buf.len(),
            });
        };
        let len = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        let (msg_type, flags) = (h[4], h[5]);
        if len as usize > MAX_PAYLOAD {
            return Err(ProtoError::InvalidFrame(format!("payload {len} > {MAX_PAYLOAD}")));
        }
        if !KNOWN_MSG_TYPES.contains(&msg_type) {
            return Err(ProtoError::InvalidFrame(format!("unknown msg_type {msg_type}")));
        }
        if flags != 0 {
            return Err(ProtoError::InvalidFrame(format!("unknown flags {flags:#04x}")));
        }
        if h[6] != 0 || h[7] != 0 {
            return Err(ProtoError::InvalidFrame("reserved bytes must be 0".into()));
        }
        Ok(FrameHeader { len, msg_type, flags })
    }
}

/// Split one complete frame from the front of `buf`: `(header, payload, rest)`.
/// Returns [`ProtoError::Incomplete`] when more bytes are needed.
pub fn split_frame(buf: &[u8]) -> Result<(FrameHeader, &[u8], &[u8]), ProtoError> {
    let h = FrameHeader::decode(buf)?;
    let end = HEADER_LEN + h.len as usize;
    match buf.get(HEADER_LEN..end) {
        Some(payload) => Ok((h, payload, &buf[end..])),
        None => Err(ProtoError::Incomplete {
            need: end - buf.len(),
        }),
    }
}
