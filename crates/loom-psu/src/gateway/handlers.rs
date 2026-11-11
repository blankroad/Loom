//! Command handlers. Supply access goes to the worker; `watch` and `wait_for` read the journal
//! (the same `psu.meas` records the UI plots).

use std::time::{Duration, Instant};

use loom_domain_api::{BoxFuture, CallCtx, DomainError, Handler, TargetRef, WriteHooks, WriteRequest};
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, PublishedRecord};
use loom_proto::policy::WriteClass;
use serde_json::{Value, json};

use super::common::*;
use super::hooks::PsuHooks;
use crate::model::*;
use crate::{KIND_MEAS, WORKER_KIND};

/// Poll period of `wait_for` (twice the sample rate).
pub const WAIT_POLL: Duration = Duration::from_millis(100);
/// Raw rows `watch --raw` may return.
pub const WATCH_MAX_ROWS: usize = 1000;
/// Default raw rows.
pub const WATCH_DEFAULT_ROWS: usize = 100;
/// Steps a wait keeps (hold ≤ 3600 s at 5 Hz is 18 000).
pub const WAIT_MAX_STEPS: usize = 40_000;

fn deny(msg: String, detail: Value) -> DomainError {
    DomainError::new(ErrorCode::Denied, msg).with_detail(detail)
}

fn dry(p: &Value) -> bool {
    p.get("dry_run").and_then(Value::as_bool).unwrap_or(false)
}

/// Worker op of `psu.out` — the RPC and the emergency-OFF path build exactly this.
pub fn out_op(ch: &str, state: &str) -> Value {
    json!({"action": "out", "ch": ch, "state": state})
}

/// Forwards a call to the worker (`psu.status`, `psu.meas`, `psu.sim.inject`).
#[derive(Debug)]
pub struct Forward(pub &'static str, pub ChannelBook);

impl Handler for Forward {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let ch = resolve_channel(&params, &self.1)?;
            let mut args = params;
            args["ch"] = json!(ch);
            ctx.worker(WORKER_KIND)?.call(self.0, args, ctx.deadline()).await
        })
    }
}

/// `psu.connect{ch, backend?, vmax?, imax?, seed?, load?}` — M1 drives SimPsu only.
#[derive(Debug)]
pub struct Connect;

impl Handler for Connect {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            match params.get("backend").and_then(Value::as_str) {
                None | Some("sim") => {}
                Some(b) => {
                    return Err(DomainError::new(
                        ErrorCode::NotImplemented,
                        format!("backend {b:?}: real supplies come when one is bought (M1 = sim)"),
                    ));
                }
            }
            let mut args = params;
            if param_ch(&args).is_none() {
                args["ch"] = json!("bench");
            }
            ctx.worker(WORKER_KIND)?
                .call("psu.connect", args, ctx.deadline())
                .await
        })
    }
}

/// Dry run: class and reason as the hooks see it; nothing reaches the worker.
fn preview(ctx: &CallCtx, hooks: &PsuHooks, method: &str, ch: &str, params: &Value, op: Value) -> Value {
    let target = TargetRef {
        channel: Some(ch.to_owned()),
        target: None,
        generation: None,
    };
    let req = WriteRequest {
        domain: crate::DOMAIN,
        method,
        target: &target,
        bytes: None,
        args: params,
        origin: ctx.origin(),
        client_id: "preview",
        env: Env::Sim,
        write_class: WriteClass::Request,
        job: None,
    };
    let c = hooks.classify(&req);
    json!({"dry_run": true, "risk": c.risk, "reason": c.reason, "blocked": c.block.is_some(), "op": op})
}

async fn write(ctx: &CallCtx, rid: &str, op: Value) -> Result<Value, DomainError> {
    ctx.worker(WORKER_KIND)?.write(rid, op, ctx.deadline()).await
}

/// `psu.set{ch, volt?, curr?, dry_run?}` (`v`/`i` accepted for the UI panel).
#[derive(Debug)]
pub struct Set(pub PsuHooks);

