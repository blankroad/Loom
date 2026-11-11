//! RPC handlers. Each one parses its params, runs through [`UdsManager`] and returns after the
//! events of the call were published (publish-before-reply).

use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::{BoxFuture, CallCtx, DomainError, Handler, JournalQuery};
use loom_proto::cursor::Cursor;
use loom_proto::error::ErrorCode;
use loom_proto::event::WriteState;
use loom_proto::rpc::SubFilter;
use loom_uds_codec::request;
use serde_json::{Value, json};

use crate::hooks::dry_run;
use crate::manager::{ConnectSpec, DEFAULT_TESTER_LA, Outcome, ReqSpec, SIM_ECU_LA, UdsManager};
use crate::text::{encode_typed, parse_did, parse_text};

/// Most rows `uds.trace` returns.
pub const MAX_TRACE_ROWS: u32 = 1000;
/// Bytes `uds.trace` may scan in the journal.
pub const TRACE_SCAN_BYTES: u64 = 4 * 1024 * 1024;

type Run = for<'a> fn(&'a UdsManager, &'a mut CallCtx, Value) -> BoxFuture<'a, Result<Value, DomainError>>;

/// A handler = manager + function.
struct H {
    m: Arc<UdsManager>,
    f: Run,
}

impl std::fmt::Debug for H {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdsHandler").finish_non_exhaustive()
    }
}

impl Handler for H {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        (self.f)(&self.m, ctx, params)
    }
}

fn mk(m: Arc<UdsManager>, f: Run) -> Arc<dyn Handler> {
    Arc::new(H { m, f })
}

fn usage(msg: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::Usage, msg)
}

fn str_arg<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

fn u16_arg(p: &Value, k: &str) -> Result<Option<u16>, DomainError> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| usage(format!("{k} must be 0..=65535"))),
        Some(Value::String(s)) => parse_did(s)
            .map(Some)
            .ok_or_else(|| usage(format!("{k} must be hex (0x1001)"))),
        Some(_) => Err(usage(format!("{k} must be a number or hex string"))),
    }
}

fn timeout_arg(p: &Value) -> Option<Duration> {
    p.get("timeout_ms")
        .and_then(Value::as_u64)
        .map(Duration::from_millis)
}

fn target_arg(p: &Value) -> Option<&str> {
    str_arg(p, "target")
}

/// Send `pdu` to the target named in `p` (or the only target).
async fn send(m: &UdsManager, ctx: &CallCtx, p: &Value, pdu: Vec<u8>) -> Result<Outcome, DomainError> {
    let target = m.resolve(target_arg(p))?;
    // Safe-state recovery (A15) runs in the Abort lane; only a viewer can ask for it.
    let recovery = p.get("recovery").and_then(Value::as_bool) == Some(true)
        && ctx.origin() == loom_proto::event::Origin::User;
    m.request(
        ctx,
        ReqSpec {
            target: target.name().to_owned(),
            pdu,
            timeout: timeout_arg(p),
            request_id: None,
            recovery,
        },
    )
    .await
}

pub fn connect(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            let name = str_arg(&p, "name")
                .ok_or_else(|| usage("connect needs name"))?
                .to_owned();
            // `sim: true` or the profile name (`--sim bench`); only `bench` exists.
            let sim = match p.get("sim") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(Value::String(s)) if s == "bench" => true,
                Some(other) => return Err(usage(format!("unknown sim profile {other} (bench)"))),
            };
            let spec = ConnectSpec {
                name,
                sim,
                host: str_arg(&p, "host").map(str::to_owned),
                port: u16_arg(&p, "port")?.unwrap_or(loom_uds_doip::DOIP_PORT),
                la: u16_arg(&p, "la")?.unwrap_or(SIM_ECU_LA),
                tester_la: u16_arg(&p, "tester_la")?.unwrap_or(DEFAULT_TESTER_LA),
                activation: p
                    .get("activation")
                    .and_then(Value::as_u64)
                    .map(|v| u8::try_from(v).map_err(|_| usage("activation must be 0..=255")))
                    .transpose()?
                    .unwrap_or(0),
                auto_reconnect: p.get("auto_reconnect").and_then(Value::as_bool).unwrap_or(true),
                timeout: timeout_arg(&p).unwrap_or(Duration::from_secs(3)),
            };
            if !sim && spec.host.is_none() {
                return Err(usage("connect needs sim:true or host"));
            }
            let mut data = m.connect(ctx, spec).await?;
            data["others_since_last"] = m.others_since_last(ctx.client_id());
            Ok(data)
        })
    })
}

pub fn status(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            let targets: Vec<Value> = match target_arg(&p) {
                Some(name) => vec![json!(m.target(name)?.state())],
                None => m.targets().iter().map(|t| json!(t.state())).collect(),
            };
            Ok(json!({"targets": targets, "others_since_last": m.others_since_last(ctx.client_id())}))
        })
    })
}

