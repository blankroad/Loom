//! Test worker for `loom-worker-host` integration tests (kind `test`).
//!
//! * `write{op:{data}}` → priority rows `sent`, `confirmed` (with `request_id`), then the data is
//!   echoed back as a device `test.rx` on the data lane (like the echo domain).
//! * calls: `echo` (returns args), `rx{n, channel?}` (fake device producer: emits n RX records
//!   as fast as possible, returns ok/err counts), `prio{n}`, `latched`, `sleep{ms}` (async),
//!   `hang` (blocks the dispatcher thread), `panic`, `abort`, `flood` (background RX thread).
//! * flags: `--fault panic|abort|hang|flood` (panic/abort 100 ms after start; hang = first call
//!   blocks; flood = first call starts a flood), `--latch-file <path>` (appends `on`/`off` on
//!   every latch change), `--fake-rss <bytes>` (reported in Health).

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, TsSource, WriteState};
use loom_proto::worker::{Call, Input, WorkerError, Write};
use loom_worker_sdk::{
    BoxFuture, EventDraft, JobCmd, RunHooks, Worker, WorkerArgs, WorkerCtx, WriteOutcome, run_worker_with,
};
use serde_json::{Value, json};

struct TestWorker {
    fault: Option<String>,
    latch_file: Option<PathBuf>,
    faulted: bool,
}

fn err(code: ErrorCode, msg: &str) -> WorkerError {
    WorkerError {
        code,
        message: msg.into(),
        detail: Value::Null,
    }
}

impl TestWorker {
    fn first_call_fault(&mut self, cx: &WorkerCtx) {
        if self.faulted {
            return;
        }
        self.faulted = true;
        match self.fault.as_deref() {
            Some("hang") => std::thread::sleep(Duration::from_secs(3600)),
            Some("flood") => start_flood(cx),
            _ => {}
        }
    }
}

fn start_flood(cx: &WorkerCtx) {
    let em = cx.emitter("ch0");
    let latch = cx.latch().clone();
    std::thread::spawn(move || {
        let mut i = 0u64;
        loop {
            let _ = em.rx(
                "test.rx",
                em.clock().ts(TsSource::HostReadComplete),
                json!({"i": i}),
            );
            i += 1;
            if i.is_multiple_of(1000) {
                std::thread::sleep(Duration::from_millis(1));
            }
            if latch.is_on() && i > 1_000_000 {
                break;
            }
        }
    });
}

