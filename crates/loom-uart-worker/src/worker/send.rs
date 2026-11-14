//! Agent writes (`uart.send` / `uart.run`), expect waits and viewer input (plan §3.5, §3.7, §4.3–4.6).

use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_console::arbiter::{AgentAdmit, TickAction};
use loom_console::matcher::MatchHit;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Origin, TsSource, WriteState};
use loom_proto::worker::{WorkerError, Write};
use loom_worker_sdk::{WorkerCtx, WriteOutcome};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::port::{MAX_EXPECT_WAITERS, MAX_TX_WAITERS, Port, TxWait, bytes_obj};
use super::util::{
    arg_str, arg_u64, done_error_code, draft, escape_preview, kind, submit_error, werr, werr_detail,
    write_row,
};
use crate::channel::{Eol, ExpectFrom, PatternKind, PatternSpec};
use crate::tx::{CTRLC_QUEUE_BYTES, TxLane, TxRequest, TxState};

/// Default `uart.send --expect` wait (plan §4.1).
pub const SEND_EXPECT_TIMEOUT_MS: u64 = 10_000;
/// Default `uart.run` / `uart.expect` wait (plan §4.1).
pub const RUN_TIMEOUT_MS: u64 = 30_000;
/// Default `--defer-timeout` (plan §3.7-4).
pub const DEFER_TIMEOUT_MS: u64 = 10_000;
/// Longest single-result wait (A6).
pub const MAX_WAIT_MS: u64 = 3_600_000;
/// Extra time on top of a deadline before a missing internal reply is treated as lost.
pub const INTERNAL_GRACE: Duration = Duration::from_secs(2);
/// Characters of the `uart.write.text` preview (UI contract ≤ 256).
pub const WRITE_TEXT_CHARS: usize = 256;
/// Characters of `sent_escaped` in results.
pub const SENT_ESCAPED_CHARS: usize = 1024;
/// Characters of the agent-activity label (arbiter cuts at 80).
pub const ACTIVITY_LABEL_CHARS: usize = 80;

/// Remaining time until an absolute SDK-clock deadline.
pub(crate) fn remaining(cx: &WorkerCtx, deadline_mono_ns: u64) -> Duration {
    Duration::from_nanos(deadline_mono_ns.saturating_sub(cx.clock().mono_ns()))
}

/// Parse `[{kind, pattern}]` (an empty prompt pattern = the profile prompt, resolved by the core).
pub(crate) fn parse_patterns(v: Option<&Value>) -> Result<Vec<PatternSpec>, WorkerError> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| werr(ErrorCode::Usage, format!("bad patterns: {e}"))),
    }
}

/// One expect wait: register, wait for a hit or the timeout, always deregister.
pub(crate) struct ExpectWait {
    id: u64,
    started_rx: u64,
    rx: Option<oneshot::Receiver<MatchHit>>,
    immediate: Option<MatchHit>,
}

/// Outcome of [`ExpectWait::wait`].
pub(crate) enum ExpectEnd {
    Hit(MatchHit),
    Timeout,
    Closed,
}

impl ExpectWait {
    /// Register before anything is transmitted (send --expect: `ExpectFrom::Now`).
    pub(crate) fn register(
        port: &Port,
        id: u64,
        patterns: &[PatternSpec],
        from: ExpectFrom,
    ) -> Result<Self, WorkerError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut w = port.waiters();
            if w.expect.len() >= MAX_EXPECT_WAITERS {
                return Err(werr(
                    ErrorCode::RateLimit,
                    "too many concurrent expects on this channel",
                ));
            }
            w.expect.insert(id, tx);
        }
        // The waiter is in the map before the matcher can fire (pump thread race).
        let reg = {
            let mut core = port.core();
            let started_rx = core.rx_end();
            core.expect_add(id, patterns, from).map(|hits| (started_rx, hits))
        };
        match reg {
            Ok((started_rx, hits)) => {
                let immediate = hits.into_iter().next();
                if immediate.is_some() {
                    port.waiters().expect.remove(&id);
                    port.core().expect_remove(id);
                }
                Ok(Self {
                    id,
                    started_rx,
                    rx: Some(rx),
                    immediate,
                })
            }
            Err(e) => {
                port.waiters().expect.remove(&id);
                Err(channel_error(&e))
            }
        }
    }

    pub(crate) fn started_rx(&self) -> u64 {
        self.started_rx
    }

    pub(crate) async fn wait(mut self, port: &Port, timeout: Duration) -> ExpectEnd {
        if let Some(h) = self.immediate.take() {
            return ExpectEnd::Hit(h);
        }
        let Some(mut rx) = self.rx.take() else {
            return ExpectEnd::Closed;
        };
        let end = match tokio::time::timeout(timeout, &mut rx).await {
            Ok(Ok(hit)) => ExpectEnd::Hit(hit),
            Ok(Err(_)) => ExpectEnd::Closed,
            // A hit may have landed between the timer and here.
            Err(_) => match rx.try_recv() {
                Ok(hit) => ExpectEnd::Hit(hit),
                Err(_) => ExpectEnd::Timeout,
            },
        };
        self.cancel(port);
        end
    }

    fn cancel(&self, port: &Port) {
        port.waiters().expect.remove(&self.id);
        port.core().expect_remove(self.id);
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }
}

