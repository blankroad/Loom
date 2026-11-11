//! HTTP/WS surface of `loomd` (contract 01 §1.2·§1.7): `/rpc`, `/api/v1/*`, static UI.
//!
//! Every request passes [`guard`]: Host must be `localhost:<port>` or `127.0.0.1:<port>` (DNS
//! rebinding), a present `Origin` must be the canonical origin (or an allowed dev origin), page
//! loads on `127.0.0.1` are redirected to the canonical `localhost` origin, and security headers
//! are added to every response.

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use loom_proto::rpc::{Rights, Role};
use serde_json::{Value, json};

use crate::auth::{Auth, CODE_TTL};
use crate::daemon::{Caller, DEFAULT_PORT, Daemon};

async fn bind(port: Option<u16>) -> std::io::Result<tokio::net::TcpListener> {
    match port {
        Some(p) => tokio::net::TcpListener::bind(("127.0.0.1", p)).await,
        None => match tokio::net::TcpListener::bind(("127.0.0.1", DEFAULT_PORT)).await {
            Ok(l) => Ok(l),
            Err(e) => {
                tracing::warn!("port {DEFAULT_PORT} unavailable ({e}); using an OS-assigned port");
                tokio::net::TcpListener::bind(("127.0.0.1", 0)).await
            }
        },
    }
}

/// Bind and serve in the background.
pub async fn serve(d: Arc<Daemon>) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = bind(d.cfg.port).await?;
    let addr = listener.local_addr()?;
    d.set_port(addr.port());
    let app = Router::new()
        .route("/rpc", get(rpc))
        .route("/api/v1/bootstrap/new", get(bootstrap_new))
        .route("/api/v1/bootstrap", post(bootstrap))
        .route("/api/v1/stop", post(http_stop))
        .route("/", get(|| async { Redirect::temporary("/ui/") }))
        .with_state(d.clone());
    let assets = if d.cfg.ui_dir.is_some() {
        Router::new()
            .route("/ui", get(|| async { Redirect::permanent("/ui/") }))
            .route("/ui/", get(static_file))
            .route("/ui/{*path}", get(static_file))
            .with_state(d.clone())
    } else {
        loom_ui_assets::router()
    };
    let app = app.merge(assets).layer(middleware::from_fn_with_state(d, guard));
    let h = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("loomd server stopped: {e}");
        }
    });
    Ok((addr, h))
}

fn deny(status: StatusCode, msg: &str) -> Response {
    (status, msg.to_owned()).into_response()
}

/// Canonical UI origin.
pub fn canonical_origin(port: u16) -> String {
    format!("http://localhost:{port}")
}

async fn guard(State(d): State<Arc<Daemon>>, req: Request, next: Next) -> Response {
    let port = d.port();
    let canonical_host = format!("localhost:{port}");
    let ip_host = format!("127.0.0.1:{port}");
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    if host != canonical_host && host != ip_host {
        return deny(StatusCode::FORBIDDEN, "host not allowed");
    }
    if let Some(o) = req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        let ok = o == canonical_origin(port)
            || o == format!("http://{ip_host}")
            || d.cfg.allow_origins.iter().any(|a| a == o);
        if !ok {
            return deny(StatusCode::FORBIDDEN, "origin not allowed");
        }
    }
    let path = req.uri().path().to_owned();
    if host == ip_host && req.method() == Method::GET && path != "/rpc" && !path.starts_with("/api/") {
        let pq = req.uri().path_and_query().map_or("/", |p| p.as_str());
        let to = format!("{}{pq}", canonical_origin(port));
        let mut r = StatusCode::PERMANENT_REDIRECT.into_response();
        if let Ok(v) = HeaderValue::from_str(&to) {
            r.headers_mut().insert(header::LOCATION, v);
        }
        return r;
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    let csp = format!(
        "default-src 'self'; connect-src 'self' ws://localhost:{port}; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'"
    );
    if let Ok(v) = HeaderValue::from_str(&csp) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("serial=(), usb=(), hid=(), bluetooth=()"),
    );
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    resp
}

