//! ROI analysis (operator upgrade B4, FUNCTIONS.md §B4) — pure, deterministic, no IO.
//!
//! JPEG → luma plane ([`decode_luma`], worker feature only), named rectangular ROIs in normalized
//! coordinates ([`RoiSpec`]), bounded grid sampling ([`sample_roi`]), explainable pixel metrics
//! against a baseline ([`metrics`]) and the per-ROI judgment state machine ([`RoiJudge`]).
//! Everything here measures pixel change only; it never claims scene understanding.
//! Judgments are computed once (in the worker, B5) and shared with UI and agents as events.

use serde_json::{Value, json};
#[cfg(feature = "worker")]
use zune_jpeg::JpegDecoder;
#[cfg(feature = "worker")]
use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

/// ROIs per worker (inspection config).
pub const MAX_ROIS: usize = 8;
/// Samples per ROI (grid subsampling bound).
pub const MAX_SAMPLES: usize = 4096;
/// Largest decoded width/height (pixels).
pub const MAX_DECODE_DIM: usize = 4096;
/// Analysis rate cap (frames analysed per second, B5).
pub const ANALYSIS_HZ: u64 = 10;
/// Smallest analysis gap that turns a judged ROI UNKNOWN (`analysis_gap`); the effective limit is
/// `max(dwell_ms, GAP_MIN_MS)`.
pub const GAP_MIN_MS: u64 = 1000;
/// ROI name length bound (chars).
pub const ROI_NAME_MAX: usize = 32;
/// Default per-sample absolute luma difference counted by `changed_px`.
pub const PIXEL_THRESHOLD_DEFAULT: u8 = 24;
/// Default dwell (ms) a candidate state must hold continuously.
pub const DWELL_DEFAULT_MS: u64 = 300;
/// Default cooldown (ms) between normal↔changed transitions.
pub const COOLDOWN_DEFAULT_MS: u64 = 1000;
/// Upper bound of `dwell_ms` and `cooldown_ms`.
pub const TIMING_MAX_MS: u64 = 60_000;
/// Upper bound of `evidence_pre` / `evidence_post`.
pub const EVIDENCE_FRAMES_MAX: u8 = 8;
/// Default `evidence_pre` / `evidence_post`.
pub const EVIDENCE_FRAMES_DEFAULT: u8 = 2;

/// A decoded luma plane (row-major, `width * height` bytes, full range 0..=255).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LumaFrame {
    /// Pixels per row.
    pub width: u32,
    /// Rows.
    pub height: u32,
    /// `width * height` luma samples.
    pub luma: Vec<u8>,
}

/// Magnitude metric an ROI is judged on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// `|mean(cur) − mean(base)|`, luma levels 0..=255.
    LumaDelta,
    /// Mean per-sample `|cur − base|` as % of 255 (0..=100).
    MeanAbsDiff,
    /// % of samples with `|cur − base| ≥ pixel_threshold` (0..=100).
    ChangedPx,
}

/// Normalized ROI rectangle (fractions of frame width/height, origin top-left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoiRect {
    /// Left edge, 0 ≤ x < 1.
    pub x: f64,
    /// Top edge, 0 ≤ y < 1.
    pub y: f64,
    /// Width, 0 < w, x + w ≤ 1.
    pub w: f64,
    /// Height, 0 < h, y + h ≤ 1.
    pub h: f64,
}

/// One named ROI with its judgment parameters (validated; see [`RoiSpec::from_json`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RoiSpec {
    /// Machine name `[A-Za-z0-9_-]{1,32}`, unique within the set.
    pub name: String,
    /// Rectangle.
    pub rect: RoiRect,
    /// Judged metric.
    pub metric: Metric,
    /// Enter threshold (`value ≥ enter` → changed candidate).
    pub enter: f64,
    /// Exit threshold (`value ≤ exit` → normal candidate), `0 ≤ exit ≤ enter`.
    pub exit: f64,
    /// Candidate hold time (ms).
    pub dwell_ms: u64,
    /// Minimum time between normal↔changed transitions (ms).
    pub cooldown_ms: u64,
    /// `changed_px` per-sample threshold, 1..=255.
    pub pixel_threshold: u8,
    /// Collect an evidence bundle on → changed (B5).
    pub evidence: bool,
    /// Frames before the trigger in the bundle, 0..=8.
    pub evidence_pre: u8,
    /// Frames after the trigger in the bundle, 0..=8.
    pub evidence_post: u8,
}