impl Worker for TestWorker {
    const KIND: &'static str = "test";

    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move {
            self.first_call_fault(cx);
            let args = call.args;
            match call.method.as_str() {
                "echo" => Ok(args),
                "latched" => Ok(json!(cx.latch().is_on())),
                "rx" => {
                    let n = args.get("n").and_then(Value::as_u64).unwrap_or(1);
                    let ch = args.get("channel").and_then(Value::as_str).unwrap_or("ch0");
                    let em = cx.emitter(ch);
                    let delay = args
                        .get("producer_delay_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        .min(5000);
                    // Device production must not block the dispatcher that sends Health.
                    tokio::task::spawn_blocking(move || {
                        std::thread::sleep(Duration::from_millis(delay));
                        let (mut ok, mut bad) = (0u64, 0u64);
                        let (mut first, mut last) = (None, None);
                        for i in 0..n {
                            match em.rx(
                                "test.rx",
                                em.clock().ts(TsSource::HostReadComplete),
                                json!({"i": i}),
                            ) {
                                Ok(seq) => {
                                    ok += 1;
                                    first.get_or_insert(seq.0);
                                    last = Some(seq.0);
                                }
                                Err(_) => bad += 1,
                            }
                        }
                        json!({"ok": ok, "err": bad, "first": first, "last": last})
                    })
                    .await
                    .map_err(|e| err(ErrorCode::Internal, &format!("RX producer failed: {e}")))
                }
                "prio" => {
                    let n = args.get("n").and_then(Value::as_u64).unwrap_or(1);
                    let em = cx.emitter("ch0");
                    for i in 0..n {
                        let _ = em.priority(draft("test.state", json!({"i": i}), None, None));
                    }
                    Ok(json!({"n": n}))
                }
                "sleep" => {
                    let ms = args.get("ms").and_then(Value::as_u64).unwrap_or(10);
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    Ok(json!({"slept_ms": ms}))
                }
                "hang" => {
                    std::thread::sleep(Duration::from_secs(3600));
                    Ok(Value::Null)
                }
                "panic" => panic!("loom-test-worker: requested panic"),
                "abort" => std::process::abort(),
                "flood" => {
                    start_flood(cx);
                    Ok(json!({"flood": true}))
                }
                other => Err(err(ErrorCode::UnknownMethod, &format!("unknown method {other}"))),
            }
        })
    }

    fn on_write<'a>(&'a mut self, cx: &'a WorkerCtx, write: Write) -> BoxFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let em = cx.emitter("ch0");
            let rid = Some(write.request_id.clone());
            let data = write.op.get("data").cloned().unwrap_or(Value::Null);
            if let Some(ms) = write.op.get("delay_ms").and_then(Value::as_u64) {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            if cx.latch().is_on() {
                let _ = em.priority(draft(
                    "test.tx",
                    json!({"write_state": "failed", "data": data}),
                    rid,
                    Some(&write),
                ));
                return WriteOutcome {
                    state: WriteState::Failed,
                    detail: json!({"latched": true}),
                    error: Some(err(ErrorCode::Stopped, "STOP latch is on")),
                };
            }
            let _ = em.priority(draft(
                "test.tx",
                json!({"write_state": "sent", "data": data}),
                rid.clone(),
                Some(&write),
            ));
            let _ = em.priority(draft(
                "test.tx",
                json!({"write_state": "confirmed", "data": data}),
                rid,
                Some(&write),
            ));
            let _ = em.rx(
                "test.rx",
                em.clock().ts(TsSource::HostReadComplete),
                json!({"data": data}),
            );
            WriteOutcome {
                state: WriteState::Confirmed,
                detail: json!({"echo": data}),
                error: None,
            }
        })
    }

    fn on_job<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        job: JobCmd,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move {
            Ok(match job {
                JobCmd::Start(j) => json!({"started": j.job_id}),
                JobCmd::Update(j) => json!({"updated": j.job_id}),
                JobCmd::Renew(j) => json!({"renewed": j.job_id, "lease_ms": j.lease_ms}),
                JobCmd::Stop(j) => json!({"stopped": j.job_id}),
            })
        })
    }

    fn on_input<'a>(&'a mut self, cx: &'a WorkerCtx, input: Input) -> BoxFuture<'a, Result<(), WorkerError>> {
        Box::pin(async move {
            let em = cx.emitter("ch0");
            let _ = em.priority(draft(
                "test.input",
                json!({"len": input.bytes.0.len(), "priority": input.priority}),
                None,
                None,
            ));
            Ok(())
        })
    }

    fn on_latch(&mut self, on: bool) {
        if let Some(p) = &self.latch_file
            && let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p)
        {
            let _ = writeln!(f, "{}", if on { "on" } else { "off" });
        }
    }
}

fn draft(kind: &str, payload: Value, request_id: Option<String>, w: Option<&Write>) -> EventDraft {
    EventDraft {
        kind: kind.into(),
        ts: loom_proto::event::Ts {
            mono_ns: loom_proto::num::U64(0),
            wall_ns: loom_proto::num::U64(0),
            clock_id: "test".into(),
            source: TsSource::Synthetic,
            quality: None,
        },
        dir: Some(Dir::Tx),
        origin: w.map_or(loom_proto::event::Origin::Device, |w| w.origin),
        client_id: w.and_then(|w| w.client_id.clone()),
        request_id,
        job_id: None,
        generation: None,
        purpose: None,
        risk: None,
        env: None,
        payload,
        metrics: Vec::new(),
        quality: Vec::new(),
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        argv.iter()
            .position(|a| a == name)
            .and_then(|i| argv.get(i + 1))
            .cloned()
    };
    let fault = flag("--fault");
    let latch_file = flag("--latch-file").map(PathBuf::from);
    let fake_rss = flag("--fake-rss").and_then(|s| s.parse::<u64>().ok());
    let args = match WorkerArgs::parse(argv.clone()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("loom-test-worker: {e}");
            std::process::exit(2);
        }
    };
    match fault.as_deref() {
        Some("panic") => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                panic!("loom-test-worker: fault panic");
            });
        }
        Some("abort") => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                std::process::abort();
            });
        }
        _ => {}
    }
    let hooks = RunHooks {
        rss: fake_rss.map(|v| Arc::new(move || Some(v)) as loom_worker_sdk::runtime::RssHook),
        ..RunHooks::default()
    };
    run_worker_with(
        TestWorker {
            fault,
            latch_file,
            faulted: false,
        },
        args,
        hooks,
    )
}
