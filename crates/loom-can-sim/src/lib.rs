//! Loom CAN sim: the sim bus owner and fake ECUs (SimNodes) for the `can-sim` worker
//! (plan §9.1, D72). A library only: no threads, no IPC. The worker drives it with
//! [`SimHub::send`] and [`SimHub::poll`]; a local-socket endpoint is added later behind the
//! [`Endpoint`] trait once CORE IPC exists.
//!
//! Guarantee: every frame on a bus (client sends, Tx echoes, node replies) gets one
//! `source_seq` and reaches every endpoint in the same order with the same origin.
//!
//! ```
//! use loom_can_frame::parse_frame;
//! use loom_can_sim::{ManualClock, Origin, SimHub, HubLimits, parse_nodes};
//!
//! let clock = ManualClock::new(0);
//! let mut hub = SimHub::new(clock.clone(), HubLimits::default());
//! let nodes = parse_nodes(r#"
//!     [[node]]
//!     name = "BCM"
//!     channel = "sim1"
//!     [[node.on]]
//!     id = 0x300
//!     reply_id = 0x301
//!     reply = "copy"
//!     delay_ms = 5.0
//! "#).unwrap();
//! hub.start_node(nodes[0].clone()).unwrap();
//! let (agent, rx) = hub.attach_local("sim1", "agent:cli", Origin::Agent).unwrap();
//! hub.send(agent, parse_frame("300#01", false).unwrap()).unwrap();
//! clock.advance_ms(5);
//! hub.poll();
//! let got: Vec<_> = rx.recv(10).iter().map(|r| (r.source_seq, r.origin)).collect();
//! assert_eq!(got, [(1, Origin::Agent), (2, Origin::Device)]);
//! ```

mod clock;
mod config;
mod endpoint;
mod error;
mod hub;
mod record;

pub use clock::{Clock, ManualClock, MonotonicClock};
pub use config::{
    Follow, MAX_CONFIG_BYTES, MAX_DROP_IDS, MAX_FOLLOWS, MAX_NODES_PER_FILE, MAX_PERIODIC, MAX_RULES,
    MIN_PERIOD_NS, NodeConfig, NodeFaults, Periodic, ReplyData, Rule, parse_nodes,
};
pub use endpoint::{DEFAULT_QUEUE_FRAMES, Endpoint, LocalEndpoint, LocalReceiver, local_endpoint};
pub use error::{ConfigError, SimError};
pub use hub::{
    BusStats, DropStats, EndpointStats, Gap, HubLimits, MAX_GAPS_KEPT, NodeInfo, NodeState, PollReport,
    SimHub,
};
pub use record::{BusId, EndpointId, NodeId, Origin, Sender, SimRecord};
