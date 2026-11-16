//! ROI inspection state (operator upgrade B5a, FUNCTIONS.md §B5a) — pure, deterministic, no IO.
//!
//! [`RoiConfig`] is the worker-scoped inspection config (revisioned; survives reopen).
//! [`Inspector`] is the capture-scoped judgment state: one [`RoiJudge`] and baseline per ROI,
//! analysis counters and a bounded transition history. The worker (B5b) feeds it one round per
//! analysed frame (or per liveness change) **while holding the config lock**, so a config edit can
//! never interleave with a round: results of an old config revision are never published.
//! [`EvidencePlan`] decides which exact frames a before/after evidence bundle holds.
//! Every payload this module builds is plain JSON; the worker only adds capture identity.

use std::collections::{BTreeMap, VecDeque};

use loom_proto::num::U64;
use serde_json::{Value, json};

use crate::analysis::{
    Baseline, LumaFrame, Metrics, Observation, RoiJudge, RoiSpec, RoiState, Transition, UnknownReason,
    check_set, observe, roi_rect_px, sample_roi,
};

/// Transitions kept per ROI for `video.roi.list` / `video.wait --roi … --new`.
pub const ROI_HISTORY: usize = 16;
/// Evidence bundles collected at the same time (more → `video.evidence` `skipped/evidence_busy`).
pub const MAX_PENDING_EVIDENCE: usize = 2;
/// Bytes of base64 JPEG data one evidence bundle may hold.
pub const EVIDENCE_BYTES_MAX: usize = 24 << 20;
/// Post-frame collection grace added to `post / fps`.
pub const EVIDENCE_POST_GRACE_MS: u64 = 2000;
/// Pin TTL the worker uses for evidence frames (owner `Evidence(n)`; released when the bundle ends).
pub const EVIDENCE_PIN_TTL_MS: u64 = 30_000;
/// `video.roi_metric` period.
pub const METRIC_PERIOD_MS: u64 = 1000;
/// Evidence manifest schema id.
pub const EVIDENCE_SCHEMA: &str = "loom.video.evidence/1";
/// Wording carried by every ROI payload that could be mistaken for scene understanding.
pub const PIXEL_NOTE: &str = "pixel-change metrics only; no scene understanding";

/// Worker-scoped inspection config. `rev` starts at 0 (empty) and grows by one per applied edit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoiConfig {
    /// Config revision.
    pub rev: u64,
    /// ROIs in insertion order (put of an existing name replaces it in place).
    pub specs: Vec<RoiSpec>,
}

/// Why a config edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// `expect_rev` given and different from the current revision.
    RevMismatch {
        /// Current revision.
        current: u64,
    },
    /// Validation failure (message from [`RoiSpec::from_json`] / [`check_set`] / list parsing).
    Invalid(String),
}

/// Capture-level input condition of a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Capture `live`, health fresh, no STOP latch.
    Live,
    /// Capture not `live` (opening, stalled, stopping, closed, failed) or STOP latched.
    NotLive,
    /// Live but health stale.
    Stale,
}

/// One analysed frame: identity + decoded luma (or the decode error).
#[derive(Debug, Clone, Copy)]
pub struct FrameRef<'a> {
    /// Frame id.
    pub id: u64,
    /// SHA-256 hex of the JPEG.
    pub sha256: &'a str,
    /// Daemon monotonic receive time.
    pub recv_mono_ns: u64,
    /// UTC receive time.
    pub recv_utc_ns: u64,
    /// Decoded luma, or the decode error message.
    pub luma: Result<&'a LumaFrame, &'a str>,
}

/// Capture-scoped judgment state (one per capture; config synced from [`RoiConfig`]).
#[derive(Debug, Default)]
pub struct Inspector {
    synced: Option<u64>,
    specs: Vec<RoiSpec>,
    judges: BTreeMap<String, RoiJudge>,
    baselines: BTreeMap<String, Baseline>,
    last: BTreeMap<String, (Metrics, u64)>,
    history: BTreeMap<String, VecDeque<Value>>,
    last_frame_id: u64,
    analysed: u64,
    skipped: u64,
    decode_errors: u64,
}

