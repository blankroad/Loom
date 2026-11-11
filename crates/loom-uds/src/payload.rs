//! Event payload types (`payload` of `uds.*` events) and the dry-run result.
//!
//! Canonical for the UI (`docs/impl/requests/ui-uds.md` "UDS 답변", reconciled in P1-a — see
//! `docs/impl/requests/uds-ui.md`). JSON Schema is generated from these types and
//! `crates/loom-uds/fixtures/*.json` are generated from them (`tests/fixtures.rs`).
//!
//! **Row events** (`uds.tx`, `uds.pending`, `uds.result`) all carry the same self-sufficient
//! [`RowPayload`]: the UI row store keys rows by `request_id` and *replaces* the payload with the
//! latest event of a higher-or-equal `write_state` stage, so every row event repeats the request
//! fields. Raw received PDUs (`uds.rx`) carry no `write_state` and never touch the row (a late
//! response is recorded there only). Bytes travel as `payload.bytes{encoding:"hex", data}`
//! (request) and `payload.resp_bytes` (response). Monotonic times are gateway `mono_ns` as
//! decimal strings (D3); compare via `core.time`, wall is display only.

use loom_proto::event::{Origin, PayloadBytes, Risk, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::{BlockClass, PolicyAction};
use loom_uds_codec::did::standard_did;
use loom_uds_codec::{DidValue, nrc_name};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::risk::{UdsClassification, classify_pdu};
use crate::session::session_name;
use crate::text::ParseError;

/// Link state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Up,
    Down,
    Connecting,
}

/// How sure Loom is about a believed ECU state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Seen in a response.
    Confirmed,
    /// Inferred (after connect, reset, suppressed response).
    Assumed,
    /// Probably out of date (S3 elapsed, link lost, NRC 0x7E/0x7F).
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LinkPayload {
    pub state: LinkState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionPayload {
    pub id: u8,
    pub name: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SecurityPayload {
    /// Unlocked level (requestSeed sub-function), `null` = locked.
    pub level: Option<u8>,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TesterPresentPayload {
    pub active: bool,
    pub interval_ms: u32,
    /// Positive / suppressed-ok TesterPresent exchanges since it was started.
    pub ok_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LeasePayload {
    pub owner_origin: Origin,
    /// Viewer account id for users, LOOM_CLIENT for agents.
    pub actor_id: String,
    /// Connection id (conn_id).
    pub client_id: String,
}

/// `uds.state` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StatePayload {
    pub target: String,
    pub generation: u32,
    /// `sim | doip | cantp | canoe`, `null` before connect.
    pub backend: Option<String>,
    /// Display address (`DoIP 0x1001`, `CAN 7E0/7E8`).
    pub addr: Option<String>,
    pub link: LinkPayload,
    pub session: SessionPayload,
    pub security: SecurityPayload,
    pub tester_present: Option<TesterPresentPayload>,
    /// Active P2server_max / P2*server_max limits (ms).
    pub p2_ms: u16,
    pub p2_star_ms: u32,
    pub lease: Option<LeasePayload>,
}

/// Why a request was refused before sending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BlockPayload {
    pub class: BlockClass,
    pub reason: String,
}

/// One decoded DID value of a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DecodedDid {
    /// 4 hex digits, upper case (`F190`).
    pub did: String,
    pub name: Option<String>,
    /// String (text, hex, BCD, enum name) or number (scaled uint).
    pub value: Value,
    pub unit: Option<String>,
}

impl DecodedDid {
    pub fn new(did: u16, name: Option<String>, v: &DidValue) -> Self {
        let (value, unit) = match v {
            DidValue::Text { text } => (json!(text), None),
            DidValue::Hex { hex, ascii } => (json!(ascii.clone().unwrap_or_else(|| hex.clone())), None),
            DidValue::Number { value, unit, .. } => (json!(value), unit.clone()),
            DidValue::Bcd { digits } => (json!(digits), None),
            DidValue::Enum { raw, name } => {
                (json!(name.clone().unwrap_or_else(|| format!("0x{raw:X}"))), None)
            }
        };
        Self {
            did: format!("{did:04X}"),
            name,
            value,
            unit,
        }
    }
}

/// One DTC of a `19 02` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DtcItem {
    /// Raw 24-bit DTC, 6 hex digits (always kept).
    pub dtc: String,
    pub status: u8,
}

/// Payload of every row event (`uds.tx`, `uds.pending`, `uds.result`) of one `request_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RowPayload {
    pub target: String,
    /// A14 stage. `uds.tx`: requested/accepted/sent, `uds.pending`: sent, `uds.result`: final.
    pub write_state: WriteState,
    /// Request bytes.
    pub bytes: PayloadBytes,
    pub sid: u8,
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subfunction: Option<u8>,
    /// One-line summary (`ReadDataByIdentifier F190 VIN`).
    pub summary: String,
    pub suppress_pos: bool,
    pub functional: bool,
    pub risk: Risk,
    pub reason: String,
    pub policy_action: PolicyAction,
    /// `7F <sid> 78` received so far.
    pub pending_count: u32,
    /// Active P2 / P2* limits for this request (ms) — the in-flight timer limit.
    pub p2_ms: u16,
    pub p2_star_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t_send_mono_ns: Option<U64>,
    // ---- final fields (uds.result) ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resp_bytes: Option<PayloadBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nrc: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nrc_name: Option<String>,
    /// Read timeout → `failed`; write or link loss → `outcome_unknown`. Never resent.
    #[serde(default)]
    pub timeout: bool,
    /// SPRMIB request with no negative response: `write_state = sent`.
    #[serde(default)]
    pub ok_suppressed: bool,
    /// Refused before sending (`write_state = failed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockPayload>,
    /// Measured send → first response byte (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_rx_ms: Option<f64>,
    /// Measured send → final response (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t_first_rx_mono_ns: Option<U64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t_final_rx_mono_ns: Option<U64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoded: Option<Vec<DecodedDid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dtcs: Option<Vec<DtcItem>>,
}

