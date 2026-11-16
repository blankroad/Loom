//! `loom-video` — video capture domain (CONTRACT: `work/reports/video-capture/CONTRACT.md`).
//!
//! The worker owns one acquisition FFmpeg process per capture. Its normalized MJPEG stdout is
//! split into JPEG frames held in a bounded in-memory ring; frames never enter the journal. The
//! journal gets metadata only (`video.capture_state`, `video.health`, `video.snapshot`,
//! `video.record_state`). Frame bytes reach the UI and agents through a separate read-only media
//! plane: a private Unix socket served by the worker and authenticated loomd HTTP GET routes.
//!
//! Pieces: [`analysis`] (ROI metrics + judgment — pure), [`inspect`] (ROI config + per-capture state — pure), [`model`] (validation, argv, JPEG splitter, ring, ids — pure), [`gateway`]
//! (registration, handlers, reducer), [`worker`] (process supervision, media socket, recorder),
//! [`media`] (gateway-side socket client used by loomd's HTTP routes), [`cli`].

/// ROI analysis: luma decode, sampling, metrics, judgment state machine (pure; decode = worker).
pub mod analysis;
#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "gateway")]
pub mod gateway;
/// ROI inspection state: config revisions, per-capture judges/baselines, evidence plans (pure).
pub mod inspect;
#[cfg(feature = "media")]
pub mod media;
pub mod model;
#[cfg(feature = "worker")]
pub mod worker;

/// Domain name (RPC prefix, CLI group, event `domain`).
pub const DOMAIN: &str = "video";
/// Worker kind.
pub const WORKER_KIND: &str = "video";

/// Capture lifecycle (priority lane).
pub const KIND_CAPTURE_STATE: &str = "video.capture_state";
/// 1 s acquisition health (data lane).
pub const KIND_HEALTH: &str = "video.health";
/// Snapshot artifact written (priority lane).
pub const KIND_SNAPSHOT: &str = "video.snapshot";
/// Recording lifecycle (priority lane).
pub const KIND_RECORD_STATE: &str = "video.record_state";
/// ROI inspection config (full set + change; priority lane). Operator upgrade B5b.
pub const KIND_ROI_CONFIG: &str = "video.roi_config";
/// ROI judgment transition (priority lane): one per state/reason change, never inferred.
/// Deliberately not `*_state`: the shared hub/UI treat `<domain>.*_state` on a channel as that
/// channel's connectivity, and a ROI judgment must never replace the capture's channel state.
pub const KIND_ROI_STATE: &str = "video.roi_judgment";
/// 1 s ROI metric summary (data lane).
pub const KIND_ROI_METRIC: &str = "video.roi_metric";
/// Before/after evidence bundle lifecycle (priority lane).
pub const KIND_EVIDENCE: &str = "video.evidence";
/// Channel of `video.roi_config` edits made while no capture exists (otherwise the capture id).
pub const ROI_IDLE_CHANNEL: &str = "inspect";