async fn rpc(State(d): State<Arc<Daemon>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let cookie = d.auth.cookie_rights(&headers);
    // A browser (Origin present) must hold a viewer cookie; non-browser clients authenticate with
    // the agent token in `core.hello`.
    if headers.contains_key(header::ORIGIN) && cookie.is_none() {
        return deny(
            StatusCode::UNAUTHORIZED,
            "viewer cookie required (open the bootstrap URL)",
        );
    }
    ws.max_message_size(1024 * 1024)
        .on_upgrade(move |s| crate::conn::run(s, d, cookie))
}

fn query_param<'a>(q: Option<&'a str>, key: &str) -> Option<&'a str> {
    q?.split('&')
        .find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

async fn bootstrap_new(State(d): State<Arc<Daemon>>, req: Request) -> Response {
    if !d.auth.bearer_ok(req.headers()) {
        return deny(StatusCode::UNAUTHORIZED, "agent token required");
    }
    // Only downgrades are possible (`loom ui pair` = watch).
    let rights = match query_param(req.uri().query(), "rights") {
        Some("watch") => Rights::Watch,
        Some("operate") => Rights::Operate.min(d.cfg.bootstrap_rights),
        _ => d.cfg.bootstrap_rights,
    };
    let code = d.auth.issue_code(rights);
    // UI deep link (Q18): `#boot=<code>[&next=<route>]`
    let next = query_param(req.uri().query(), "next")
        .map(|n| format!("&next={n}"))
        .unwrap_or_default();
    let url = format!("{}/ui/#boot={code}{next}", canonical_origin(d.port()));
    axum::Json(
        json!({"code": code, "url": url, "rights": rights, "expires_in_ms": CODE_TTL.as_millis() as u64}),
    )
    .into_response()
}

async fn bootstrap(State(d): State<Arc<Daemon>>, body: axum::body::Bytes) -> Response {
    // UI Q18: 204 + Set-Cookie; malformed body = 400, unknown/used/expired code = 410 (no detail).
    let Some(code) = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v["code"].as_str().map(str::to_owned))
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some((session, _rights)) = d.auth.redeem(&code) else {
        return StatusCode::GONE.into_response();
    };
    let mut r = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = HeaderValue::from_str(&Auth::set_cookie(&session)) {
        r.headers_mut().insert(header::SET_COOKIE, v);
    }
    r
}

/// `POST /api/v1/stop` — STOP while the WS is down. Agent token or any viewer cookie.
async fn http_stop(State(d): State<Arc<Daemon>>, headers: HeaderMap) -> Response {
    let caller = if d.auth.bearer_ok(&headers) {
        Caller {
            role: Role::Agent,
            rights: None,
            client_id: "http:agent".into(),
            conn_id: 0,
        }
    } else if let Some(r) = d.auth.cookie_rights(&headers) {
        Caller {
            role: Role::Viewer,
            rights: Some(r),
            client_id: "http:viewer".into(),
            conn_id: 0,
        }
    } else {
        return deny(StatusCode::UNAUTHORIZED, "agent token or viewer cookie required");
    };
    axum::Json(d.stop(&caller, true, None).await).into_response()
}

fn safe_join(root: &Path, url_path: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for c in Path::new(url_path.trim_start_matches('/')).components() {
        match c {
            Component::Normal(s) => p.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(p)
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

async fn static_file(State(d): State<Arc<Daemon>>, req: Request) -> Response {
    let Some(root) = &d.cfg.ui_dir else {
        return (
            StatusCode::OK,
            format!(
                "loomd {} — WebSocket at ws://localhost:{}/rpc (no --ui-dir given)\n",
                env!("CARGO_PKG_VERSION"),
                d.port()
            ),
        )
            .into_response();
    };
    let path = req.uri().path().strip_prefix("/ui").unwrap_or("/");
    let Some(mut file) = safe_join(root, if path == "/" { "/index.html" } else { path }) else {
        return deny(StatusCode::BAD_REQUEST, "bad path");
    };
    if !file.is_file() && file.extension().is_none() {
        file = root.join("index.html"); // SPA route
    }
    match tokio::fs::read(&file).await {
        Ok(b) => Response::builder()
            .header(header::CONTENT_TYPE, content_type(&file))
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from(b))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Err(_) => deny(StatusCode::NOT_FOUND, "not found"),
    }
}
