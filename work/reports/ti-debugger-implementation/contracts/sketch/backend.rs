// PROPOSED — design sketch for crates/loom-dbg/src/backend.rs. NOT compiled, NOT part of the repo.
// Ties to existing contracts: loom-domain-api (Registry, CommandSpec, WorkerSpec, Reopen::None),
// loom-worker-sdk (worker process, latch), loom-proto ErrorCode (existing codes only, see dbg-dto.schema.json
// ProposedErrorMapping). No CCS method names appear here: the CCS mapping lives in the bridge executor,
// see REPORT §4 / API-EVIDENCE.md.

use std::time::Duration;

/// `<probe_alias>/<core_alias>` from loom.toml; the opaque CCS core name is kept beside it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TargetId { pub probe: String, pub core: String }

/// Per-target counters. All serialized as decimal strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Epochs {
    pub session_epoch: u64, // +1 per successful session.open of this core (persisted high-water, never reused)
    pub run_epoch: u64,     // +1 per observed transition into running/halted/stepping within the session
    pub state_rev: u64,     // +1 per any change of run_state/quality/link/session (monotonic per target)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState { Unknown, Running, Halted, Stepping, Disconnected, Lost, Error }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality { Fresh, Stale, Unknown }

/// What the backend can do for ONE core. Unknown = not established (never treated as true).
#[derive(Clone, Debug, Default)]
pub struct TargetCaps {
    pub au_bits: Option<u8>,                     // addressable unit; None → mem.read refused until configured
    pub nonintrusive_read: Tri,                  // user-declared per region in loom.toml, or Unknown
    pub register_enumeration: Tri,
    pub step: Tri,
    pub breakpoints: Tri,
    pub source_lines: Tri,
    pub disassembly: Tri,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tri { #[default] Unknown, Yes, No }

pub struct ReadItem { pub key: String, pub loc: Loc, pub bits: u8, pub signed: bool, pub region: RegionClass }
pub enum Loc { Symbol(String), Address { addr: u64, space: Option<String> }, Register(String) }
#[derive(Clone, Copy)] pub enum RegionClass { Ram, Flash, Mmio, Unknown }

pub struct RawValue { pub key: String, pub bits: u8, pub raw: u128 /* ≤ 64 bits used */, pub err: Option<BackendErr> }

pub struct Timing { pub bridge_recv_ns: u64, pub bridge_start_ns: Option<u64>, pub bridge_end_ns: Option<u64> }

/// Outcome of a call that may have touched the target.
pub enum Effect { None, Applied, Unknown }

pub struct CallOut<T> { pub data: Option<T>, pub effect: Effect, pub timing: Timing, pub run_state_after: Option<RunState> }

#[derive(Clone, Debug)]
pub enum BackendErr {
    Busy, QueueFull, Auth, Version, NotOpen, NoSuchCore, EpochMismatch, LinkLost,
    Deadline { effect_unknown: bool }, CancelledByStop, WouldHalt, Unsupported, Partial, Sdk(String), BadArgs(String),
}

/// One instance per probe (JTAG chain) inside the dbg worker process. Implementations:
///  - FakeBackend   (crates/loom-dbg/src/fake.rs): scripted run-state/memory map, latency/drop/kill injection.
///  - CcsRemote     (crates/loom-dbg/src/ccs_remote.rs): JSON-lines client of loom-ccs-bridge v1.
/// Every method is bounded by `deadline`; none of them retries a write.
pub trait DebugBackend: Send {
    fn kind(&self) -> &'static str;                         // "fake" | "ccs-remote"
    fn link(&mut self) -> Result<LinkInfo, BackendErr>;     // hello/auth; no target access
    fn cores(&mut self, deadline: Duration) -> Result<Vec<String>, BackendErr>;
    fn open(&mut self, core_alias: &str, core_name: &str, ccxml: &str, deadline: Duration) -> Result<CallOut<(u64, TargetCaps)>, BackendErr>;
    fn connect(&mut self, t: &TargetId, epoch: u64, deadline: Duration) -> Result<CallOut<()>, BackendErr>;
    fn close(&mut self, t: &TargetId, epoch: u64, deadline: Duration) -> Result<CallOut<()>, BackendErr>; // disconnect + session.close; sends no halt/run/reset request. SDK/GEL disconnect hooks unverified → state afterwards Disconnected/Quality::Unknown, never 'preserved'
    fn run_state(&mut self, t: &TargetId, epoch: u64, deadline: Duration) -> Result<CallOut<RunState>, BackendErr>;
    fn halt(&mut self, t: &TargetId, epoch: u64, deadline: Duration) -> Result<CallOut<()>, BackendErr>;
    fn run(&mut self, t: &TargetId, epoch: u64, deadline: Duration) -> Result<CallOut<()>, BackendErr>;   // never waits for halt
    fn step(&mut self, t: &TargetId, epoch: u64, kind: StepKind, deadline: Duration) -> Result<CallOut<()>, BackendErr>;
    /// Batched constrained read. MUST return WouldHalt (no SDK call) when the target is not known halted and
    /// every item's region is not declared non-intrusive. Never halts+resumes on its own.
    fn read(&mut self, t: &TargetId, epoch: u64, items: &[ReadItem], deadline: Duration) -> Result<CallOut<Vec<RawValue>>, BackendErr>;
    fn read_block(&mut self, t: &TargetId, epoch: u64, addr: u64, space: Option<&str>, count_au: u32, deadline: Duration) -> Result<CallOut<Vec<u8>>, BackendErr>;
    /// control lane — must answer while a call is in flight (CcsRemote: supervisor ctl; Fake: separate channel)
    fn control(&self) -> &dyn ControlLane;
}
pub enum StepKind { Into, Over, Out }
pub struct LinkInfo { pub bridge_instance: String, pub ccs_version: Option<String>, pub rtt: Duration, pub offset_ns: i128, pub offset_err_ns: u64 }

pub trait ControlLane: Send + Sync {
    fn ping(&self) -> Result<InFlight, BackendErr>;
    /// Cancel QUEUED requests (effect none). Returns the in-flight call, which keeps running.
    fn stop(&self) -> Result<(Vec<String>, InFlight), BackendErr>;
    /// Last resort after deadline+grace: kill the executor. Session epoch is dead, run_state → Lost/Unknown.
    fn kill_executor(&self) -> Result<(), BackendErr>;
}
pub struct InFlight { pub id: Option<String>, pub op: Option<String>, pub started_ns: Option<u64> }

// ---- worker-side scheduling (crates/loom-dbg/src/worker.rs) -------------------------------------------
// Per probe: one serial queue, priority action > read > poll; one outstanding poll per watch (a late poll is
// counted as missed, never queued behind itself); queue bound 16 (proposed) → QueueFull = E_WORKER_BUSY.
// on_latch(true) (WorkerHost::latch, ack ≤ 200 ms): stop all watches, ControlLane::stop(), mark queued
// writes write_state=failed(E_STOPPED); in-flight action keeps its eventual result. NO halt/run/reset/close.
// Reconnect: Reopen::None — after a crash or link loss the worker reports Lost and waits for an explicit dbg.open.