/// ROI rectangle in pixels of one frame size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    /// Left column.
    pub x: u32,
    /// Top row.
    pub y: u32,
    /// Columns (≥ 2).
    pub w: u32,
    /// Rows (≥ 2).
    pub h: u32,
}

/// Grid samples of one ROI of one frame (geometry is part of the identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Samples {
    /// Source frame width.
    pub width: u32,
    /// Source frame height.
    pub height: u32,
    /// Sampled rectangle.
    pub rect: PxRect,
    /// Grid step (pixels) in both axes.
    pub step: u32,
    /// Luma at `(rect.x + i*step, rect.y + j*step)`, row-major (j outer, i inner).
    pub values: Vec<u8>,
}

/// Metrics of current samples against baseline samples (raw, unrounded).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Mean luma of the current samples (0..=255).
    pub mean_luma: f64,
    /// Signed `mean(cur) − mean(base)` (−255..=255); judged as its absolute value.
    pub luma_delta: f64,
    /// Mean `|cur − base|` as % of 255.
    pub mean_abs_diff: f64,
    /// % of samples with `|cur − base| ≥ pixel_threshold`.
    pub changed_px: f64,
}

/// Baseline of one ROI: identity of the exact frame plus its samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// Frame id (capture-scoped).
    pub frame_id: u64,
    /// SHA-256 hex of the baseline JPEG.
    pub sha256: String,
    /// Samples taken with the ROI rect at baseline time.
    pub samples: Samples,
}

/// Judgment state of one ROI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoiState {
    /// No trustworthy judgment (see [`UnknownReason`]).
    Unknown,
    /// Below the exit threshold (or below enter, coming from unknown) for the dwell.
    Normal,
    /// At/above the enter threshold for the dwell.
    Changed,
}

/// Why an ROI is UNKNOWN. Never inferred away: the next judgment needs a fresh dwell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    /// Capture not `live` (opening, stalled, stopping, closed, failed, STOP latched).
    NotLive,
    /// Health stale (no frame within the stall window).
    Stale,
    /// No baseline for this ROI in this capture.
    NoBaseline,
    /// Frame size or ROI pixel rect differs from the baseline.
    BaselineMismatch,
    /// ROI smaller than 2×2 pixels at this frame size.
    RoiTooSmall,
    /// Frame could not be decoded.
    DecodeError,
    /// Analysed frames further apart than `max(dwell_ms, GAP_MIN_MS)`.
    AnalysisGap,
    /// ROI set / parameters changed (results of the old config discarded).
    ConfigChanged,
}

/// One analysis input for the judge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Observation {
    /// A metric value measured on a frame received at `mono_ns`.
    Value {
        /// Frame `recv_mono_ns`.
        mono_ns: u64,
        /// [`Metrics::value`] of the ROI's metric.
        value: f64,
    },
    /// No trustworthy value at `mono_ns`.
    Unknown {
        /// Daemon monotonic time.
        mono_ns: u64,
        /// Why.
        reason: UnknownReason,
    },
}

/// A state change emitted by [`RoiJudge::step`] (also emitted for a change of UNKNOWN reason).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    /// Previous state.
    pub from: RoiState,
    /// Previous UNKNOWN reason (`None` unless `from == Unknown`).
    pub from_reason: Option<UnknownReason>,
    /// New state.
    pub to: RoiState,
    /// New UNKNOWN reason (`None` unless `to == Unknown`).
    pub reason: Option<UnknownReason>,
    /// Per-ROI transition counter after this change (first transition = 1).
    pub seq: u64,
    /// Observation time.
    pub mono_ns: u64,
    /// Value that completed the dwell (`None` for UNKNOWN transitions).
    pub value: Option<f64>,
}

