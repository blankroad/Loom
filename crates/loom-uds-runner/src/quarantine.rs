//! Unresolved set U (plan §3.4 D19): requests that timed out and whose complete final response
//! has not arrived yet.
//!
//! Overlap is by **SID** — a negative response `7F <SID> <NRC>` carries no DID or sub-function,
//! so echo checks alone cannot tell a late NRC from the answer to a new request with the same SID.
//! Release evidence only: a late complete final response, a generation change (reconnect +
//! re-activation), a confirmed ECU reset, or a verified-profile `late_response_max`. Never LRU/TTL
//! eviction, never a probe success.

use std::collections::VecDeque;

use loom_uds_codec::classify_response;
use tokio::time::{Duration, Instant};

/// Most unresolved requests per target. Full: every new request to the target is refused with
/// `E_QUARANTINE` (reason capacity) until evidence releases an entry — entries are never evicted.
pub const MAX_UNRESOLVED_PER_TARGET: usize = 256;
/// Request bytes kept per entry: enough for every echo check of the codec (SID + 4 bytes).
pub const UNRESOLVED_REQ_PREFIX: usize = 8;

/// One unresolved request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedEntry {
    pub sid: u8,
    prefix: [u8; UNRESOLVED_REQ_PREFIX],
    len: usize,
    /// When the request was declared unresolved (timeout).
    pub since: Instant,
}

impl UnresolvedEntry {
    /// The stored request prefix.
    pub fn request(&self) -> &[u8] {
        self.prefix.get(..self.len).unwrap_or_default()
    }
}

/// Why entries left U.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseCause {
    /// Its complete final response (positive, or `7F` other than `0x78`) arrived late.
    LateFinalResponse,
    /// Reconnect + re-activation: old-link responses cannot arrive.
    GenerationChanged,
    /// ECU reset confirmed by a positive `0x51` response.
    EcuReset,
    /// Verified profile `late_response_max` elapsed.
    LateWindowElapsed,
}

/// The unresolved set of one target.
#[derive(Debug, Clone, Default)]
pub struct Unresolved {
    entries: VecDeque<UnresolvedEntry>,
}

impl Unresolved {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.entries.len() >= MAX_UNRESOLVED_PER_TARGET
    }

    pub fn entries(&self) -> impl Iterator<Item = &UnresolvedEntry> {
        self.entries.iter()
    }

    /// A new request with this SID cannot be told apart from a late response of U.
    pub fn overlaps(&self, sid: u8) -> bool {
        self.entries.iter().any(|e| e.sid == sid)
    }

    /// Newest entry time (start of the quiet period).
    pub fn newest(&self) -> Option<Instant> {
        self.entries.iter().map(|e| e.since).max()
    }

    /// Add a timed-out request. Returns `false` when full (callers check [`Self::is_full`] before
    /// sending, so this only happens if that check was skipped).
    pub fn insert(&mut self, pdu: &[u8], now: Instant) -> bool {
        let Some(&sid) = pdu.first() else {
            return false;
        };
        if self.is_full() {
            return false;
        }
        let len = pdu.len().min(UNRESOLVED_REQ_PREFIX);
        let mut prefix = [0u8; UNRESOLVED_REQ_PREFIX];
        prefix[..len].copy_from_slice(&pdu[..len]);
        self.entries.push_back(UnresolvedEntry {
            sid,
            prefix,
            len,
            since: now,
        });
        true
    }

    /// Release the oldest entry for which `resp` is a complete final response. Returns its SID.
    /// `7F xx 78` and foreign/malformed responses release nothing.
    pub fn release_by_response(&mut self, resp: &[u8]) -> Option<u8> {
        let i = self
            .entries
            .iter()
            .position(|e| classify_response(e.request(), resp).is_final())?;
        self.entries.remove(i).map(|e| e.sid)
    }

    /// Release everything (generation change, ECU reset). Returns the number released.
    pub fn clear(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }

    /// Release entries older than `max_age` (verified profiles only). Returns the number released.
    pub fn expire(&mut self, now: Instant, max_age: Duration) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|e| now.saturating_duration_since(e.since) < max_age);
        before - self.entries.len()
    }
}
