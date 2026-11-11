//! The simulated ECU.

use std::collections::BTreeMap;
use std::time::Duration;

use loom_uds_codec::did::MAX_DID_DATA_LEN;
use loom_uds_codec::request::MAX_DIDS_PER_READ;
use loom_uds_codec::{MAX_PDU_LEN, NEGATIVE_RESPONSE_SID, SPRMIB, service};

use crate::fault::{FaultAction, FaultRule};
use crate::{Addressing, MAX_FAULT_RULES, MAX_REPLIES_PER_REQUEST, MAX_SIM_DIDS, MAX_SIM_DTCS};
use crate::{Replies, Reply, SimError};

/// Default SecurityAccess secret: `key = seed XOR SECURITY_XOR_DEFAULT` (test-only algorithm).
pub const SECURITY_XOR_DEFAULT: u32 = 0x1234_5678;

const SESSION_DEFAULT: u8 = 0x01;
const SESSION_EXTENDED: u8 = 0x03;
const SECURITY_SEED_SUB: u8 = 0x01;
const DID_ACTIVE_SESSION: u16 = 0xF186;

// NRCs used by the model.
const NRC_SERVICE_NOT_SUPPORTED: u8 = 0x11;
const NRC_SUBFUNCTION_NOT_SUPPORTED: u8 = 0x12;
const NRC_INCORRECT_LENGTH: u8 = 0x13;
const NRC_RESPONSE_TOO_LONG: u8 = 0x14;
const NRC_SEQUENCE_ERROR: u8 = 0x24;
const NRC_OUT_OF_RANGE: u8 = 0x31;
const NRC_INVALID_KEY: u8 = 0x35;
const NRC_EXCEEDED_ATTEMPTS: u8 = 0x36;
const NRC_DELAY_NOT_EXPIRED: u8 = 0x37;
const NRC_PENDING: u8 = 0x78;
const NRC_SERVICE_NOT_IN_SESSION: u8 = 0x7F;
/// NRCs an ECU must not send for functionally addressed requests (ISO 14229-1 §7.5).
const FUNCTIONAL_SUPPRESSED_NRCS: [u8; 5] = [0x11, 0x12, 0x31, 0x7E, 0x7F];

/// One stored DID. Writes must keep the stored length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimDid {
    pub data: Vec<u8>,
    pub writable: bool,
}

impl SimDid {
    pub fn read_only(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            writable: false,
        }
    }

    pub fn writable(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
            writable: true,
        }
    }
}

/// ECU behaviour parameters. [`SimEcuConfig::bench`] is the profile used by E2E-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimEcuConfig {
    /// S3server: a non-default session without requests for longer than this returns to default.
    pub s3: Duration,
    pub p2_ms: u16,
    /// Must be a multiple of 10 (sent in 10 ms units), at most 655 350.
    pub p2_star_ms: u32,
    pub security_xor: u32,
    /// xorshift64 state for seeds (non-zero).
    pub seed_state: u64,
    /// Wrong keys before NRC 0x36 and the lockout.
    pub max_key_attempts: u8,
    pub lockout: Duration,
    pub dids: BTreeMap<u16, SimDid>,
    /// `(24-bit DTC, status)`.
    pub dtcs: Vec<(u32, u8)>,
}

impl Default for SimEcuConfig {
    fn default() -> Self {
        Self::bench()
    }
}

impl SimEcuConfig {
    /// `F190` VIN `LOOMSIM0000000001`, `F1A0` writable 1 byte `0x01`, `F18C` serial, `F40D`
    /// speed 0, `F186` active session (dynamic); two stored DTCs.
    pub fn bench() -> Self {
        let mut dids = BTreeMap::new();
        dids.insert(0xF190, SimDid::read_only(b"LOOMSIM0000000001"));
        dids.insert(0xF1A0, SimDid::writable(&[0x01]));
        dids.insert(0xF18C, SimDid::read_only(b"LOOMSIM-0001"));
        dids.insert(0xF40D, SimDid::read_only(&[0x00]));
        Self {
            s3: Duration::from_secs(5),
            p2_ms: 50,
            p2_star_ms: 5000,
            security_xor: SECURITY_XOR_DEFAULT,
            seed_state: 0x5EED_0000_0000_0001,
            max_key_attempts: 3,
            lockout: Duration::from_secs(10),
            dids,
            dtcs: vec![(0x92_3400, 0x09), (0xC1_0000, 0x2F)],
        }
    }

