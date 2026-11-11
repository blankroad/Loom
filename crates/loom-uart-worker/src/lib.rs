//! `loom-uart-worker` — the UART device owner (UART plan §3.1, §3.5, §9.1).
//!
//! One worker process per port (max 8). P0 provides the device-side parts:
//! * [`transport`] — `SerialTransport` (real ports and sim ptys) and `SimTransport` (`sim://`).
//! * [`reader`] + [`bridge`] — RX thread and the bounded, loss-accounting hand-off (§3.13).
//! * [`tx`] — the TX actor with Ctrl-C > user > agent lanes and absolute deadlines (§3.7).
//! * [`sim`] — fake devices (`bootshell`, `flood`, `binary`, `ansi`) and the pty harness.
//! * [`clock`] — injectable time source and the shared `source_seq` allocator.
//!
//! Device handles never leave the worker: [`transport::open`] and
//! [`transport::SerialTransport::open`] are for the worker binary only (contract 08 §8.3).

pub mod bridge;
pub mod channel;
pub mod clock;
pub mod reader;
pub mod sim;
pub mod transport;
pub mod tx;
pub mod worker;