/// What triggered an evidence bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceTrigger {
    /// Bundle number (capture-scoped, 1-based; pin owner `Evidence(n)`).
    pub n: u64,
    /// ROI name.
    pub roi: String,
    /// ROI transition that triggered it.
    pub transition_seq: u64,
    /// Trigger frame id.
    pub frame_id: u64,
    /// Trigger frame `recv_mono_ns`.
    pub mono_ns: u64,
}

/// Exact frame ids of one evidence bundle and why it may be partial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePlan {
    /// Trigger.
    pub trigger: EvidenceTrigger,
    /// Pre-trigger frame ids (ascending).
    pub pre: Vec<u64>,
    /// Post-trigger frame ids collected so far (ascending).
    pub post: Vec<u64>,
    /// Post frames wanted.
    pub post_want: u8,
    /// Collection deadline (daemon monotonic ns).
    pub deadline_mono_ns: u64,
    /// Partial reasons in first-seen order, each once.
    pub partial: Vec<String>,
}

impl ConfigError {
    /// Operator message.
    pub fn message(&self) -> String {
        match self {
            ConfigError::RevMismatch { current } => format!("config_rev mismatch: current {current}"),
            ConfigError::Invalid(m) => m.clone(),
        }
    }
    /// Error detail object.
    pub fn detail(&self) -> Value {
        match self {
            ConfigError::RevMismatch { current } => {
                json!({"reason": "config_rev_mismatch", "config_rev": current})
            }
            ConfigError::Invalid(m) => json!({"reason": "invalid", "error": m}),
        }
    }
}

impl RoiConfig {
    /// `{"config_rev", "rois": [spec.to_json() …]}`.
    pub fn json(&self) -> Value {
        json!({"config_rev": self.rev, "rois": self.specs.iter().map(RoiSpec::to_json).collect::<Vec<_>>()})
    }
    /// ROI by name.
    pub fn get(&self, name: &str) -> Option<&RoiSpec> {
        self.specs.iter().find(|s| s.name == name)
    }
    /// Add or replace one ROI.
    pub fn put(&mut self, spec: RoiSpec, expect_rev: Option<u64>) -> Result<(), ConfigError> {
        if let Some(r) = expect_rev
            && r != self.rev
        {
            return Err(ConfigError::RevMismatch { current: self.rev });
        }
        let mut list = self.specs.clone();
        match list.iter_mut().find(|s| s.name == spec.name) {
            Some(slot) => *slot = spec,
            None => list.push(spec),
        }
        check_set(&list).map_err(ConfigError::Invalid)?;
        self.specs = list;
        self.rev += 1;
        Ok(())
    }
    /// Remove one ROI; `Ok(false)` (no revision change) when absent.
    pub fn remove(&mut self, name: &str, expect_rev: Option<u64>) -> Result<bool, ConfigError> {
        if let Some(r) = expect_rev
            && r != self.rev
        {
            return Err(ConfigError::RevMismatch { current: self.rev });
        }
        let Some(i) = self.specs.iter().position(|s| s.name == name) else {
            return Ok(false);
        };
        self.specs.remove(i);
        self.rev += 1;
        Ok(true)
    }
    /// Replace the whole set (recipe re-apply; empty clears).
    pub fn replace(&mut self, specs: Vec<RoiSpec>, expect_rev: Option<u64>) -> Result<(), ConfigError> {
        if let Some(r) = expect_rev
            && r != self.rev
        {
            return Err(ConfigError::RevMismatch { current: self.rev });
        }
        check_set(&specs).map_err(ConfigError::Invalid)?;
        self.specs = specs;
        self.rev += 1;
        Ok(())
    }
    /// Parse a JSON array of ROI objects (each via [`RoiSpec::from_json`]) and check the set.
    pub fn parse_list(v: &Value) -> Result<Vec<RoiSpec>, String> {
        let Some(items) = v.as_array() else {
            return Err("rois: expected an array".into());
        };
        let mut specs = Vec::with_capacity(items.len());
        for (k, item) in items.iter().enumerate() {
            specs.push(RoiSpec::from_json(item).map_err(|e| format!("rois[{k}]: {e}"))?);
        }
        check_set(&specs)?;
        Ok(specs)
    }
}

impl Liveness {
    /// The UNKNOWN reason a non-live round reports (`None` for `Live`).
    pub fn reason(self) -> Option<UnknownReason> {
        match self {
            Liveness::Live => None,
            Liveness::NotLive => Some(UnknownReason::NotLive),
            Liveness::Stale => Some(UnknownReason::Stale),
        }
    }
}