/// ChannelCore error → worker error.
pub(crate) fn channel_error(e: &crate::channel::ChannelError) -> WorkerError {
    werr_detail(e.code(), e.to_string(), json!({"error": format!("{e:?}")}))
}

/// `uart.expect_result` event + the result object (`matched` always present).
fn expect_payload(mut v: Value, matched: bool) -> Value {
    if let Value::Object(m) = &mut v {
        m.entry("matched").or_insert(json!(matched));
    }
    v
}

/// Emit `uart.agent_activity{kind:"expect"}` for a waiting expect.
pub(crate) fn activity_expect(
    port: &Port,
    label: &str,
    timeout_ms: u64,
    request_id: Option<&str>,
    client: &str,
) {
    port.emit(draft(
        kind::AGENT_ACTIVITY,
        port.ts_now(TsSource::HostReadComplete),
        Origin::Agent,
        json!({
            "kind": "expect", "text": escape_preview(label.as_bytes(), ACTIVITY_LABEL_CHARS),
            "elapsed_ms": 0, "timeout_ms": timeout_ms, "request_id": request_id, "client": client,
        }),
    ));
}

/// Emit `idle` when no deferred send is shown either (the arbiter owns that line).
pub(crate) fn activity_idle(port: &Port) {
    if port.arb().pending_agent_sends() == 0 {
        port.emit(draft(
            kind::AGENT_ACTIVITY,
            port.ts_now(TsSource::HostReadComplete),
            Origin::Agent,
            json!({"kind": "idle"}),
        ));
    }
}

/// Finish an expect (shared by `uart.expect`, `send --expect`, `run`): event + data.
pub(crate) fn expect_finish(
    port: &Port,
    end: &ExpectEnd,
    id: u64,
    started_rx: u64,
    request_id: Option<&str>,
    sent_for_run: Option<&[u8]>,
) -> Result<Value, WorkerError> {
    let (data, matched) = match end {
        ExpectEnd::Hit(hit) => {
            let core = port.core();
            let data = match sent_for_run {
                Some(sent) => core.run_output(sent, hit, started_rx),
                None => core.expect_result(hit, started_rx),
            };
            (data, true)
        }
        ExpectEnd::Timeout => (port.core().expect_timeout(id, started_rx), false),
        ExpectEnd::Closed => return Err(werr(ErrorCode::NotOpen, "channel closed while waiting")),
    };
    let payload = expect_payload(data, matched);
    let mut d = draft(
        kind::EXPECT_RESULT,
        port.ts_now(TsSource::HostReadComplete),
        Origin::Agent,
        payload.clone(),
    );
    d.request_id = request_id.map(str::to_owned);
    port.emit(d);
    if matched {
        Ok(payload)
    } else {
        Err(werr_detail(
            ErrorCode::Timeout,
            "no match before the timeout",
            payload,
        ))
    }
}

/// Parsed `uart.send` / `uart.run` operation.
struct SendOp {
    bytes: Vec<u8>,
    secret: bool,
    patterns: Vec<PatternSpec>,
    timeout_ms: u64,
    defer_timeout_ms: u64,
}

