//! Pure pieces: open-config validation, FFmpeg argv (fixed templates, never caller argv), the
//! MJPEG stream splitter, the bounded frame ring, `-progress` parsing, opaque ids and the
//! artifact / media-socket path rules. No I/O except [`resolve_ffmpeg`]'s PATH lookup.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use loom_proto::error::ErrorCode;
use loom_proto::num::U64;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// Largest accepted JPEG frame.
pub const MAX_FRAME_BYTES: usize = 2 << 20;
/// Bytes kept in the latest-frame ring.
pub const RING_BYTES: usize = 4 << 20;
/// Frames kept in the ring (whichever bound is hit first).
pub const RING_FRAMES: usize = 120;
/// Output size bounds.
pub const MIN_WIDTH: u32 = 160;
/// Output size bounds.
pub const MAX_WIDTH: u32 = 3840;
/// Output size bounds.
pub const MIN_HEIGHT: u32 = 120;
/// Output size bounds.
pub const MAX_HEIGHT: u32 = 2160;
/// Output rate bounds (integer fps).
pub const MAX_FPS: u32 = 60;
/// MJPEG quality scale (`-q:v`, lower is better).
pub const MIN_QUALITY: u32 = 2;
/// MJPEG quality scale.
pub const MAX_QUALITY: u32 = 31;
/// Longest accepted source URL.
pub const MAX_URL: usize = 512;
/// Longest accepted AVFoundation device name.
pub const MAX_DEVICE_NAME: usize = 128;
/// Unix socket path bound (macOS `sun_path` is 104 bytes including NUL; keep margin).
pub const MAX_SOCKET_PATH: usize = 100;
/// No frame for this long → `stalled`.
pub const STALL_MS: u64 = 2000;
/// First frame deadline after spawn.
pub const FIRST_FRAME_MS: u64 = 5000;

/// Validation error (code + message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError {
    /// Error code.
    pub code: ErrorCode,
    /// Message.
    pub message: String,
}

fn usage(m: impl Into<String>) -> ModelError {
    ModelError {
        code: ErrorCode::Usage,
        message: m.into(),
    }
}

// ---------------------------------------------------------------------------------------------
// Sources.

/// Source kinds the worker implements. RTSP/DeckLink/audio are deliberately absent.
pub const SOURCE_KINDS: &[&str] = &["http-mjpeg", "replay", "avfoundation"];

/// Explicitly selected source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Loopback HTTP multipart MJPEG (`http://127.0.0.1:PORT/path`).
    HttpMjpeg {
        /// Full URL handed to FFmpeg (may carry a query string).
        url: String,
        /// Display form without the query string (events, status).
        display: String,
    },
    /// A file in `$LOOM_HOME/video/replay/`, by name only.
    Replay {
        /// File name.
        name: String,
    },
    /// AVFoundation video device selected by exact name; audio is always `none`.
    Avfoundation {
        /// Device name as listed by `-list_devices`.
        device: String,
    },
}

impl Source {
    /// Wire kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Source::HttpMjpeg { .. } => "http-mjpeg",
            Source::Replay { .. } => "replay",
            Source::Avfoundation { .. } => "avfoundation",
        }
    }
    /// Safe description (no query string, no path).
    pub fn label(&self) -> String {
        match self {
            Source::HttpMjpeg { display, .. } => display.clone(),
            Source::Replay { name } => format!("replay:{name}"),
            Source::Avfoundation { device } => format!("avfoundation:{device}"),
        }
    }
    /// Event `env`: physical devices are bench, the rest simulated/external fixtures.
    pub fn is_device(&self) -> bool {
        matches!(self, Source::Avfoundation { .. })
    }
}

/// Validate a loopback HTTP URL. Returns the display form (no query).
pub fn check_http_url(url: &str) -> Result<String, ModelError> {
    if url.is_empty() || url.len() > MAX_URL {
        return Err(usage(format!("url: expected 1..={MAX_URL} bytes")));
    }
    if url.bytes().any(|b| b <= b' ' || b == 0x7f || b == b'\\' || b == b'#') {
        return Err(usage("url: whitespace, control characters, '\\' and '#' are not allowed"));
    }
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| usage("url: only http:// loopback sources are implemented"))?;
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return Err(usage("url: credentials (user@host) are not accepted"));
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let end = v6.find(']').ok_or_else(|| usage("url: bad IPv6 host"))?;
        let host = &v6[..end];
        let after = &v6[end + 1..];
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p),
            None if after.is_empty() => None,
            None => return Err(usage("url: bad authority")),
        };
        (host, port)
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if !matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1") {
        return Err(usage(format!(
            "url: host {host:?} is not loopback (127.0.0.1, localhost, [::1] only)"
        )));
    }
    if let Some(p) = port
        && !matches!(p.parse::<u16>(), Ok(n) if n > 0)
    {
        return Err(usage(format!("url: bad port {p:?}")));
    }
    let path = tail.split('?').next().unwrap_or("");
    if path.split('/').any(|seg| seg == "..") {
        return Err(usage("url: '..' path segments are not allowed"));
    }
    let path = if path.is_empty() { "/" } else { path };
    Ok(format!("http://{authority}{path}"))
}

/// Replay file names: `[A-Za-z0-9._-]{1,64}`, no leading dot.
pub fn check_replay_name(name: &str) -> Result<(), ModelError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(usage(
            "name: expected a file name [A-Za-z0-9._-]{1,64} inside $LOOM_HOME/video/replay (no paths)",
        ))
    }
}

