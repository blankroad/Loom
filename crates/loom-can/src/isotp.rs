//! ISO-TP endpoints in the `can_sim` worker (K10 minimal, M2a; contract `_xteam/can-uds-isotp.md`,
//! "v1.1 이름·상태 정본"). Separate TX/RX sans-IO engines allow cancellation of only one TX.
//! Each endpoint is driven by
//! the worker pump exactly as the engine README says: feed matching frames, drain **events, then
//! frames** (so `can.isotp_rx_start` is published before its FC goes out), submit a frame only when it
//! is still pending and due, report the submission, wake at `poll_at`. The sim backend gives no TX
//! confirmations, so every transfer is released after `DriverAccepted` (`tx_state: driver_accepted`).
//!
//! Who sends: a transfer's frames go out with the **request's** origin (agent/user/script sender of
//! the channel), FC frames with the origin of the endpoint's last request. Server endpoints (sim ECU,
//! [`IsoTpServerModel`]) send as `device` and are never latched by STOP.
//!
//! Bounds (C2): [`MAX_ENDPOINTS_PER_CHANNEL`] / [`MAX_ENDPOINTS`]; per endpoint ≤ [`MAX_PENDING_FRAMES`]
//! frames waiting for their send time, ≤ [`MAX_TRANSFER_TAGS`] transfer→request tags (oldest evicted,
//! counted), ≤ [`MAX_REPLY_QUEUE`] scheduled model replies (more dropped, counted), ≤
//! [`MAX_REPLIES_PER_PDU`] replies per request, reply delay ≤ [`MAX_REPLY_DELAY`]; engine queues and
//! PDU sizes are the engine's own hard limits (4095-byte PDU). Completed PDU retention is at most
//! [`MAX_COMPLETED_PDUS`] bounded rows; recv replies respect [`MAX_RECV_REPLY_BYTES`].

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use loom_can_frame::{CanFrame, CanId};
use loom_can_isotp::{
    Engine, Error as TpError, Event, FaultPlan, FrameRequest, IsoTpConfig, Pci, TransferId, TxState, decode,
};
use loom_can_sim::{EndpointId, SimHub};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Env, Origin, PayloadBytes, TsSource};
use loom_worker_sdk::{Emitter, EventDraft};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::schema::IsoTpTag;
use crate::{
    KIND_ISOTP_RX_COMPLETE, KIND_ISOTP_RX_ERROR, KIND_ISOTP_RX_START, KIND_ISOTP_STATE, KIND_ISOTP_TX_DONE,
    KIND_ISOTP_TX_ERROR,
};

/// Endpoints per channel (`can.isotp.open` → `E_BUSY` above).
pub const MAX_ENDPOINTS_PER_CHANNEL: usize = 32;
/// Endpoints per worker.
pub const MAX_ENDPOINTS: usize = 128;
/// Frames popped from the engine and waiting for `not_before_ns` / submission.
pub const MAX_PENDING_FRAMES: usize = 64;
/// Transfer → request tags kept per endpoint (one active TX plus finished ones awaiting events).
pub const MAX_TRANSFER_TAGS: usize = 8;
/// Scheduled server-model replies per endpoint.
pub const MAX_REPLY_QUEUE: usize = 16;
/// Replies taken from one model call.
pub const MAX_REPLIES_PER_PDU: usize = 8;
/// Longest model reply delay (longer delays are clamped).
pub const MAX_REPLY_DELAY: Duration = Duration::from_secs(60);
/// Longest N_Bs / N_Cr accepted from `can.isotp.open`.
pub const MAX_TIMEOUT_MS: u32 = 60_000;
/// Completed PDU retention per endpoint; overflow evicts the oldest and increments `dropped`.
pub const MAX_COMPLETED_PDUS: usize = 32;
/// Maximum PDUs returned by one nonblocking recv call.
pub const MAX_RECV_PDUS: usize = 16;
/// Leave space in the 64 KiB worker envelope even for maximum-size hex PDUs.
pub const MAX_RECV_REPLY_BYTES: usize = 48 * 1024;
/// Maximum serialized retained PDU row (including bounded provenance metadata).
pub const MAX_COMPLETED_ROW_BYTES: usize = 12 * 1024;
/// Frames one endpoint submits per drive step (the pump comes back at once if more are due).
const MAX_SUBMIT_PER_DRIVE: usize = 64;

/// A virtual ECU behind a server endpoint (same shape as UDS `SimEcu::handle(pdu, now) -> Replies`):
/// a request PDU in, `(delay, response PDU)` pairs out. No I/O, no threads, deterministic.
pub trait IsoTpServerModel: Send {
    fn handle(&mut self, pdu: &[u8], now_ns: u64) -> Vec<(Duration, Vec<u8>)>;
}

/// Builds a fresh model for each server endpoint that names it.
pub type ModelFactory = Arc<dyn Fn() -> Box<dyn IsoTpServerModel> + Send + Sync>;

/// Built-in test model `vin_demo`: `22 F1 90` → `62 F1 90` + 17-byte VIN (20 bytes, multi-frame on
/// Classic CAN); anything else → `7F <sid> 11` (serviceNotSupported). Replies after [`VinDemo::DELAY`].
#[derive(Debug, Clone)]
pub struct VinDemo {
    pub vin: [u8; 17],
}

impl VinDemo {
    /// Reply delay (a fast ECU).
    pub const DELAY: Duration = Duration::from_millis(2);
    /// VIN answered by default.
    pub const VIN: &'static [u8; 17] = b"LOOMSIM0000000001";
}

impl Default for VinDemo {
    fn default() -> Self {
        Self { vin: *Self::VIN }
    }
}

