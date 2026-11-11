//! `TargetSession`: what Loom believes about one ECU (plan §3.3). Pure state machine — time and
//! events come in as arguments. It is the single source for `uds.state` events.

use std::time::Duration;

use loom_proto::event::Origin;
use loom_uds_codec::response::parse_session_response;
use loom_uds_codec::{ResponseKind, SPRMIB, classify_response};
use schemars::JsonSchema;
use serde::Serialize;

use crate::payload::{
    Confidence, LeasePayload, LinkPayload, LinkState, SecurityPayload, SessionPayload, StatePayload,
    TesterPresentPayload,
};

/// ISO default session id.
pub const SESSION_DEFAULT: u8 = 0x01;
/// Default S3server when the ECU profile does not say otherwise.
pub const DEFAULT_S3: Duration = Duration::from_secs(5);
/// Default P2server_max / P2*server_max until a `0x50` response says otherwise.
pub const DEFAULT_P2_MS: u16 = 50;
pub const DEFAULT_P2_STAR_MS: u32 = 5000;

/// Name of a session id (`0x03 → "extended"`, unknown → `"0x40"`).
pub fn session_name(id: u8) -> String {
    match id {
        0x01 => "default".into(),
        0x02 => "programming".into(),
        0x03 => "extended".into(),
        0x04 => "safetySystem".into(),
        other => format!("0x{other:02X}"),
    }
}

/// Who holds the write lease on a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Lease {
    pub owner_origin: Origin,
    pub actor_id: String,
    /// Connection id for users, LOOM_CLIENT for agents.
    pub client_id: String,
}

/// Belief about one ECU.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetSession {
    target: String,
    /// 0 = never connected; +1 on every link-up (first connect and each reconnect).
    generation: u32,
    link: LinkState,
    link_cause: Option<String>,
    session: u8,
    session_confidence: Confidence,
    security: Option<u8>,
    security_confidence: Confidence,
    tester_present: Option<Duration>,
    p2_ms: u16,
    p2_star_ms: u32,
    s3: Duration,
    last_activity: Option<Duration>,
    lease: Option<Lease>,
    backend: Option<String>,
    addr: Option<String>,
    tp_ok: u32,
}

