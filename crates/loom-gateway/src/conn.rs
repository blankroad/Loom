//! One `loomd` WebSocket connection: hello/role, subscriptions, dispatch, control-first delivery,
//! heartbeat, ping (20 s, closed after 40 s of silence).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use loom_proto::error::ErrorCode;
use loom_proto::num::U64;
use loom_proto::rpc::{
    Hello, HelloResult, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, Response, Rights, Role, RpcError,
    ServerInfo, SubscribeResult,
};
use loom_proto::stream::{CloseReason, Closed, NOTIFY_CLOSED};
use serde_json::{Value, json};

use crate::daemon::{Caller, Daemon, PROTO};
use crate::hub::stream::{drain, heartbeat, notification, parse_subscribe, start_frames};
use crate::hub::{ConnShared, MAX_SUBS_PER_CONN, MAX_SUBS_TOTAL};
use crate::rpcutil::{app_error, app_error_detail, params, parse_request, reply, resp, rpc_error};

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

/// Ping interval.
pub const PING: Duration = Duration::from_secs(20);
/// Silence after which the connection is dropped.
pub const IDLE: Duration = Duration::from_secs(40);

struct Conn {
    d: Arc<Daemon>,
    cookie: Option<Rights>,
    caller: Option<Caller>,
    shared: Option<Arc<ConnShared>>,
    next_sub: u32,
    close: Option<Closed>,
}

impl Conn {
    async fn hello(&mut self, p: Option<Value>) -> Result<Value, RpcError> {
        let h: Hello = params(p)?;
        if !h.proto.starts_with("loom/1.") {
            self.close = Some(Closed {
                reason: CloseReason::ProtoMismatch,
                detail: Some(format!("server speaks {PROTO}")),
                retry_after_ms: None,
            });
            return Err(app_error(ErrorCode::ProtoMismatch, "proto must be loom/1.x"));
        }
        if self.caller.is_some() {
            return Err(rpc_error(INVALID_REQUEST, "core.hello already done"));
        }
        // Role comes from credentials only; `want_role` can only narrow agent → script.
        let (role, rights) = match (&h.token, self.cookie) {
            (Some(t), _) if self.d.auth.is_token(t) => (
                if h.want_role == Role::Script {
                    Role::Script
                } else {
                    Role::Agent
                },
                None,
            ),
            (Some(_), _) => {
                self.close = Some(Closed {
                    reason: CloseReason::AuthExpired,
                    detail: Some("bad token".into()),
                    retry_after_ms: None,
                });
                return Err(app_error(
                    ErrorCode::Denied,
                    "invalid token (re-read daemon.json)",
                ));
            }
            (None, Some(r)) => (Role::Viewer, Some(r)),
            (None, None) => {
                self.close = Some(Closed {
                    reason: CloseReason::AuthExpired,
                    detail: Some("no credentials".into()),
                    retry_after_ms: None,
                });
                return Err(app_error(
                    ErrorCode::Denied,
                    "no credentials: agent token or viewer cookie required",
                ));
            }
        };
        let id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
        let shared = ConnShared::new(id, role == Role::Viewer);
        self.d
            .hub
            .connect(shared.clone())
            .await
            .map_err(|code| app_error(code, "too many connections"))?;
        if role == Role::Viewer {
            self.d.viewer_joined(id);
        }
        self.shared = Some(shared);
        self.caller = Some(Caller {
            role,
            rights,
            client_id: h.client_id,
            conn_id: id,
        });
        let res = HelloResult {
            role,
            rights,
            conn_id: format!("c{id}"),
            stream_epoch: U64(self.d.hub.epoch),
            server: ServerInfo {
                version: env!("CARGO_PKG_VERSION").into(),
                build: "loomd".into(),
                schema_hash: None,
            },
            env: self.d.env(),
            latch: self.d.latched(),
            viewers: self.d.viewers(),
            capabilities: vec!["core/1".into()],
        };
        serde_json::to_value(res).map_err(|e| rpc_error(INTERNAL_ERROR, e.to_string()))
    }

    async fn subscribe(&mut self, p: Option<Value>) -> (Result<Value, RpcError>, Vec<String>) {
        let req = match parse_subscribe(p) {
            Ok(r) => r,
            Err(e) => return (Err(rpc_error(INVALID_PARAMS, e.to_string())), vec![]),
        };
        let Some(shared) = self.shared.clone() else {
            return (Err(rpc_error(INVALID_REQUEST, "no hello")), vec![]);
        };
        self.next_sub += 1;
        let sub = format!("s{}", self.next_sub);
        match self.d.hub.subscribe(shared.clone(), &sub, req).await {
            Ok(start) => {
                let frames = start_frames(&self.d.hub, &shared, &sub, start);
                let r = serde_json::to_value(SubscribeResult {
                    sub_id: sub,
                    stream_epoch: U64(self.d.hub.epoch),
                })
                .map_err(|e| rpc_error(INTERNAL_ERROR, e.to_string()));
                (r, frames)
            }
            Err(code) => (
                Err(app_error_detail(
                    code,
                    "subscription limit reached",
                    json!({"per_connection": MAX_SUBS_PER_CONN, "total": MAX_SUBS_TOTAL}),
                )),
                vec![],
            ),
        }
    }