impl IsoTpServerModel for VinDemo {
    fn handle(&mut self, pdu: &[u8], _now_ns: u64) -> Vec<(Duration, Vec<u8>)> {
        let reply = match pdu {
            [0x22, 0xF1, 0x90] => {
                let mut r = vec![0x62, 0xF1, 0x90];
                r.extend_from_slice(&self.vin);
                r
            }
            [sid, ..] => vec![0x7F, *sid, 0x11],
            [] => return Vec::new(),
        };
        vec![(Self::DELAY, reply)]
    }
}

/// Built-in transport test model `echo`: answers every PDU with the same bytes at once.
#[derive(Debug, Default, Clone)]
pub struct Echo;

impl IsoTpServerModel for Echo {
    fn handle(&mut self, pdu: &[u8], _now_ns: u64) -> Vec<(Duration, Vec<u8>)> {
        vec![(Duration::ZERO, pdu.to_vec())]
    }
}

/// Built-in models by name.
pub fn builtin_model(name: &str) -> Option<Box<dyn IsoTpServerModel>> {
    match name {
        "vin_demo" => Some(Box::new(VinDemo::default())),
        "echo" => Some(Box::new(Echo)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------------------------

/// Endpoint role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Client,
    /// Sim ECU side (R-TP-12): sends as `device`, keeps running under STOP.
    Server,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Client => "client",
            Role::Server => "server",
        }
    }
}

/// Keeps `null` distinct from "absent" (`padding: null` = no padding).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// Simulation faults (R-TP-13 minimum). Sim worker only.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultArgs {
    /// 1-based CF number this endpoint drops when sending.
    #[serde(default)]
    pub drop_cf: Option<u16>,
    /// 1-based CF number sent with a wrong sequence number.
    #[serde(default)]
    pub wrong_sn_cf: Option<u16>,
    /// Delay of this endpoint's FC (ms, ≤ 60000).
    #[serde(default)]
    pub fc_delay_ms: u32,
    /// WAIT frames before each CTS (≤ 32).
    #[serde(default)]
    pub fc_wait_count: u8,
    /// Answer every FF with FC OVFLW.
    #[serde(default)]
    pub fc_overflow: bool,
}

/// `can.isotp.open` arguments.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    pub channel: String,
    pub tx_id: u32,
    pub rx_id: u32,
    /// 29-bit ids (also implied by an id above 0x7FF).
    #[serde(default)]
    pub ext: bool,
    #[serde(default)]
    pub fd: bool,
    /// Bit rate switch (FD only).
    #[serde(default)]
    pub brs: bool,
    /// TX data length: 8 (CC), 8..64 for FD (default 8 / 64).
    #[serde(default)]
    pub tx_dl: Option<u8>,
    /// Padding byte; `null` = none. Default 0xCC (contract can 의견 §8-2).
    #[serde(default, deserialize_with = "present")]
    pub padding: Option<Value>,
    /// BS we ask for in our FC (default 8).
    #[serde(default)]
    pub block_size: Option<u8>,
    /// STmin we ask for in our FC (0x00..=0x7F ms, 0xF1..=0xF9 = 100..900 µs; default 0).
    #[serde(default)]
    pub st_min: Option<u8>,
    #[serde(default)]
    pub n_bs_ms: Option<u32>,
    #[serde(default)]
    pub n_cr_ms: Option<u32>,
    /// Largest PDU (≤ 4095).
    #[serde(default)]
    pub max_pdu: Option<usize>,
    #[serde(default)]
    pub role: Role,
    /// Free-form owner label (e.g. `uds:session-3`), shown in `list` and `E_LOCKED`.
    #[serde(default)]
    pub owner: Option<String>,
    /// Server model name (`vin_demo`, `echo`, or one registered on the worker).
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub faults: Option<FaultArgs>,
}

fn usage(msg: impl Into<String>) -> (ErrorCode, String) {
    (ErrorCode::Usage, msg.into())
}

fn can_id(raw: u32, ext: bool) -> Result<CanId, (ErrorCode, String)> {
    let r = if ext {
        CanId::extended(raw)
    } else {
        CanId::standard(raw)
    };
    r.map_err(|e| usage(format!("bad CAN id {raw:#X}: {e}")))
}

/// `0x7E0` / `0x18DA10F1`.
pub fn id_text(id: CanId) -> String {
    if id.is_extended() {
        format!("0x{:08X}", id.raw())
    } else {
        format!("0x{:03X}", id.raw())
    }
}

impl OpenParams {
    /// Validated engine configuration.
    pub fn config(&self) -> Result<IsoTpConfig, (ErrorCode, String)> {
        let ext = self.ext || self.tx_id > 0x7FF || self.rx_id > 0x7FF;
        let (tx, rx) = (can_id(self.tx_id, ext)?, can_id(self.rx_id, ext)?);
        if tx == rx {
            return Err(usage("tx_id and rx_id must differ"));
        }
        let mut cfg = IsoTpConfig::new(tx, rx);
        cfg.can_fd = self.fd;
        cfg.bitrate_switch = self.brs;
        cfg.tx_dl = self.tx_dl.unwrap_or(if self.fd { 64 } else { 8 });
        cfg.padding = match &self.padding {
            None => Some(0xCC),
            Some(Value::Null) => None,
            Some(Value::String(s)) if s == "none" => None,
            Some(v) => Some(
                v.as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(|| usage("padding must be a byte 0..=255, null or \"none\""))?,
            ),
        };
        if let Some(bs) = self.block_size {
            cfg.block_size = bs;
        }
        if let Some(st) = self.st_min {
            cfg.st_min = st;
        }
        for (name, v) in [("n_bs_ms", self.n_bs_ms), ("n_cr_ms", self.n_cr_ms)] {
            if v.is_some_and(|ms| ms > MAX_TIMEOUT_MS) {
                return Err(usage(format!("{name} must be ≤ {MAX_TIMEOUT_MS}")));
            }
        }
        cfg.n_bs_ms = self.n_bs_ms.unwrap_or(cfg.n_bs_ms);
        cfg.n_cr_ms = self.n_cr_ms.unwrap_or(cfg.n_cr_ms);
        if let Some(m) = self.max_pdu {
            cfg.max_pdu = m;
        }
        if let Some(f) = &self.faults {
            cfg.faults = FaultPlan {
                drop_cf_number: f.drop_cf,
                wrong_sn_cf_number: f.wrong_sn_cf,
                fc_delay_ms: f.fc_delay_ms,
                fc_wait_count: f.fc_wait_count,
                fc_overflow: f.fc_overflow,
            };
        }
        cfg.validate()
            .map_err(|e| usage(format!("invalid ISO-TP parameters: {e}")))?;
        if self.role == Role::Client && self.model.is_some() {
            return Err(usage("`model` needs role \"server\""));
        }
        Ok(cfg)
    }
}

