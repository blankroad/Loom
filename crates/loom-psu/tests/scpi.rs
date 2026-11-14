//! `scpi-tcp` backend against an in-test std TCP SCPI server (not the Python fixture, not
//! SimPsu): real sockets, real IO thread, real (system) clock. Checks the CONTRACT.md rules:
//! queries-only connect, host-receive timestamps without `simulated`, read-back verification,
//! device errors, timeout → outcome_unknown + lost + no retry, STOP blocks agent not user and
//! never switches the output, manual reconnect = new generation, refusals, bounded join.

mod support;

use std::io::{BufRead, BufReader, Write as _};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Call, Write};
use loom_psu::gateway::Psu;
use loom_psu::worker::PsuWorker;
use loom_testkit::RegistryProbe;
use loom_worker_sdk::{Clock, Emitter, LatchFlag, MemorySink, SystemClock, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};
use support::{Services, block_on, call};

// --- fake SCPI server -------------------------------------------------------------------------

#[derive(Default)]
struct Dev {
    v: f64,
    i: f64,
    out: bool,
    errs: Vec<String>,
}

#[derive(Clone, Default)]
struct Server {
    dev: Arc<Mutex<Dev>>,
    log: Arc<Mutex<Vec<String>>>,
    /// Do not answer queries whose header starts with this.
    mute: Arc<Mutex<Option<String>>>,
    accepts: Arc<AtomicUsize>,
    conn: Arc<Mutex<Option<TcpStream>>>,
    stop: Arc<AtomicBool>,
    meas_n: Arc<AtomicU32>,
}

impl Server {
    fn start() -> (Server, String) {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = l.local_addr().expect("addr").to_string();
        let s = Server::default();
        s.dev.lock().expect("lock").i = 1.0;
        let srv = s.clone();
        std::thread::spawn(move || {
            for c in l.incoming() {
                if srv.stop.load(Ordering::Acquire) {
                    break;
                }
                let Ok(c) = c else { continue };
                srv.accepts.fetch_add(1, Ordering::AcqRel);
                *srv.conn.lock().expect("lock") = c.try_clone().ok();
                let s2 = srv.clone();
                std::thread::spawn(move || s2.serve(c));
            }
        });
        (s, addr)
    }

    fn serve(&self, c: TcpStream) {
        let mut w = c.try_clone().expect("clone");
        for line in BufReader::new(c).lines() {
            let Ok(line) = line else { break };
            self.log.lock().expect("lock").push(line.clone());
            let Some(reply) = self.answer(&line) else { continue };
            let muted = self
                .mute
                .lock()
                .expect("lock")
                .as_deref()
                .is_some_and(|m| line.starts_with(m));
            if muted {
                continue;
            }
            if w.write_all(format!("{reply}\n").as_bytes()).is_err() {
                break;
            }
        }
    }

    fn answer(&self, line: &str) -> Option<String> {
        let mut d = self.dev.lock().expect("lock");
        let (h, a) = line.split_once(' ').map_or((line, None), |(h, a)| (h, Some(a)));
        match (h, a) {
            ("*IDN?", _) => Some("TEST,FAKE-PSU,0,1".into()),
            ("VOLT?", _) => Some(format!("{:.4}", d.v)),
            ("CURR?", _) => Some(format!("{:.4}", d.i)),
            ("OUTP?", _) => Some(if d.out { "1" } else { "0" }.into()),
            ("MEAS:VOLT?", _) => {
                let n = self.meas_n.fetch_add(1, Ordering::AcqRel);
                Some(format!(
                    "{:.4}",
                    if d.out {
                        d.v - 0.0001 * f64::from(n % 3)
                    } else {
                        0.0
                    }
                ))
            }
            ("MEAS:CURR?", _) => Some(format!("{:.5}", if d.out { (d.v / 10.0).min(d.i) } else { 0.0 })),
            ("SYST:ERR?", _) => Some(if d.errs.is_empty() {
                "0,\"No error\"".into()
            } else {
                d.errs.remove(0)
            }),
            ("VOLT", Some(x)) => {
                match x.parse::<f64>() {
                    Ok(v) if v <= 10.0 => d.v = v,
                    _ => d.errs.push("-222,\"Data out of range\"".into()),
                }
                None
            }
            ("CURR", Some(x)) => {
                d.i = x.parse().unwrap_or(d.i);
                None
            }
            ("OUTP", Some(x)) => {
                d.out = x == "ON";
                None
            }
            _ => {
                d.errs.push("-113,\"Undefined header\"".into());
                None
            }
        }
    }