/// Candidate state held since `since_mono_ns` (private).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Candidate {
    to: RoiState,
    since_mono_ns: u64,
}

/// Per-ROI judgment state machine (FUNCTIONS.md §B4-J). Starts UNKNOWN(`config_changed`), seq 0.
#[derive(Debug, Clone, PartialEq)]
pub struct RoiJudge {
    state: RoiState,
    reason: Option<UnknownReason>,
    seq: u64,
    candidate: Option<Candidate>,
    last_mono_ns: Option<u64>,
    last_switch_mono_ns: Option<u64>,
}

impl Default for RoiJudge {
    fn default() -> Self {
        RoiJudge::new()
    }
}

impl Metric {
    /// Wire name: `luma_delta` | `mean_abs_diff` | `changed_px`.
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::LumaDelta => "luma_delta",
            Metric::MeanAbsDiff => "mean_abs_diff",
            Metric::ChangedPx => "changed_px",
        }
    }
    /// Parse a wire name.
    pub fn parse(s: &str) -> Option<Metric> {
        match s {
            "luma_delta" => Some(Metric::LumaDelta),
            "mean_abs_diff" => Some(Metric::MeanAbsDiff),
            "changed_px" => Some(Metric::ChangedPx),
            _ => None,
        }
    }
    /// Largest meaningful threshold (255 for `luma_delta`, 100 for the percentages).
    pub fn max_threshold(self) -> f64 {
        match self {
            Metric::LumaDelta => 255.0,
            Metric::MeanAbsDiff | Metric::ChangedPx => 100.0,
        }
    }
}

impl RoiState {
    /// Wire name: `unknown` | `normal` | `changed`.
    pub fn as_str(self) -> &'static str {
        match self {
            RoiState::Unknown => "unknown",
            RoiState::Normal => "normal",
            RoiState::Changed => "changed",
        }
    }
}

impl UnknownReason {
    /// Wire name (snake_case of the variant).
    pub fn as_str(self) -> &'static str {
        match self {
            UnknownReason::NotLive => "not_live",
            UnknownReason::Stale => "stale",
            UnknownReason::NoBaseline => "no_baseline",
            UnknownReason::BaselineMismatch => "baseline_mismatch",
            UnknownReason::RoiTooSmall => "roi_too_small",
            UnknownReason::DecodeError => "decode_error",
            UnknownReason::AnalysisGap => "analysis_gap",
            UnknownReason::ConfigChanged => "config_changed",
        }
    }
}