/// `can.isotp.send` write op (`{"method": "can.isotp.send", endpoint, pdu}`).
#[derive(Debug, Clone, Deserialize)]
pub struct SendOp {
    #[serde(default)]
    pub method: Option<String>,
    pub endpoint: String,
    /// PDU as hex (`22F190`, `22 F1 90`).
    pub pdu: String,
}

/// Hex → bytes (spaces, `.` and `:` ignored).
pub fn parse_pdu(s: &str) -> Result<Vec<u8>, String> {
    let clean: String = s.chars().filter(|c| !matches!(c, ' ' | '.' | ':')).collect();
    if clean.is_empty() || !clean.len().is_multiple_of(2) || clean.len() > 2 * loom_can_isotp::MAX_PDU {
        return Err(format!(
            "pdu must be 1..={} bytes of hex",
            loom_can_isotp::MAX_PDU
        ));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|_| format!("bad hex in pdu: {s:?}")))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Endpoint
// ---------------------------------------------------------------------------------------------

/// Who asked for a transfer.
#[derive(Debug, Clone)]
pub(crate) struct ReqMeta {
    pub request_id: Option<String>,
    pub client_id: Option<String>,
    pub origin: Origin,
}

#[derive(Debug)]
struct Transfer {
    id: TransferId,
    req: ReqMeta,
    frames: u32,
    first_ns: Option<u64>,
    last_ns: Option<u64>,
}

/// Frames of the receive in progress. Seqs are the `source_seq` of their `can.frame` events (same
/// producer), `None` if that event could not be emitted.
#[derive(Debug, Clone, Copy)]
struct RxTrack {
    first_seq: Option<u64>,
    last_seq: Option<u64>,
    frames: u32,
    first_ns: u64,
    last_ns: u64,
}

/// Counters shown by `can.isotp.list`.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Counters {
    tx_pdu: u64,
    rx_pdu: u64,
    tx_err: u64,
    rx_err: u64,
    frames_tx: u64,
    frames_rx: u64,
    frames_stale: u64,
    cancelled_frames: u64,
    unconfirmed_released: u64,
    tags_evicted: u64,
    replies: u64,
    replies_dropped: u64,
    reply_errors: u64,
    ingress_errors: u64,
    events_failed: u64,
    send_failed: u64,
}

/// A frame this endpoint put on the bus through a Loom sender (for its echo tag).
#[derive(Debug)]
pub(crate) struct Submitted {
    pub seq: u64,
    pub tag: IsoTpTag,
    pub request_id: Option<String>,
    pub client_id: Option<String>,
}

/// What an endpoint needs from its channel while driving.
pub(crate) struct Io<'a> {
    pub hub: &'a mut SimHub,
    /// Agent / user / script senders (worker `SENDER_ORIGINS` order).
    pub senders: [EndpointId; 3],
    pub device_sender: Option<EndpointId>,
    pub emitter: &'a Emitter,
    pub now: u64,
}

/// One open ISO-TP endpoint.
pub(crate) struct Endpoint {
    pub id: String,
    pub channel: String,
    pub generation: u32,
    pub epoch: u64,
    pub role: Role,
    pub opener: Origin,
    pub owner: Option<String>,
    pub client_id: Option<String>,
    pub model_name: Option<String>,
    model: Option<Box<dyn IsoTpServerModel>>,
    engine: Engine,
    /// Only receives data and generates FC; never owns an outgoing PDU.
    rx_engine: Engine,
    pending: VecDeque<FrameRequest>,
    transfers: VecDeque<Transfer>,
    replies: VecDeque<(u64, Vec<u8>)>,
    /// Origin / request of the last send: used for FC frames and receive events.
    last: ReqMeta,
    /// Origin of the transfer in progress.
    active: Option<Origin>,
    rx: Option<RxTrack>,
    /// A Single Frame was just fed: publish a synthetic `rx_start` before its `rx_complete`.
    sf_len: Option<usize>,
    completed: VecDeque<(u64, Value, usize)>,
    pdu_seq: u64,
    dropped: u64,
    transport_event_seq: u64,
    proof_failed: bool,
    cancelling: Option<TransferId>,
    stopped: bool,
    c: Counters,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("id", &self.id)
            .field("channel", &self.channel)
            .field("epoch", &self.epoch)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

fn sender_index(o: Origin) -> Option<usize> {
    match o {
        Origin::Agent => Some(0),
        Origin::User => Some(1),
        Origin::Script => Some(2),
        Origin::Device => None,
    }
}

fn stoppable(o: Origin) -> bool {
    matches!(o, Origin::Agent | Origin::Script)
}

/// Wire code of an engine error (contract §3, v0.2).
pub fn error_code(e: TpError) -> &'static str {
    match e {
        TpError::NBsTimeout => "N_BS_TIMEOUT",
        TpError::NCrTimeout => "N_CR_TIMEOUT",
        TpError::WrongSequence { .. } => "WRONG_SN",
        TpError::UnexpectedPdu => "UNEXPECTED_PDU",
        TpError::FlowOverflow => "FC_OVERFLOW",
        TpError::PduTooLarge { .. } => "OVERFLOW",
        TpError::WaitLimit => "WFT_OVRN",
        TpError::Aborted => "ABORTED",
        TpError::InvalidPci => "INVALID_PCI",
        TpError::InvalidLength => "INVALID_LENGTH",
        TpError::InvalidFlowStatus => "INVALID_FLOW_STATUS",
        TpError::Busy => "BUSY",
        _ => "INTERNAL",
    }
}

