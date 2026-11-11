//! One fixture-replay WebSocket connection: JSON-RPC dispatch on top of the shared stream logic.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use loom_proto::core_events::{
    SafeStateItem, SafeStateKind, SafeStateList, SafeStateOutcome, SafeStateRun, SafeStateRunItem,
    SafeStateRunResult,
};
use loom_proto::envelope::{RecordHealth, Viewers};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, Risk};
use loom_proto::num::U64;
use loom_proto::rpc::{
    Hello, HelloResult, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, Response, Rights,
    Role, RpcError, ServerInfo, SubscribeResult, validate_method,
};
use loom_proto::stream::{CloseReason, Closed, NOTIFY_CLOSED, TimeResult};
use serde_json::{Value, json};

use super::ReplayConfig;
use super::hub::ReplayHub;
use super::scenario::latch_source;
use crate::hub::stream::{drain, heartbeat, notification, parse_subscribe, start_frames};
use crate::hub::{ConnShared, MAX_SUBS_PER_CONN, MAX_SUBS_TOTAL};
use crate::rpcutil::{app_error, app_error_detail, params, parse_request, reply, resp, rpc_error};

/// Write RPCs echoed in `--echo-writes` mode → event kind of their write rows.
const ECHO_WRITES: &[(&str, &str, &str)] = &[
    ("uart.send", "uart.write", "dut"),
    ("uart.input", "uart.write", "dut"),
    ("can.send", "can.tx", "ch0"),
    ("uds.request", "uds.tx", "ecu1"),
];

struct Conn {
    rh: Arc<ReplayHub>,
    cfg: Arc<ReplayConfig>,
    shared: Option<Arc<ConnShared>>,
    client_id: String,
    next_sub: u32,
    next_req: u64,
}

impl Conn {
    fn require(&self, need: Rights) -> Result<(), RpcError> {
        if self.cfg.rights >= need {
            Ok(())
        } else {
            Err(app_error(ErrorCode::Denied, "insufficient viewer rights"))
        }
    }

    async fn dispatch(&mut self, method: &str, p: Option<Value>) -> (Result<Value, RpcError>, Vec<String>) {
        if method != "core.hello" && self.shared.is_none() {
            return (
                Err(rpc_error(INVALID_REQUEST, "core.hello must be the first call")),
                vec![],
            );
        }
        let r = match method {
            "core.hello" => self.hello(p).await,
            "core.subscribe" => return self.subscribe(p).await,
            "core.unsubscribe" => {
                let v: Value = match params(p) {
                    Ok(v) => v,
                    Err(e) => return (Err(e), vec![]),
                };
                if let (Some(s), Some(id)) = (&self.shared, v.get("sub_id").and_then(Value::as_str)) {
                    self.rh.hub.unsubscribe(s.clone(), id);
                }
                Ok(json!({}))
            }
            "core.time" => {
                let clock = &self.rh.hub.clock;
                let recv = clock.mono_ns();
                let t = TimeResult {
                    recv_mono_ns: U64(recv),
                    send_mono_ns: U64(clock.mono_ns()),
                    wall_ns: U64(clock.wall_ns()),
                    clock_id: clock.clock_id.clone(),
                };
                Ok(serde_json::to_value(t).unwrap_or(Value::Null))
            }
            "core.viewer.presence" | "core.metrics.report" => {
                let report = p.unwrap_or(Value::Null).to_string();
                tracing::info!(%method, %report, "ui report");
                Ok(json!({}))
            }
            // STOP is allowed for every viewer (contract v1.1, UI Q3); release needs operate.
            "core.stop" => Ok(self.latch(true).await),
            "core.stop.release" => match self.require(Rights::Operate) {
                Ok(()) => Ok(self.latch(false).await),
                Err(e) => Err(e),
            },
            "core.safe_state.list" => {
                Ok(serde_json::to_value(SafeStateList { items: safe_items() }).unwrap_or(Value::Null))
            }
            "core.safe_state.run" => match self.require(Rights::Operate) {
                Ok(()) => self.safe_run(p).await,
                Err(e) => Err(e),
            },
            m => match validate_method(m) {
                Err(_) => Err(rpc_error(METHOD_NOT_FOUND, format!("method not found: {m}"))),
                Ok(()) => self.echo_write(m, p).await,
            },
        };
        (r, vec![])
    }