impl RoiSpec {
    /// Validate one ROI object (strict keys; defaults filled). See FUNCTIONS.md §B4-S.
    pub fn from_json(v: &Value) -> Result<RoiSpec, String> {
        const KEYS: [&str; 14] = [
            "name",
            "x",
            "y",
            "w",
            "h",
            "metric",
            "enter",
            "exit",
            "dwell_ms",
            "cooldown_ms",
            "pixel_threshold",
            "evidence",
            "evidence_pre",
            "evidence_post",
        ];
        let Some(o) = v.as_object() else {
            return Err("roi: expected an object".into());
        };
        if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(format!("roi.{k}: unknown key"));
        }
        let name = o
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| {
                let c = n.chars().count();
                (1..=ROI_NAME_MAX).contains(&c)
                    && n.chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            })
            .ok_or_else(|| format!("roi.name: expected 1..={ROI_NAME_MAX} chars of [A-Za-z0-9_-]"))?
            .to_owned();
        let req_f64 = |k: &str| -> Result<f64, String> {
            o.get(k)
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("roi.{k}: expected a number"))
        };
        let x = req_f64("x")?;
        if !(0.0..1.0).contains(&x) {
            return Err("roi.x: expected 0 <= x < 1".into());
        }
        let y = req_f64("y")?;
        if !(0.0..1.0).contains(&y) {
            return Err("roi.y: expected 0 <= y < 1".into());
        }
        let w = req_f64("w")?;
        if w <= 0.0 || x + w > 1.0 + 1e-9 {
            return Err("roi.w: expected 0 < w and x + w <= 1".into());
        }
        let h = req_f64("h")?;
        if h <= 0.0 || y + h > 1.0 + 1e-9 {
            return Err("roi.h: expected 0 < h and y + h <= 1".into());
        }
        let metric = match o.get("metric") {
            None | Some(Value::Null) => Metric::MeanAbsDiff,
            Some(m) => m
                .as_str()
                .and_then(Metric::parse)
                .ok_or("roi.metric: expected luma_delta | mean_abs_diff | changed_px")?,
        };
        let enter = req_f64("enter")?;
        if !(enter > 0.0 && enter <= metric.max_threshold()) {
            return Err(format!(
                "roi.enter: expected 0 < enter <= {}",
                metric.max_threshold()
            ));
        }
        let exit = match o.get("exit") {
            None | Some(Value::Null) => enter / 2.0,
            Some(e) => e
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("roi.exit: expected a number")?,
        };
        if !(0.0..=enter).contains(&exit) {
            return Err("roi.exit: expected 0 <= exit <= enter".into());
        }
        let opt_u64 = |k: &str, def: u64, hi: u64, lo: u64| -> Result<u64, String> {
            match o.get(k) {
                None | Some(Value::Null) => Ok(def),
                Some(v) => v
                    .as_u64()
                    .filter(|n| (lo..=hi).contains(n))
                    .ok_or_else(|| format!("roi.{k}: expected an integer {lo}..={hi}")),
            }
        };
        let dwell_ms = opt_u64("dwell_ms", DWELL_DEFAULT_MS, TIMING_MAX_MS, 0)?;
        let cooldown_ms = opt_u64("cooldown_ms", COOLDOWN_DEFAULT_MS, TIMING_MAX_MS, 0)?;
        let pixel_threshold = opt_u64("pixel_threshold", u64::from(PIXEL_THRESHOLD_DEFAULT), 255, 1)? as u8;
        let evidence = match o.get("evidence") {
            None | Some(Value::Null) => true,
            Some(e) => e.as_bool().ok_or("roi.evidence: expected a boolean")?,
        };
        let evidence_pre = opt_u64(
            "evidence_pre",
            u64::from(EVIDENCE_FRAMES_DEFAULT),
            u64::from(EVIDENCE_FRAMES_MAX),
            0,
        )? as u8;
        let evidence_post = opt_u64(
            "evidence_post",
            u64::from(EVIDENCE_FRAMES_DEFAULT),
            u64::from(EVIDENCE_FRAMES_MAX),
            0,
        )? as u8;
        Ok(RoiSpec {
            name,
            rect: RoiRect { x, y, w, h },
            metric,
            enter,
            exit,
            dwell_ms,
            cooldown_ms,
            pixel_threshold,
            evidence,
            evidence_pre,
            evidence_post,
        })
    }
    /// Canonical object with every key (round-trips through [`RoiSpec::from_json`]).
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name, "x": self.rect.x, "y": self.rect.y, "w": self.rect.w, "h": self.rect.h,
            "metric": self.metric.as_str(), "enter": self.enter, "exit": self.exit,
            "dwell_ms": self.dwell_ms, "cooldown_ms": self.cooldown_ms, "pixel_threshold": self.pixel_threshold,
            "evidence": self.evidence, "evidence_pre": self.evidence_pre, "evidence_post": self.evidence_post,
        })
    }
}