/// AVFoundation selector: an explicit device name; never `default`, `none` or an index.
pub fn check_device_name(device: &str) -> Result<(), ModelError> {
    let t = device.trim();
    if t.is_empty() || device.len() > MAX_DEVICE_NAME || t != device {
        return Err(usage(format!(
            "device: expected an exact device name of 1..={MAX_DEVICE_NAME} bytes"
        )));
    }
    if device.chars().any(|c| c.is_control() || c == ':') {
        return Err(usage("device: control characters and ':' are not allowed"));
    }
    if device.eq_ignore_ascii_case("default") || device.eq_ignore_ascii_case("none") {
        return Err(usage("device: implicit default/none selection is refused; name the device"));
    }
    if device.bytes().all(|b| b.is_ascii_digit()) {
        return Err(usage(
            "device: index selection is refused (indexes change on hot-plug); use the exact name",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Open config.

/// Validated `video.open` parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenConfig {
    /// Source.
    pub source: Source,
    /// Output width (even).
    pub width: u32,
    /// Output height (even).
    pub height: u32,
    /// Declared output rate (integer fps).
    pub fps: u32,
    /// MJPEG `-q:v`.
    pub quality: u32,
}

/// Keys accepted by `video.open`.
pub const OPEN_KEYS: &[&str] = &["source", "url", "name", "device", "width", "height", "fps", "quality"];

fn get_u32(o: &serde_json::Map<String, Value>, k: &str, lo: u32, hi: u32, def: u32) -> Result<u32, ModelError> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(def),
        Some(v) => match v.as_u64() {
            Some(n) if (u64::from(lo)..=u64::from(hi)).contains(&n) => Ok(n as u32),
            _ => Err(usage(format!("{k}: expected an integer {lo}..={hi}"))),
        },
    }
}

fn get_str<'a>(o: &'a serde_json::Map<String, Value>, k: &str) -> Result<Option<&'a str>, ModelError> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(usage(format!("{k}: expected a string"))),
    }
}

impl OpenConfig {
    /// Strict parse (unknown keys rejected, no defaults for the source).
    pub fn from_params(p: &Value) -> Result<OpenConfig, ModelError> {
        let o = p.as_object().ok_or_else(|| usage("params must be an object"))?;
        if let Some(k) = o.keys().find(|k| !OPEN_KEYS.contains(&k.as_str())) {
            return Err(usage(format!(
                "unknown parameter {k:?} (allowed: {})",
                OPEN_KEYS.join(", ")
            )));
        }
        let kind = get_str(o, "source")?.ok_or_else(|| {
            usage("source: required (http-mjpeg, replay or avfoundation); there is no default source")
        })?;
        let url = get_str(o, "url")?;
        let name = get_str(o, "name")?;
        let device = get_str(o, "device")?;
        let only = |want: &str| -> Result<(), ModelError> {
            for (k, v) in [("url", url.is_some()), ("name", name.is_some()), ("device", device.is_some())] {
                if v && k != want {
                    return Err(usage(format!("{k}: not valid for source {kind}")));
                }
            }
            Ok(())
        };
        let source = match kind {
            "http-mjpeg" => {
                only("url")?;
                let url = url.ok_or_else(|| usage("url: required for http-mjpeg"))?;
                let display = check_http_url(url)?;
                Source::HttpMjpeg {
                    url: url.to_owned(),
                    display,
                }
            }
            "replay" => {
                only("name")?;
                let name = name.ok_or_else(|| usage("name: required for replay"))?;
                check_replay_name(name)?;
                Source::Replay { name: name.to_owned() }
            }
            "avfoundation" => {
                only("device")?;
                let device = device.ok_or_else(|| usage("device: required for avfoundation"))?;
                check_device_name(device)?;
                Source::Avfoundation {
                    device: device.to_owned(),
                }
            }
            "rtsp" | "decklink" => {
                return Err(ModelError {
                    code: ErrorCode::Unsupported,
                    message: format!("source {kind}: not available (not implemented or validated)"),
                });
            }
            o => {
                return Err(usage(format!(
                    "source {o:?}: expected http-mjpeg, replay or avfoundation"
                )));
            }
        };
        let width = get_u32(o, "width", MIN_WIDTH, MAX_WIDTH, 1280)?;
        let height = get_u32(o, "height", MIN_HEIGHT, MAX_HEIGHT, 720)?;
        if width % 2 != 0 || height % 2 != 0 {
            return Err(usage("width/height: must be even"));
        }
        let fps = get_u32(o, "fps", 1, MAX_FPS, 30)?;
        let quality = get_u32(o, "quality", MIN_QUALITY, MAX_QUALITY, 5)?;
        Ok(OpenConfig {
            source,
            width,
            height,
            fps,
            quality,
        })
    }

    /// Wire params (round-trips through [`OpenConfig::from_params`]; includes the full URL).
    pub fn to_params(&self) -> Value {
        let mut v = json!({"source": self.source.kind(), "width": self.width, "height": self.height,
                           "fps": self.fps, "quality": self.quality});
        match &self.source {
            Source::HttpMjpeg { url, .. } => v["url"] = json!(url),
            Source::Replay { name } => v["name"] = json!(name),
            Source::Avfoundation { device } => v["device"] = json!(device),
        }
        v
    }

    /// Safe public form for events/status (URL without query string).
    pub fn public(&self) -> Value {
        json!({"source": self.source.kind(), "source_label": self.source.label(),
               "width": self.width, "height": self.height, "fps": self.fps, "quality": self.quality,
               "audio": false})
    }

