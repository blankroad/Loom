//! Adapter: the UDS team's virtual ECU (`loom-uds-sim`) as a CAN ISO-TP server model, so a sim
//! channel can answer real UDS requests over CAN-TP (`can.isotp.open{role:"server", model:"uds_sim"}`).
//! Sim only; enabled by the `uds-sim` feature (default on).

use std::sync::Arc;
use std::time::Duration;

use loom_proto::worker::WorkerError;
use loom_uds_sim::{SimEcu, SimEcuConfig};

use crate::isotp::{IsoTpServerModel, ModelFactory};
use crate::worker::CanSimWorker;

/// Model name used in `can.isotp.open{model}`.
pub const MODEL_NAME: &str = "uds_sim";

/// One virtual ECU per server endpoint. `None` if the config was rejected: the endpoint then stays
/// silent (the client sees its own N_Bs/P2 timeout) instead of inventing replies.
#[derive(Debug)]
pub struct UdsSimModel(Option<SimEcu>);

impl UdsSimModel {
    pub fn new(cfg: SimEcuConfig) -> Self {
        Self(SimEcu::new(cfg).ok())
    }

    /// Whether the ECU was built (config valid).
    pub fn is_ready(&self) -> bool {
        self.0.is_some()
    }
}

impl IsoTpServerModel for UdsSimModel {
    fn handle(&mut self, pdu: &[u8], now_ns: u64) -> Vec<(Duration, Vec<u8>)> {
        match &mut self.0 {
            Some(ecu) => ecu
                .handle(Duration::from_nanos(now_ns), pdu)
                .into_vec()
                .into_iter()
                .map(|r| (r.delay, r.bytes))
                .collect(),
            None => Vec::new(),
        }
    }
}

/// Factory building a fresh ECU (state per endpoint) from `cfg`.
pub fn factory(cfg: SimEcuConfig) -> ModelFactory {
    Arc::new(move || Box::new(UdsSimModel::new(cfg.clone())) as Box<dyn IsoTpServerModel>)
}

/// Register `uds_sim` (UDS bench profile: VIN `LOOMSIM0000000001`, DTCs, security access) on a worker.
pub fn register(worker: &CanSimWorker) -> Result<(), WorkerError> {
    worker.register_isotp_model(MODEL_NAME, factory(SimEcuConfig::bench()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn answers_read_vin_and_unknown_did() {
        let mut m = UdsSimModel::new(SimEcuConfig::bench());
        assert!(m.is_ready());
        let r = m.handle(&[0x22, 0xF1, 0x90], 0);
        assert_eq!(r.len(), 1);
        let mut want = vec![0x62, 0xF1, 0x90];
        want.extend_from_slice(b"LOOMSIM0000000001");
        assert_eq!(r[0].1, want);
        // Unknown DID → negative response (0x31 requestOutOfRange).
        let r = m.handle(&[0x22, 0x12, 0x34], 1_000_000);
        assert_eq!(r.last().unwrap().1, vec![0x7F, 0x22, 0x31]);
    }

    #[test]
    fn registers_on_the_worker() {
        let w = CanSimWorker::new();
        register(&w).unwrap();
        // Registering again replaces the factory (same name is allowed).
        register(&w).unwrap();
    }
}
