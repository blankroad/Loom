//! `loom-video` — video capture domain (CONTRACT: `work/reports/video-capture/CONTRACT.md`).
//!
//! The worker owns one acquisition FFmpeg process per capture. Its normalized MJPEG stdout is
//! split into JPEG frames held in a bounded in-memory ring; frames never enter the journal. The
//! journal gets metadata only (`video.capture_state`, `video.health`, `video.snapshot`,
//! `video.record_state`). Frame bytes reach the UI and agents through a separate read-only media
//! plane: a private Unix socket served by the worker and authenticated loomd HTTP GET routes.
//!
//! Pieces: [`model`] (validation, argv, JPEG splitter, ring, ids — pure), [`gateway`]
//! (registration, handlers, reducer), [`worker`] (process supervision, media socket, recorder),
//! [`media`] (gateway-side socket client used by loomd's HTTP routes), [`cli`].

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "gateway")]
pub mod gateway;
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