    /// Acquisition argv (after the program). `replay_dir` is `$LOOM_HOME/video/replay`.
    pub fn acquisition_args(&self, replay_dir: &Path) -> Vec<String> {
        let mut a: Vec<String> = ["-hide_banner", "-nostdin", "-loglevel", "warning", "-nostats"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        fn push(a: &mut Vec<String>, xs: &[&str]) {
            a.extend(xs.iter().map(|s| (*s).to_owned()));
        }
        match &self.source {
            Source::HttpMjpeg { url, .. } => {
                push(&mut a, &[
                    "-max_redirects",
                    "0",
                    "-reconnect",
                    "0",
                    "-reconnect_at_eof",
                    "0",
                    "-reconnect_streamed",
                    "0",
                    "-reconnect_on_network_error",
                    "0",
                    "-rw_timeout",
                    "5000000",
                    // multipart parts carry no timestamps: stamp arrival time so the fps filter
                    // holds the declared output rate (its dup/drop counts are reported in health)
                    "-use_wallclock_as_timestamps",
                    "1",
                    "-f",
                    "mpjpeg",
                    "-i",
                ]);
                a.push(url.clone());
            }
            Source::Replay { name } => {
                push(&mut a, &["-re", "-i"]);
                a.push(replay_dir.join(name).display().to_string());
            }
            Source::Avfoundation { device } => {
                push(&mut a, &["-f", "avfoundation", "-framerate"]);
                a.push(self.fps.to_string());
                push(&mut a, &["-i"]);
                a.push(format!("{device}:none"));
            }
        }
        let vf = format!("fps={},scale={}:{}", self.fps, self.width, self.height);
        for s in [
            "-an".to_owned(),
            "-sn".to_owned(),
            "-dn".to_owned(),
            "-vf".to_owned(),
            vf,
            "-c:v".to_owned(),
            "mjpeg".to_owned(),
            "-pix_fmt".to_owned(),
            "yuvj420p".to_owned(),
            "-q:v".to_owned(),
            self.quality.to_string(),
            "-f".to_owned(),
            "mjpeg".to_owned(),
            "-progress".to_owned(),
            "pipe:2".to_owned(),
            "pipe:1".to_owned(),
        ] {
            a.push(s);
        }
        a
    }
}

/// Recorder argv: same normalized JPEG stream copied (no re-encode) into Matroska at the
/// declared constant rate. `-n`: never overwrite.
pub fn recorder_args(fps: u32, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "warning",
        "-nostats",
        "-f",
        "mjpeg",
        "-framerate",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    a.push(fps.to_string());
    for s in ["-i", "pipe:0", "-map", "0:v", "-c:v", "copy", "-f", "matroska", "-n"] {
        a.push(s.to_owned());
    }
    a.push(out.display().to_string());
    a
}

/// Explicit local enumeration argv (`video.sources {local:true}` only; may trigger TCC).
pub fn list_devices_args() -> Vec<String> {
    ["-hide_banner", "-nostdin", "-f", "avfoundation", "-list_devices", "true", "-i", ""]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// Parse `-list_devices` stderr → video devices `[{index, name}]` (audio section ignored).
pub fn parse_avf_devices(stderr: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut video = false;
    for line in stderr.lines() {
        if line.contains("AVFoundation video devices") {
            video = true;
            continue;
        }
        if line.contains("AVFoundation audio devices") {
            video = false;
            continue;
        }
        if !video {
            continue;
        }
        // `[AVFoundation indev @ 0x…] [0] Name`
        let Some(rest) = line.rsplit_once("] [").map(|(_, r)| r) else { continue };
        let Some((idx, name)) = rest.split_once("] ") else { continue };
        if let Ok(i) = idx.parse::<u32>() {
            out.push(json!({"index": i, "name": name.trim()}));
        }
    }
    out
}

/// `[video] ffmpeg` (absolute) or the first `ffmpeg` on PATH. Never taken from RPC params.
pub fn resolve_ffmpeg(configured: Option<&str>, path_env: Option<&str>) -> Result<PathBuf, String> {
    if let Some(c) = configured {
        let p = PathBuf::from(c);
        if !p.is_absolute() {
            return Err(format!("[video] ffmpeg {c:?} must be an absolute path"));
        }
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("[video] ffmpeg {c:?} not found"))
        };
    }
    for dir in path_env.unwrap_or("").split(':').filter(|d| !d.is_empty()) {
        let p = Path::new(dir).join("ffmpeg");
        if p.is_file() && p.is_absolute() {
            return Ok(p);
        }
    }
    Err("ffmpeg not found on PATH (set [video] ffmpeg in loom.toml)".into())
}

// ---------------------------------------------------------------------------------------------
// Ids and paths.

/// Opaque ids: `[a-z0-9-]{1,64}`.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Capture id: `v` + 12 lowercase hex.
pub fn valid_capture_id(s: &str) -> bool {
    s.len() == 13 && s.starts_with('v') && s[1..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// New capture id from entropy the caller supplies (time, pid, counter).
pub fn new_capture_id(seed: &str) -> String {
    let h = hex::encode(Sha256::digest(seed.as_bytes()));
    format!("v{}", &h[..12])
}

/// Artifact kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// JPEG snapshot.
    Snapshot,
    /// Matroska recording.
    Recording,
}

impl ArtifactKind {
    /// Id prefix.
    pub fn prefix(self) -> &'static str {
        match self {
            ArtifactKind::Snapshot => "snap",
            ArtifactKind::Recording => "rec",
        }
    }
    /// File extension.
    pub fn ext(self) -> &'static str {
        match self {
            ArtifactKind::Snapshot => "jpg",
            ArtifactKind::Recording => "mkv",
        }
    }
    /// HTTP content type.
    pub fn content_type(self) -> &'static str {
        match self {
            ArtifactKind::Snapshot => "image/jpeg",
            ArtifactKind::Recording => "video/x-matroska",
        }
    }
}

/// `snap-<capture>-<n>` / `rec-<capture>-<n>`.
pub fn artifact_id(kind: ArtifactKind, capture_id: &str, n: u64) -> String {
    format!("{}-{capture_id}-{n}", kind.prefix())
}

