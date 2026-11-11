//! Deterministic fault injection.

use std::time::Duration;

/// Which requests a rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultMatch {
    pub sid: u8,
    /// DID (0x22 first DID, 0x2E, 0x2F), routine id (0x31) or sub-function without bit 7
    /// (sub-function services). `None` matches every request of `sid`.
    pub id: Option<u16>,
}

impl FaultMatch {
    pub fn sid(sid: u8) -> Self {
        Self { sid, id: None }
    }

    pub fn sid_id(sid: u8, id: u16) -> Self {
        Self { sid, id: Some(id) }
    }

    pub(crate) fn matches(&self, req: &[u8], has_subfunction: bool) -> bool {
        if req.first() != Some(&self.sid) {
            return false;
        }
        let Some(want) = self.id else { return true };
        let got = match self.sid {
            0x31 => req.get(2..4).map(|b| u16::from_be_bytes([b[0], b[1]])),
            _ if has_subfunction => req.get(1).map(|b| u16::from(b & 0x7F)),
            _ => req.get(1..3).map(|b| u16::from_be_bytes([b[0], b[1]])),
        };
        got == Some(want)
    }
}

/// What happens to a matching request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultAction {
    /// Every reply is delayed by this much more.
    Delay(Duration),
    /// Answer `7F <sid> <nrc>`; the request is **not** executed.
    ForceNrc(u8),
    /// `count` × `7F <sid> 78`, one every `every`, then the real answer `every` after the last.
    /// The request is executed.
    PendingThen { count: u8, every: Duration },
    /// The request is executed but every reply is lost (models outcome-unknown writes).
    Drop,
    /// The final reply is sent exactly `after` the request (use a value past P2/P2*).
    Late { after: Duration },
    /// Positive replies carry a wrong identifier echo (byte 2 flipped): the runner must see
    /// them as foreign. The request is executed.
    WrongEcho,
}

/// A rule: match + action + how many more times it fires (`None` = always).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultRule {
    pub matcher: FaultMatch,
    pub action: FaultAction,
    pub remaining: Option<u32>,
}

impl FaultRule {
    pub fn always(matcher: FaultMatch, action: FaultAction) -> Self {
        Self {
            matcher,
            action,
            remaining: None,
        }
    }

    pub fn times(matcher: FaultMatch, action: FaultAction, n: u32) -> Self {
        Self {
            matcher,
            action,
            remaining: Some(n),
        }
    }
}
