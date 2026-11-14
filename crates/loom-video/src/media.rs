//! Gateway-side media plane: loomd's authenticated HTTP GET routes call these after the
//! existing Host/Origin guard and bearer/cookie check. They only *read*: a frame or metadata
//! from the worker's private Unix socket (`$LOOM_HOME/run/v/<capture>.sock`, protocol
//! `FRAME [id]` / `META`), or a finished artifact from the current session's `video/` tree by
//! opaque id. No path, URL or token is ever taken from the request.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

use crate::model::{self, ArtifactKind, MAX_FRAME_BYTES};

/// Whole media request bound.
pub const MEDIA_TIMEOUT: Duration = Duration::from_secs(3);
/// Header line bound.
pub const MAX_HEADER: u64 = 16 * 1024;
/// Artifact read chunk.
pub const FILE_CHUNK: usize = 256 * 1024;

/// Response body.
#[derive(Debug)]
pub enum Body {
    /// In-memory bytes.
    Bytes(Vec<u8>),
    /// Stream this file (length known).
    File {
        /// File.
        path: PathBuf,
        /// Length.
        len: u64,
    },
}

/// Transport-neutral reply (converted to an axum response by loomd).
#[derive(Debug)]
pub struct Reply {
    /// HTTP status.
    pub status: u16,
    /// Content type.
    pub content_type: &'static str,
    /// Extra headers (`cache-control: no-store` always included).
    pub headers: Vec<(&'static str, String)>,
    /// Body.
    pub body: Body,
}

fn json_reply(status: u16, v: Value) -> Reply {
    Reply {
        status,
        content_type: "application/json",
        headers: vec![("cache-control", "no-store".into())],
        body: Body::Bytes(v.to_string().into_bytes()),
    }
}

fn error(status: u16, code: &str) -> Reply {
    json_reply(status, json!({"error": code}))
}

/// Parse the frame query: absent, or exactly `id=<1..=20 digits>`. Anything else is refused
/// (in particular no tokens in the URL).
pub fn parse_frame_query(q: Option<&str>) -> Result<Option<u64>, &'static str> {
    let Some(q) = q.filter(|q| !q.is_empty()) else { return Ok(None) };
    let v = q.strip_prefix("id=").ok_or("bad_query")?;
    if v.is_empty() || v.len() > 20 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return Err("bad_query");
    }
    v.parse::<u64>().ok().filter(|n| *n > 0).map(Some).ok_or("bad_query")
}

async fn ask(sock: &Path, req: &str) -> Result<(Value, Vec<u8>), Reply> {
    let mut s = match tokio::net::UnixStream::connect(sock).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound || e.kind() == std::io::ErrorKind::ConnectionRefused => {
            return Err(error(404, "no_media"));
        }
        Err(_) => return Err(error(503, "media_unavailable")),
    };
    s.write_all(format!("{req}\n").as_bytes())
        .await
        .map_err(|_| error(503, "media_unavailable"))?;
    let mut r = BufReader::new(s);
    let mut line = Vec::new();
    (&mut r)
        .take(MAX_HEADER)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|_| error(503, "media_unavailable"))?;
    let head: Value = serde_json::from_slice(&line).map_err(|_| error(502, "bad_media_reply"))?;
    let len = head["len"].as_u64().unwrap_or(0);
    if len > MAX_FRAME_BYTES as u64 {
        return Err(error(502, "bad_media_reply"));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)
        .await
        .map_err(|_| error(503, "media_unavailable"))?;
    Ok((head, body))
}

fn socket(home: &Path, capture_id: &str) -> Result<PathBuf, Reply> {
    if !model::valid_capture_id(capture_id) {
        return Err(error(400, "bad_capture_id"));
    }
    model::media_socket_path(home, capture_id).map_err(|_| error(503, "media_unavailable"))
}

fn reply_error(head: &Value) -> Reply {
    match head["error"].as_str().unwrap_or("media_error") {
        "busy" => error(503, "busy"),
        "bad_request" => error(400, "bad_request"),
        e @ ("no_frame" | "expired" | "not_received") => error(404, e),
        _ => error(502, "media_error"),
    }
}