fn transfer_text(t: TransferId) -> String {
    format!("{}.{}", t.generation, t.sequence)
}

impl Endpoint {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: String,
        channel: String,
        generation: u32,
        epoch: u64,
        p: &OpenParams,
        cfg: IsoTpConfig,
        opener: Origin,
        client_id: Option<String>,
        model: Option<Box<dyn IsoTpServerModel>>,
    ) -> Result<Self, (ErrorCode, String)> {
        let rx_engine =
            Engine::new(cfg.clone()).map_err(|e| usage(format!("invalid ISO-TP parameters: {e}")))?;
        let engine = Engine::new(cfg).map_err(|e| usage(format!("invalid ISO-TP parameters: {e}")))?;
        Ok(Self {
            id,
            channel,
            generation,
            epoch,
            role: p.role,
            opener,
            owner: p.owner.clone(),
            client_id: client_id.clone(),
            model_name: p.model.clone(),
            model,
            engine,
            rx_engine,
            pending: VecDeque::new(),
            transfers: VecDeque::new(),
            replies: VecDeque::new(),
            last: ReqMeta {
                request_id: None,
                client_id,
                origin: if p.role == Role::Server {
                    Origin::Device
                } else {
                    opener
                },
            },
            active: None,
            rx: None,
            sf_len: None,
            completed: VecDeque::new(),
            pdu_seq: 0,
            dropped: 0,
            transport_event_seq: 0,
            proof_failed: false,
            cancelling: None,
            stopped: false,
            c: Counters::default(),
        })
    }

    pub(crate) fn tx_id(&self) -> CanId {
        self.engine.config().tx_id
    }

    pub(crate) fn rx_id(&self) -> CanId {
        self.engine.config().rx_id
    }

    /// Annotation for raw frames of this endpoint.
    pub(crate) fn tag(&self) -> IsoTpTag {
        IsoTpTag {
            endpoint: self.id.clone(),
            request_id: self.last.request_id.clone(),
        }
    }

    fn sender(&self, io: &Io<'_>, origin: Origin) -> Option<EndpointId> {
        if self.role == Role::Server {
            return io.device_sender;
        }
        sender_index(origin).map(|i| io.senders[i])
    }

    /// Next instant this endpoint has work (engine timer, a frame's send time, a model reply).
    pub(crate) fn next_wake(&self) -> Option<u64> {
        let frames = self.pending.iter().map(|f| f.not_before_ns).min();
        let reply = self.replies.front().map(|r| r.0);
        [self.engine.poll_at(), self.rx_engine.poll_at(), frames, reply]
            .into_iter()
            .flatten()
            .min()
    }

    // ---- events ---------------------------------------------------------------------------

    fn base(&self) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        m.insert("endpoint".into(), json!(self.id));
        m.insert("epoch".into(), json!(self.epoch));
        m.insert("channel".into(), json!(self.channel));
        m
    }

    fn emit(&mut self, em: &Emitter, kind: &str, dir: Option<Dir>, req: &ReqMeta, extra: Value) {
        let mut payload = self.base();
        if let Value::Object(e) = extra {
            payload.extend(e);
        }
        let d = EventDraft {
            kind: kind.into(),
            ts: em.clock().ts(TsSource::Synthetic),
            dir,
            origin: req.origin,
            client_id: req.client_id.clone(),
            request_id: req.request_id.clone(),
            job_id: None,
            generation: Some(self.generation),
            purpose: None,
            risk: None,
            env: Some(Env::Sim),
            payload: Value::Object(payload),
            metrics: vec![],
            quality: vec![],
        };
        // Priority lane (R-TP-15): PDU events never wait behind raw frame batches.
        match em.priority(d) {
            Ok(seq) => self.transport_event_seq = seq.get(),
            Err(_) => {
                self.c.events_failed += 1;
                self.proof_failed = true;
            }
        }
    }

    fn mono(em: &Emitter, now: u64, at: u64) -> String {
        crate::worker::ts_at(em, now, at, TsSource::Synthetic)
            .mono_ns
            .get()
            .to_string()
    }

    fn rx_meta(&self) -> ReqMeta {
        ReqMeta {
            origin: Origin::Device,
            ..self.last.clone()
        }
    }

    pub(crate) fn emit_state(&mut self, em: &Emitter, state: &str, reason: Option<&str>) {
        let req = ReqMeta {
            request_id: None,
            client_id: self.client_id.clone(),
            origin: self.opener,
        };
        let extra = json!({
            "state": state, "reason": reason, "role": self.role.as_str(),
            "tx_id": id_text(self.tx_id()), "rx_id": id_text(self.rx_id()),
        });
        self.emit(em, KIND_ISOTP_STATE, None, &req, extra);
    }

    fn take_transfer(&mut self, id: TransferId) -> Option<Transfer> {
        let i = self.transfers.iter().position(|t| t.id == id)?;
        self.transfers.remove(i)
    }

    fn on_event(&mut self, ev: Event, em: &Emitter, now: u64) {
        match ev {
            Event::TxDone {
                transfer_id,
                tx_state,
            } => {
                let t = self.take_transfer(transfer_id);
                let req = t.as_ref().map_or_else(|| self.last.clone(), |t| t.req.clone());
                let mut unconfirmed = 0;
                if tx_state == TxState::DriverAccepted {
                    // Sim gives no TX confirmations: release now (engine README step "no confirms").
                    unconfirmed = self.engine.release_transfer(transfer_id).unwrap_or(0);
                    self.c.unconfirmed_released += unconfirmed as u64;
                }
                self.active = None;
                self.c.tx_pdu += 1;
                let state = match tx_state {
                    TxState::DriverAccepted => "driver_accepted",
                    TxState::BusConfirmed => "bus_confirmed",
                };
                let extra = json!({
                    "transfer": transfer_text(transfer_id),
                    "tx_state": state,
                    "frames": t.as_ref().map_or(0, |t| t.frames),
                    "ts_first_frame": t.as_ref().and_then(|t| t.first_ns).map(|a| Self::mono(em, now, a)),
                    "ts_last_frame": t.as_ref().and_then(|t| t.last_ns).map(|a| Self::mono(em, now, a)),
                    "ts_source": "synthetic",
                    "unconfirmed_frames": unconfirmed,
                });
                self.emit(em, KIND_ISOTP_TX_DONE, Some(Dir::Tx), &req, extra);
            }
            Event::TxError {
                transfer_id,
                error,
                residual_tx_possible,
            } => {
                let t = self.take_transfer(transfer_id);
                let req = t.map_or_else(|| self.last.clone(), |t| t.req);
                let _ = self.engine.release_transfer(transfer_id);
                self.active = None;
                self.c.tx_err += 1;
                let extra = json!({
                    "transfer": transfer_text(transfer_id),
                    "code": if self.cancelling == Some(transfer_id) { "CANCELLED" } else { error_code(error) },
                    "detail": format!("{error:?}"),
                    "residual_tx_possible": residual_tx_possible,
                });
                self.emit(em, KIND_ISOTP_TX_ERROR, Some(Dir::Tx), &req, extra);
            }
            Event::RxStart { expected_length, .. } => {
                let rx = self.rx;
                let extra = json!({
                    "expected_length": expected_length,
                    "rx_id": id_text(self.rx_id()),
                    "first_seq": rx.and_then(|r| r.first_seq),
                    "ts_first_frame": rx.map(|r| Self::mono(em, now, r.first_ns)),
                    "ts_source": "synthetic",
                });
                self.emit(em, KIND_ISOTP_RX_START, Some(Dir::Rx), &self.rx_meta(), extra);
            }
            Event::RxComplete { pdu, .. } => {
                let rx = self.rx.take();
                if let Some(len) = self.sf_len.take() {
                    // A Single Frame is its own first frame: same `rx_start` as for an FF (contract v0.2 §9-3).
                    let extra = json!({
                        "expected_length": len,
                        "rx_id": id_text(self.rx_id()),
                        "first_seq": rx.and_then(|r| r.first_seq),
                        "ts_first_frame": rx.map(|r| Self::mono(em, now, r.first_ns)),
                        "ts_source": "synthetic",
                        "single_frame": true,
                    });
                    self.emit(em, KIND_ISOTP_RX_START, Some(Dir::Rx), &self.rx_meta(), extra);
                }
                self.c.rx_pdu += 1;
                let extra = json!({
                    "pdu_seq": self.pdu_seq.saturating_add(1).to_string(),
                    "pdu": PayloadBytes::hex(&pdu),
                    "len": pdu.len(),
                    "rx_id": id_text(self.rx_id()),
                    "first_seq": rx.and_then(|r| r.first_seq),
                    "last_seq": rx.and_then(|r| r.last_seq),
                    "frames": rx.map_or(0, |r| r.frames),
                    "ts_first_frame": rx.map(|r| Self::mono(em, now, r.first_ns)),
                    "ts_last_frame": rx.map(|r| Self::mono(em, now, r.last_ns)),
                    "ts_source": "synthetic",
                });
                self.retain_pdu(extra.clone());
                self.emit(em, KIND_ISOTP_RX_COMPLETE, Some(Dir::Rx), &self.rx_meta(), extra);
                self.on_request(&pdu, now);
            }
            Event::RxError { error, .. } => {
                self.rx = None;
                self.sf_len = None;
                self.c.rx_err += 1;
                let extra = json!({"code": error_code(error), "detail": format!("{error:?}")});
                self.emit(em, KIND_ISOTP_RX_ERROR, Some(Dir::Rx), &self.rx_meta(), extra);
            }
        }
    }

    /// Server role: ask the model and schedule its replies (bounded, sorted by due time).
    fn on_request(&mut self, pdu: &[u8], now: u64) {
        let Some(model) = self.model.as_mut() else { return };
        for (delay, bytes) in model.handle(pdu, now).into_iter().take(MAX_REPLIES_PER_PDU) {
            if self.replies.len() >= MAX_REPLY_QUEUE {
                self.c.replies_dropped += 1;
                continue;
            }
            let ns = u64::try_from(delay.min(MAX_REPLY_DELAY).as_nanos()).unwrap_or(u64::MAX);
            let due = now.saturating_add(ns);
            let at = self
                .replies
                .iter()
                .position(|r| r.0 > due)
                .unwrap_or(self.replies.len());
            self.replies.insert(at, (due, bytes));
        }
    }

    /// Publishes all engine events, then moves frames into the pending queue (events first, so
    /// `rx_start` precedes its FC).
    fn drain(&mut self, em: &Emitter, now: u64) {
        while let Some(ev) = self.engine.pop_event() {
            self.on_event(ev, em, now);
        }
        while let Some(ev) = self.rx_engine.pop_event() {
            self.on_event(ev, em, now);
        }
        while self.pending.len() < MAX_PENDING_FRAMES {
            match self.engine.pop_frame().or_else(|| self.rx_engine.pop_frame()) {
                Some(f) => self.pending.push_back(f),
                None => break,
            }
        }
        self.sf_len = None;
    }

    /// Runs an engine input; on `OutputFull` drains once and retries the **same** input.
    fn with_room<T>(
        &mut self,
        em: &Emitter,
        now: u64,
        receive: bool,
        mut f: impl FnMut(&mut Engine) -> Result<T, TpError>,
    ) -> Result<T, TpError> {
        let engine = if receive {
            &mut self.rx_engine
        } else {
            &mut self.engine
        };
        match f(engine) {
            Err(TpError::OutputFull) => {
                self.drain(em, now);
                f(if receive {
                    &mut self.rx_engine
                } else {
                    &mut self.engine
                })
            }
            r => r,
        }
    }

    // ---- inputs -----------------------------------------------------------------------------

    /// A bus frame with this endpoint's `rx_id`. `seq` = `source_seq` of its `can.frame` event.
    pub(crate) fn feed(&mut self, frame: &CanFrame, seq: Option<u64>, frame_ns: u64, em: &Emitter, now: u64) {
        self.c.frames_rx += 1;
        if seq.is_none() {
            self.proof_failed = true;
        }
        let receive = !matches!(decode(frame), Ok(Pci::FlowControl { .. }));
        match decode(frame) {
            Ok(Pci::Single(d)) => {
                self.rx = Some(RxTrack {
                    first_seq: seq,
                    last_seq: seq,
                    frames: 1,
                    first_ns: frame_ns,
                    last_ns: frame_ns,
                });
                self.sf_len = Some(d.len());
            }
            Ok(Pci::First { .. }) => {
                self.rx = Some(RxTrack {
                    first_seq: seq,
                    last_seq: seq,
                    frames: 1,
                    first_ns: frame_ns,
                    last_ns: frame_ns,
                });
            }
            Ok(Pci::Consecutive { .. }) => {
                if let Some(r) = self.rx.as_mut() {
                    r.last_seq = seq;
                    r.last_ns = frame_ns;
                    r.frames += 1;
                }
            }
            _ => {}
        }
        if self
            .with_room(em, now, receive, |e| e.on_frame(frame, now))
            .is_err()
        {
            // Malformed input already produced an RxError event; OutputFull twice is counted.
            self.c.ingress_errors += 1;
            self.proof_failed = true;
        }
        self.drain(em, now);
    }

    /// Admits one PDU (`can.isotp.send`).
    pub(crate) fn send(
        &mut self,
        pdu: &[u8],
        req: ReqMeta,
        em: &Emitter,
        now: u64,
    ) -> Result<TransferId, (ErrorCode, String)> {
        let id = self
            .with_room(em, now, false, |e| e.send(pdu, now))
            .map_err(|e| match e {
                TpError::Busy => (
                    ErrorCode::Busy,
                    format!("{}: a transfer is still in progress", self.id),
                ),
                TpError::PduTooLarge { len, max } => {
                    usage(format!("pdu of {len} bytes is above max_pdu {max}"))
                }
                TpError::InvalidLength => usage("empty pdu"),
                other => (
                    ErrorCode::Busy,
                    format!("{}: cannot send now ({other:?})", self.id),
                ),
            })?;
        if self.transfers.len() >= MAX_TRANSFER_TAGS {
            self.transfers.pop_front();
            self.c.tags_evicted += 1;
        }
        self.transfers.push_back(Transfer {
            id,
            req: req.clone(),
            frames: 0,
            first_ns: None,
            last_ns: None,
        });
        self.active = Some(req.origin);
        self.last = req;
        self.stopped = false;
        self.drain(em, now);
        Ok(id)
    }

    /// Timers, due model replies and due frames. Returns whether anything happened.
    pub(crate) fn drive(&mut self, io: &mut Io<'_>, out: &mut Vec<Submitted>) -> bool {
        let now = io.now;
        let mut progress = false;
        for receive in [false, true] {
            let due = if receive {
                self.rx_engine.poll_at()
            } else {
                self.engine.poll_at()
            };
            if due.is_some_and(|t| t <= now) {
                if self
                    .with_room(io.emitter, now, receive, |e| e.on_timer(now))
                    .is_err()
                {
                    self.c.ingress_errors += 1;
                    self.proof_failed = true;
                }
                progress = true;
            }
        }
        while self.replies.front().is_some_and(|r| r.0 <= now) && !self.engine.is_sending() {
            let Some((_, bytes)) = self.replies.pop_front() else {
                break;
            };
            let req = ReqMeta {
                request_id: None,
                client_id: None,
                origin: Origin::Device,
            };
            match self.send(&bytes, req, io.emitter, now) {
                Ok(_) => self.c.replies += 1,
                Err(_) => self.c.reply_errors += 1,
            }
            progress = true;
        }
        self.drain(io.emitter, now);
        for _ in 0..MAX_SUBMIT_PER_DRIVE {
            // Stale requests (old generation, replaced FC) are dropped; due ones go in queue order.
            let before = self.pending.len();
            let (engine, rx_engine) = (&self.engine, &self.rx_engine);
            self.pending.retain(|f| {
                if f.transfer_id.is_some() {
                    engine
                } else {
                    rx_engine
                }
                .is_frame_pending(f.handle)
            });
            self.c.frames_stale += (before - self.pending.len()) as u64;
            let Some(i) = self.pending.iter().position(|f| f.not_before_ns <= now) else {
                break;
            };
            let Some(req) = self.pending.remove(i) else { break };
            let meta = req
                .transfer_id
                .and_then(|t| self.transfers.iter().find(|x| x.id == t))
                .map_or_else(|| self.last.clone(), |t| t.req.clone());
            let Some(sender) = self.sender(io, meta.origin) else {
                self.c.send_failed += 1;
                continue;
            };
            match io.hub.send(sender, req.frame) {
                Ok(seq) => {
                    self.c.frames_tx += 1;
                    let engine = if req.transfer_id.is_some() {
                        &mut self.engine
                    } else {
                        &mut self.rx_engine
                    };
                    let _ = engine.on_frame_submitted_at(req.handle, now);
                    if let Some(t) = req
                        .transfer_id
                        .and_then(|t| self.transfers.iter_mut().find(|x| x.id == t))
                    {
                        t.frames += 1;
                        t.first_ns.get_or_insert(now);
                        t.last_ns = Some(now);
                    }
                    if self.role == Role::Client {
                        out.push(Submitted {
                            seq,
                            tag: IsoTpTag {
                                endpoint: self.id.clone(),
                                request_id: meta.request_id.clone(),
                            },
                            request_id: meta.request_id,
                            client_id: meta.client_id,
                        });
                    }
                    self.drain(io.emitter, now);
                    progress = true;
                }
                Err(e) => {
                    // Latched origin or bus gone: the transfer cannot continue.
                    self.c.send_failed += 1;
                    self.abort(io.emitter, now, &format!("send failed: {e}"));
                    progress = true;
                    break;
                }
            }
        }
        progress
    }

    /// Abort TX and RX (R-TP-08). Clears queued frames and scheduled replies.
    pub(crate) fn abort(&mut self, em: &Emitter, now: u64, _reason: &str) -> Value {
        let queued = self.pending.len();
        self.pending.clear();
        let replies = self.replies.len();
        self.replies.clear();
        let r = self.engine.abort(now);
        let rx_result = self.rx_engine.abort(now);
        self.drain(em, now);
        self.rx = None;
        self.active = None;
        self.c.cancelled_frames += queued as u64;
        match r {
            Ok(r) => json!({
                "endpoint": self.id,
                "epoch": self.epoch,
                "aborted_tx": r.aborted_tx,
                "aborted_rx": rx_result.as_ref().is_ok_and(|rx| rx.aborted_rx),
                "residual_tx_possible": r.residual_tx_possible.saturating_add(rx_result.as_ref().map_or(0,|rx|rx.residual_tx_possible)),
                "cancelled_frames": r.cancelled_frames.saturating_add(rx_result.as_ref().map_or(0,|rx|rx.cancelled_frames)),
                "discarded_events": r.discarded_events.saturating_add(rx_result.as_ref().map_or(0,|rx|rx.discarded_events)),
                "cancelled_replies": replies,
            }),
            Err(e) => json!({"endpoint": self.id, "epoch": self.epoch, "error": format!("{e:?}")}),
        }
    }

    /// Cancel one active TX owned by the requesting client; RX/FC and other endpoints survive.
    pub(crate) fn cancel(
        &mut self,
        transfer: &str,
        req: &ReqMeta,
        em: &Emitter,
        now: u64,
    ) -> Result<Value, (ErrorCode, String)> {
        let t = self
            .transfers
            .iter()
            .find(|t| transfer_text(t.id) == transfer)
            .ok_or_else(|| {
                (
                    ErrorCode::NotFound,
                    "transfer is unknown or already terminal".into(),
                )
            })?;
        if t.req.origin != req.origin || t.req.client_id != req.client_id {
            return Err((
                ErrorCode::Locked,
                "transfer belongs to another client/origin".into(),
            ));
        }
        let id = t.id;
        if !self.engine.is_sending() {
            return Err((ErrorCode::NotFound, "transfer is already terminal".into()));
        }
        let report = self
            .engine
            .abort(now)
            .map_err(|e| (ErrorCode::Busy, format!("cannot cancel: {e:?}")))?;
        self.pending.retain(|f| f.transfer_id != Some(id));
        self.cancelling = Some(id);
        self.drain(em, now);
        self.cancelling = None;
        self.c.cancelled_frames = self
            .c
            .cancelled_frames
            .saturating_add(report.cancelled_frames as u64);
        Ok(json!({"endpoint":self.id,"epoch":self.epoch,"transfer":transfer,
            "state":"cancelled","residual_tx_possible":report.residual_tx_possible}))
    }

    fn retain_pdu(&mut self, mut row: Value) {
        let Some(seq) = self.pdu_seq.checked_add(1) else {
            self.proof_failed = true;
            return;
        };
        row["endpoint"] = json!(self.id);
        row["epoch"] = json!(self.epoch);
        row["request_id"] = json!(self.last.request_id);
        // Admission bounds metadata and payload <=4095B; cap the serialized row too.
        let Ok(bytes) = serde_json::to_vec(&row) else {
            self.proof_failed = true;
            return;
        };
        if bytes.len() > MAX_COMPLETED_ROW_BYTES {
            self.proof_failed = true;
            return;
        }
        if self.completed.len() == MAX_COMPLETED_PDUS {
            self.completed.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.pdu_seq = seq;
        self.completed.push_back((seq, row, bytes.len()));
    }

    /// Snapshot retained completions. Cursor is the last returned PDU, never an unseen head.
    pub(crate) fn recv(&self, after: u64, max: usize) -> Result<Value, (ErrorCode, String)> {
        if !(1..=MAX_RECV_PDUS).contains(&max) {
            return Err(usage("max_pdus must be1..16"));
        }
        if after > self.pdu_seq {
            return Err(usage("after_pdu_seq is ahead of this endpoint"));
        }
        let mut pdus = Vec::with_capacity(max);
        let mut bytes = 0;
        let mut cursor = after;
        for (seq, row, size) in self.completed.iter().filter(|(seq, _, _)| *seq > after) {
            if pdus.len() == max || bytes + size > MAX_RECV_REPLY_BYTES {
                break;
            }
            pdus.push(row.clone());
            bytes += size;
            cursor = *seq;
        }
        let oldest = self.completed.front().map(|(seq, _, _)| *seq);
        Ok(
            json!({"endpoint":self.id,"epoch":self.epoch,"pdus":pdus,"next_pdu_seq":cursor.to_string(),
            "latest_pdu_seq":self.pdu_seq.to_string(),"oldest_pdu_seq":oldest.map(|v|v.to_string()),
            "dropped":self.dropped.to_string(),"cursor_gap":oldest.is_some_and(|v| after < v.saturating_sub(1))}),
        )
    }

    /// Loss/publication failure invalidates negative-response proofs until endpoint reopen.
    pub(crate) fn note_ingress_loss(&mut self) {
        self.proof_failed = true;
    }

    /// Last published transport event and whether a barrier proof is still valid.
    pub(crate) fn barrier_status(&self) -> (u64, bool) {
        (self.transport_event_seq, !self.proof_failed)
    }

    /// STOP (A3): abort work started by agent/script requests. Returns whether it aborted.
    pub(crate) fn stop(&mut self, em: &Emitter, now: u64) -> bool {
        if self.role == Role::Server {
            return false; // sim ECU keeps answering
        }
        let tx = self.engine.is_sending() && self.active.is_some_and(stoppable);
        let rx = (self.rx_engine.is_receiving() || !self.pending.is_empty()) && stoppable(self.last.origin);
        if !(tx || rx) {
            return false;
        }
        self.abort(em, now, "stop");
        self.stopped = true;
        self.emit_state(
            em,
            "stopped",
            Some("STOP latched: agent/script ISO-TP transfers aborted"),
        );
        true
    }

    /// STOP released: endpoints aborted by STOP report `open` again.
    pub(crate) fn resume(&mut self, em: &Emitter) {
        if std::mem::take(&mut self.stopped) {
            self.emit_state(em, "open", Some("STOP released"));
        }
    }

    pub(crate) fn info(&self) -> Value {
        let cfg = self.engine.config();
        let s = self.engine.stats();
        let r = self.rx_engine.stats();
        let mut info = json!({
            "endpoint": self.id,
            "epoch": self.epoch,
            "channel": self.channel,
            "role": self.role.as_str(),
            "tx_id": id_text(cfg.tx_id),
            "rx_id": id_text(cfg.rx_id),
            "ext": cfg.tx_id.is_extended(),
            "fd": cfg.can_fd,
            "tx_dl": cfg.tx_dl,
            "padding": cfg.padding,
            "block_size": cfg.block_size,
            "st_min": cfg.st_min,
            "n_bs_ms": cfg.n_bs_ms,
            "n_cr_ms": cfg.n_cr_ms,
            "max_pdu": cfg.max_pdu,
            "owner": self.owner,
            "opened_by": self.opener,
            "client_id": self.client_id,
            "model": self.model_name,
            "state": if self.stopped { "stopped" } else { "open" },
            "sending": self.engine.is_sending(),
            "receiving": self.rx_engine.is_receiving(),
            "pending_frames": self.pending.len(),
            "scheduled_replies": self.replies.len(),
            "counters": {
                "tx_pdu": self.c.tx_pdu, "rx_pdu": self.c.rx_pdu, "tx_err": self.c.tx_err,
                "rx_err": self.c.rx_err, "frames_tx": self.c.frames_tx, "frames_rx": self.c.frames_rx,
                "frames_stale": self.c.frames_stale, "cancelled_frames": self.c.cancelled_frames,
                "unconfirmed_released": self.c.unconfirmed_released, "tags_evicted": self.c.tags_evicted,
                "replies": self.c.replies, "replies_dropped": self.c.replies_dropped,
                "reply_errors": self.c.reply_errors, "ingress_errors": self.c.ingress_errors,
                "events_failed": self.c.events_failed, "send_failed": self.c.send_failed,
            },
            "engine": {
                "ignored_flow_controls": s.ignored_flow_controls.saturating_add(r.ignored_flow_controls),
                "ignored_consecutive_frames": s.ignored_consecutive_frames.saturating_add(r.ignored_consecutive_frames),
                "ignored_generations": s.ignored_generations.saturating_add(r.ignored_generations),
                "ignored_callbacks": s.ignored_callbacks.saturating_add(r.ignored_callbacks),
                "ignored_identifiers": s.ignored_identifiers.saturating_add(r.ignored_identifiers),
                "output_full": s.output_full.saturating_add(r.output_full),
            },
        });
        info["completed_pdus"] = json!(self.completed.len());
        info["pdu_seq"] = json!(self.pdu_seq.to_string());
        info["dropped"] = json!(self.dropped.to_string());
        info["max_completed_pdus"] = json!(MAX_COMPLETED_PDUS);
        info
    }
}
