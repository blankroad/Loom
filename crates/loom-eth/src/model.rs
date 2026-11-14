//! eth schema v1 — payloads and RPC shapes shared by gateway, worker, CLI and UI.
//! Human summary: `docs/impl/eth/schema-v1.md`. Changing a field here is a contract change:
//! tell the UI team (`docs/impl/requests/ui-eth.md`).
//! 64-bit counters and seqs are [`U64`] (contract v1 §6.1: canonical decimal string in JSON).

use loom_proto::num::U64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Event kind: one captured packet.
pub const KIND_PKT: &str = "eth.pkt";
/// Event kind: coverage of packets not published individually (every 100 ms).
pub const KIND_BATCH: &str = "eth.batch";
/// Event kind: send row (write state, E8).
pub const KIND_TX: &str = "eth.tx";
/// Event kind: link found later (UDS request ↔ DoIP packet, send ↔ captured copy).
pub const KIND_LINK: &str = "eth.link";
/// Event kind: capture channel state.
pub const KIND_CAPTURE_STATE: &str = "eth.capture_state";
/// Event kind: 1 s rate statistics.
pub const KIND_STATS: &str = "eth.stats";

/// Largest frame carried inline in `eth.pkt.bytes` (M1). Larger frames are cut to this size
/// and flagged `bytes_truncated` until CORE raw storage (`raw_ref`) exists.
pub const MAX_INLINE_FRAME: usize = 16 * 1024;
/// Upper bound for the serialized `layers` object in `eth.pkt` (bytes). Over → keep the
/// first layers and add `"_truncated": true`.
pub const MAX_LAYERS_JSON: usize = 4 * 1024;
/// `eth.query` default and hard row limits.
pub const QUERY_DEFAULT_ROWS: u32 = 100;
/// `eth.query` hard row limit (D5 hard limit 1,000 rows).
pub const QUERY_MAX_ROWS: u32 = 1000;
/// `eth.follow` byte limit.
pub const FOLLOW_MAX_BYTES: u32 = 64 * 1024;
/// Send preview bytes in `eth.tx.hex_preview`.
pub const TX_PREVIEW_BYTES: usize = 64;

/// Origin attributed to a captured frame that Loom itself sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributedOrigin {
    /// Sent by an agent command.
    Agent,
    /// Sent by the user from the UI.
    User,
    /// Sent by a script / runner.
    Script,
}

/// Packet direction relative to the capture host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PktDir {
    /// Received.
    Rx,
    /// Transmitted by this host.
    Tx,
}

/// Timestamp source of a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TsSrc {
    /// libpcap/Npcap host timestamp.
    PcapHost,
    /// Adapter (hardware) timestamp.
    PcapAdapter,
    /// Replayed from a file.
    Replay,
    /// Synthesized by the sim tap.
    Sim,
    /// Written by an external software producer (`stream` backend); not a host capture clock.
    External,
}

/// Link from a packet to a request in some domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PktLink {
    /// Linked domain (`eth` for own sends, `uds` for diagnostic requests).
    pub domain: String,
    /// Request id in that domain.
    pub request_id: String,
    /// `request|ack|nack|response|tx`.
    pub role: String,
    /// `exact|probable`.
    pub confidence: String,
    /// Target name (e.g. UDS target), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// `eth.pkt` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PktPayload {
    /// Capture channel name.
    pub ch: String,
    /// pcap linktype of `bytes`.
    pub linktype: u16,
    /// Original length on the wire.
    pub len: u32,
    /// Captured length.
    pub caplen: u32,
    /// Direction, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PktDir>,
    /// Set when Loom itself sent this frame (never counted as device evidence, TEST R-8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributed_origin: Option<AttributedOrigin>,
    /// Timestamp source.
    pub ts_src: TsSrc,
    /// Top protocol (tshark "Protocol" column).
    pub proto: String,
    /// Source address column.
    pub src: String,
    /// Destination address column.
    pub dst: String,
    /// tshark-style "Info" column.
    pub info: String,
    /// `tcp.stream` / UDP conversation number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<u32>,
    /// Key fields per layer (≤ [`MAX_LAYERS_JSON`]).
    pub layers: Value,
    /// Link known at publish time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<PktLink>,
    /// Source seq of the first TCP segment of a PDU spanning several segments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdu_first_src_seq: Option<U64>,
    /// Frame bytes (`{encoding:"base64", data}` in M1). Replayed frames may also carry
    /// `comments` (user pcapng comments, kept verbatim) and `loom_comment` (the original X10
    /// `loom1 …` string); export writes both back as EPB comments.
    pub bytes: Value,
    /// True when `bytes` was cut to [`MAX_INLINE_FRAME`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bytes_truncated: bool,
}