fn parse_send(port: &Port, op: &Value, run: bool) -> Result<SendOp, WorkerError> {
    let hex_s = arg_str(op, "data").unwrap_or_default();
    let mut bytes =
        hex::decode(hex_s).map_err(|e| werr(ErrorCode::Usage, format!("data is not hex: {e}")))?;
    let profile = port.core().profile().clone();
    let eol = match arg_str(op, "eol") {
        Some(s) => Eol::parse(s).ok_or_else(|| werr(ErrorCode::Usage, format!("bad eol '{s}'")))?,
        None => profile.eol,
    };
    bytes.extend_from_slice(eol.bytes());
    if bytes.is_empty() {
        return Err(werr(ErrorCode::Usage, "nothing to send"));
    }
    let patterns = if run {
        // `--prompt REGEX` or the profile prompt (empty pattern).
        vec![PatternSpec {
            kind: PatternKind::Prompt,
            pattern: arg_str(op, "prompt").unwrap_or_default().to_owned(),
        }]
    } else {
        parse_patterns(op.get("expect"))?
    };
    let default_wait = if run {
        RUN_TIMEOUT_MS
    } else {
        SEND_EXPECT_TIMEOUT_MS
    };
    Ok(SendOp {
        bytes,
        secret: op.get("secret").and_then(Value::as_bool).unwrap_or(false),
        patterns,
        timeout_ms: arg_u64(op, "timeout_ms").unwrap_or(default_wait).min(MAX_WAIT_MS),
        defer_timeout_ms: arg_u64(op, "defer_timeout_ms")
            .unwrap_or(DEFER_TIMEOUT_MS)
            .min(MAX_WAIT_MS),
    })
}

/// Write-row emitter for one request.
struct Rows<'a> {
    port: &'a Port,
    origin: Origin,
    client: String,
    request_id: String,
}

impl Rows<'_> {
    fn emit(&self, state: &str, extra: Value) {
        let mut d = draft(
            kind::WRITE,
            self.port.ts_now(TsSource::Driver),
            self.origin,
            write_row(state, extra),
        );
        d.dir = Some(Dir::Tx);
        d.client_id = Some(self.client.clone());
        d.request_id = Some(self.request_id.clone());
        self.port.emit(d);
    }

    fn fail(&self, state: WriteState, err: WorkerError, extra: Value) -> WriteOutcome {
        let mut p = json!({"final": true, "error": {"code": err.code.as_str(), "message": err.message}});
        if let (Value::Object(m), Value::Object(e)) = (&mut p, extra) {
            m.extend(e);
        }
        let name = match state {
            WriteState::OutcomeUnknown => "outcome_unknown",
            _ => "failed",
        };
        self.emit(name, p);
        WriteOutcome {
            state,
            detail: json!({"request_id": self.request_id}),
            error: Some(err),
        }
    }
}