/// Validate a whole set: at most [`MAX_ROIS`], unique names.
pub fn check_set(specs: &[RoiSpec]) -> Result<(), String> {
    if specs.len() > MAX_ROIS {
        return Err(format!("roi: at most {MAX_ROIS} ROIs"));
    }
    for (i, a) in specs.iter().enumerate() {
        if specs.iter().take(i).any(|b| b.name == a.name) {
            return Err(format!("roi.name: duplicate {}", a.name));
        }
    }
    Ok(())
}

/// Decode a JPEG to its luma plane (worker only; refuses > [`MAX_DECODE_DIM`]).
#[cfg(feature = "worker")]
pub fn decode_luma(jpeg: &[u8]) -> Result<LumaFrame, String> {
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::Luma)
        .set_max_width(MAX_DECODE_DIM)
        .set_max_height(MAX_DECODE_DIM);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(jpeg), options);
    let luma = decoder.decode().map_err(|e| format!("jpeg decode: {e}"))?;
    let info = decoder.info().ok_or("jpeg decode: no image info")?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if luma.len() != (width as usize).saturating_mul(height as usize) {
        return Err("jpeg decode: luma size mismatch".into());
    }
    Ok(LumaFrame { width, height, luma })
}

/// Pixel rectangle of `rect` in a `width × height` frame; `None` when smaller than 2×2.
pub fn roi_rect_px(rect: &RoiRect, width: u32, height: u32) -> Option<PxRect> {
    let (fw, fh) = (f64::from(width), f64::from(height));
    let edge = |v: f64, max: f64| v.round().clamp(0.0, max) as u32;
    let x0 = edge(rect.x * fw, fw);
    let x1 = edge((rect.x + rect.w) * fw, fw);
    let y0 = edge(rect.y * fh, fh);
    let y1 = edge((rect.y + rect.h) * fh, fh);
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w < 2 || h < 2 {
        return None;
    }
    Some(PxRect { x: x0, y: y0, w, h })
}

/// Smallest grid step `s ≥ 1` with `ceil(w/s) * ceil(h/s) ≤ MAX_SAMPLES`.
pub fn sample_step(px: &PxRect) -> u32 {
    let area = u64::from(px.w) * u64::from(px.h);
    let mut s = ((area as f64 / MAX_SAMPLES as f64).sqrt().ceil() as u32).max(1);
    while u64::from(px.w.div_ceil(s)) * u64::from(px.h.div_ceil(s)) > MAX_SAMPLES as u64 {
        s += 1;
    }
    s
}

/// Sample `px` of `frame` on the [`sample_step`] grid.
pub fn sample_roi(frame: &LumaFrame, px: &PxRect) -> Samples {
    let step = sample_step(px);
    let mut values = Vec::new();
    for j in (0..px.h).step_by(step as usize) {
        for i in (0..px.w).step_by(step as usize) {
            let idx = (px.y as usize + j as usize) * frame.width as usize + px.x as usize + i as usize;
            values.push(frame.luma.get(idx).copied().unwrap_or(0));
        }
    }
    Samples {
        width: frame.width,
        height: frame.height,
        rect: *px,
        step,
        values,
    }
}

/// Metrics of `cur` against `base`; `None` when the geometry (width, height, rect, step, len)
/// differs.
pub fn metrics(cur: &Samples, base: &Samples, pixel_threshold: u8) -> Option<Metrics> {
    let n = cur.values.len();
    if cur.width != base.width
        || cur.height != base.height
        || cur.rect != base.rect
        || cur.step != base.step
        || n != base.values.len()
        || n == 0
    {
        return None;
    }
    let thr = i32::from(pixel_threshold.max(1));
    let (mut sc, mut sb, mut sd, mut changed) = (0u64, 0u64, 0u64, 0u64);
    for (c, b) in cur.values.iter().zip(&base.values) {
        let d = (i32::from(*c) - i32::from(*b)).abs();
        sc += u64::from(*c);
        sb += u64::from(*b);
        sd += d as u64;
        if d >= thr {
            changed += 1;
        }
    }
    let nf = n as f64;
    Some(Metrics {
        mean_luma: sc as f64 / nf,
        luma_delta: sc as f64 / nf - sb as f64 / nf,
        mean_abs_diff: sd as f64 / nf / 255.0 * 100.0,
        changed_px: changed as f64 / nf * 100.0,
    })
}

