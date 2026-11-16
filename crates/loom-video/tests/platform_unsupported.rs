//! Non-Unix platforms (Windows): the worker's live media plane is a Unix domain socket and no
//! portable transport exists yet. These tests pin the honest behavior there. A capture open is
//! refused with `E_UNSUPPORTED` before FFmpeg is started, and the gateway's live frame/meta
//! routes answer `503 media_unavailable`. Nothing reports a fabricated preview or capture.
//! On Unix the real behavior is covered by `fixture_e2e.rs`, so this file compiles to nothing.
#![cfg(not(unix))]
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use loom_proto::error::ErrorCode;
use loom_testkit::FakeWorker;
use loom_video::media;
use loom_video::worker::VideoWorker;
use serde_json::{Value, json};

/// Short private home under the OS temp dir (media socket path bound), removed on drop.
struct Home(PathBuf);
impl Home {
    fn new(tag: &str) -> Home {
        let p = std::env::temp_dir().join(format!("lv{}{tag}", std::process::id()));
        std::fs::create_dir_all(p.join("records").join("s1")).unwrap();
        Home(p)
    }
    fn storage(&self) -> Value {
        json!({"home": self.0, "session": self.0.join("records").join("s1")})
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn open_is_refused_as_unsupported_without_starting_ffmpeg() {
    let home = Home::new("a");
    // Absolute but nonexistent: if the worker tried to spawn it, the error would be a spawn
    // failure (not E_UNSUPPORTED). The refusal must come first.
    let ffmpeg = std::env::temp_dir().join("loom-video-no-ffmpeg.exe");
    let mut fw = FakeWorker::new(VideoWorker::new());
    let e = fw
        .call(
            "video.open",
            json!({"config": {"source": "http-mjpeg", "url": "http://127.0.0.1:9/stream"},
                   "storage": home.storage(), "ffmpeg": ffmpeg}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported, "{e:?}");
    assert!(e.message.contains("not available on this platform"), "{e:?}");
    let st = fw.call("video.status", json!({})).unwrap();
    assert_ne!(st["running"], true, "{st}");
}

#[test]
fn live_frame_and_meta_routes_report_media_unavailable() {
    let home = Home::new("b");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let f = rt.block_on(media::frame(&home.0, "v0123456789ab", None));
    assert_eq!(f.status, 503);
    let m = rt.block_on(media::meta(&home.0, "v0123456789ab"));
    assert_eq!(m.status, 503);
    for r in [f, m] {
        let media::Body::Bytes(b) = r.body else {
            panic!("json body expected")
        };
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["error"], "media_unavailable", "{v}");
    }
}