/// `uart.send` / `uart.run`.
pub(crate) async fn agent_write(port: Arc<Port>, cx: &WorkerCtx, w: Write, run: bool) -> WriteOutcome {
    let client = w.client_id.clone().unwrap_or_else(|| "agent".into());
    let rows = Rows {
        port: &port,
        origin: w.origin,
        client: client.clone(),
        request_id: w.request_id.clone(),
    };
    let op = match parse_send(&port, &w.op, run) {
        Ok(op) => op,
        Err(e) => {
            rows.emit("requested", json!({}));
            return rows.fail(WriteState::Failed, e, json!({}));
        }
    };
    let preview = if op.secret {
        format!("[secret {} B]", op.bytes.len())
    } else {
        escape_preview(&op.bytes, WRITE_TEXT_CHARS)
    };
    let mut requested = json!({"text": preview, "len": op.bytes.len()});
    if op.secret {
        // No digest crate in this build: length only (see report); never the bytes.
        requested["secret"] = json!({"len": op.bytes.len()});
    } else {
        requested["bytes"] = bytes_obj(&op.bytes);
    }
    rows.emit("requested", requested);

    // STOP latch: agent/script units are refused (A1-②, A3); user writes never come here.
    if cx.latch().is_on() && w.origin != Origin::User {
        return rows.fail(
            WriteState::Failed,
            werr(ErrorCode::Stopped, "STOP latched"),
            json!({}),
        );
    }
    if !port.connected.load(std::sync::atomic::Ordering::Acquire) {
        return rows.fail(
            WriteState::Failed,
            werr(ErrorCode::Disconnected, "port disconnected"),
            json!({}),
        );
    }

    // ① Admission by the InputArbiter (user always wins, plan §3.7).
    let label = if op.secret {
        preview.clone()
    } else {
        escape_preview(&op.bytes, ACTIVITY_LABEL_CHARS)
    };
    let deferred_ms = match admit(&port, &client, &w.request_id, &label, op.defer_timeout_ms).await {
        Ok(ms) => ms,
        Err(e) => return rows.fail(WriteState::Failed, e, json!({})),
    };
    rows.emit("accepted", json!({"deferred_ms": deferred_ms}));

    // ② Expect registered BEFORE the first byte leaves (send --expect / run).
    let expect = if op.patterns.is_empty() {
        None
    } else {
        let id = port.next_tx_id() | (1 << 63);
        match ExpectWait::register(&port, id, &op.patterns, ExpectFrom::Now) {
            Ok(e) => Some(e),
            Err(e) => {
                release_lease(&port, &w.request_id);
                return rows.fail(WriteState::Failed, e, json!({"deferred_ms": deferred_ms}));
            }
        }
    };

    // ③ TX actor, agent lane. STOP / HOLD may have arrived while we waited for admission: the
    // lane cancel on those events is a snapshot, so a send that is not queued yet checks here.
    if let Some((code, why)) = blocked_now(&port, cx, w.origin) {
        if let Some(e) = &expect {
            port.waiters().expect.remove(&e.id());
            port.core().expect_remove(e.id());
        }
        release_lease(&port, &w.request_id);
        return rows.fail(
            WriteState::Failed,
            werr(code, why),
            json!({"deferred_ms": deferred_ms}),
        );
    }
    let tx_id = port.next_tx_id();
    let (done_tx, done_rx) = oneshot::channel();
    {
        let mut ws = port.waiters();
        if ws.tx.len() >= MAX_TX_WAITERS {
            drop(ws);
            release_lease(&port, &w.request_id);
            return rows.fail(
                WriteState::Failed,
                werr(ErrorCode::RateLimit, "TX queue full"),
                json!({}),
            );
        }
        ws.tx.insert(
            tx_id,
            TxWait {
                done: Some(done_tx),
                agent_request: Some(w.request_id.clone()),
                origin: w.origin,
                interrupt: false,
            },
        );
    }
    let budget = remaining(cx, w.deadline_mono_ns.get());
    let req = TxRequest {
        request_id: tx_id,
        lane: TxLane::Agent,
        client_id: client.clone(),
        bytes: op.bytes.clone(),
        secret: op.secret,
        deadline: Instant::now() + budget,
    };
    if let Err(e) = port.tx.submit(req) {
        port.waiters().tx.remove(&tx_id);
        release_lease(&port, &w.request_id);
        return rows.fail(
            WriteState::Failed,
            submit_error(&e),
            json!({"deferred_ms": deferred_ms}),
        );
    }
    // Closes the race with a STOP / HOLD that landed between the check above and the submit.
    if let Some((code, _)) = blocked_now(&port, cx, w.origin) {
        let cause = if code == ErrorCode::Stopped {
            "stopped"
        } else {
            "held_by_user"
        };
        let _ = port.tx.cancel(tx_id, cause);
    }
    let done = match tokio::time::timeout(budget + INTERNAL_GRACE, done_rx).await {
        Ok(Ok(d)) => d,
        _ => {
            return rows.fail(
                WriteState::OutcomeUnknown,
                werr(ErrorCode::NotOpen, "TX result lost (actor stopped)"),
                json!({"deferred_ms": deferred_ms}),
            );
        }
    };
    let mut common = json!({
        "written_bytes": done.written_bytes, "remaining_bytes": done.remaining_bytes,
        "deferred_ms": deferred_ms,
    });
    if let Some(cause) = done.cause {
        common["cause"] = json!(cause);
    }
    match done.state {
        TxState::Sent => {
            let mut p = common.clone();
            p["final"] = json!(true);
            rows.emit("sent", p);
        }
        TxState::Failed | TxState::OutcomeUnknown => {
            let code = done_error_code(&done);
            let state = if done.state == TxState::Failed {
                WriteState::Failed
            } else {
                WriteState::OutcomeUnknown
            };
            if let Some(e) = &expect {
                port.waiters().expect.remove(&e.id());
                port.core().expect_remove(e.id());
            }
            let mut out = rows.fail(state, werr(code, "write did not complete"), common.clone());
            out.detail = result_detail(&w.request_id, &op, &done, deferred_ms, port.info.generation);
            return out;
        }
    }

    // ④ Expect / run output.
    let mut detail = result_detail(&w.request_id, &op, &done, deferred_ms, port.info.generation);
    let Some(exp) = expect else {
        return WriteOutcome {
            state: WriteState::Sent,
            detail,
            error: None,
        };
    };
    let label = op.patterns.first().map(|p| p.pattern.clone()).unwrap_or_default();
    activity_expect(&port, &label, op.timeout_ms, Some(&w.request_id), &client);
    let (id, started) = (exp.id(), exp.started_rx());
    let end = exp.wait(&port, Duration::from_millis(op.timeout_ms)).await;
    activity_idle(&port);
    if let ExpectEnd::Hit(hit) = &end {
        // send --expect / run: the agent has seen up to the match end (plan §4.5).
        let _ = port.core().commit_cursor(&client, hit.end_off);
    }
    let res = expect_finish(
        &port,
        &end,
        id,
        started,
        Some(&w.request_id),
        run.then_some(op.bytes.as_slice()),
    );
    let key = if run { "output" } else { "expect_result" };
    match res {
        Ok(v) => {
            detail[key] = if run {
                v.get("output").cloned().unwrap_or(v)
            } else {
                v
            };
            WriteOutcome {
                state: WriteState::Sent,
                detail,
                error: None,
            }
        }
        Err(e) => {
            detail["expect_result"] = e.detail.clone();
            // The bytes went out (state stays `sent`); the wait failed (exit 3, A6).
            WriteOutcome {
                state: WriteState::Sent,
                detail,
                error: Some(e),
            }
        }
    }
}