/// `eth.batch` payload: coverage of a source-seq range of which only `shown` of `total`
/// packets were published individually (`unrecorded` = originals not stored).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchPayload {
    /// Capture channel.
    pub ch: String,
    /// First covered source seq.
    pub covered_first_src_seq: U64,
    /// Last covered source seq.
    pub covered_last_src_seq: U64,
    /// Packets captured in the range.
    pub total: U64,
    /// Packets published individually in the range.
    pub shown: U64,
    /// Packets in the range whose original bytes were stored nowhere (M1: no worker journal
    /// yet, so `total - shown` unless a journal writer recorded them). Never hidden.
    #[serde(default)]
    pub unrecorded: U64,
    /// Count per top protocol.
    pub by_proto: std::collections::BTreeMap<String, U64>,
}

/// `eth.tx` payload (write row; `write_state` per E8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TxPayload {
    /// Capture/send channel.
    pub ch: String,
    /// `requested|accepted|sent|confirmed|failed|outcome_unknown`.
    pub write_state: String,
    /// `raw|udp|tcp`.
    pub kind: String,
    /// Destination `IP:PORT` for socket sends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Payload or frame length.
    pub len: u32,
    /// First [`TX_PREVIEW_BYTES`] bytes as hex.
    pub hex_preview: String,
    /// Source seq of the captured copy, once observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_src_seq: Option<U64>,
}

/// `eth.link` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkPayload {
    /// Packet REF `eth:<ch>@seq:<n>`.
    pub eth_ref: String,
    /// Link (domain, request id, role, confidence, target).
    #[serde(flatten)]
    pub link: PktLink,
}

/// Capture channel state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStateKind {
    /// Opening.
    Starting,
    /// Capturing.
    Live,
    /// Stopped by request.
    Stopped,
    /// Failed (see `error`).
    Error,
    /// No capture permission (Npcap missing, no CAP_NET_RAW, BPF access).
    PermissionDenied,
    /// Replay reached the end of the file.
    ReplayDone,
}

/// `eth.capture_state` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureStatePayload {
    /// Channel.
    pub ch: String,
    /// State.
    pub state: CaptureStateKind,
    /// `live|replay|sim|stream`.
    pub backend: String,
    /// Interface (or replay file name).
    pub iface: String,
    /// BPF capture filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpf: Option<String>,
    /// Snap length.
    pub snaplen: u32,
    /// Promiscuous mode.
    pub promisc: bool,
    /// Requested kernel buffer.
    pub buffer_bytes_requested: U64,
    /// Actual kernel buffer, when the OS reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_bytes_actual: Option<U64>,
    /// `micro|nano`.
    pub ts_precision: String,
    /// `available|partial|unavailable` (never treat unavailable as zero drops).
    pub drop_stats: String,
    /// pcap linktype.
    pub linktype: u16,
    /// Monotonic start time (relative-time origin for the UI).
    pub started_mono_ns: U64,
    /// Error text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why a capture ended (`requested`, `producer_eof`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `eth.stats` payload (1 s).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsPayload {
    /// Channel.
    pub ch: String,
    /// Packets per second captured.
    pub pps: U64,
    /// Bits per second captured.
    pub bps: U64,
    /// Kernel drops in the last second (None = unknown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_drop: Option<U64>,
    /// Interface drops in the last second (None = unknown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_drop: Option<U64>,
    /// Decode lag in milliseconds.
    pub decode_lag_ms: U64,
    /// Packets published individually in the last second.
    pub shown: U64,
    /// Packets captured in the last second.
    pub total: U64,
}

/// `eth.capture.start` params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureStartParams {
    /// Interface name (or replay file label).
    pub iface: String,
    /// Channel name (default = iface).
    #[serde(default)]
    pub ch: Option<String>,
    /// `live|replay|sim`.
    pub backend: String,
    /// BPF capture filter.
    #[serde(default)]
    pub bpf: Option<String>,
    /// Snap length.
    #[serde(default = "default_snaplen")]
    pub snaplen: u32,
    /// Promiscuous mode.
    #[serde(default)]
    pub promisc: bool,
    /// Kernel buffer in MiB.
    #[serde(default = "default_buffer_mib")]
    pub buffer_mib: u32,
    /// Replay options (backend = replay).
    #[serde(default)]
    pub replay: Option<ReplayParams>,
    /// External producer options (backend = stream).
    #[serde(default)]
    pub stream: Option<StreamParams>,
}