    fn lines(&self) -> Vec<String> {
        self.log.lock().expect("lock").clone()
    }

    fn writes(&self) -> Vec<String> {
        self.lines().into_iter().filter(|l| !l.ends_with('?')).collect()
    }

    fn drop_client(&self) {
        if let Some(c) = self.conn.lock().expect("lock").take() {
            let _ = c.shutdown(Shutdown::Both);
        }
    }
}

// --- worker driver ----------------------------------------------------------------------------

struct W {
    worker: PsuWorker,
    ctx: WorkerCtx,
    clock: Arc<SystemClock>,
    sink: Arc<MemorySink>,
    recs: Vec<Value>,
    n: u64,
}

impl W {
    fn new() -> W {
        let clock = Arc::new(SystemClock::new());
        let sink = Arc::new(MemorySink::new(100_000));
        let base = Emitter::new(
            "t",
            "psu",
            "psu-t",
            U64(1),
            1,
            "bench",
            sink.clone(),
            clock.clone(),
        );
        let factory: Arc<dyn Fn(&str) -> Emitter + Send + Sync> =
            Arc::new(move |ch: &str| base.for_channel(ch));
        let ctx = WorkerCtx::new(LatchFlag::new(), clock.clone(), factory);
        W {
            worker: PsuWorker::default(),
            ctx,
            clock,
            sink,
            recs: Vec::new(),
            n: 0,
        }
    }

    fn drain(&mut self) -> &[Value] {
        for (_, r) in self.sink.drain() {
            self.recs.push(serde_json::to_value(&r).expect("json"));
        }
        &self.recs
    }

    fn call(&mut self, method: &str, args: Value) -> Result<Value, loom_proto::worker::WorkerError> {
        self.n += 1;
        let c = Call {
            conn_id: None,
            call_id: U64(self.n),
            method: method.into(),
            args,
            origin: Origin::Agent,
            client_id: Some("test#1".into()),
            actor_id: None,
            request_id: None,
            deadline_mono_ns: U64(self.clock.mono_ns() + 10_000_000_000),
            approval_ref: None,
        };
        block_on(self.worker.on_call(&self.ctx, c))
    }

    fn write_dl(&mut self, rid: &str, op: Value, origin: Origin, dl: Duration) -> WriteOutcome {
        self.n += 1;
        let w = Write {
            call_id: U64(self.n),
            request_id: rid.into(),
            write_class: WriteClass::Request,
            op,
            origin,
            client_id: Some("test#1".into()),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: U64(self.clock.mono_ns() + dl.as_nanos() as u64),
        };
        block_on(self.worker.on_write(&self.ctx, w))
    }

    fn write(&mut self, rid: &str, op: Value, origin: Origin) -> WriteOutcome {
        self.write_dl(rid, op, origin, Duration::from_secs(10))
    }

    fn connect(&mut self, addr: &str) -> Value {
        self.call(
            "psu.connect",
            json!({"ch": "bench", "backend": "scpi-tcp", "address": addr}),
        )
        .expect("connect")
    }

    fn kinds(&mut self, kind: &str) -> Vec<Value> {
        self.drain()
            .iter()
            .filter(|r| r["kind"] == kind)
            .cloned()
            .collect()
    }

    fn rows(&mut self, rid: &str) -> Vec<String> {
        self.kinds("psu.write")
            .iter()
            .filter(|r| r["request_id"] == rid)
            .map(|r| r["payload"]["write_state"].as_str().unwrap_or("").to_owned())
            .collect()
    }
}