impl Handler for Set {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let ch = resolve_channel(&params, &self.0.book)?;
            let (v, i) = (param_volt(&params), param_curr(&params));
            if v.is_none() && i.is_none() {
                return Err(usage("set needs volt and/or curr"));
            }
            if [v, i].into_iter().flatten().any(|x| !x.is_finite() || x < 0.0) {
                return Err(usage("volt/curr must be ≥ 0"));
            }
            let op = json!({"action": "set", "ch": ch, "volt": v, "curr": i});
            if dry(&params) {
                return Ok(preview(ctx, &self.0, "psu.set", &ch, &params, op));
            }
            let limits = self.0.book.get(&ch).map(|c| c.limits()).unwrap_or_default();
            if let Some(why) = limits.violation(v, i) {
                return Err(deny(
                    why,
                    json!({"vmax": limits.vmax, "imax": limits.imax, "volt": v, "curr": i}),
                ));
            }
            let rid = ctx.request_id().unwrap_or("psu-set").to_owned();
            write(ctx, &rid, op).await
        })
    }
}

/// `psu.out{ch, state, dry_run?}`. Also the emergency-OFF and safe-state action: with
/// `state:"off"` and no channel it switches every known supply off.
#[derive(Debug)]
pub struct Out(pub PsuHooks);

impl Handler for Out {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let state = match params.get("state").and_then(Value::as_str) {
                Some(s @ ("on" | "off")) => s.to_owned(),
                _ => return Err(usage("out needs state on|off")),
            };
            let chans = match (param_ch(&params), state.as_str()) {
                (Some(c), _) => vec![c.to_owned()],
                (None, "off") if !self.0.book.names().is_empty() => self.0.book.names(),
                _ => vec![resolve_channel(&params, &self.0.book)?],
            };
            if dry(&params) {
                let ch = chans.first().cloned().unwrap_or_default();
                return Ok(preview(
                    ctx,
                    &self.0,
                    "psu.out",
                    &ch,
                    &params,
                    out_op(&ch, &state),
                ));
            }
            let rid = ctx.request_id().unwrap_or("psu-out").to_owned();
            if let [ch] = chans.as_slice() {
                return write(ctx, &rid, out_op(ch, &state)).await;
            }
            // Several supplies: one request id per supply keeps each write-row chain valid (E8).
            let mut results = Vec::new();
            for ch in &chans {
                let r = write(ctx, &format!("{rid}/{ch}"), out_op(ch, &state)).await;
                results.push(match r {
                    Ok(v) => json!({"ch": ch, "ok": true, "result": v}),
                    Err(e) => json!({"ch": ch, "ok": false, "error": e.to_string()}),
                });
            }
            Ok(json!({"results": results}))
        })
    }
}

/// `psu.cycle{ch, off_ms, dry_run?}`.
#[derive(Debug)]
pub struct Cycle(pub PsuHooks);

impl Handler for Cycle {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let ch = resolve_channel(&params, &self.0.book)?;
            let off_ms = params
                .get("off_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| usage("cycle needs off_ms"))?;
            if off_ms == 0 || off_ms > MAX_CYCLE_OFF_MS {
                return Err(usage(format!("off_ms must be 1..={MAX_CYCLE_OFF_MS}")));
            }
            let op = json!({"action": "cycle", "ch": ch, "off_ms": off_ms});
            if dry(&params) {
                return Ok(preview(ctx, &self.0, "psu.cycle", &ch, &params, op));
            }
            let rid = ctx.request_id().unwrap_or("psu-cycle").to_owned();
            write(ctx, &rid, op).await
        })
    }
}

fn ms_cursor(ms: u64) -> Cursor {
    Cursor::Relative {
        amount: ms,
        unit: TimeUnit::Millis,
    }
}

#[derive(Debug, Default)]
struct Stat {
    n: u64,
    min: f64,
    max: f64,
    sum: f64,
    last: Option<f64>,
}