impl TargetSession {
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            generation: 0,
            link: LinkState::Down,
            link_cause: None,
            session: SESSION_DEFAULT,
            session_confidence: Confidence::Assumed,
            security: None,
            security_confidence: Confidence::Assumed,
            tester_present: None,
            p2_ms: DEFAULT_P2_MS,
            p2_star_ms: DEFAULT_P2_STAR_MS,
            s3: DEFAULT_S3,
            last_activity: None,
            lease: None,
            backend: None,
            addr: None,
            tp_ok: 0,
        }
    }

    /// ECU profile S3server.
    pub fn with_s3(mut self, s3: Duration) -> Self {
        self.s3 = s3;
        self
    }

    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn generation(&self) -> u32 {
        self.generation
    }
    pub fn link(&self) -> LinkState {
        self.link
    }
    pub fn session(&self) -> (u8, Confidence) {
        (self.session, self.session_confidence)
    }
    pub fn security(&self) -> (Option<u8>, Confidence) {
        (self.security, self.security_confidence)
    }
    pub fn timing(&self) -> (u16, u32) {
        (self.p2_ms, self.p2_star_ms)
    }
    pub fn lease(&self) -> Option<&Lease> {
        self.lease.as_ref()
    }

    fn reset_to_assumed_default(&mut self) {
        self.session = SESSION_DEFAULT;
        self.session_confidence = Confidence::Assumed;
        self.security = None;
        self.security_confidence = Confidence::Assumed;
        self.tester_present = None;
        self.p2_ms = DEFAULT_P2_MS;
        self.p2_star_ms = DEFAULT_P2_STAR_MS;
    }

    pub fn link_connecting(&mut self) {
        self.link = LinkState::Connecting;
        self.link_cause = None;
    }

    /// Link (re)established: generation + 1, session assumed default, security locked.
    pub fn link_up(&mut self, now: Duration) {
        self.link_cause = Some(
            if self.generation == 0 {
                "connected"
            } else {
                "reconnected"
            }
            .into(),
        );
        self.generation = self.generation.saturating_add(1);
        self.link = LinkState::Up;
        self.reset_to_assumed_default();
        self.last_activity = Some(now);
    }

    /// Link lost: what we believed about the session can no longer be trusted.
    pub fn link_down(&mut self, cause: impl Into<String>) {
        self.link = LinkState::Down;
        self.link_cause = Some(cause.into());
        self.session_confidence = Confidence::Stale;
        self.security_confidence = Confidence::Stale;
        self.tester_present = None;
    }

    pub fn set_tester_present(&mut self, interval: Option<Duration>) {
        if interval.is_some() != self.tester_present.is_some() {
            self.tp_ok = 0;
        }
        self.tester_present = interval;
    }

    /// A TesterPresent exchange succeeded (positive or suppressed-ok).
    pub fn on_tester_present_ok(&mut self, now: Duration) {
        self.last_activity = Some(now);
        self.tp_ok = self.tp_ok.saturating_add(1);
    }

    /// Transport and display address, e.g. `("doip", "DoIP 0x1001")`.
    pub fn set_endpoint(&mut self, backend: impl Into<String>, addr: impl Into<String>) {
        self.backend = Some(backend.into());
        self.addr = Some(addr.into());
    }

    pub fn set_lease(&mut self, lease: Option<Lease>) {
        self.lease = lease;
    }

    /// Time passes: a non-default session with no traffic for longer than S3 has probably ended.
    /// Returns `true` if the state changed.
    pub fn tick(&mut self, now: Duration) -> bool {
        let expired = self.session != SESSION_DEFAULT
            && self.session_confidence != Confidence::Stale
            && self.tester_present.is_none()
            && self
                .last_activity
                .is_some_and(|t| now.saturating_sub(t) > self.s3);
        if expired {
            self.session_confidence = Confidence::Stale;
            self.security_confidence = Confidence::Stale;
        }
        expired
    }

    /// A request went out (any outcome): it counts as activity for S3.
    pub fn on_sent(&mut self, now: Duration) {
        self.last_activity = Some(now);
    }

    /// A suppressed-positive request (`SPRMIB`) completed without a negative response:
    /// apply its effect with `assumed` confidence.
    pub fn on_suppressed_ok(&mut self, now: Duration, req: &[u8]) {
        self.last_activity = Some(now);
        match req {
            [0x10, sub, ..] if sub & SPRMIB != 0 => self.enter_session(sub & !SPRMIB, Confidence::Assumed),
            [0x11, sub, ..] if sub & SPRMIB != 0 => self.reset_to_assumed_default(),
            _ => {}
        }
    }

    fn enter_session(&mut self, id: u8, confidence: Confidence) {
        if id != self.session {
            self.security = None;
            self.security_confidence = confidence;
        }
        self.session = id;
        self.session_confidence = confidence;
    }

    /// Apply the final response of `req`. Returns `true` if the published state changed.
    pub fn on_response(&mut self, now: Duration, req: &[u8], resp: &[u8]) -> bool {
        let before = self.clone();
        self.last_activity = Some(now);
        match classify_response(req, resp) {
            ResponseKind::Positive => match req.first() {
                Some(0x10) => {
                    if let Ok(parsed) = parse_session_response(resp) {
                        self.enter_session(parsed.session, Confidence::Confirmed);
                        if let Some(t) = parsed.timing {
                            self.p2_ms = t.p2_ms;
                            self.p2_star_ms = t.p2_star_ms;
                        }
                    }
                }
                Some(0x11) => self.reset_to_assumed_default(),
                Some(0x27) => {
                    if let Some(&sub) = req.get(1) {
                        let sub = sub & !SPRMIB;
                        if sub.is_multiple_of(2) && sub > 0 {
                            self.security = Some(sub - 1);
                            self.security_confidence = Confidence::Confirmed;
                        }
                    }
                }
                _ => {}
            },
            ResponseKind::Negative { nrc: 0x7E | 0x7F } => {
                self.session_confidence = Confidence::Stale;
            }
            ResponseKind::Negative { nrc: 0x33 } => {
                self.security = None;
                self.security_confidence = Confidence::Confirmed;
            }
            _ => {}
        }
        before.state_fields() != self.state_fields()
    }

    fn state_fields(&self) -> (u32, LinkState, u8, Confidence, Option<u8>, Confidence, u16, u32) {
        (
            self.generation,
            self.link,
            self.session,
            self.session_confidence,
            self.security,
            self.security_confidence,
            self.p2_ms,
            self.p2_star_ms,
        )
    }

    /// `uds.state` payload (ui-uds answer #3).
    pub fn state_payload(&self) -> StatePayload {
        StatePayload {
            target: self.target.clone(),
            generation: self.generation,
            backend: self.backend.clone(),
            addr: self.addr.clone(),
            link: LinkPayload {
                state: self.link,
                cause: self.link_cause.clone(),
            },
            session: SessionPayload {
                id: self.session,
                name: session_name(self.session),
                confidence: self.session_confidence,
            },
            security: SecurityPayload {
                level: self.security,
                confidence: self.security_confidence,
            },
            tester_present: self.tester_present.map(|d| TesterPresentPayload {
                active: true,
                interval_ms: u32::try_from(d.as_millis()).unwrap_or(u32::MAX),
                ok_count: self.tp_ok,
            }),
            p2_ms: self.p2_ms,
            p2_star_ms: self.p2_star_ms,
            lease: self.lease.as_ref().map(|l| LeasePayload {
                owner_origin: l.owner_origin,
                actor_id: l.actor_id.clone(),
                client_id: l.client_id.clone(),
            }),
        }
    }
}