    pub fn validate(&self) -> Result<(), SimError> {
        let bad = |m: &str| Err(SimError::Config(m.to_string()));
        if self.dids.len() > MAX_SIM_DIDS {
            return Err(SimError::TooMany {
                what: "DIDs",
                max: MAX_SIM_DIDS,
            });
        }
        if self.dtcs.len() > MAX_SIM_DTCS {
            return Err(SimError::TooMany {
                what: "DTCs",
                max: MAX_SIM_DTCS,
            });
        }
        if self.dids.contains_key(&DID_ACTIVE_SESSION) {
            return bad("F186 is dynamic (current session) and cannot be configured");
        }
        if self
            .dids
            .values()
            .any(|d| d.data.is_empty() || d.data.len() > MAX_DID_DATA_LEN)
        {
            return bad("DID data must be 1..=4092 bytes");
        }
        if self.dtcs.iter().any(|(d, _)| *d > 0x00FF_FFFF) {
            return bad("DTC exceeds 24 bits");
        }
        if !self.p2_star_ms.is_multiple_of(10) || self.p2_star_ms > 655_350 {
            return bad("p2_star_ms must be a multiple of 10 and at most 655350");
        }
        if self.seed_state == 0 || self.max_key_attempts == 0 {
            return bad("seed_state and max_key_attempts must be non-zero");
        }
        Ok(())
    }
}

/// A virtual ECU. All state changes happen inside [`SimEcu::handle`].
#[derive(Debug, Clone)]
pub struct SimEcu {
    cfg: SimEcuConfig,
    session: u8,
    last_request: Option<Duration>,
    unlocked: Option<u8>,
    pending_seed: Option<(u8, u32)>,
    failed_keys: u8,
    lockout_until: Option<Duration>,
    faults: Vec<FaultRule>,
}

fn nrc(sid: u8, code: u8) -> Option<Vec<u8>> {
    Some(vec![NEGATIVE_RESPONSE_SID, sid, code])
}

impl SimEcu {
    pub fn new(cfg: SimEcuConfig) -> Result<Self, SimError> {
        cfg.validate()?;
        Ok(Self {
            cfg,
            session: SESSION_DEFAULT,
            last_request: None,
            unlocked: None,
            pending_seed: None,
            failed_keys: 0,
            lockout_until: None,
            faults: Vec::new(),
        })
    }

    /// The bench profile (always valid).
    pub fn bench() -> Self {
        Self {
            cfg: SimEcuConfig::bench(),
            session: SESSION_DEFAULT,
            last_request: None,
            unlocked: None,
            pending_seed: None,
            failed_keys: 0,
            lockout_until: None,
            faults: Vec::new(),
        }
    }

    pub fn add_fault(&mut self, rule: FaultRule) -> Result<(), SimError> {
        if self.faults.len() >= MAX_FAULT_RULES {
            return Err(SimError::TooMany {
                what: "fault rules",
                max: MAX_FAULT_RULES,
            });
        }
        self.faults.push(rule);
        Ok(())
    }

    pub fn clear_faults(&mut self) {
        self.faults.clear();
    }

    pub fn session(&self) -> u8 {
        self.session
    }

    /// Unlocked SecurityAccess level (the requestSeed sub-function), if any.
    pub fn security_level(&self) -> Option<u8> {
        self.unlocked
    }

    pub fn did(&self, did: u16) -> Option<&[u8]> {
        self.cfg.dids.get(&did).map(|d| d.data.as_slice())
    }

    pub fn dtcs(&self) -> &[(u32, u8)] {
        &self.cfg.dtcs
    }

    /// Physically addressed request.
    pub fn handle(&mut self, now: Duration, req: &[u8]) -> Replies {
        self.handle_addressed(now, req, Addressing::Physical)
    }