impl Metrics {
    /// Judged magnitude of `metric` (`luma_delta` → absolute value).
    pub fn value(&self, metric: Metric) -> f64 {
        match metric {
            Metric::LumaDelta => self.luma_delta.abs(),
            Metric::MeanAbsDiff => self.mean_abs_diff,
            Metric::ChangedPx => self.changed_px,
        }
    }
    /// `{"mean_luma","luma_delta","mean_abs_diff","changed_px"}`, each rounded to 2 decimals.
    pub fn to_json(&self) -> Value {
        let r = |x: f64| (x * 100.0).round() / 100.0;
        json!({"mean_luma": r(self.mean_luma), "luma_delta": r(self.luma_delta),
               "mean_abs_diff": r(self.mean_abs_diff), "changed_px": r(self.changed_px)})
    }
}

impl Baseline {
    /// Identity: `{"frame_id","sha256","width","height","rect":{"x","y","w","h"},"step","mean_luma"}`.
    pub fn to_json(&self) -> Value {
        let v = &self.samples.values;
        let mean = if v.is_empty() {
            0.0
        } else {
            v.iter().map(|b| f64::from(*b)).sum::<f64>() / v.len() as f64
        };
        let r = &self.samples.rect;
        json!({"frame_id": self.frame_id, "sha256": self.sha256, "width": self.samples.width,
               "height": self.samples.height, "rect": {"x": r.x, "y": r.y, "w": r.w, "h": r.h},
               "step": self.samples.step, "mean_luma": (mean * 100.0).round() / 100.0})
    }
}

/// One ROI on one decoded frame → judge input (+ metrics when measurable). See §B4-O.
pub fn observe(
    spec: &RoiSpec,
    frame: &LumaFrame,
    baseline: Option<&Baseline>,
    mono_ns: u64,
) -> (Observation, Option<Metrics>) {
    let u = |reason: UnknownReason| (Observation::Unknown { mono_ns, reason }, None);
    let Some(px) = roi_rect_px(&spec.rect, frame.width, frame.height) else {
        return u(UnknownReason::RoiTooSmall);
    };
    let Some(b) = baseline else {
        return u(UnknownReason::NoBaseline);
    };
    if b.samples.width != frame.width || b.samples.height != frame.height || b.samples.rect != px {
        return u(UnknownReason::BaselineMismatch);
    }
    let cur = sample_roi(frame, &px);
    let Some(m) = metrics(&cur, &b.samples, spec.pixel_threshold) else {
        return u(UnknownReason::BaselineMismatch);
    };
    (
        Observation::Value {
            mono_ns,
            value: m.value(spec.metric),
        },
        Some(m),
    )
}