fn result_detail(
    rid: &str,
    op: &SendOp,
    done: &crate::tx::TxDone,
    deferred_ms: u64,
    generation: u32,
) -> Value {
    let state = match done.state {
        TxState::Sent => "sent",
        TxState::Failed => "failed",
        TxState::OutcomeUnknown => "outcome_unknown",
    };
    let sent = &op.bytes[..usize::try_from(done.written_bytes)
        .unwrap_or(op.bytes.len())
        .min(op.bytes.len())];
    json!({
        "request_id": rid,
        "state": state,
        "sent_escaped": if op.secret { format!("[secret {} B]", sent.len()) } else { escape_preview(sent, SENT_ESCAPED_CHARS) },
        "written_bytes": done.written_bytes,
        "remaining_bytes": done.remaining_bytes,
        "deferred_ms": deferred_ms,
        "generation": generation,
    })
}

/// Release an admitted lease that will not be used (error before TX).
fn release_lease(port: &Port, request_id: &str) {
    let now = port.now_ms();
    let (acts, evs) = {
        let mut a = port.arb();
        let acts = a.agent_done(request_id, now);
        (acts, a.take_events())
    };
    port.emit_arbiter(evs);
    port.dispatch(acts);
}

/// Ask the arbiter; wait while deferred. Returns `deferred_ms`.
async fn admit(
    port: &Port,
    client: &str,
    request_id: &str,
    label: &str,
    defer_ms: u64,
) -> Result<u64, WorkerError> {
    let now = port.now_ms();
    let (admit, rx, evs) = {
        let mut a = port.arb();
        let admit = a.agent_send(client, request_id, label, defer_ms, now);
        let rx = if matches!(admit, AgentAdmit::Deferred { .. }) {
            // Inserted under the arbiter lock: a tick cannot produce the action before we wait.
            let (tx, rx) = oneshot::channel();
            port.waiters().admit.insert(request_id.to_owned(), tx);
            Some(rx)
        } else {
            None
        };
        (admit, rx, a.take_events())
    };
    port.emit_arbiter(evs);
    match admit {
        AgentAdmit::Now => Ok(0),
        AgentAdmit::Reject { code, detail } => Err(werr_detail(code, "refused by input arbitration", detail)),
        AgentAdmit::Deferred { .. } => {
            let Some(rx) = rx else {
                return Err(werr(ErrorCode::Internal, "deferred without waiter"));
            };
            match tokio::time::timeout(Duration::from_millis(defer_ms) + INTERNAL_GRACE, rx).await {
                Ok(Ok(TickAction::AgentReady { deferred_ms, .. })) => Ok(deferred_ms),
                Ok(Ok(TickAction::AgentRejected { code, detail, .. })) => {
                    Err(werr_detail(code, "refused by input arbitration", detail))
                }
                Ok(Err(_)) => Err(werr(ErrorCode::NotOpen, "channel closed while waiting")),
                Err(_) => {
                    port.waiters().admit.remove(request_id);
                    Err(werr(
                        ErrorCode::UserActive,
                        "user kept the input past the defer timeout",
                    ))
                }
            }
        }
    }
}