impl Inspector {
    /// Empty, never synced.
    pub fn new() -> Inspector {
        Inspector::default()
    }
    /// Apply a config revision. Returns `video.roi_state` payload fragments (see §B5a-I1).
    pub fn sync(&mut self, cfg: &RoiConfig, now_mono_ns: u64) -> Vec<Value> {
        if self.synced == Some(cfg.rev) {
            return Vec::new();
        }
        self.synced = Some(cfg.rev);
        let keep = |name: &str| cfg.specs.iter().any(|s| s.name == name);
        self.judges.retain(|k, _| keep(k));
        self.baselines.retain(|k, _| keep(k));
        self.last.retain(|k, _| keep(k));
        self.history.retain(|k, _| keep(k));
        let mut out = Vec::new();
        for spec in &cfg.specs {
            if !self.judges.contains_key(&spec.name) {
                self.judges.insert(spec.name.clone(), RoiJudge::new());
                self.history.insert(spec.name.clone(), VecDeque::new());
                continue;
            }
            let Some(prev) = self.specs.iter().find(|p| p.name == spec.name) else {
                continue;
            };
            if prev == spec {
                continue;
            }
            if prev.rect != spec.rect {
                self.baselines.remove(&spec.name);
                self.last.remove(&spec.name);
            }
            let obs = Observation::Unknown {
                mono_ns: now_mono_ns,
                reason: UnknownReason::ConfigChanged,
            };
            let t = self.judges.get_mut(&spec.name).and_then(|j| j.step(spec, obs));
            if let Some(t) = t {
                let v = self.fragment(&spec.name, &t, None, None);
                out.push(self.record(&spec.name, v));
            }
        }
        self.specs = cfg.specs.clone();
        out
    }
    /// One analysis round. Returns `video.roi_state` payload fragments (see §B5a-I2).
    pub fn round(&mut self, liveness: Liveness, frame: Option<FrameRef<'_>>, now_mono_ns: u64) -> Vec<Value> {
        if self.synced.is_none() || self.specs.is_empty() {
            return Vec::new();
        }
        let specs = self.specs.clone();
        let mut out = Vec::new();
        if let Some(reason) = liveness.reason() {
            for spec in &specs {
                let t = self.judges.get_mut(&spec.name).and_then(|j| {
                    j.step(
                        spec,
                        Observation::Unknown {
                            mono_ns: now_mono_ns,
                            reason,
                        },
                    )
                });
                if let Some(t) = t {
                    let v = self.fragment(&spec.name, &t, None, None);
                    out.push(self.record(&spec.name, v));
                }
            }
            return out;
        }
        let Some(f) = frame else {
            return out;
        };
        if f.id <= self.last_frame_id {
            return out;
        }
        if self.last_frame_id > 0 {
            self.skipped += f.id - self.last_frame_id - 1;
        }
        self.last_frame_id = f.id;
        self.analysed += 1;
        if f.luma.is_err() {
            self.decode_errors += 1;
        }
        for spec in &specs {
            let (obs, m) = match f.luma {
                Err(_) => (
                    Observation::Unknown {
                        mono_ns: f.recv_mono_ns,
                        reason: UnknownReason::DecodeError,
                    },
                    None,
                ),
                Ok(l) => observe(spec, l, self.baselines.get(&spec.name), f.recv_mono_ns),
            };
            if let Some(m) = m {
                self.last.insert(spec.name.clone(), (m, f.id));
            }
            let t = self.judges.get_mut(&spec.name).and_then(|j| j.step(spec, obs));
            if let Some(t) = t {
                let v = self.fragment(&spec.name, &t, Some(&f), m.as_ref());
                out.push(self.record(&spec.name, v));
            }
        }
        out
    }
    /// Set the baseline of one ROI (`Some(name)`) or of every synced ROI from one exact frame.
    pub fn set_baseline(
        &mut self,
        roi: Option<&str>,
        frame_id: u64,
        sha256: &str,
        luma: &LumaFrame,
    ) -> Result<Vec<Value>, String> {
        if self.specs.is_empty() {
            return Err("roi: no ROIs configured".into());
        }
        if let Some(name) = roi
            && !self.specs.iter().any(|s| s.name == name)
        {
            return Err(format!("roi.name: unknown ROI {name}"));
        }
        let targets: Vec<RoiSpec> = self
            .specs
            .iter()
            .filter(|s| roi.is_none_or(|n| s.name == n))
            .cloned()
            .collect();
        let mut items = Vec::new();
        for spec in &targets {
            match roi_rect_px(&spec.rect, luma.width, luma.height) {
                None => items.push(json!({"roi": spec.name, "baseline": null, "reason": "roi_too_small"})),
                Some(px) => {
                    let b = Baseline {
                        frame_id,
                        sha256: sha256.to_owned(),
                        samples: sample_roi(luma, &px),
                    };
                    items.push(json!({"roi": spec.name, "baseline": b.to_json(), "reason": null}));
                    self.baselines.insert(spec.name.clone(), b);
                }
            }
        }
        Ok(items)
    }
    /// Current transition count of one ROI (`None` when not configured).
    pub fn seq(&self, roi: &str) -> Option<u64> {
        self.judges.get(roi).map(RoiJudge::seq)
    }
    /// Full state for `video.status` / `video.roi.list` (with history). See §B5a-I5.
    pub fn json(&self) -> Value {
        let analysis = json!({"analysed": self.analysed, "skipped": self.skipped, "decode_errors": self.decode_errors,
                              "last_frame_id": (self.last_frame_id != 0).then_some(self.last_frame_id)});
        let rois: Vec<Value> = self
            .specs
            .iter()
            .map(|spec| {
                let j = self.judges.get(&spec.name).cloned().unwrap_or_default();
                let last = self.last.get(&spec.name);
                json!({"roi": spec.name, "state": j.state().as_str(), "reason": j.reason().map(UnknownReason::as_str),
                       "transition_seq": j.seq(), "candidate": j.to_json()["candidate"],
                       "baseline": self.baselines.get(&spec.name).map(Baseline::to_json),
                       "metrics": last.map(|(m, _)| m.to_json()), "metrics_frame_id": last.map(|(_, id)| *id),
                       "history": self.history.get(&spec.name).map(|h| h.iter().cloned().collect::<Vec<_>>()).unwrap_or_default()})
            })
            .collect();
        json!({"config_rev": self.synced, "analysis": analysis, "rois": rois, "note": PIXEL_NOTE})
    }
    /// Compact 1 Hz `video.roi_metric` body (no history). See §B5a-I6.
    pub fn metric_json(&self) -> Value {
        let analysis = json!({"analysed": self.analysed, "skipped": self.skipped, "decode_errors": self.decode_errors,
                              "last_frame_id": (self.last_frame_id != 0).then_some(self.last_frame_id)});
        let rois: Vec<Value> = self
            .specs
            .iter()
            .map(|spec| {
                let j = self.judges.get(&spec.name).cloned().unwrap_or_default();
                let last = self.last.get(&spec.name);
                json!({"roi": spec.name, "state": j.state().as_str(), "reason": j.reason().map(UnknownReason::as_str),
                       "transition_seq": j.seq(), "metrics": last.map(|(m, _)| m.to_json()),
                       "metrics_frame_id": last.map(|(_, id)| *id)})
            })
            .collect();
        json!({"config_rev": self.synced, "analysis": analysis, "rois": rois, "note": PIXEL_NOTE})
    }
    /// One `video.roi_state` payload fragment for a transition of `roi`. See §B5a-I7.
    fn fragment(
        &self,
        roi: &str,
        t: &Transition,
        frame: Option<&FrameRef<'_>>,
        metrics: Option<&Metrics>,
    ) -> Value {
        let evidence = t.to == RoiState::Changed
            && self
                .specs
                .iter()
                .find(|s| s.name == roi)
                .is_some_and(|s| s.evidence);
        json!({"roi": roi, "config_rev": self.synced, "state": t.to.as_str(), "reason": t.reason.map(UnknownReason::as_str),
               "transition_seq": t.seq, "transition": t.to_json(),
               "frame_id": frame.map(|f| f.id), "sha256": frame.map(|f| f.sha256),
               "recv_utc_ns": frame.map(|f| U64(f.recv_utc_ns)), "recv_mono_ns": frame.map(|f| U64(f.recv_mono_ns)),
               "metrics": metrics.map(Metrics::to_json),
               "baseline": self.baselines.get(roi).map(|b| json!({"frame_id": b.frame_id, "sha256": b.sha256})),
               "evidence": evidence, "note": PIXEL_NOTE})
    }
    /// Append `v` to the history of `roi` (bounded by [`ROI_HISTORY`]) and return it. See §B5a-I7.
    fn record(&mut self, roi: &str, v: Value) -> Value {
        let h = self.history.entry(roi.to_owned()).or_default();
        h.push_back(v.clone());
        while h.len() > ROI_HISTORY {
            h.pop_front();
        }
        v
    }
}