fn wait_until(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn samples(w: &mut W) -> u64 {
    w.call("psu.status", json!({})).expect("status")["samples"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

// --- tests ------------------------------------------------------------------------------------

#[test]
fn connect_only_queries_and_polls_real_replies() {
    let (srv, addr) = Server::start();
    {
        let mut d = srv.dev.lock().expect("lock");
        d.v = 5.0;
        d.out = true;
    }
    let mut w = W::new();
    let st = w.connect(&addr);
    assert_eq!(st["backend"], "scpi-tcp");
    assert_eq!(st["link"], "up");
    assert_eq!(st["idn"], "TEST,FAKE-PSU,0,1");
    assert_eq!(st["set_v"], 5.0);
    assert_eq!(st["out"], true, "output read, not switched");
    assert_eq!(st["prot"], "unknown");
    assert_eq!(st["mode"], "unknown");
    assert!(
        srv.writes().is_empty(),
        "connect must only query: {:?}",
        srv.lines()
    );
    wait_until("3 samples", || srv.meas_n.load(Ordering::Acquire) >= 3);
    std::thread::sleep(Duration::from_millis(50));
    let meas = w.kinds("psu.meas");
    assert!(meas.len() >= 3, "{meas:?}");
    for m in &meas {
        assert_eq!(m["ts"]["source"], "host_read_complete");
        assert!(
            m.get("quality")
                .is_none_or(|q| q.as_array().is_some_and(Vec::is_empty)),
            "{m}"
        );
        assert_eq!(m["origin"], "device");
        assert_eq!(m["payload"]["backend"], "scpi-tcp");
        assert!(
            m["payload"]["poll_n"].is_string(),
            "u64 counters are decimal strings"
        );
        let v = m["metrics"][0]["value"].as_f64().expect("v");
        assert!((4.9997..=5.0).contains(&v), "{v}");
    }
    assert!(srv.writes().is_empty(), "polling must only query");
    let me = w.call("psu.meas", json!({})).expect("meas");
    assert_eq!(me["backend"], "scpi-tcp");
    assert!(me["since_ms"].as_u64().is_some_and(|s| s <= 1000));
    // Rate is the real one (~5 Hz), measured from receive timestamps.
    std::thread::sleep(Duration::from_millis(1200));
    let hz = w.call("psu.status", json!({})).expect("status")["poll_hz_actual"]
        .as_f64()
        .expect("hz");
    assert!((3.5..=6.0).contains(&hz), "{hz}");
    let r = w
        .call("psu.disconnect", json!({"ch": "bench"}))
        .expect("disconnect");
    assert_eq!(r["io_thread_finished"], true);
    assert!(
        srv.writes().is_empty(),
        "disconnect sends nothing: {:?}",
        srv.writes()
    );
    assert!(srv.dev.lock().expect("lock").out, "output held after disconnect");
}

#[test]
fn set_and_out_are_verified_by_readback() {
    let (srv, addr) = Server::start();
    let mut w = W::new();
    w.connect(&addr);
    let o = w.write(
        "r-set",
        json!({"action": "set", "volt": 3.3, "curr": 0.5}),
        Origin::Agent,
    );
    assert_eq!(o.state, WriteState::EffectiveVerified, "{o:?}");
    assert_eq!(o.detail["readback"]["set_v"], 3.3);
    let o = w.write("r-on", json!({"action": "out", "state": "on"}), Origin::Agent);
    assert_eq!(o.state, WriteState::EffectiveVerified);
    assert_eq!(srv.writes(), vec!["VOLT 3.3", "CURR 0.5", "OUTP ON"]);
    assert_eq!(w.rows("r-set"), vec!["sent", "effective_verified"]);
    assert_eq!(w.rows("r-on"), vec!["sent", "effective_verified"]);
    let d = srv.dev.lock().expect("lock");
    assert!(d.out && (d.v - 3.3).abs() < 1e-9 && (d.i - 0.5).abs() < 1e-9);
}

#[test]
fn device_error_and_limits_are_failed() {
    let (srv, addr) = Server::start();
    let mut w = W::new();
    w.connect(&addr);
    // Within the 16 V profile limit but over the fake device's 10 V: the device refuses.
    let o = w.write("r-dev", json!({"action": "set", "volt": 12.0}), Origin::Agent);
    assert_eq!(o.state, WriteState::Failed);
    assert!(
        o.error.as_ref().is_some_and(|e| e.message.contains("-222")),
        "{o:?}"
    );
    assert_eq!(w.rows("r-dev"), vec!["sent", "failed"]);
    // Over the profile limit: refused before anything is sent.
    let before = srv.writes().len();
    let o = w.write("r-lim", json!({"action": "set", "volt": 20.0}), Origin::Agent);
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::Denied));
    assert_eq!(srv.writes().len(), before);
}

#[test]
fn unanswered_readback_is_outcome_unknown_link_lost_no_retry() {
    let (srv, addr) = Server::start();
    let mut w = W::new();
    w.connect(&addr);
    *srv.mute.lock().expect("lock") = Some("SYST:ERR?".into());
    let o = w.write("r-x", json!({"action": "set", "volt": 2.0}), Origin::Agent);
    assert_eq!(o.state, WriteState::OutcomeUnknown, "{o:?}");
    assert_eq!(w.rows("r-x"), vec!["sent", "outcome_unknown"]);
    assert_eq!(srv.writes(), vec!["VOLT 2"], "sent exactly once, never retried");
    let st = w.call("psu.status", json!({})).expect("status");
    assert_eq!(st["link"], "lost");
    assert_eq!(st["connected"], false);
    let gaps = w.kinds("source.gap");
    assert!(
        gaps.iter().any(|g| g["payload"]["cause"] == "scpi_no_response"),
        "{gaps:?}"
    );
    // No automatic reconnect; further writes are refused without sending.
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(srv.accepts.load(Ordering::Acquire), 1);
    let o = w.write("r-y", json!({"action": "out", "state": "on"}), Origin::Agent);
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(srv.writes(), vec!["VOLT 2"]);
    assert!(
        w.call("psu.meas", json!({})).is_err(),
        "no fabricated sample while lost"
    );
}

#[test]
fn stop_blocks_agent_not_user_and_never_switches_the_output() {
    let (srv, addr) = Server::start();
    let mut w = W::new();
    w.connect(&addr);
    assert_eq!(
        w.write("r-on", json!({"action": "out", "state": "on"}), Origin::Agent)
            .state,
        WriteState::EffectiveVerified
    );
    w.ctx.latch().set(true);
    w.worker.on_latch(true);
    let n0 = samples(&mut w);
    std::thread::sleep(Duration::from_millis(700));
    assert!(samples(&mut w) > n0, "observation continues under STOP");
    assert!(
        srv.dev.lock().expect("lock").out,
        "STOP does not switch the output (HOLD)"
    );
    let o = w.write("r-agent", json!({"action": "out", "state": "off"}), Origin::Agent);
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::Stopped));
    assert_eq!(w.rows("r-agent"), vec!["failed"]);
    assert_eq!(
        srv.writes(),
        vec!["OUTP ON"],
        "nothing sent for the latched agent write"
    );
    let o = w.write("r-user", json!({"action": "out", "state": "off"}), Origin::User);
    assert_eq!(
        o.state,
        WriteState::EffectiveVerified,
        "user OFF is never latched"
    );
    assert_eq!(srv.writes(), vec!["OUTP ON", "OUTP OFF"]);
}

