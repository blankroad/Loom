//! `loomd --fixture-replay`: a WebSocket server that speaks the contract v1 stream protocol from
//! generated / fixture events, so the UI can develop against it before the real gateway (P1).
//!
//! **No authentication** — binds 127.0.0.1 only and warns on start. Viewer role with fixed rights.

pub mod conn;
pub mod hub;
pub mod scenario;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use loom_proto::rpc::Rights;

pub use hub::ReplayHub;
pub use scenario::Scenario;

/// Where events come from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Built-in generators.
    Builtin,
    /// `event_*.json` files of a fixture directory, replayed in a loop.
    Dir(PathBuf),
}

/// Replay server configuration.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Event source.
    pub source: Source,
    /// TCP port (0 = any free port).
    pub port: u16,
    /// Time scale for generators (2.0 = twice as fast).
    pub speed: f64,
    /// Built-in scenario.
    pub scenario: Scenario,
    /// Answer viewer write RPCs with simulated write rows.
    pub echo_writes: bool,
    /// Rights granted to every viewer.
    pub rights: Rights,
    /// Extra allowed browser origins (Vite dev server, e.g. `http://localhost:5173`).
    pub allow_origins: Vec<String>,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        ReplayConfig {
            source: Source::Builtin,
            port: 0,
            speed: 1.0,
            scenario: Scenario::Normal,
            echo_writes: false,
            rights: Rights::Operate,
            allow_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct AppState {
    hub: Arc<ReplayHub>,
    cfg: Arc<ReplayConfig>,
    port: u16,
}

fn allowed(headers: &HeaderMap, st: &AppState) -> bool {
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == format!("localhost:{}", st.port) || h == format!("127.0.0.1:{}", st.port));
    let origin_ok = match headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        None => true, // non-browser clients (CLI, tests) send no Origin
        Some(o) => {
            o == format!("http://localhost:{}", st.port)
                || o == format!("http://127.0.0.1:{}", st.port)
                || st.cfg.allow_origins.iter().any(|a| a == o)
        }
    };
    host_ok && origin_ok
}

async fn rpc(State(st): State<AppState>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !allowed(&headers, &st) {
        return (StatusCode::FORBIDDEN, "origin/host not allowed").into_response();
    }
    // Oversized client messages are answered with a JSON-RPC error by the connection, so allow
    // frames above the 64 KiB contract limit at the WS layer.
    ws.max_message_size(1024 * 1024)
        .on_upgrade(move |socket| conn::run(socket, st.hub, st.cfg))
}

/// `POST /api/v1/stop` (UI C-7): STOP while the WebSocket is down. Replay has no auth.
async fn http_stop(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if !allowed(&headers, &st) {
        return (StatusCode::FORBIDDEN, "origin/host not allowed").into_response();
    }
    st.hub.set_latch(true);
    let rec = st
        .hub
        .publish_wait(vec![scenario::latch_source(&st.hub, true, "http:viewer")])
        .await;
    axum::Json(serde_json::json!({"on": true, "seq": rec.first().map(|r| r.rec.seq)})).into_response()
}

async fn index(State(st): State<AppState>) -> impl IntoResponse {
    format!(
        "loomd fixture replay — WebSocket at ws://localhost:{}/rpc (contract v1, no auth, 127.0.0.1 only)\n",
        st.port
    )
}

/// Bind, start generators and serve in the background. Returns the bound address.
pub async fn serve(cfg: ReplayConfig) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let hub = ReplayHub::start()?;
    match &cfg.source {
        Source::Builtin => scenario::start_builtin(hub.clone(), cfg.scenario, cfg.speed),
        Source::Dir(d) => {
            let recs = scenario::load_dir(d)?;
            scenario::start_dir(hub.clone(), recs, cfg.speed);
        }
    }
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await?;
    let addr = listener.local_addr()?;
    let state = AppState {
        hub,
        cfg: Arc::new(cfg),
        port: addr.port(),
    };
    let app = Router::new()
        .route("/rpc", get(rpc))
        .route("/", get(index))
        .route("/api/v1/stop", axum::routing::post(http_stop))
        .with_state(state);
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("fixture replay server stopped: {e}");
        }
    });
    Ok((addr, handle))
}