    /// Handle one request arriving at monotonic time `now`.
    pub fn handle_addressed(&mut self, now: Duration, req: &[u8], addressing: Addressing) -> Replies {
        let Some(&sid) = req.first() else {
            return Replies::none();
        };
        self.expire_s3(now);
        self.last_request = Some(now);

        let action = self.take_fault(req);
        if let Some(FaultAction::ForceNrc(code)) = action {
            return Replies::one(vec![NEGATIVE_RESPONSE_SID, sid, code]);
        }

        let mut resp = self.execute(now, req);
        if addressing == Addressing::Functional
            && let Some(r) = &resp
            && r.len() == 3
            && r[0] == NEGATIVE_RESPONSE_SID
            && FUNCTIONAL_SUPPRESSED_NRCS.contains(&r[2])
        {
            resp = None;
        }

        match action {
            None | Some(FaultAction::ForceNrc(_)) => resp.map_or_else(Replies::none, Replies::one),
            Some(FaultAction::Drop) => Replies::none(),
            Some(FaultAction::Delay(d)) => Self::single_at(resp, d),
            Some(FaultAction::Late { after }) => Self::single_at(resp, after),
            Some(FaultAction::WrongEcho) => {
                let resp = resp.map(|mut r| {
                    Self::corrupt_echo(&mut r);
                    r
                });
                resp.map_or_else(Replies::none, Replies::one)
            }
            Some(FaultAction::PendingThen { count, every }) => {
                let count = usize::from(count).min(MAX_REPLIES_PER_REQUEST - 1);
                let mut out = Replies::none();
                let mut at = Duration::ZERO;
                for _ in 0..count {
                    out.push(Reply {
                        delay: at,
                        bytes: vec![NEGATIVE_RESPONSE_SID, sid, NRC_PENDING],
                    });
                    at = at.saturating_add(every);
                }
                if let Some(bytes) = resp {
                    out.push(Reply { delay: at, bytes });
                }
                out
            }
        }
    }

    fn single_at(resp: Option<Vec<u8>>, delay: Duration) -> Replies {
        let mut out = Replies::none();
        if let Some(bytes) = resp {
            out.push(Reply { delay, bytes });
        }
        out
    }

    /// Flip the low bit of the identifier echo of a positive response.
    fn corrupt_echo(r: &mut [u8]) {
        let Some(&first) = r.first() else { return };
        if first == NEGATIVE_RESPONSE_SID {
            return;
        }
        let req_sid = first.wrapping_sub(0x40);
        let idx = match req_sid {
            0x22 | 0x24 | 0x2E | 0x2F => 2,
            0x31 => 3,
            s if service(s).is_some_and(|i| i.has_subfunction) => 1,
            _ => return,
        };
        if let Some(b) = r.get_mut(idx) {
            *b ^= 0x01;
        }
    }

    fn take_fault(&mut self, req: &[u8]) -> Option<FaultAction> {
        let has_sub = req
            .first()
            .and_then(|s| service(*s))
            .is_some_and(|i| i.has_subfunction);
        let rule = self
            .faults
            .iter_mut()
            .find(|r| r.remaining != Some(0) && r.matcher.matches(req, has_sub))?;
        if let Some(n) = rule.remaining.as_mut() {
            *n -= 1;
        }
        Some(rule.action)
    }

    fn lock_security(&mut self) {
        self.unlocked = None;
        self.pending_seed = None;
    }

    fn expire_s3(&mut self, now: Duration) {
        if self.session != SESSION_DEFAULT
            && let Some(last) = self.last_request
            && now.saturating_sub(last) > self.cfg.s3
        {
            self.session = SESSION_DEFAULT;
            self.lock_security();
        }
    }

    fn execute(&mut self, now: Duration, req: &[u8]) -> Option<Vec<u8>> {
        let sid = req[0];
        match sid {
            0x10 => self.session_control(req),
            0x11 => self.ecu_reset(req),
            0x14 => self.clear_dtc(req),
            0x19 => self.read_dtc(req),
            0x22 => self.read_did(req),
            0x27 => self.security_access(now, req),
            0x2E => self.write_did(req),
            0x3E => Self::tester_present(req),
            _ => nrc(sid, NRC_SERVICE_NOT_SUPPORTED),
        }
    }