pub fn read_did(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            let pdu = encode_typed("uds.read_did", &p).map_err(usage)?;
            send(m, ctx, &p, pdu).await?.into_result()
        })
    })
}

pub fn session(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            let pdu = encode_typed("uds.session", &p).map_err(usage)?;
            send(m, ctx, &p, pdu).await?.into_result()
        })
    })
}

pub fn dtc_list(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            let pdu = encode_typed("uds.dtc.list", &p).map_err(usage)?;
            send(m, ctx, &p, pdu).await?.into_result()
        })
    })
}

/// `uds.write_did`: write, then read back to decide `effective_verified` / `effect_unverified`.
/// The read-back is a real request with its own row (`<request_id>.verify`).
pub fn write_did(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            let pdu = encode_typed("uds.write_did", &p).map_err(usage)?;
            let (did, data) = (u16::from_be_bytes([pdu[1], pdu[2]]), pdu[3..].to_vec());
            let mut out = send(m, ctx, &p, pdu).await?;
            if out.row.write_state == WriteState::Confirmed {
                let target = m.resolve(target_arg(&p))?;
                // A read-back that cannot run (STOP latched, lease, queue full) leaves the
                // confirmed write `effect_unverified`; it must not turn into a failed write.
                let verify = m
                    .request(
                        ctx,
                        ReqSpec {
                            target: target.name().to_owned(),
                            pdu: request::read_dids(&[did]).map_err(|e| usage(e.to_string()))?,
                            timeout: timeout_arg(&p),
                            request_id: Some(format!("{}.verify", out.request_id)),
                            recovery: false,
                        },
                    )
                    .await
                    .ok();
                let read_back = verify
                    .as_ref()
                    .and_then(|v| v.result.as_ref())
                    .and_then(|r| r.response.as_deref())
                    .filter(|r| r.len() >= 3 && r[0] == 0x62)
                    .map(|r| r[3..].to_vec());
                out.row.write_state = match read_back {
                    Some(v) if v == data => WriteState::EffectiveVerified,
                    _ => WriteState::EffectUnverified,
                };
                m.republish(ctx, &target, &out.request_id, &out.row);
            }
            out.into_result()
        })
    })
}

/// `uds.request` (CLI `raw`, UI console line): dry-run preview or send.
pub fn request(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            if p.get("dry_run").and_then(Value::as_bool) == Some(true) {
                let target = target_arg(&p).unwrap_or("");
                return serde_json::to_value(dry_run(target, &p))
                    .map_err(|e| DomainError::new(ErrorCode::Internal, e.to_string()));
            }
            if p.get("functional").and_then(Value::as_bool) == Some(true) {
                return Err(DomainError::new(
                    ErrorCode::Unsupported,
                    "functional addressing is not wired yet (M3b)",
                ));
            }
            let pdu = match (str_arg(&p, "bytes_hex"), str_arg(&p, "text")) {
                (Some(_), _) => encode_typed("uds.request", &p).map_err(usage)?,
                (None, Some(t)) => parse_text(t).map_err(|e| {
                    usage(format!("at {}: {}", e.at, e.message)).with_detail(json!({"parse_error": e}))
                })?,
                (None, None) => return Err(usage("uds.request needs text or bytes_hex")),
            };
            send(m, ctx, &p, pdu).await?.into_result()
        })
    })
}

pub fn abort(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move { m.abort(ctx, target_arg(&p), str_arg(&p, "scope")) })
    })
}

pub fn lease_takeover(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move { m.lease_takeover(ctx, target_arg(&p)) })
    })
}

pub fn lease_release(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move { m.lease_release(ctx, target_arg(&p)) })
    })
}

/// `uds.trace`: bounded journal read of the target's `uds.*` events.
pub fn trace(m: Arc<UdsManager>) -> Arc<dyn Handler> {
    mk(m, |m, ctx, p| {
        Box::pin(async move {
            let tail = p
                .get("tail")
                .and_then(Value::as_u64)
                .unwrap_or(40)
                .min(u64::from(MAX_TRACE_ROWS));
            let channels = match target_arg(&p) {
                Some(t) => vec![m.target(t)?.name().to_owned()],
                None => Vec::new(),
            };
            let q = JournalQuery {
                from: Cursor::Last,
                until: None,
                filter: SubFilter {
                    domains: vec![crate::DOMAIN.into()],
                    channels,
                    ..SubFilter::default()
                },
                request_id: None,
                max_events: u32::try_from(tail).unwrap_or(MAX_TRACE_ROWS),
            };
            let r = ctx.journal().scan(&q, TRACE_SCAN_BYTES).await?;
            let rows: Vec<Value> = r
                .events
                .iter()
                .map(|e| json!({"seq": e.seq, "kind": e.kind, "origin": e.origin, "request_id": e.request_id, "payload": e.payload}))
                .collect();
            Ok(
                json!({"events": rows, "complete": r.complete, "others_since_last": m.others_since_last(ctx.client_id())}),
            )
        })
    })
}