/// One-line summary of a request (shown in the row's summary column).
pub fn summarize(bytes: &[u8], c: &UdsClassification) -> String {
    let service = c.service.unwrap_or("unknown service");
    let did_at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let did_label = |d: u16| match standard_did(d) {
        Some(def) => format!("{d:04X} {}", def.name),
        None => format!("{d:04X}"),
    };
    match bytes.first() {
        Some(0x22) => {
            let dids: Vec<String> = bytes[1..]
                .chunks_exact(2)
                .map(|p| did_label(u16::from_be_bytes([p[0], p[1]])))
                .collect();
            format!("{service} {}", dids.join(", "))
        }
        Some(0x2E) => match did_at(1) {
            Some(d) => format!("{service} {} = {}", did_label(d), hex::encode_upper(&bytes[3..])),
            None => service.to_string(),
        },
        Some(0x10) => c
            .subfunction
            .map_or(service.to_string(), |s| format!("{service} {}", session_name(s))),
        Some(0x19) => match bytes.get(2) {
            Some(m) => format!("{service} byStatusMask 0x{m:02X}"),
            None => service.to_string(),
        },
        _ => match c.subfunction {
            Some(s) => format!("{service} 0x{s:02X}"),
            None => service.to_string(),
        },
    }
}

impl RowPayload {
    /// Row fields known before sending (the core fills `request_id` and the event envelope).
    pub fn request(
        target: &str,
        bytes: &[u8],
        functional: bool,
        write_state: WriteState,
        timing: (u16, u32),
    ) -> Self {
        let c = classify_pdu(bytes, functional);
        let policy_action = if c.block.is_some() {
            PolicyAction::Deny
        } else {
            PolicyAction::Auto
        };
        RowPayload {
            target: target.into(),
            write_state,
            bytes: PayloadBytes::hex(bytes),
            sid: c.sid.unwrap_or(0),
            service: c.service.map(Into::into),
            subfunction: c.subfunction,
            summary: summarize(bytes, &c),
            suppress_pos: c.suppress_pos,
            functional,
            risk: c.risk,
            reason: c.reason.clone(),
            policy_action,
            pending_count: 0,
            p2_ms: timing.0,
            p2_star_ms: timing.1,
            t_send_mono_ns: None,
            resp_bytes: None,
            nrc: None,
            nrc_name: None,
            timeout: false,
            ok_suppressed: false,
            block: c.block.map(|class| BlockPayload {
                class,
                reason: c.reason,
            }),
            first_rx_ms: None,
            rtt_ms: None,
            t_first_rx_mono_ns: None,
            t_final_rx_mono_ns: None,
            decoded: None,
            dtcs: None,
        }
    }

    /// Set the NRC and its ISO name together.
    pub fn with_nrc(mut self, nrc: u8) -> Self {
        self.nrc = Some(nrc);
        self.nrc_name = nrc_name(nrc).map(Into::into);
        self
    }
}

/// What a received PDU was for the request in flight (`loom_uds_codec::ResponseKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RxKind {
    Positive,
    Negative,
    Pending,
    Foreign,
    Malformed,
}

/// `uds.rx` payload: one received PDU (pendings, late responses included). No `write_state`:
/// it never creates or reopens a row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RxPayload {
    pub target: String,
    pub bytes: PayloadBytes,
    pub kind: RxKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nrc: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nrc_name: Option<String>,
    /// Arrived after the request had already ended (quarantine): recorded only.
    pub late: bool,
    pub t_rx_mono_ns: U64,
}

/// Result of `uds.request{dry_run:true}`. Creates no row, event, audit or journal entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DryRunResult {
    pub dry_run: bool,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<PayloadBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subfunction: Option<u8>,
    /// Name of the (first) DID for 0x22/0x2E when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub suppress_pos: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<Risk>,
    pub reason: String,
    pub policy_action: PolicyAction,
    pub block: Option<BlockClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<ParseError>,
}

impl RxKind {
    pub fn from_codec(k: loom_uds_codec::ResponseKind) -> (Self, Option<u8>) {
        use loom_uds_codec::ResponseKind as K;
        match k {
            K::Positive => (RxKind::Positive, None),
            K::Negative { nrc } => (RxKind::Negative, Some(nrc)),
            K::Pending => (RxKind::Pending, Some(loom_uds_codec::NRC_RESPONSE_PENDING)),
            K::Foreign { .. } => (RxKind::Foreign, None),
            K::Malformed { .. } => (RxKind::Malformed, None),
        }
    }
}