    fn session_control(&mut self, req: &[u8]) -> Option<Vec<u8>> {
        if req.len() != 2 {
            return nrc(0x10, NRC_INCORRECT_LENGTH);
        }
        let sub = req[1] & !SPRMIB;
        if sub != SESSION_DEFAULT && sub != SESSION_EXTENDED {
            return nrc(0x10, NRC_SUBFUNCTION_NOT_SUPPORTED);
        }
        if sub != self.session {
            self.lock_security();
        }
        self.session = sub;
        if req[1] & SPRMIB != 0 {
            return None;
        }
        let p2 = self.cfg.p2_ms.to_be_bytes();
        let p2s = u16::try_from(self.cfg.p2_star_ms / 10)
            .unwrap_or(u16::MAX)
            .to_be_bytes();
        Some(vec![0x50, sub, p2[0], p2[1], p2s[0], p2s[1]])
    }

    fn ecu_reset(&mut self, req: &[u8]) -> Option<Vec<u8>> {
        if req.len() != 2 {
            return nrc(0x11, NRC_INCORRECT_LENGTH);
        }
        let sub = req[1] & !SPRMIB;
        if !(0x01..=0x03).contains(&sub) {
            return nrc(0x11, NRC_SUBFUNCTION_NOT_SUPPORTED);
        }
        self.session = SESSION_DEFAULT;
        self.lock_security();
        self.last_request = None;
        (req[1] & SPRMIB == 0).then(|| vec![0x51, sub])
    }

    fn clear_dtc(&mut self, req: &[u8]) -> Option<Vec<u8>> {
        if req.len() != 4 {
            return nrc(0x14, NRC_INCORRECT_LENGTH);
        }
        let group = u32::from_be_bytes([0, req[1], req[2], req[3]]);
        if group == 0x00FF_FFFF {
            self.cfg.dtcs.clear();
        } else if self.cfg.dtcs.iter().any(|(d, _)| *d == group) {
            self.cfg.dtcs.retain(|(d, _)| *d != group);
        } else {
            return nrc(0x14, NRC_OUT_OF_RANGE);
        }
        Some(vec![0x54])
    }

    fn read_dtc(&self, req: &[u8]) -> Option<Vec<u8>> {
        if req.len() < 2 {
            return nrc(0x19, NRC_INCORRECT_LENGTH);
        }
        if req[1] & !SPRMIB != 0x02 {
            return nrc(0x19, NRC_SUBFUNCTION_NOT_SUPPORTED);
        }
        if req.len() != 3 {
            return nrc(0x19, NRC_INCORRECT_LENGTH);
        }
        if req[1] & SPRMIB != 0 {
            return None;
        }
        let mask = req[2];
        let mut out = vec![0x59, 0x02, 0xFF];
        for (dtc, status) in self.cfg.dtcs.iter().filter(|(_, s)| s & mask != 0) {
            let b = dtc.to_be_bytes();
            out.extend_from_slice(&[b[1], b[2], b[3], *status]);
        }
        Some(out)
    }

    fn read_did(&self, req: &[u8]) -> Option<Vec<u8>> {
        if req.len() < 3 || req.len().is_multiple_of(2) || (req.len() - 1) / 2 > MAX_DIDS_PER_READ {
            return nrc(0x22, NRC_INCORRECT_LENGTH);
        }
        let mut out = vec![0x62];
        for pair in req[1..].chunks_exact(2) {
            let did = u16::from_be_bytes([pair[0], pair[1]]);
            let session_byte = [self.session];
            let data: &[u8] = if did == DID_ACTIVE_SESSION {
                &session_byte
            } else {
                match self.cfg.dids.get(&did) {
                    Some(d) => &d.data,
                    None => return nrc(0x22, NRC_OUT_OF_RANGE),
                }
            };
            if out.len() + 2 + data.len() > MAX_PDU_LEN {
                return nrc(0x22, NRC_RESPONSE_TOO_LONG);
            }
            out.extend_from_slice(pair);
            out.extend_from_slice(data);
        }
        Some(out)
    }