    async fn dispatch(&mut self, method: &str, p: Option<Value>) -> (Result<Value, RpcError>, Vec<String>) {
        if method == "core.hello" {
            return (self.hello(p).await, vec![]);
        }
        let Some(caller) = self.caller.clone() else {
            return (
                Err(rpc_error(INVALID_REQUEST, "core.hello must be the first call")),
                vec![],
            );
        };
        match method {
            "core.subscribe" => self.subscribe(p).await,
            "core.unsubscribe" => {
                let v: Value = match params(p) {
                    Ok(v) => v,
                    Err(e) => return (Err(e), vec![]),
                };
                if let (Some(s), Some(id)) = (&self.shared, v.get("sub_id").and_then(Value::as_str)) {
                    self.d.hub.unsubscribe(s.clone(), id);
                }
                (Ok(json!({})), vec![])
            }
            m => (self.d.call(&caller, m, p).await, vec![]),
        }
    }

    async fn handle_text(&mut self, text: &str) -> Vec<String> {
        let req = match parse_request(text) {
            Ok(r) => r,
            Err(frame) => return vec![frame],
        };
        let (result, extra) = self.dispatch(&req.method, req.params).await;
        // Control events this call caused go out before its reply.
        let mut out = match &self.shared {
            Some(s) => drain(&self.d.hub, s, true, self.d.record_health()).frames,
            None => vec![],
        };
        out.extend(reply(req.id, result));
        out.extend(extra);
        out
    }
}

/// Serve one upgraded WebSocket until it closes.
pub async fn run(ws: WebSocket, d: Arc<Daemon>, cookie: Option<Rights>) {
    let (mut sink, mut stream) = ws.split();
    let mut c = Conn {
        d: d.clone(),
        cookie,
        caller: None,
        shared: None,
        next_sub: 0,
        close: None,
    };
    let mut hb = tokio::time::interval(Duration::from_secs(1));
    hb.tick().await;
    let mut ping = tokio::time::interval(PING);
    ping.tick().await;
    let mut last_rx = Instant::now();
    let mut deadline: Option<Instant> = None;
    loop {
        let shared = c.shared.clone();
        let notified = async {
            match &shared {
                Some(s) => s.notify.notified().await,
                None => std::future::pending().await,
            }
        };
        let sleep = async {
            match deadline {
                Some(t) => tokio::time::sleep_until(t.into()).await,
                None => std::future::pending().await,
            }
        };
        let mut out = Vec::new();
        tokio::select! {
            msg = stream.next() => {
                last_rx = Instant::now();
                match msg {
                    Some(Ok(Message::Text(t))) => out = c.handle_text(t.as_str()).await,
                    Some(Ok(Message::Binary(_))) => out.push(resp(Response::err(None, rpc_error(INVALID_REQUEST, "binary frames are not accepted")))),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                }
            }
            () = notified => {}
            _ = hb.tick() => if let Some(s) = &c.shared {
                out.push(heartbeat(&d.hub, s, d.hw(), d.latched(), d.record_health()));
            },
            _ = ping.tick() => {
                if last_rx.elapsed() > IDLE {
                    break;
                }
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            () = sleep => {}
        }
        if let Some(s) = &c.shared {
            let dr = drain(&d.hub, s, false, d.record_health());
            deadline = dr.deadline;
            out.extend(dr.frames);
            if dr.overflow {
                c.close = Some(Closed {
                    reason: CloseReason::SlowConsumer,
                    detail: Some("control queue overflow — reconnect and resubscribe".into()),
                    retry_after_ms: Some(500),
                });
            }
        }
        let mut ok = true;
        for f in out {
            if sink.send(Message::Text(f.into())).await.is_err() {
                ok = false;
                break;
            }
        }
        if let Some(cl) = c.close.take() {
            let _ = sink
                .send(Message::Text(notification(NOTIFY_CLOSED, &cl).into()))
                .await;
            let _ = sink.send(Message::Close(None)).await;
            break;
        }
        if !ok {
            break;
        }
    }
    if let Some(s) = &c.shared {
        d.hub.disconnect(s.id);
        d.conn_closed(s.id, s.viewer);
    }
}