impl EvidencePlan {
    /// Plan pre frames from what the ring still holds and the post deadline. See §B5a-E1.
    pub fn new(trigger: EvidenceTrigger, spec: &RoiSpec, fps: u32, ring_first: Option<u64>) -> EvidencePlan {
        let pre_n = u64::from(spec.evidence_pre);
        let start = trigger
            .frame_id
            .saturating_sub(pre_n)
            .max(1)
            .max(ring_first.unwrap_or(trigger.frame_id));
        let pre: Vec<u64> = (start..trigger.frame_id).collect();
        let partial = if (pre.len() as u64) < pre_n {
            vec!["pre_unavailable".to_owned()]
        } else {
            Vec::new()
        };
        let post_want = spec.evidence_post;
        let wait_ms =
            EVIDENCE_POST_GRACE_MS.saturating_add(u64::from(post_want) * 1000 / u64::from(fps.max(1)));
        let deadline_mono_ns = trigger.mono_ns.saturating_add(wait_ms.saturating_mul(1_000_000));
        EvidencePlan {
            trigger,
            pre,
            post: Vec::new(),
            post_want,
            deadline_mono_ns,
            partial,
        }
    }
    /// Post ids newly available up to `latest_id` (appended to `post` and returned).
    pub fn offer(&mut self, latest_id: u64) -> Vec<u64> {
        let next = self.trigger.frame_id + 1 + self.post.len() as u64;
        let end = latest_id.min(self.trigger.frame_id + u64::from(self.post_want));
        if next > end {
            return Vec::new();
        }
        let ids: Vec<u64> = (next..=end).collect();
        self.post.extend(&ids);
        ids
    }
    /// Remove a frame that could not be held and record `reason` (once).
    pub fn drop_frame(&mut self, id: u64, reason: &str) {
        self.pre.retain(|p| *p != id);
        if !self.partial.iter().any(|r| r == reason) {
            self.partial.push(reason.to_owned());
        }
    }
    /// Every wanted post frame collected.
    pub fn complete(&self) -> bool {
        self.post.len() == usize::from(self.post_want)
    }
    /// Ready to write: complete, or the deadline passed.
    pub fn due(&self, now_mono_ns: u64) -> bool {
        self.complete() || now_mono_ns >= self.deadline_mono_ns
    }
    /// Final frame list `(role, id)` within [`EVIDENCE_BYTES_MAX`] (base64 size), ascending id.
    pub fn fit(&mut self, sizes: &BTreeMap<u64, usize>) -> Vec<(&'static str, u64)> {
        let add = |partial: &mut Vec<String>, r: &str| {
            if !partial.iter().any(|x| x == r) {
                partial.push(r.to_owned());
            }
        };
        if !self.complete() {
            add(&mut self.partial, "post_deadline");
        }
        let mut cands: Vec<(&'static str, u64)> = vec![("trigger", self.trigger.frame_id)];
        cands.extend(self.pre.iter().rev().map(|id| ("pre", *id)));
        cands.extend(self.post.iter().map(|id| ("post", *id)));
        let mut used = 0usize;
        let mut kept = Vec::new();
        for (role, id) in cands {
            let Some(size) = sizes.get(&id) else {
                add(&mut self.partial, "frame_unavailable");
                continue;
            };
            let b64 = size.saturating_add(2) / 3 * 4;
            if used.saturating_add(b64) > EVIDENCE_BYTES_MAX {
                add(&mut self.partial, "bytes_cap");
                continue;
            }
            used += b64;
            kept.push((role, id));
        }
        kept.sort_by_key(|(_, id)| *id);
        kept
    }
}
