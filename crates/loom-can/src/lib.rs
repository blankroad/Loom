//! `loom-can` — the CAN domain crate (contract 08 §8.1): [`gateway`] (registration, hooks,
//! queries, reducer — runs in `loomd`), [`worker`] (device owner — `loomd worker can-sim`) and
//! [`cli`] (the `loom can …` fragment). [`schema`] is the event/RPC shape shared by all three and
//! promised to the UI in `docs/impl/requests/can-ui.md`.

#[cfg(feature = "cli")]
pub mod cli;
pub mod condition;
#[cfg(feature = "runtime")]
pub mod gateway;
#[cfg(feature = "runtime")]
pub mod isotp;
pub mod pdu;
pub mod schema;
#[cfg(feature = "runtime")]
pub mod signals;
#[cfg(all(feature = "runtime", feature = "uds-sim"))]
pub mod uds_sim;
#[cfg(feature = "runtime")]
pub mod worker;

/// Domain name (RPC prefix, CLI group, event `domain`).
pub const DOMAIN: &str = "can";
/// Worker kind of the simulation backend (sim bus + fake ECU nodes). Registry rule `[a-z][a-z0-9_]*`.
pub const WORKER_KIND_SIM: &str = "can_sim";
/// Bus frame (rx and tx echo). Payload: [`schema::FramePayload`].
pub const KIND_FRAME: &str = "can.frame";
/// Write-state row of one `can.send` request (`payload.write_state`, E8). Payload: [`schema::TxPayload`].
pub const KIND_TX: &str = "can.tx";
/// Channel open/close/bus state/worker restart (priority lane).
pub const KIND_CHANNEL_STATE: &str = "can.channel_state";
/// 1 Hz channel statistics.
pub const KIND_STATS: &str = "can.stats";

/// RPC: send a frame (or a short finite burst).
pub const RPC_SEND: &str = "can.send";
/// RPC: bounded trace query.
pub const RPC_TRACE_QUERY: &str = "can.trace.query";
/// RPC: last frame per id (Fixed view).
pub const RPC_LAST: &str = "can.last";
/// RPC: wait for a matching frame within a deadline (returns evidence).
pub const RPC_EXPECT: &str = "can.expect";
/// RPC: channel status.
pub const RPC_STATUS: &str = "can.status";
/// RPC: load a DBC for a channel (M2a).
pub const RPC_DB_LOAD: &str = "can.db.load";
/// RPC: loaded DBC of a channel.
pub const RPC_DB_INFO: &str = "can.db.info";
/// RPC: current value of one signal (with evidence).
pub const RPC_SIG_GET: &str = "can.sig.get";
/// RPC: wait until a signal condition first becomes true.
pub const RPC_SIG_WAIT_FOR: &str = "can.sig.wait_for";

/// ISO-TP RPCs (`_xteam/can-uds-isotp.md` "v1.1 이름·상태 정본"). Writes: `can.isotp.send`.
pub const RPC_ISOTP_OPEN: &str = "can.isotp.open";
pub const RPC_ISOTP_SEND: &str = "can.isotp.send";
pub const RPC_ISOTP_ABORT: &str = "can.isotp.abort";
pub const RPC_ISOTP_CLOSE: &str = "can.isotp.close";
pub const RPC_ISOTP_LIST: &str = "can.isotp.list";
pub const RPC_ISOTP_CANCEL: &str = "can.isotp.cancel";
pub const RPC_ISOTP_RECV: &str = "can.isotp.recv";
pub const RPC_ISOTP_BARRIER: &str = "can.isotp.barrier";
/// ISO-TP events (priority lane).
pub const KIND_ISOTP_TX_DONE: &str = "can.isotp_tx_done";
pub const KIND_ISOTP_TX_ERROR: &str = "can.isotp_tx_error";
pub const KIND_ISOTP_RX_START: &str = "can.isotp_rx_start";
pub const KIND_ISOTP_RX_COMPLETE: &str = "can.isotp_rx_complete";
pub const KIND_ISOTP_RX_ERROR: &str = "can.isotp_rx_error";
pub const KIND_ISOTP_STATE: &str = "can.isotp_state";

/// Periodic sending (M2a K9). Worker calls/writes and gateway RPCs share these names.
pub const RPC_PERIODIC_START: &str = "can.periodic.start";
pub const RPC_PERIODIC_UPDATE: &str = "can.periodic.update";
pub const RPC_PERIODIC_STOP: &str = "can.periodic.stop";
pub const RPC_PERIODIC_RENEW: &str = "can.periodic.renew";
pub const RPC_PERIODIC_TAKEOVER: &str = "can.periodic.takeover";
pub const RPC_PERIODIC_LIST: &str = "can.periodic.list";
/// Gateway-only: encode DBC signal values into a periodic job (IG), via `can.periodic.update`.
pub const RPC_SIG_SET: &str = "can.sig.set";
/// Job lifecycle event (priority lane): started / patched / renewed / stopped{reason} / paused.
pub const KIND_JOB_STATE: &str = "can.job_state";

/// UI data queries (Graphics / Data / IG panels, `_xteam/can-ui.md` §5.9).
pub const RPC_SIGNAL_QUERY: &str = "can.signal.query";
pub const RPC_SIGNAL_AT: &str = "can.signal.at";
pub const RPC_DB_CATALOG: &str = "can.db.catalog";
pub const RPC_ENCODE_PREVIEW: &str = "can.encode.preview";