impl RoiJudge {
    /// UNKNOWN(`config_changed`), seq 0, no candidate, no history.
    pub fn new() -> RoiJudge {
        RoiJudge {
            state: RoiState::Unknown,
            reason: Some(UnknownReason::ConfigChanged),
            seq: 0,
            candidate: None,
            last_mono_ns: None,
            last_switch_mono_ns: None,
        }
    }
    /// Current state.
    pub fn state(&self) -> RoiState {
        self.state
    }
    /// Current UNKNOWN reason (`None` when not unknown).
    pub fn reason(&self) -> Option<UnknownReason> {
        self.reason
    }
    /// Transitions so far.
    pub fn seq(&self) -> u64 {
        self.seq
    }
    /// Advance with one observation; at most one transition per step. See §B4-J.
    pub fn step(&mut self, spec: &RoiSpec, obs: Observation) -> Option<Transition> {
        let emit = |j: &mut RoiJudge,
                    to: RoiState,
                    reason: Option<UnknownReason>,
                    mono: u64,
                    value: Option<f64>|
         -> Transition {
            let t = Transition {
                from: j.state,
                from_reason: j.reason,
                to,
                reason,
                seq: j.seq + 1,
                mono_ns: mono,
                value,
            };
            if j.state != RoiState::Unknown && to != RoiState::Unknown {
                j.last_switch_mono_ns = Some(mono);
            }
            j.state = to;
            j.reason = reason;
            j.seq += 1;
            j.candidate = None;
            t
        };
        let target = |st: RoiState, v: f64| -> Option<RoiState> {
            match st {
                RoiState::Unknown => Some(if v >= spec.enter {
                    RoiState::Changed
                } else {
                    RoiState::Normal
                }),
                RoiState::Normal => (v >= spec.enter).then_some(RoiState::Changed),
                RoiState::Changed => (v <= spec.exit).then_some(RoiState::Normal),
            }
        };
        let obs = match obs {
            Observation::Value { mono_ns, value } if !value.is_finite() => Observation::Unknown {
                mono_ns,
                reason: UnknownReason::DecodeError,
            },
            o => o,
        };
        match obs {
            Observation::Unknown { mono_ns, reason } => {
                self.candidate = None;
                self.last_mono_ns = Some(mono_ns);
                if self.state == RoiState::Unknown && self.reason == Some(reason) {
                    None
                } else {
                    Some(emit(self, RoiState::Unknown, Some(reason), mono_ns, None))
                }
            }
            Observation::Value { mono_ns: t, value: v } => {
                let prev = self.last_mono_ns;
                self.last_mono_ns = Some(t);
                let gap_ns = spec.dwell_ms.max(GAP_MIN_MS).saturating_mul(1_000_000);
                if let Some(p) = prev
                    && t.saturating_sub(p) > gap_ns
                {
                    let out = if self.state == RoiState::Unknown
                        && self.reason == Some(UnknownReason::AnalysisGap)
                    {
                        None
                    } else {
                        Some(emit(
                            self,
                            RoiState::Unknown,
                            Some(UnknownReason::AnalysisGap),
                            t,
                            None,
                        ))
                    };
                    self.candidate = target(self.state, v).map(|to| Candidate { to, since_mono_ns: t });
                    return out;
                }
                let Some(to) = target(self.state, v) else {
                    self.candidate = None;
                    return None;
                };
                let since = match self.candidate {
                    Some(c) if c.to == to => c.since_mono_ns,
                    _ => {
                        self.candidate = Some(Candidate { to, since_mono_ns: t });
                        t
                    }
                };
                if t.saturating_sub(since) < spec.dwell_ms.saturating_mul(1_000_000) {
                    return None;
                }
                if self.state != RoiState::Unknown
                    && let Some(ls) = self.last_switch_mono_ns
                    && t.saturating_sub(ls) < spec.cooldown_ms.saturating_mul(1_000_000)
                {
                    return None;
                }
                Some(emit(self, to, None, t, Some(v)))
            }
        }
    }
    /// `{"state","reason","transition_seq","candidate": null | {"to","since_mono_ns"}}`.
    pub fn to_json(&self) -> Value {
        json!({"state": self.state.as_str(), "reason": self.reason.map(UnknownReason::as_str),
               "transition_seq": self.seq,
               "candidate": self.candidate.map(|c| json!({"to": c.to.as_str(), "since_mono_ns": c.since_mono_ns}))})
    }
}

impl Transition {
    /// `{"from","from_reason","to","reason","transition_seq","mono_ns","value"}`
    /// (value rounded to 2 decimals or null; reasons null when not unknown).
    pub fn to_json(&self) -> Value {
        json!({"from": self.from.as_str(), "from_reason": self.from_reason.map(UnknownReason::as_str),
               "to": self.to.as_str(), "reason": self.reason.map(UnknownReason::as_str),
               "transition_seq": self.seq, "mono_ns": self.mono_ns,
               "value": self.value.map(|v| (v * 100.0).round() / 100.0)})
    }
}