    async fn hello(&mut self, p: Option<Value>) -> Result<Value, RpcError> {
        let h: Hello = params(p)?;
        if !h.proto.starts_with("loom/1.") {
            return Err(app_error(ErrorCode::ProtoMismatch, "proto must be loom/1.x"));
        }
        if self.shared.is_none() {
            let c = ConnShared::new(self.rh.next_conn_id(), true);
            self.rh
                .hub
                .connect(c.clone())
                .await
                .map_err(|code| app_error(code, "too many connections"))?;
            self.shared = Some(c);
        }
        self.client_id = h.client_id;
        let res = HelloResult {
            role: Role::Viewer,
            rights: Some(self.cfg.rights),
            conn_id: format!("c{}", self.shared.as_ref().map_or(0, |s| s.id)),
            stream_epoch: U64(self.rh.epoch()),
            server: ServerInfo {
                version: env!("CARGO_PKG_VERSION").into(),
                build: "fixture-replay".into(),
                schema_hash: None,
            },
            env: Env::Sim,
            latch: self.rh.latch(),
            viewers: Viewers {
                subscribed: 1,
                visible: 1,
            },
            capabilities: vec!["core/1".into(), "fixture-replay".into()],
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
        match self.rh.hub.subscribe(shared.clone(), &sub, req).await {
            Ok(start) => {
                let frames = start_frames(&self.rh.hub, &shared, &sub, start);
                let r = serde_json::to_value(SubscribeResult {
                    sub_id: sub,
                    stream_epoch: U64(self.rh.epoch()),
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

    async fn latch(&mut self, on: bool) -> Value {
        self.rh.set_latch(on);
        let rec = self
            .rh
            .publish_wait(vec![latch_source(&self.rh, on, &self.client_id)])
            .await;
        json!({"latch": on, "seq": rec.first().map(|r| r.rec.seq)})
    }

    async fn rows(&mut self, method: &str, kind: &str, channel: &str, echo: &Value) -> (String, Vec<U64>) {
        self.next_req += 1;
        let rid = format!(
            "replay-user-{}-{}",
            self.shared.as_ref().map_or(0, |s| s.id),
            self.next_req
        );
        let domain = method.split('.').next().unwrap_or("core");
        let states = ["requested", "accepted", "sent", "confirmed"];
        let srcs = states
            .iter()
            .map(|st| {
                let mut payload = json!({"write_state": st, "echo_of": echo});
                if *st == "confirmed" {
                    payload["final"] = json!(true);
                }
                let mut s = self
                    .rh
                    .source(domain, Some(channel), kind, None, Origin::User, payload);
                s.client_id = Some(self.client_id.clone());
                s.request_id = Some(rid.clone());
                s.risk = Some(Risk::R1);
                s.purpose = Some("user_send".into());
                s
            })
            .collect();
        let recs = self.rh.publish_wait(srcs).await;
        (rid, recs.iter().map(|r| r.rec.seq).collect())
    }

    async fn echo_write(&mut self, method: &str, p: Option<Value>) -> Result<Value, RpcError> {
        let Some(&(_, kind, channel)) = ECHO_WRITES
            .iter()
            .find(|(m, ..)| *m == method)
            .filter(|_| self.cfg.echo_writes)
        else {
            return Err(app_error(
                ErrorCode::NotImplemented,
                &format!("{method} is not implemented in fixture replay"),
            ));
        };
        self.require(Rights::Operate)?;
        let (rid, seqs) = self.rows(method, kind, channel, &p.unwrap_or(Value::Null)).await;
        Ok(json!({"request_id": rid, "state": "confirmed", "seq": seqs.last(), "simulated": true}))
    }

    async fn safe_run(&mut self, p: Option<Value>) -> Result<Value, RpcError> {
        let run: SafeStateRun = params(p)?;
        let known = safe_items();
        let mut items = Vec::new();
        for id in run.ids {
            match known.iter().find(|i| i.id == id) {
                Some(i) => {
                    let method = i.method.clone();
                    let (rid, _) = self
                        .rows(&method, "core.recover", "ch0", &json!({"safe_state": id}))
                        .await;
                    items.push(SafeStateRunItem {
                        id,
                        outcome: SafeStateOutcome::Sent,
                        finally_blocked: false,
                        reason: None,
                        request_id: Some(rid),
                    });
                }
                None => items.push(SafeStateRunItem {
                    id,
                    outcome: SafeStateOutcome::Skipped,
                    finally_blocked: false,
                    reason: Some("unknown safe-state id".into()),
                    request_id: None,
                }),
            }
        }
        serde_json::to_value(SafeStateRunResult { items })
            .map_err(|e| rpc_error(INTERNAL_ERROR, e.to_string()))
    }

    /// Answer one text frame. Everything this call published is sent before the reply.
    async fn handle_text(&mut self, text: &str) -> Vec<String> {
        let req = match parse_request(text) {
            Ok(r) => r,
            Err(frame) => return vec![frame],
        };
        let (result, extra) = self.dispatch(&req.method, req.params).await;
        let mut out = match &self.shared {
            Some(s) => drain(&self.rh.hub, s, true, RecordHealth::Ok).frames,
            None => vec![],
        };
        out.extend(reply(req.id, result));
        out.extend(extra);
        out
    }
}

fn safe_items() -> Vec<SafeStateItem> {
    vec![
        SafeStateItem {
            kind: SafeStateKind::Recover,
            id: "can.periodic_stop".into(),
            domain: "can".into(),
            channel: Some("ch0".into()),
            method: "can.periodic.stop".into(),
            description: "stop all periodic CAN transmissions".into(),
            blocked: false,
        },
        SafeStateItem {
            kind: SafeStateKind::Recover,
            id: "uds.recover".into(),
            domain: "uds".into(),
            channel: Some("ecu1".into()),
            method: "uds.recover".into(),
            description: "return ECU to default session".into(),
            blocked: false,
        },
    ]
}

/// Serve one upgraded WebSocket until it closes.
pub async fn run(ws: WebSocket, rh: Arc<ReplayHub>, cfg: Arc<ReplayConfig>) {
    let (mut sink, mut stream) = ws.split();
    let mut c = Conn {
        rh: rh.clone(),
        cfg,
        shared: None,
        client_id: "web:replay".into(),
        next_sub: 0,
        next_req: 0,
    };
    let mut hb = tokio::time::interval(Duration::from_secs(1));
    hb.tick().await;
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
                Some(d) => tokio::time::sleep_until(d.into()).await,
                None => std::future::pending().await,
            }
        };
        let mut out = Vec::new();
        tokio::select! {
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(t))) => out = c.handle_text(t.as_str()).await,
                Some(Ok(Message::Binary(_))) => out.push(resp(Response::err(None, rpc_error(INVALID_REQUEST, "binary frames are not accepted")))),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            },
            () = notified => {}
            _ = hb.tick() => if let Some(s) = &c.shared {
                out.push(heartbeat(&rh.hub, s, rh.hw().await, rh.latch(), RecordHealth::Ok));
            },
            () = sleep => {}
        }
        if let Some(s) = &c.shared {
            let d = drain(&rh.hub, s, false, RecordHealth::Ok);
            deadline = d.deadline;
            out.extend(d.frames);
            if d.overflow {
                let closed = Closed {
                    reason: CloseReason::SlowConsumer,
                    detail: Some("control queue overflow — reconnect and resubscribe".into()),
                    retry_after_ms: Some(500),
                };
                let _ = sink
                    .send(Message::Text(notification(NOTIFY_CLOSED, &closed).into()))
                    .await;
                let _ = sink.send(Message::Close(None)).await;
                break;
            }
        }
        let mut ok = true;
        for f in out {
            if sink.send(Message::Text(f.into())).await.is_err() {
                ok = false;
                break;
            }
        }
        if !ok {
            break;
        }
    }
    if let Some(s) = &c.shared {
        rh.hub.disconnect(s.id);
    }
}
