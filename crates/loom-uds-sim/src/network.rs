//! Several ECUs behind one gateway, keyed by logical address.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::{Addressing, MAX_SIM_ECUS, Replies, SimEcu, SimError};

/// Logical address → ECU, at most [`MAX_SIM_ECUS`].
#[derive(Debug, Clone, Default)]
pub struct SimNetwork {
    ecus: BTreeMap<u16, SimEcu>,
}

impl SimNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace the ECU at `addr`. A new address beyond [`MAX_SIM_ECUS`] is rejected.
    pub fn insert(&mut self, addr: u16, ecu: SimEcu) -> Result<(), SimError> {
        if !self.ecus.contains_key(&addr) && self.ecus.len() >= MAX_SIM_ECUS {
            return Err(SimError::TooMany {
                what: "ECUs",
                max: MAX_SIM_ECUS,
            });
        }
        self.ecus.insert(addr, ecu);
        Ok(())
    }

    pub fn get(&self, addr: u16) -> Option<&SimEcu> {
        self.ecus.get(&addr)
    }

    pub fn get_mut(&mut self, addr: u16) -> Option<&mut SimEcu> {
        self.ecus.get_mut(&addr)
    }

    pub fn addresses(&self) -> impl Iterator<Item = u16> + '_ {
        self.ecus.keys().copied()
    }

    pub fn len(&self) -> usize {
        self.ecus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ecus.is_empty()
    }

    /// Physically addressed request. `None` if no ECU has `addr`.
    pub fn handle_physical(&mut self, addr: u16, now: Duration, req: &[u8]) -> Option<Replies> {
        self.ecus
            .get_mut(&addr)
            .map(|ecu| ecu.handle_addressed(now, req, Addressing::Physical))
    }

    /// Functionally addressed request: every ECU handles it; ECUs with no reply are omitted.
    /// At most [`MAX_SIM_ECUS`] entries.
    pub fn handle_functional(&mut self, now: Duration, req: &[u8]) -> Vec<(u16, Replies)> {
        self.ecus
            .iter_mut()
            .map(|(addr, ecu)| (*addr, ecu.handle_addressed(now, req, Addressing::Functional)))
            .filter(|(_, r)| !r.is_empty())
            .collect()
    }
}