#[test]
fn far_end_drop_then_manual_reconnect_is_a_new_generation_and_holds() {
    let (srv, addr) = Server::start();
    let mut w = W::new();
    let g1 = w.connect(&addr)["generation"].as_u64().expect("gen");
    w.write("r-on", json!({"action": "out", "state": "on"}), Origin::Agent);
    srv.drop_client();
    let t = Instant::now();
    while !w.worker_link_lost() {
        assert!(t.elapsed() < Duration::from_secs(5), "timed out waiting for lost");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(srv.accepts.load(Ordering::Acquire), 1);
    let gaps = w.kinds("source.gap");
    assert!(
        gaps.iter().any(|g| g["payload"]["cause"] == "scpi_link_lost"),
        "{gaps:?}"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(srv.accepts.load(Ordering::Acquire), 1, "no automatic reconnect");
    let writes = srv.writes();
    let st = w.connect(&addr);
    assert_eq!(st["generation"].as_u64(), Some(g1 + 1));
    assert_eq!(st["out"], true, "output held across the reconnect");
    assert_eq!(srv.writes(), writes, "reconnect only queries");
    assert_eq!(srv.accepts.load(Ordering::Acquire), 2);
}

trait LinkLost {
    fn worker_link_lost(&mut self) -> bool;
}
impl LinkLost for W {
    fn worker_link_lost(&mut self) -> bool {
        self.call("psu.status", json!({}))
            .is_ok_and(|s| s["link"] == "lost")
    }
}

#[test]
fn unsupported_operations_are_refused_not_simulated() {
    let (srv, addr) = Server::start();
    let mut w = W::new();
    w.connect(&addr);
    let o = w.write("r-cyc", json!({"action": "cycle", "off_ms": 500}), Origin::Agent);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::NotImplemented));
    assert_eq!(w.rows("r-cyc"), vec!["failed"]);
    let e = w
        .call("psu.sim.inject", json!({"no_response": 1}))
        .expect_err("inject");
    assert_eq!(e.code, ErrorCode::NotImplemented);
    let e = w
        .call("psu.connect", json!({"ch": "bench"}))
        .expect_err("sim while external");
    assert_eq!(e.code, ErrorCode::PortBusy);
    assert!(srv.writes().is_empty());
}