impl Stat {
    fn add(&mut self, x: f64) {
        if self.n == 0 {
            self.min = x;
            self.max = x;
        }
        self.n += 1;
        self.min = self.min.min(x);
        self.max = self.max.max(x);
        self.sum += x;
        self.last = Some(x);
    }
    fn json(&self) -> Value {
        if self.n == 0 {
            return Value::Null;
        }
        json!({"min": self.min, "max": self.max, "mean": self.sum / self.n as f64, "last": self.last})
    }
}

/// Summary of a window of records (`watch`).
pub fn summarize(ch: &str, events: &[PublishedRecord], for_ms: u64, raw_max: Option<usize>) -> Value {
    let (mut v, mut i, mut p) = (Stat::default(), Stat::default(), Stat::default());
    let mut timeouts = 0u64;
    let mut user = Vec::new();
    let mut rows = Vec::new();
    let mut times = (None, None);
    let (mut first, mut last) = (None, None);
    for e in events {
        first.get_or_insert(e.seq.get());
        last = Some(e.seq.get());
        match e.kind.as_str() {
            KIND_MEAS => {
                let m = |n| metric(&e.metrics, n);
                if let (Some(a), Some(b)) = (m("voltage"), m("current")) {
                    v.add(a);
                    i.add(b);
                    p.add(m("power").unwrap_or(a * b));
                    let t = e.ts.mono_ns.get();
                    times = (times.0.or(Some(t)), Some(t));
                    if raw_max.is_some_and(|mx| rows.len() < mx) {
                        rows.push(json!({"seq": e.seq, "ts": e.ts.wall_ns, "v": a, "i": b, "age_ms": e.payload.get("age_ms")}));
                    }
                }
            }
            "source.gap" | "record.gap" => timeouts += 1,
            _ if e.origin == Origin::User && e.kind == crate::KIND_WRITE => {
                if e.payload.get("write_state").and_then(Value::as_str) == Some("sent") {
                    user.push(json!({"seq": e.seq, "action": e.payload.get("action"), "state": e.payload.get("state")}));
                }
            }
            _ => {}
        }
    }
    let rate = match times {
        (Some(a), Some(b)) if b > a && v.n > 1 => Some((v.n - 1) as f64 / ((b - a) as f64 / 1e9)),
        _ => None,
    };
    let mut out = json!({
        "ch": ch, "for_ms": for_ms, "voltage": v.json(), "current": i.json(), "power": p.json(),
        "samples": v.n, "expected": for_ms / (POLL_PERIOD_NS / 1_000_000), "timeouts": timeouts,
        "rate_hz": rate, "rate_req_hz": POLL_HZ, "user_actions": user,
        "ref": match (first, last) { (Some(a), Some(b)) => Value::from(ref_range(ch, a, b)), _ => Value::Null },
        "next_cursor": last.map(|s| format!("seq:{}", s + 1)),
    });
    if raw_max.is_some() {
        out["rows"] = json!(rows);
        out["truncated"] = json!(v.n as usize > rows.len());
    }
    out
}

/// `psu.watch{ch, for_ms, raw?, max?, since?}`: waits `for_ms`, then summarizes the window.
#[derive(Debug)]
pub struct Watch(pub ChannelBook);

impl Handler for Watch {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let ch = resolve_channel(&params, &self.0)?;
            let for_ms = params
                .get("for_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| usage("watch needs for_ms"))?;
            if for_ms > MAX_WATCH_MS {
                return Err(usage(format!("for_ms ≤ {MAX_WATCH_MS}")));
            }
            let raw = params
                .get("raw")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                .then(|| {
                    params
                        .get("max")
                        .and_then(Value::as_u64)
                        .map_or(WATCH_DEFAULT_ROWS, |m| (m as usize).clamp(1, WATCH_MAX_ROWS))
                });
            let since = cursor_param(params.get("since"))?;
            let wait =
                Duration::from_millis(for_ms).min(ctx.deadline().saturating_duration_since(Instant::now()));
            Delay::new(wait).await;
            let from = since.unwrap_or_else(|| ms_cursor(for_ms));
            let (events, _, cut) = scan(ctx, &ch, from, SCAN_BUDGET_BYTES).await?;
            let mut s = summarize(&ch, &events, for_ms, raw);
            s["complete"] = json!(!cut);
            Ok(s)
        })
    }
}