    fn write_did(&mut self, req: &[u8]) -> Option<Vec<u8>> {
        if self.session == SESSION_DEFAULT {
            return nrc(0x2E, NRC_SERVICE_NOT_IN_SESSION);
        }
        if req.len() < 4 {
            return nrc(0x2E, NRC_INCORRECT_LENGTH);
        }
        let did = u16::from_be_bytes([req[1], req[2]]);
        let Some(entry) = self.cfg.dids.get_mut(&did) else {
            return nrc(0x2E, NRC_OUT_OF_RANGE);
        };
        if !entry.writable {
            return nrc(0x2E, NRC_OUT_OF_RANGE);
        }
        if req.len() - 3 != entry.data.len() {
            return nrc(0x2E, NRC_INCORRECT_LENGTH);
        }
        entry.data.copy_from_slice(&req[3..]);
        Some(vec![0x6E, req[1], req[2]])
    }

    fn tester_present(req: &[u8]) -> Option<Vec<u8>> {
        if req.len() != 2 {
            return nrc(0x3E, NRC_INCORRECT_LENGTH);
        }
        if req[1] & !SPRMIB != 0x00 {
            return nrc(0x3E, NRC_SUBFUNCTION_NOT_SUPPORTED);
        }
        (req[1] & SPRMIB == 0).then(|| vec![0x7E, 0x00])
    }

    fn next_seed(&mut self) -> u32 {
        let mut x = self.cfg.seed_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.cfg.seed_state = x;
        let b = x.to_be_bytes();
        u32::from_be_bytes([b[0], b[1], b[2], b[3]]) | 1
    }

    fn security_access(&mut self, now: Duration, req: &[u8]) -> Option<Vec<u8>> {
        if self.session == SESSION_DEFAULT {
            return nrc(0x27, NRC_SERVICE_NOT_IN_SESSION);
        }
        if req.len() < 2 {
            return nrc(0x27, NRC_INCORRECT_LENGTH);
        }
        let sub = req[1] & !SPRMIB;
        if sub != SECURITY_SEED_SUB && sub != SECURITY_SEED_SUB + 1 {
            return nrc(0x27, NRC_SUBFUNCTION_NOT_SUPPORTED);
        }
        if self.lockout_until.is_some_and(|until| now < until) {
            return nrc(0x27, NRC_DELAY_NOT_EXPIRED);
        }
        if sub == SECURITY_SEED_SUB {
            if req.len() != 2 {
                return nrc(0x27, NRC_INCORRECT_LENGTH);
            }
            if self.unlocked == Some(sub) {
                return Some(vec![0x67, sub, 0, 0, 0, 0]);
            }
            let seed = self.next_seed();
            self.pending_seed = Some((sub, seed));
            let s = seed.to_be_bytes();
            return Some(vec![0x67, sub, s[0], s[1], s[2], s[3]]);
        }
        let Some((seed_sub, seed)) = self.pending_seed.take() else {
            return nrc(0x27, NRC_SEQUENCE_ERROR);
        };
        if seed_sub + 1 != sub {
            return nrc(0x27, NRC_SEQUENCE_ERROR);
        }
        if req.len() != 6 {
            return nrc(0x27, NRC_INCORRECT_LENGTH);
        }
        let key = u32::from_be_bytes([req[2], req[3], req[4], req[5]]);
        if key == seed ^ self.cfg.security_xor {
            self.unlocked = Some(seed_sub);
            self.failed_keys = 0;
            return Some(vec![0x67, sub]);
        }
        self.failed_keys = self.failed_keys.saturating_add(1);
        if self.failed_keys >= self.cfg.max_key_attempts {
            self.failed_keys = 0;
            self.lockout_until = Some(now.saturating_add(self.cfg.lockout));
            return nrc(0x27, NRC_EXCEEDED_ATTEMPTS);
        }
        nrc(0x27, NRC_INVALID_KEY)
    }
}