#[test]
fn write_deadline_bounds_the_transaction() {
    let (srv, addr) = Server::start();
    let mut w = W::new();
    w.connect(&addr);
    let t = Instant::now();
    *srv.mute.lock().expect("lock") = Some("VOLT?".into());
    let o = w.write_dl(
        "r-dl",
        json!({"action": "set", "volt": 1.0}),
        Origin::Agent,
        Duration::from_millis(300),
    );
    assert!(t.elapsed() < Duration::from_millis(900), "{:?}", t.elapsed());
    assert_ne!(o.state, WriteState::EffectiveVerified);
    let o = w.write_dl(
        "r-dl0",
        json!({"action": "set", "volt": 1.0}),
        Origin::Agent,
        Duration::ZERO,
    );
    assert_eq!(o.state, WriteState::Failed);
}

#[test]
fn connect_refusals() {
    let mut w = W::new();
    for (args, code) in [
        (
            json!({"backend": "scpi-tcp", "address": "10.1.2.3:5025"}),
            ErrorCode::Usage,
        ),
        (
            json!({"backend": "scpi-tcp", "address": "localhost:5025"}),
            ErrorCode::Usage,
        ),
        (json!({"backend": "scpi-tcp"}), ErrorCode::Usage),
        (
            json!({"backend": "scpi-tcp", "address": "127.0.0.1:5025", "seed": 1}),
            ErrorCode::Usage,
        ),
    ] {
        assert_eq!(
            w.call("psu.connect", args.clone()).expect_err("refused").code,
            code,
            "{args}"
        );
    }
    // Nothing listening: a truthful connect error, no device.
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let dead = l.local_addr().expect("addr").to_string();
    drop(l);
    let e = w
        .call("psu.connect", json!({"backend": "scpi-tcp", "address": dead}))
        .expect_err("refused");
    assert_eq!(e.code, ErrorCode::Disconnected);
    assert_eq!(
        w.call("psu.status", json!({})).expect_err("none").code,
        ErrorCode::NotOpen
    );

    // Gateway: visa still refused, non-loopback refused before the worker.
    let reg = RegistryProbe::assert_ok(&Psu::default()).0;
    let svc = Services::new();
    let e = call(
        &reg,
        &svc,
        "psu.connect",
        json!({"ch": "bench", "backend": "visa"}),
    )
    .expect_err("visa");
    assert_eq!(e.code, ErrorCode::NotImplemented);
    let e = call(
        &reg,
        &svc,
        "psu.connect",
        json!({"ch": "bench", "backend": "scpi-tcp", "address": "192.168.0.9:5025"}),
    )
    .expect_err("lan");
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(reg.commands().iter().any(|c| c.method == "psu.disconnect"));
}
