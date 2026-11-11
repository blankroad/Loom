//! Loom gateway (`loomd`): event hub, connections, dispatcher, write pipeline, STOP, audit
//! (contract 01·03·04·05), plus the `--fixture-replay` server ([`replay`]) for UI development.

pub mod audit;
pub mod auth;
pub mod conn;
pub mod daemon;
pub mod domains;
pub mod home;
pub mod hub;
pub mod journal;
pub mod policy;
pub mod pool;
pub mod replay;
pub mod rpcutil;
pub mod server;
pub mod state;
#[cfg(feature = "test")]
pub mod test_services;
pub mod worker_host;