/// Parse an artifact id → (kind, capture id). Strict: anything else is `None`.
pub fn parse_artifact_id(id: &str) -> Option<(ArtifactKind, &str)> {
    if !valid_id(id) {
        return None;
    }
    let mut it = id.splitn(3, '-');
    let kind = match it.next()? {
        "snap" => ArtifactKind::Snapshot,
        "rec" => ArtifactKind::Recording,
        _ => return None,
    };
    let cap = it.next()?;
    let n = it.next()?;
    if !valid_capture_id(cap) || n.is_empty() || n.len() > 12 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((kind, cap))
}

/// `<session>/video/<capture>`.
pub fn capture_dir(session: &Path, capture_id: &str) -> PathBuf {
    session.join("video").join(capture_id)
}

/// Final artifact file for an id (`None` for malformed ids).
pub fn artifact_path(session: &Path, id: &str) -> Option<(ArtifactKind, PathBuf)> {
    let (kind, cap) = parse_artifact_id(id)?;
    Some((kind, capture_dir(session, cap).join(format!("{id}.{}", kind.ext()))))
}

/// In-progress recording file (renamed to the final path when finalized).
pub fn partial_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Marker present from `record.start` until finalize (FFmpeg creates the `.part` file only after its
/// first output, so the marker is what tells readers a recording is in progress).
pub fn recording_marker(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".recording");
    PathBuf::from(s)
}

/// `$LOOM_HOME/video/replay`.
pub fn replay_dir(home: &Path) -> PathBuf {
    home.join("video").join("replay")
}

/// `$LOOM_HOME/run/v`.
pub fn socket_dir(home: &Path) -> PathBuf {
    home.join("run").join("v")
}

/// Media socket of a capture; explicit error when the path would not fit `sun_path`.
pub fn media_socket_path(home: &Path, capture_id: &str) -> Result<PathBuf, ModelError> {
    if !valid_capture_id(capture_id) {
        return Err(usage("bad capture id"));
    }
    let p = socket_dir(home).join(format!("{capture_id}.sock"));
    let n = p.as_os_str().len();
    if n > MAX_SOCKET_PATH {
        return Err(ModelError {
            code: ErrorCode::Config,
            message: format!(
                "media socket path is {n} bytes (> {MAX_SOCKET_PATH}); use a shorter LOOM_HOME (no fallback path is used)"
            ),
        });
    }
    Ok(p)
}

// ---------------------------------------------------------------------------------------------
// Storage quota. Video artifacts live outside the journal's chunk retention, so they get their
// own finite bounds. Nothing is ever deleted automatically: hitting a bound refuses new
// artifacts (`E_LIMIT`) or ends the recording early with an explicit reason.

/// Artifacts (snapshots + recordings) per daemon session.
pub const MAX_SESSION_ARTIFACTS: u64 = 500;
/// Video bytes per daemon session (`<session>/video`).
pub const MAX_SESSION_BYTES: u64 = 2 << 30;
/// Video bytes across all sessions under `$LOOM_HOME/records` (fail closed when exceeded).
pub const MAX_HOME_BYTES: u64 = 8 << 30;
/// One recording: bytes (MJPEG copy is large: ~3 MB/s at 720p30 q5).
pub const MAX_RECORD_BYTES: u64 = 1 << 30;
/// One recording: duration.
pub const MAX_RECORD_SECS: u64 = 600;
/// Minimum free budget to start a recording.
pub const MIN_RECORD_BUDGET: u64 = 16 << 20;
/// Directory entries examined per usage scan (beyond this the scan fails closed).
pub const MAX_SCAN_ENTRIES: usize = 50_000;

/// Measured video storage usage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Files under `<session>/video`.
    pub session_files: u64,
    /// Bytes under `<session>/video`.
    pub session_bytes: u64,
    /// Bytes under every `$LOOM_HOME/records/*/*/video`.
    pub home_bytes: u64,
    /// The scan hit [`MAX_SCAN_ENTRIES`] (treated as over quota).
    pub incomplete: bool,
}

fn walk(dir: &Path, depth: u32, budget: &mut usize, files: &mut u64, bytes: &mut u64) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return true };
    for e in rd.flatten() {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
        if m.is_file() {
            *files += 1;
            *bytes += m.len();
        } else if m.is_dir() && depth > 0 && !walk(&e.path(), depth - 1, budget, files, bytes) {
            return false;
        }
    }
    true
}

/// Scan current-session and home-wide video usage (bounded).
pub fn scan_usage(home: &Path, session: &Path) -> Usage {
    let mut budget = MAX_SCAN_ENTRIES;
    let mut u = Usage::default();
    let ok_s = walk(&session.join("video"), 1, &mut budget, &mut u.session_files, &mut u.session_bytes);
    let mut ok_h = ok_s;
    let mut files = 0;
    if let Ok(dates) = std::fs::read_dir(home.join("records")) {
        'outer: for d in dates.flatten() {
            let Ok(sessions) = std::fs::read_dir(d.path()) else { continue };
            for s in sessions.flatten() {
                if budget == 0 {
                    ok_h = false;
                    break 'outer;
                }
                budget -= 1;
                if !walk(&s.path().join("video"), 1, &mut budget, &mut files, &mut u.home_bytes) {
                    ok_h = false;
                    break 'outer;
                }
            }
        }
    }
    u.incomplete = !(ok_s && ok_h);
    u
}