/// Header text of a meta field: decimal strings are kept verbatim (wide `U64` values), small
/// numbers are printed; anything else is empty.
pub fn header_value(head: &Value, k: &str) -> String {
    match &head[k] {
        Value::String(s) if !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()) => s.clone(),
        Value::Number(n) => n.as_u64().map(|n| n.to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

/// `GET /api/v1/video/{capture_id}/frame[?id=N]` → `image/jpeg` + frame identity headers.
pub async fn frame(home: &Path, capture_id: &str, query: Option<&str>) -> Reply {
    let sock = match socket(home, capture_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Ok(id) = parse_frame_query(query) else {
        return error(400, "bad_query");
    };
    let req = id.map_or_else(|| "FRAME".to_owned(), |n| format!("FRAME {n}"));
    let (head, body) = match tokio::time::timeout(MEDIA_TIMEOUT, ask(&sock, &req)).await {
        Ok(Ok(v)) => v,
        Ok(Err(r)) => return r,
        Err(_) => return error(504, "media_timeout"),
    };
    if head["ok"] != true {
        return reply_error(&head);
    }
    if !model::looks_like_jpeg(&body) {
        return error(502, "bad_media_reply");
    }
    let h = |k: &str| header_value(&head, k);
    Reply {
        status: 200,
        content_type: "image/jpeg",
        headers: vec![
            ("cache-control", "no-store".into()),
            ("x-loom-capture-id", capture_id.to_owned()),
            ("x-loom-frame-id", h("frame_id")),
            ("x-loom-capture-epoch", h("capture_epoch")),
            ("x-loom-recv-mono-ns", h("recv_mono_ns")),
            ("x-loom-recv-utc-ns", h("recv_utc_ns")),
            ("x-loom-frame-sha256", head["sha256"].as_str().unwrap_or("").to_owned()),
            ("x-loom-frozen", head["frozen"].as_bool().unwrap_or(false).to_string()),
        ],
        body: Body::Bytes(body),
    }
}

/// `GET /api/v1/video/{capture_id}/meta` → JSON (latest frame meta, ring, health, state).
pub async fn meta(home: &Path, capture_id: &str) -> Reply {
    let sock = match socket(home, capture_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match tokio::time::timeout(MEDIA_TIMEOUT, ask(&sock, "META")).await {
        Ok(Ok((head, _))) if head["ok"] == true => {
            let mut v = head;
            if let Some(o) = v.as_object_mut() {
                o.remove("ok");
                o.remove("len");
            }
            json_reply(200, v)
        }
        Ok(Ok((head, _))) => reply_error(&head),
        Ok(Err(r)) => r,
        Err(_) => error(504, "media_timeout"),
    }
}

/// `GET /api/v1/video/artifact/{artifact_id}` → finished snapshot JPEG / recording MKV of the
/// current session. In-progress recordings answer 409.
pub async fn artifact(session: &Path, artifact_id: &str) -> Reply {
    let Some((kind, path)) = model::artifact_path(session, artifact_id) else {
        return error(400, "bad_artifact_id");
    };
    match tokio::fs::symlink_metadata(&path).await {
        Ok(m) if m.is_file() => Reply {
            status: 200,
            content_type: kind.content_type(),
            headers: vec![
                ("cache-control", "no-store".into()),
                ("x-loom-artifact-id", artifact_id.to_owned()),
            ],
            body: Body::File { path, len: m.len() },
        },
        Ok(_) => error(404, "not_found"),
        Err(_) => {
            let in_progress = tokio::fs::metadata(model::partial_path(&path)).await.is_ok()
                || tokio::fs::metadata(model::recording_marker(&path)).await.is_ok();
            if kind == ArtifactKind::Recording && in_progress {
                error(409, "recording_in_progress")
            } else {
                error(404, "not_found")
            }
        }
    }
}

enum St {
    Start(PathBuf),
    Open(tokio::fs::File),
    Done,
}

/// Chunked file stream for [`Body::File`].
pub fn file_chunks(path: PathBuf) -> impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send + 'static {
    futures_util::stream::unfold(St::Start(path), |st| async move {
        let mut f = match st {
            St::Done => return None,
            St::Start(p) => match tokio::fs::File::open(&p).await {
                Ok(f) => f,
                Err(e) => return Some((Err(e), St::Done)),
            },
            St::Open(f) => f,
        };
        let mut buf = vec![0u8; FILE_CHUNK];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(buf), St::Open(f)))
            }
            Err(e) => Some((Err(e), St::Done)),
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn wide_header_values_are_exact() {
        let head = json!({"capture_epoch": "1794645164583021002", "recv_utc_ns": "18446744073709551615",
                          "frame_id": 42, "bad": "12a", "neg": -1});
        assert_eq!(header_value(&head, "capture_epoch"), "1794645164583021002");
        assert_eq!(header_value(&head, "recv_utc_ns"), "18446744073709551615");
        assert_eq!(header_value(&head, "frame_id"), "42");
        assert_eq!(header_value(&head, "bad"), "");
        assert_eq!(header_value(&head, "neg"), "");
    }

    #[test]
    fn frame_query_is_id_only() {
        assert_eq!(parse_frame_query(None), Ok(None));
        assert_eq!(parse_frame_query(Some("")), Ok(None));
        assert_eq!(parse_frame_query(Some("id=42")), Ok(Some(42)));
        for bad in ["id=0", "id=", "id=-1", "token=x", "id=1&token=x", "id=99999999999999999999999"] {
            assert!(parse_frame_query(Some(bad)).is_err(), "{bad}");
        }
    }
}