/// Viewer bytes (write class ③): arbitrate, then the Ctrl-C or user lane.
pub(crate) fn viewer_input(port: &Port, viewer: &str, input_seq: u64, bytes: &[u8]) {
    let now = port.now_ms();
    let (d, evs) = {
        let mut a = port.arb();
        let d = a.user_input(viewer, input_seq, bytes, now);
        (d, a.take_events())
    };
    port.emit_arbiter(evs);
    submit_user(
        port,
        viewer,
        input_seq,
        &d.send,
        d.interrupt,
        d.preempt_agent.as_deref(),
    );
}

/// Hand user bytes from the arbiter to the TX actor; `preempt` = the agent request the arbiter
/// says the user just took the input from (it is cancelled by id, plan §3.7-1).
pub(crate) fn submit_user(
    port: &Port,
    viewer: &str,
    input_seq: u64,
    send: &[u8],
    interrupt: bool,
    preempt: Option<&str>,
) {
    // The user wins: stop the agent request first (next 1 KiB chunk boundary), whatever the
    // size of the user's bytes.
    if let Some(rid) = preempt {
        cancel_agent_request(port, rid, "interrupted_by_user");
    }
    if send.is_empty() {
        return;
    }
    // Ctrl-C uses its own lane (ahead of everything). A Ctrl-C batch that carries more than
    // the lane's 64 B (held keys before it) goes on the user lane in order; the agent is
    // already cancelled above and the user lane always beats the agent lane.
    let lane = if interrupt && send.len() <= CTRLC_QUEUE_BYTES {
        TxLane::CtrlC
    } else {
        TxLane::User
    };
    let tx_id = port.next_tx_id();
    {
        let mut w = port.waiters();
        if w.tx.len() < MAX_TX_WAITERS {
            w.tx.insert(
                tx_id,
                TxWait {
                    done: None,
                    agent_request: None,
                    origin: Origin::User,
                    interrupt,
                },
            );
        }
    }
    let req = TxRequest {
        request_id: tx_id,
        lane,
        client_id: viewer.to_owned(),
        bytes: send.to_vec(),
        secret: false,
        deadline: Instant::now() + Duration::from_secs(5),
    };
    if let Err(e) = port.tx.submit(req) {
        port.waiters().tx.remove(&tx_id);
        if interrupt {
            port.arb().interrupt_written(1);
        }
        // The arbiter already acked `sent`; correct it explicitly (never a silent drop).
        port.emit(draft(
            kind::INPUT_ACK,
            port.ts_now(TsSource::HostReadComplete),
            Origin::User,
            json!({"input_seq": input_seq.to_string(), "state": "discarded", "reason": "queue_full", "detail": e.to_string()}),
        ));
    }
}

/// Cancel the TX request of agent request `rid` (if it is still queued or running).
pub(crate) fn cancel_agent_request(port: &Port, rid: &str, cause: &str) {
    let tx_id = port
        .waiters()
        .tx
        .iter()
        .find(|(_, w)| w.agent_request.as_deref() == Some(rid))
        .map(|(id, _)| *id);
    if let Some(id) = tx_id {
        let _ = port.tx.cancel(id, cause);
    }
}

/// STOP (agent/script only, A3) or HOLD blocks a new agent unit right now.
fn blocked_now(port: &Port, cx: &WorkerCtx, origin: Origin) -> Option<(ErrorCode, &'static str)> {
    if cx.latch().is_on() && origin != Origin::User {
        Some((ErrorCode::Stopped, "STOP latched"))
    } else if port.arb().hold_on() {
        Some((ErrorCode::HeldByUser, "HOLD is on"))
    } else {
        None
    }
}
