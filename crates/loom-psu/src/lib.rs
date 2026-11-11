//! `loom-psu` — bench power supply domain. In M1 it drives only [`sim::SimPsu`] and exists to
//! check the domain plugin contract end to end (explore plan §5.1, §0.4 "PSU (계약 검증)").
//!
//! Pieces as in the CORE contract (§8.1): [`gateway`] (registration, hooks, waits, evidence —
//! runs in `loomd`), [`worker`] (owns the supply — runs in the psu worker process) and [`cli`]
//! (`loom psu …`). The event shapes follow the UI agreement (`ui-explore.md`, `## explore 답변`).

pub mod cli;
pub mod gateway;
pub mod model;
pub mod sim;
pub mod worker;

/// Domain name (RPC prefix, CLI group, event `domain`).
pub const DOMAIN: &str = "psu";
/// Worker kind (one instance per supply channel).
pub const WORKER_KIND: &str = "psu";
/// Maximum supplies (worker instances).
pub const MAX_CHANNELS: u16 = 4;

/// `psu.state{ch, set_v, set_i, out, prot, mode}` — on change.
pub const KIND_STATE: &str = "psu.state";
/// `psu.meas{ch, age_ms}` + record `metrics[voltage|current|power]` — polled samples.
pub const KIND_MEAS: &str = "psu.meas";
/// `psu.write{ch, action, write_state, …}` — write rows (E8).
pub const KIND_WRITE: &str = "psu.write";
/// Loss cause for a poll the supply did not answer.
pub const GAP_NO_RESPONSE: &str = "scpi_no_response";