impl Usage {
    /// Bytes still allowed for new artifacts (0 when over any bound or the scan was incomplete).
    pub fn remaining(&self) -> u64 {
        if self.incomplete {
            return 0;
        }
        MAX_SESSION_BYTES
            .saturating_sub(self.session_bytes)
            .min(MAX_HOME_BYTES.saturating_sub(self.home_bytes))
    }
    /// Why a new artifact of `size` bytes is refused, if it is.
    pub fn refuse(&self, size: u64) -> Option<&'static str> {
        if self.incomplete {
            Some("quota_scan_incomplete")
        } else if self.session_files >= MAX_SESSION_ARTIFACTS {
            Some("quota_session_artifacts")
        } else if self.session_bytes + size > MAX_SESSION_BYTES {
            Some("quota_session_bytes")
        } else if self.home_bytes + size > MAX_HOME_BYTES {
            Some("quota_home_bytes")
        } else {
            None
        }
    }
    /// Wire form with the limits.
    pub fn json(&self) -> Value {
        json!({"session_files": self.session_files, "session_bytes": self.session_bytes,
               "home_bytes": self.home_bytes, "incomplete": self.incomplete,
               "remaining_bytes": self.remaining(), "limits": quota_limits()})
    }
}

/// Limits table (capabilities/status).
pub fn quota_limits() -> Value {
    json!({"max_session_artifacts": MAX_SESSION_ARTIFACTS, "max_session_bytes": MAX_SESSION_BYTES,
           "max_home_bytes": MAX_HOME_BYTES, "max_record_bytes": MAX_RECORD_BYTES,
           "max_record_secs": MAX_RECORD_SECS, "min_record_budget": MIN_RECORD_BUDGET,
           "auto_delete": false})
}

// ---------------------------------------------------------------------------------------------
// MJPEG splitting.

/// Splitter outcome for one chunk.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Split {
    /// Complete JPEG frames (SOI..=EOI).
    pub frames: Vec<Vec<u8>>,
    /// Frames dropped for exceeding the size bound.
    pub oversize: u64,
    /// Frames cut short by a new SOI before their EOI (corrupt input).
    pub truncated: u64,
    /// Bytes outside any frame (garbage before SOI; not counted while skipping an oversize frame).
    pub junk_bytes: u64,
}

/// Concatenated-JPEG stream splitter with a hard memory bound (iterative byte state machine).
#[derive(Debug)]
pub struct JpegSplitter {
    buf: Vec<u8>,
    in_frame: bool,
    skipping: bool,
    prev_ff: bool,
    max: usize,
}

impl Default for JpegSplitter {
    fn default() -> Self {
        JpegSplitter::new(MAX_FRAME_BYTES)
    }
}

impl JpegSplitter {
    /// New splitter; frames longer than `max` bytes are dropped.
    pub fn new(max: usize) -> Self {
        JpegSplitter {
            buf: Vec::new(),
            in_frame: false,
            skipping: false,
            prev_ff: false,
            max: max.max(4),
        }
    }

    /// Buffered bytes (≤ `max`).
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Feed bytes.
    pub fn push(&mut self, data: &[u8]) -> Split {
        let mut out = Split::default();
        for &b in data {
            let soi = self.prev_ff && b == 0xD8;
            if !self.in_frame {
                if soi {
                    self.in_frame = true;
                    self.skipping = false;
                    self.buf.clear();
                    self.buf.extend_from_slice(&[0xFF, 0xD8]);
                } else {
                    // A previous 0xFF that did not start a frame is junk too.
                    if self.prev_ff && !self.skipping {
                        out.junk_bytes += 1;
                    }
                    if b != 0xFF && !self.skipping {
                        out.junk_bytes += 1;
                    }
                }
                self.prev_ff = b == 0xFF && !soi;
                continue;
            }
            if soi {
                // New SOI inside a frame: the current one never ended.
                out.truncated += 1;
                self.buf.clear();
                self.buf.extend_from_slice(&[0xFF, 0xD8]);
                self.prev_ff = false;
                continue;
            }
            if self.buf.len() >= self.max {
                out.oversize += 1;
                self.buf = Vec::new();
                self.in_frame = false;
                self.skipping = true;
                self.prev_ff = b == 0xFF;
                continue;
            }
            self.buf.push(b);
            if self.prev_ff && b == 0xD9 {
                out.frames.push(std::mem::take(&mut self.buf));
                self.in_frame = false;
                self.prev_ff = false;
                continue;
            }
            self.prev_ff = b == 0xFF;
        }
        out
    }
}

/// Minimal structural check of a JPEG (SOI, EOI, length).
pub fn looks_like_jpeg(b: &[u8]) -> bool {
    b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 && b[b.len() - 2] == 0xFF && b[b.len() - 1] == 0xD9
}

// ---------------------------------------------------------------------------------------------
// Frames and ring.

/// One received, normalized JPEG frame.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Worker frame id (1-based, monotonic within a capture; not a source frame number).
    pub id: u64,
    /// Capture epoch (changes on every open).
    pub epoch: u64,
    /// Daemon monotonic receive time (ns).
    pub recv_mono_ns: u64,
    /// UTC receive time (ns).
    pub recv_utc_ns: u64,
    /// SHA-256 hex of the JPEG bytes.
    pub sha256: String,
    /// JPEG bytes.
    pub data: Arc<Vec<u8>>,
}

impl Frame {
    /// New frame (hashes the bytes).
    pub fn new(id: u64, epoch: u64, recv_mono_ns: u64, recv_utc_ns: u64, data: Vec<u8>) -> Frame {
        let sha256 = hex::encode(Sha256::digest(&data));
        Frame {
            id,
            epoch,
            recv_mono_ns,
            recv_utc_ns,
            sha256,
            data: Arc::new(data),
        }
    }
    /// Metadata (no bytes). `source_pts` is null: the JPEG pipe carries no source PTS.
    /// Wide values (epoch, ns times) are canonical decimal strings (`U64`): they exceed 2^53 and
    /// would be rounded by JavaScript numbers. `frame_id`/`size` are bounded counters (numbers).
    pub fn meta(&self, capture_id: &str) -> Value {
        json!({"capture_id": capture_id, "frame_id": self.id, "capture_epoch": U64(self.epoch),
               "recv_mono_ns": U64(self.recv_mono_ns), "recv_utc_ns": U64(self.recv_utc_ns),
               "source_pts": null, "timestamp_kind": "receive",
               "sha256": self.sha256, "size": self.data.len()})
    }
}