/// `stream` backend options: an external pcapng producer on loopback TCP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamParams {
    /// `127.x.x.x:PORT` or `[::1]:PORT`.
    pub connect: String,
    /// Connect + first section/interface bound (ms, 1..=10000).
    #[serde(default = "default_stream_start_ms")]
    pub connect_timeout_ms: u32,
    /// Error when the producer sends nothing for this long (ms, 0 = off, ≤ 600000).
    #[serde(default = "default_stream_idle_ms")]
    pub idle_timeout_ms: u32,
}
fn default_stream_start_ms() -> u32 {
    2000
}
fn default_stream_idle_ms() -> u32 {
    30_000
}

/// Replay options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayParams {
    /// pcapng/pcap file path.
    pub path: String,
    /// `"max"` or a factor (1 = original pace).
    #[serde(default = "default_speed")]
    pub speed: Value,
    /// Loop forever.
    #[serde(default)]
    pub repeat: bool,
}

fn default_snaplen() -> u32 {
    65535
}
fn default_buffer_mib() -> u32 {
    64
}
fn default_speed() -> Value {
    Value::from(1)
}

/// `eth.send` params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendParams {
    /// Channel.
    pub ch: String,
    /// `raw|udp|tcp`.
    pub kind: String,
    /// Bytes as hex.
    pub hex: String,
    /// `IP:PORT` for udp/tcp.
    #[serde(default)]
    pub to: Option<String>,
    /// Source port for udp.
    #[serde(default)]
    pub from_port: Option<u16>,
    /// Preview only: no row, no audit.
    #[serde(default)]
    pub dry_run: bool,
    /// tcp only: keep Loom's own connection open for reuse (≤ 4 per channel, idle 30 s).
    #[serde(default)]
    pub keep_open: bool,
}

/// `eth.query` params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryParams {
    /// Channel.
    pub ch: String,
    /// Cursor range (`from` absent = the latest `max` rows up to `to`).
    #[serde(default)]
    pub range: QueryRange,
    /// Display filter.
    #[serde(default)]
    pub filter: Option<String>,
    /// Row limit (default [`QUERY_DEFAULT_ROWS`], max [`QUERY_MAX_ROWS`]).
    #[serde(default)]
    pub max: Option<u32>,
    /// Include `layers` in rows.
    #[serde(default)]
    pub detail: bool,
}

/// Cursor range for queries (cursor syntax of contract §3: `@last`, `seq:N`, `mark:X`, `-5s`, `now`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryRange {
    /// Start cursor.
    #[serde(default)]
    pub from: Option<String>,
    /// End cursor.
    #[serde(default)]
    pub to: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pkt_payload_roundtrip_and_optional_fields() -> Result<(), serde_json::Error> {
        let p = PktPayload {
            ch: "lan1".into(),
            linktype: 1,
            len: 62,
            caplen: 62,
            dir: Some(PktDir::Tx),
            attributed_origin: Some(AttributedOrigin::Agent),
            ts_src: TsSrc::Sim,
            proto: "DoIP/UDS".into(),
            src: "192.168.0.2".into(),
            dst: "192.168.0.10".into(),
            info: "Diag 0x0E80→0x1001 22 F1 90 ReadDataByIdentifier".into(),
            stream: Some(3),
            layers: json!({"doip": {"type": "0x8001"}}),
            link: None,
            pdu_first_src_seq: None,
            bytes: json!({"encoding": "base64", "data": ""}),
            bytes_truncated: false,
        };
        let v = serde_json::to_value(&p)?;
        assert!(v.get("link").is_none());
        assert!(v.get("bytes_truncated").is_none());
        assert_eq!(v["attributed_origin"], "agent");
        assert_eq!(v["ts_src"], "sim");
        let back: PktPayload = serde_json::from_value(v)?;
        assert_eq!(back, p);
        Ok(())
    }

    #[test]
    fn capture_start_defaults() -> Result<(), serde_json::Error> {
        let p: CaptureStartParams = serde_json::from_value(json!({"iface": "lo", "backend": "sim"}))?;
        assert_eq!(p.snaplen, 65535);
        assert_eq!(p.buffer_mib, 64);
        assert!(!p.promisc);
        Ok(())
    }
}
