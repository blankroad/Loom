//! `loom-worker-host` — gateway side of `loom-worker/1` (contract 05, plan §3.3).
//!
//! * [`listen`] — the two lanes (`ctl`, `data`): Unix domain sockets in a 0700 directory, or
//!   Windows named pipes; peers are checked (same user, expected pid where the OS tells).
//! * [`host`] — [`IpcHost`]: spawns `loomd worker <kind> --instance … --producer-epoch …` (or any
//!   program from [`HostConfig`], e.g. `loom-eth-worker`), runs the Hello/Welcome handshake,
//!   forwards requests and events, and supervises (watchdog, restart backoff, quarantine).
//!
//! The binary `loom-test-worker` (in this crate) is the worker used by the integration tests.

pub mod host;
pub mod listen;

pub use host::{HostConfig, IpcHost, StateCallback, StateEvent, WorkerState};
pub use listen::{LaneListeners, prepare_sock_dir};