/// Bounded latest-frame ring.
#[derive(Debug, Default)]
pub struct Ring {
    frames: VecDeque<Frame>,
    bytes: usize,
    evicted: u64,
}

impl Ring {
    /// Push, evicting oldest frames beyond [`RING_BYTES`] / [`RING_FRAMES`] (newest always kept).
    pub fn push(&mut self, f: Frame) {
        self.bytes += f.data.len();
        self.frames.push_back(f);
        while self.frames.len() > 1 && (self.bytes > RING_BYTES || self.frames.len() > RING_FRAMES) {
            if let Some(old) = self.frames.pop_front() {
                self.bytes -= old.data.len();
                self.evicted += 1;
            }
        }
    }
    /// Newest frame.
    pub fn latest(&self) -> Option<&Frame> {
        self.frames.back()
    }
    /// Frame by id.
    pub fn get(&self, id: u64) -> Option<&Frame> {
        let first = self.frames.front()?.id;
        let i = usize::try_from(id.checked_sub(first)?).ok()?;
        self.frames.get(i).filter(|f| f.id == id)
    }
    /// `(first id, last id, frames, bytes, evicted)`.
    pub fn range(&self) -> Value {
        json!({"first_frame_id": self.frames.front().map(|f| f.id),
               "last_frame_id": self.frames.back().map(|f| f.id),
               "frames": self.frames.len(), "bytes": self.bytes, "evicted": self.evicted})
    }
    /// Frames held.
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    /// Empty?
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    /// Bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

// ---------------------------------------------------------------------------------------------
// `-progress` parsing.

/// Latest FFmpeg progress counters.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Progress {
    /// Output frames.
    pub frame: u64,
    /// Frames dropped by the fps filter (source faster than output rate).
    pub drop_frames: u64,
    /// Frames duplicated by the fps filter (source slower than output rate).
    pub dup_frames: u64,
    /// `speed` (e.g. `1.0x`), as reported.
    pub speed: Option<String>,
    /// `end` seen.
    pub ended: bool,
}

impl Progress {
    /// Feed one stderr line; true if it was a progress key.
    pub fn line(&mut self, l: &str) -> bool {
        let Some((k, v)) = l.trim().split_once('=') else { return false };
        let n = || v.trim().parse::<u64>().ok();
        match k {
            "frame" => self.frame = n().unwrap_or(self.frame),
            "drop_frames" => self.drop_frames = n().unwrap_or(self.drop_frames),
            "dup_frames" => self.dup_frames = n().unwrap_or(self.dup_frames),
            "speed" => self.speed = Some(v.trim().to_owned()),
            "progress" => self.ended |= v.trim() == "end",
            "fps" | "stream_0_0_q" | "bitrate" | "total_size" | "out_time_us" | "out_time_ms" | "out_time" => {}
            _ => return false,
        }
        true
    }
}

/// Classify why acquisition ended from exit success, frames seen and stderr text.
pub fn classify_exit(success: bool, frames: u64, stderr: &str) -> (&'static str, &'static str) {
    let s = stderr.to_ascii_lowercase();
    if success {
        return ("closed", "eof");
    }
    if s.contains("connection refused") {
        return ("failed", "connection_refused");
    }
    if s.contains("no such file") {
        return ("failed", "source_not_found");
    }
    if s.contains("timed out") || s.contains("timeout") {
        return ("failed", if frames > 0 { "stall_timeout" } else { "open_timeout" });
    }
    if frames > 0 {
        ("failed", "disconnect")
    } else {
        ("failed", "open_failed")
    }
}

/// Remove a secret-bearing URL (query) from FFmpeg stderr text.
pub fn redact(text: &str, source: &Source) -> String {
    match source {
        Source::HttpMjpeg { url, display } if url != display => text.replace(url.as_str(), display),
        _ => text.to_owned(),
    }
}