/// `psu.wait_for{ch, curr_lt|curr_gt|curr_between|volt_lt|volt_gt|volt_between, hold_ms,
/// max_gap_ms?, timeout_ms, since?}`. A state condition: samples from the last `hold_ms` before
/// the call count. A user switching the output off during the wait ends it (exit 6).
#[derive(Debug)]
pub struct WaitFor(pub ChannelBook);

impl Handler for WaitFor {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move { wait_for(ctx, &self.0, &params).await })
    }
}

async fn wait_for(ctx: &CallCtx, book: &ChannelBook, params: &Value) -> Result<Value, DomainError> {
    let ch = resolve_channel(params, book)?;
    let cond = Condition::from_params(params).map_err(usage)?;
    let hold_ms = params.get("hold_ms").and_then(Value::as_u64).unwrap_or(0);
    if hold_ms > MAX_HOLD_MS {
        return Err(usage(format!("hold_ms ≤ {MAX_HOLD_MS}")));
    }
    let max_gap_ms = params
        .get("max_gap_ms")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_GAP_MS);
    let timeout_ms = params
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| usage("wait_for needs timeout_ms"))?;
    if timeout_ms > MAX_WAIT_MS {
        return Err(usage(format!("timeout_ms ≤ {MAX_WAIT_MS}")));
    }
    let deadline = (Instant::now() + Duration::from_millis(timeout_ms)).min(ctx.deadline());
    let mut from = cursor_param(params.get("since"))?.unwrap_or_else(|| ms_cursor(hold_ms + 1000));
    let mut steps: Vec<Step> = Vec::new();
    let mut first_scan = true;
    loop {
        let (events, next, _) = scan(ctx, &ch, from.clone(), SCAN_BUDGET_BYTES).await?;
        // A user cutting power after the wait started ends it (plan §5.1 h-2).
        if !first_scan && let Some(u) = events.iter().find(|e| is_user_off(e)) {
            return Err(DomainError::new(
                ErrorCode::InterruptedByUser,
                "the user switched the output off during the wait",
            )
            .with_detail(json!({"ch": ch, "user_write_seq": u.seq, "action": u.payload.get("action")})));
        }
        first_scan = false;
        steps.extend(series(&events, cond.q.metric()));
        if steps.len() > WAIT_MAX_STEPS {
            let drop = steps.len() - WAIT_MAX_STEPS;
            steps.drain(..drop);
        }
        if let Some(n) = next {
            from = Cursor::Seq { epoch: None, seq: n };
        }
        let h = find_hold(&steps, &cond, hold_ms, max_gap_ms);
        if let Some((a, b, held, n)) = h.run {
            let r = ref_range(&ch, a, b);
            return Ok(
                json!({"matched": true, "condition": cond.describe(), "held_ms": held, "samples": n,
                             "last": h.last, "ref": r,
                             "evidence": [{"ref": r, "kind": "psu.meas", "condition": cond.describe(), "hold_ms": hold_ms}]}),
            );
        }
        let now = Instant::now();
        if now + WAIT_POLL >= deadline {
            let detail = json!({"matched": false, "condition": cond.describe(), "best_ms": h.best_ms, "hold_ms": hold_ms,
                                "samples": h.samples, "losses": h.losses, "long_intervals": h.long_intervals, "last": h.last});
            return Err(DomainError::new(
                ErrorCode::Timeout,
                format!(
                    "{} not held for {hold_ms} ms before the timeout (best {} ms)",
                    cond.describe(),
                    h.best_ms
                ),
            )
            .with_detail(detail));
        }
        Delay::new(WAIT_POLL.min(deadline.saturating_duration_since(now))).await;
    }
}