/// Capability table (truthful): implemented vs verified.
pub fn capabilities(ffmpeg: &Value) -> Value {
    json!({
        "ffmpeg": ffmpeg,
        "sources": {
            "http-mjpeg": {"implemented": true, "loopback_only": true, "credentials": false,
                            "reconnect": "manual (video.open)", "verified": "external loopback fixture"},
            "replay": {"implemented": true, "dir": "$LOOM_HOME/video/replay", "by_name_only": true},
            "avfoundation": {"implemented": true, "explicit_device_name": true, "audio": false,
                              "device_verified": false, "tcc_verified": false,
                              "note": "not exercised on hardware; enumeration only via video.sources --local"},
            "rtsp": {"implemented": false, "reason": "not validated against an RTSP endpoint"},
            "decklink": {"implemented": false, "reason": "installed ffmpeg has no decklink input"},
        },
        "audio": {"available": false, "reason": "video-only MVP"},
        "output": {"codec": "mjpeg", "max_frame_bytes": MAX_FRAME_BYTES, "ring_bytes": RING_BYTES,
                   "ring_frames": RING_FRAMES, "fps": [1, MAX_FPS],
                   "width": [MIN_WIDTH, MAX_WIDTH], "height": [MIN_HEIGHT, MAX_HEIGHT]},
        "recording": {"container": "matroska", "codec": "mjpeg (copy of preview JPEGs, no H.264 encode)",
                      "size_note": "large: roughly 3 MB/s at 1280x720@30 q5", "lossless_claim": false},
        "quota": quota_limits(),
        "timestamps": {"kind": "receive", "source_pts": false},
        "media": {"frame": "/api/v1/video/{capture_id}/frame", "meta": "/api/v1/video/{capture_id}/meta",
                  "artifact": "/api/v1/video/artifact/{artifact_id}"},
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn jpeg(n: usize, fill: u8) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        v.extend(std::iter::repeat_n(fill, n));
        v.extend([0xFF, 0xD9]);
        v
    }

    #[test]
    fn url_rules() {
        assert_eq!(check_http_url("http://127.0.0.1:8080/stream").unwrap(), "http://127.0.0.1:8080/stream");
        assert_eq!(check_http_url("http://localhost:9/s?token=x").unwrap(), "http://localhost:9/s");
        assert_eq!(check_http_url("http://[::1]:80").unwrap(), "http://[::1]:80/");
        for bad in [
            "https://127.0.0.1/",
            "http://user:pw@127.0.0.1/",
            "http://10.0.0.2/",
            "http://example.com/",
            "http://127.0.0.1:0/",
            "http://127.0.0.1:99999/",
            "http://127.0.0.1/a b",
            "http://127.0.0.1/../x",
            "rtsp://127.0.0.1/",
            "",
        ] {
            assert!(check_http_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn open_config_strict_and_explicit() {
        assert!(OpenConfig::from_params(&json!({})).is_err());
        assert!(OpenConfig::from_params(&json!({"source": "avfoundation"})).is_err());
        for d in ["default", "none", "0", " Cam", ""] {
            assert!(OpenConfig::from_params(&json!({"source": "avfoundation", "device": d})).is_err(), "{d}");
        }
        assert_eq!(
            OpenConfig::from_params(&json!({"source": "rtsp"})).unwrap_err().code,
            ErrorCode::Unsupported
        );
        assert!(OpenConfig::from_params(&json!({"source": "replay", "name": "../x"})).is_err());
        assert!(OpenConfig::from_params(&json!({"source": "replay", "name": "a/b.mp4"})).is_err());
        assert!(OpenConfig::from_params(&json!({"source": "http-mjpeg", "url": "http://127.0.0.1:1/", "argv": []})).is_err());
        assert!(OpenConfig::from_params(&json!({"source": "http-mjpeg", "url": "http://127.0.0.1:1/", "name": "x"})).is_err());
        assert!(OpenConfig::from_params(&json!({"source": "http-mjpeg", "url": "http://127.0.0.1:1/", "fps": 61})).is_err());
        assert!(OpenConfig::from_params(&json!({"source": "http-mjpeg", "url": "http://127.0.0.1:1/", "width": 641})).is_err());
        let c = OpenConfig::from_params(&json!({"source": "http-mjpeg", "url": "http://127.0.0.1:8/s?k=1", "fps": 60})).unwrap();
        assert_eq!((c.width, c.height, c.fps, c.quality), (1280, 720, 60, 5));
        assert_eq!(OpenConfig::from_params(&c.to_params()).unwrap(), c);
        assert_eq!(c.public()["source_label"], "http://127.0.0.1:8/s");
        assert!(!c.public().to_string().contains("k=1"));
    }

    #[test]
    fn argv_templates() {
        let c = OpenConfig::from_params(&json!({"source": "http-mjpeg", "url": "http://127.0.0.1:8/s"})).unwrap();
        let a = c.acquisition_args(Path::new("/r"));
        let s = a.join(" ");
        assert!(s.contains("-max_redirects 0 -reconnect 0"));
        assert!(s.contains("-f mpjpeg -i http://127.0.0.1:8/s"));
        assert!(s.contains("-vf fps=30,scale=1280:720"));
        assert!(s.ends_with("-f mjpeg -progress pipe:2 pipe:1"));
        assert!(!s.contains("-follow"));
        let c = OpenConfig::from_params(&json!({"source": "avfoundation", "device": "USB Capture HDMI"})).unwrap();
        let a = c.acquisition_args(Path::new("/r"));
        let i = a.iter().position(|x| x == "-i").unwrap();
        assert_eq!(a[i + 1], "USB Capture HDMI:none");
        assert!(a.contains(&"-an".to_owned()));
        let c = OpenConfig::from_params(&json!({"source": "replay", "name": "clip.mp4"})).unwrap();
        assert!(c.acquisition_args(Path::new("/h/video/replay")).contains(&"/h/video/replay/clip.mp4".to_owned()));
        let r = recorder_args(30, Path::new("/s/rec.mkv.part")).join(" ");
        assert!(r.contains("-f mjpeg -framerate 30 -i pipe:0") && r.contains("-c:v copy -f matroska -n /s/rec.mkv.part"));
    }

    #[test]
    fn ids_and_paths() {
        let id = new_capture_id("seed");
        assert!(valid_capture_id(&id) && valid_id(&id));
        let a = artifact_id(ArtifactKind::Snapshot, &id, 3);
        let (k, p) = artifact_path(Path::new("/s"), &a).unwrap();
        assert_eq!(k, ArtifactKind::Snapshot);
        assert_eq!(p, PathBuf::from(format!("/s/video/{id}/{a}.jpg")));
        for bad in ["snap-../x-1", "snap-vZZ-1", "rec-v0123456789ab-", "x-v0123456789ab-1", "snap-v0123456789ab-1-2", "SNAP-v0123456789ab-1"] {
            assert!(parse_artifact_id(bad).is_none(), "{bad}");
        }
        assert!(media_socket_path(Path::new("/tmp/loom-video-x"), &id).is_ok());
        let long = "/".to_owned() + &"x".repeat(90);
        assert_eq!(media_socket_path(Path::new(&long), &id).unwrap_err().code, ErrorCode::Config);
    }

    #[test]
    fn splitter_frames_chunks_junk_oversize() {
        let mut s = JpegSplitter::new(64);
        let a = jpeg(10, 1);
        let b = jpeg(20, 2);
        let mut stream = vec![9, 9, 9];
        stream.extend(&a);
        stream.extend(&b);
        // byte-by-byte
        let mut frames = Vec::new();
        let mut junk = 0;
        for x in &stream {
            let r = s.push(std::slice::from_ref(x));
            frames.extend(r.frames);
            junk += r.junk_bytes;
        }
        assert_eq!(frames, vec![a.clone(), b.clone()]);
        assert_eq!(junk, 3);
        // truncated frame followed by a good one
        let mut st = vec![0xFF, 0xD8, 1, 2, 3];
        st.extend(&a);
        let r = JpegSplitter::new(64).push(&st);
        assert_eq!((r.frames.clone(), r.truncated), (vec![a.clone()], 1));
        // whole
        let r = JpegSplitter::new(64).push(&stream);
        assert_eq!(r.frames, vec![a.clone(), b.clone()]);
        // oversize then good frame
        let big = jpeg(200, 3);
        let mut s = JpegSplitter::new(64);
        let mut st = big.clone();
        st.extend(&a);
        let mut out = Split::default();
        for c in st.chunks(7) {
            let r = s.push(c);
            out.frames.extend(r.frames);
            out.oversize += r.oversize;
        }
        assert_eq!(out.oversize, 1);
        assert_eq!(out.frames, vec![a.clone()]);
        assert!(s.buffered() <= 64);
        assert!(looks_like_jpeg(&a) && !looks_like_jpeg(&[0xFF, 0xD8]));
    }

    #[test]
    fn ring_bounds_and_lookup() {
        let mut r = Ring::default();
        for i in 1..=200u64 {
            r.push(Frame::new(i, 1, i, i, jpeg(10, 0)));
        }
        assert_eq!(r.len(), RING_FRAMES);
        assert_eq!(r.latest().unwrap().id, 200);
        assert!(r.get(80).is_none());
        assert_eq!(r.get(81).unwrap().id, 81);
        let mut r = Ring::default();
        for i in 1..=5u64 {
            r.push(Frame::new(i, 1, i, i, vec![0; MAX_FRAME_BYTES]));
        }
        assert!(r.bytes() <= RING_BYTES);
        assert_eq!(r.len(), 2);
        let f = Frame::new(1, 7, 2, 3, b"abc".to_vec());
        assert_eq!(f.meta("v0")["sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(f.meta("v0")["source_pts"].is_null());
        // wide values above 2^53 survive exactly as decimal strings
        let big = Frame::new(9, 1_794_645_164_583_021_002, (1 << 53) + 1, 1_794_645_164_583_021_003, b"x".to_vec());
        let m = big.meta("v0");
        assert_eq!(m["capture_epoch"], "1794645164583021002");
        assert_eq!(m["recv_mono_ns"], "9007199254740993");
        assert_eq!(m["recv_utc_ns"], "1794645164583021003");
        assert_eq!(m["frame_id"], 9);
    }

    #[test]
    fn quota_bounds() {
        let u = Usage { session_files: 0, session_bytes: MAX_SESSION_BYTES - 10, home_bytes: 0, incomplete: false };
        assert_eq!(u.refuse(5), None);
        assert_eq!(u.refuse(11), Some("quota_session_bytes"));
        assert_eq!(u.remaining(), 10);
        let u = Usage { session_files: MAX_SESSION_ARTIFACTS, ..Usage::default() };
        assert_eq!(u.refuse(1), Some("quota_session_artifacts"));
        let u = Usage { home_bytes: MAX_HOME_BYTES, ..Usage::default() };
        assert_eq!((u.refuse(1), u.remaining()), (Some("quota_home_bytes"), 0));
        let u = Usage { incomplete: true, ..Usage::default() };
        assert_eq!((u.refuse(0), u.remaining()), (Some("quota_scan_incomplete"), 0));
        assert_eq!(quota_limits()["auto_delete"], false);
    }

    #[test]
    fn progress_and_exit() {
        let mut p = Progress::default();
        for l in ["frame=30", "fps=30.0", "drop_frames=2", "dup_frames=5", "speed=1.01x", "progress=continue", "Error x"] {
            p.line(l);
        }
        assert_eq!((p.frame, p.drop_frames, p.dup_frames, p.ended), (30, 2, 5, false));
        assert!(p.line("progress=end") && p.ended);
        assert_eq!(classify_exit(false, 0, "Connection refused"), ("failed", "connection_refused"));
        assert_eq!(classify_exit(false, 9, "I/O error"), ("failed", "disconnect"));
        assert_eq!(classify_exit(true, 9, ""), ("closed", "eof"));
        let devs = parse_avf_devices(
            "[AVFoundation indev @ 0x1] AVFoundation video devices:\n[AVFoundation indev @ 0x1] [0] Cam A\n[AVFoundation indev @ 0x1] [1] USB Capture\n[AVFoundation indev @ 0x1] AVFoundation audio devices:\n[AVFoundation indev @ 0x1] [0] Mic\n",
        );
        assert_eq!(devs, vec![json!({"index":0,"name":"Cam A"}), json!({"index":1,"name":"USB Capture"})]);
        let src = Source::HttpMjpeg { url: "http://127.0.0.1:1/s?t=SECRET".into(), display: "http://127.0.0.1:1/s".into() };
        assert!(!redact("open http://127.0.0.1:1/s?t=SECRET failed", &src).contains("SECRET"));
    }
}
