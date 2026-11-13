//! Transport boundary: the manager's UDS logic over an injected, non-DoIP connector.
//!
//! `FakeLinks` is written the way a future CAN-TP / vendor Ethernet adapter would be: it lives
//! outside the crate and only implements `loom_uds_runner::{Connector, LinkEvents}` over a
//! `FakeTransport` (exact UDS PDUs, no link framing). The same lease/lane/runner/reconnect code as
//! `uds.connect` (DoIP) runs on top of it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, CtxServices, DomainError, EventDraft, JournalQuery, JournalReader,
    ScanResult, WorkerHandle,
};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, WriteState};
use loom_proto::rpc::{Rights, Role};
use loom_uds::manager::{LINK_EVENTS_ENDED, LinkSpec, ReqSpec, UdsManager};
use loom_uds::payload::LinkState;
use loom_uds_runner::{
    ConnectError, Connector, FakeControl, FakeStep, FakeTransport, Link, LinkSignal, TxnStatus,
};
use tokio::sync::mpsc;

// ---- recording services -------------------------------------------------------------------

#[derive(Debug, Default)]
struct Rec {
    events: Mutex<Vec<EventDraft>>,
    t0: std::sync::OnceLock<Instant>,
}

#[derive(Debug)]
struct NoJournal;
impl JournalReader for NoJournal {
    fn scan<'a>(
        &'a self,
        _q: &'a JournalQuery,
        _limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async {
            Ok(ScanResult {
                events: Vec::new(),
                complete: true,
                gaps: Vec::new(),
                resume_from: None,
                scanned_through: 0,
                scanned_bytes: 0,
            })
        })
    }
}

#[derive(Debug)]
struct NoApprovals;
impl ApprovalHandle for NoApprovals {
    fn invalidate(&self, _id: &str, _reason: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

static JOURNAL: NoJournal = NoJournal;
static APPROVALS: NoApprovals = NoApprovals;

impl CtxServices for Rec {
    fn worker(&self, _kind: &str) -> Result<&dyn WorkerHandle, DomainError> {
        Err(DomainError::new(ErrorCode::NotFound, "no worker"))
    }
    fn publish(&self, draft: EventDraft) -> Result<(), DomainError> {
        let mut ev = self.events.lock().expect("lock");
        if ev.len() < 10_000 {
            ev.push(draft);
        }
        Ok(())
    }
    fn now_mono_ns(&self) -> u64 {
        let t0 = *self.t0.get_or_init(Instant::now);
        u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    fn journal(&self) -> &dyn JournalReader {
        &JOURNAL
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &APPROVALS
    }
}

fn ctx(rec: &Arc<Rec>, rid: &str) -> CallCtx {
    CallCtx::new(
        Role::Viewer,
        Some(Rights::Operate),
        "conn-7",
        Some(rid.to_owned()),
        Instant::now() + Duration::from_secs(30),
        rec.clone(),
    )
}

// ---- the injected connector ---------------------------------------------------------------

/// Counts live link resources; dropped with the link (stands in for sockets / reader tasks).
#[derive(Debug)]
struct Res(Arc<AtomicUsize>);
impl Drop for Res {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct LinkRec {
    generation: u64,
    ctl: FakeControl,
    signals: Option<mpsc::Sender<LinkSignal>>,
}

#[derive(Debug, Default)]
struct Plan {
    /// Generation of every `connect` call, in order.
    attempts: Vec<u64>,
    fail_next: u32,
    wrong_generation_next: u32,
    links: Vec<LinkRec>,
}

/// Virtual ECU behind the fake link: UDS PDUs in, timed UDS PDUs out.
fn ecu(pdu: &[u8]) -> Vec<FakeStep> {
    match pdu {
        [0x22, 0xF1, 0x90] => vec![FakeStep::respond(5, b"\x62\xF1\x90LOOMFAKE000000001")],
        [0x22, 0xF1, 0x91] => vec![
            FakeStep::respond(5, &[0x7F, 0x22, 0x78]),
            FakeStep::respond(80, &[0x62, 0xF1, 0x91, 0x01, 0x02]),
        ],
        // Answers 600 ms after send, long after P2 / the 300 ms total timeout: a late response.
        [0x22, 0xF1, 0x92] => vec![FakeStep::respond(600, &[0x62, 0xF1, 0x92, 0xAA])],
        [0x19, 0x02, 0xFF] => vec![FakeStep::respond(5, &[0x59, 0x02, 0xFF, 0x12, 0x34, 0x56, 0x2F])],
        // Writes never answer here; the tests cut the link while they are in flight.
        _ => vec![],
    }
}

#[derive(Debug, Clone, Default)]
struct FakeLinks {
    plan: Arc<Mutex<Plan>>,
    live: Arc<AtomicUsize>,
}

impl FakeLinks {
    fn plan(&self) -> std::sync::MutexGuard<'_, Plan> {
        self.plan.lock().expect("lock")
    }
    fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
    fn link(&self, i: usize) -> (u64, FakeControl) {
        let p = self.plan();
        (p.links[i].generation, p.links[i].ctl.clone())
    }
    fn signal(&self, i: usize, s: LinkSignal) {
        let tx = self.plan().links[i].signals.clone().expect("signal source open");
        tx.try_send(s).expect("signal sent");
    }
    /// End link `i`'s signal source without a `Down` (EOF).
    fn end_signals(&self, i: usize) {
        drop(self.plan().links[i].signals.take());
    }
}

impl Connector for FakeLinks {
    fn endpoint(&self) -> String {
        "FAKE 7E0/7E8".into()
    }

    fn connect(&self, generation: u64) -> BoxFuture<'_, Result<Link, ConnectError>> {
        Box::pin(async move {
            // Allocate first, so a failed attempt has something it must release.
            self.live.fetch_add(1, Ordering::SeqCst);
            let res = Res(self.live.clone());
            let mut p = self.plan();
            p.attempts.push(generation);
            if p.fail_next > 0 {
                p.fail_next -= 1;
                drop(res);
                return Err(ConnectError("fake: no answer".into()));
            }
            let reported = if p.wrong_generation_next > 0 {
                p.wrong_generation_next -= 1;
                generation + 5
            } else {
                generation
            };
            let (transport, ctl) = FakeTransport::new(ecu);
            for _ in 0..reported {
                ctl.bump_generation();
            }
            let (tx, rx) = mpsc::channel(8);
            p.links.push(LinkRec {
                generation: reported,
                ctl,
                signals: Some(tx),
            });
            Ok(Link {
                transport: Box::new(transport),
                events: Box::new(rx),
                resources: Box::new(res),
            })
        })
    }
}

fn spec(name: &str, links: &FakeLinks, auto_reconnect: bool) -> LinkSpec {
    LinkSpec {
        name: name.into(),
        backend: "fake".into(),
        env: Env::Sim,
        auto_reconnect,
        connector: Arc::new(links.clone()),
    }
}

fn req(target: &str, pdu: &[u8], timeout_ms: u64) -> ReqSpec {
    ReqSpec {
        target: target.into(),
        pdu: pdu.to_vec(),
        timeout: Some(Duration::from_millis(timeout_ms)),
        request_id: None,
        recovery: false,
    }
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn resp(o: &loom_uds::manager::Outcome) -> Vec<u8> {
    o.row
        .resp_bytes
        .as_ref()
        .and_then(|b| b.decode().ok())
        .unwrap_or_default()
}

// ---- tests --------------------------------------------------------------------------------

/// Exact DID/raw bytes, `0x78` pending, read timeout, late-response quarantine — over a non-DoIP link.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uds_logic_runs_unchanged_over_an_injected_connector() {
    let rec = Arc::new(Rec::default());
    let m = UdsManager::new();
    let links = FakeLinks::default();
    let data = m
        .connect_link(&ctx(&rec, "c1"), spec("ecu", &links, true))
        .await
        .expect("connect");
    assert_eq!(data["existing"], false);
    assert_eq!(data["state"]["backend"], "fake");
    assert_eq!(data["state"]["addr"], "FAKE 7E0/7E8");
    assert_eq!(data["state"]["generation"], 1);
    assert_eq!(links.plan().attempts, vec![1]);
    let (generation, ctl) = links.link(0);
    assert_eq!(generation, 1);

    // DID read: exact bytes out and in, decoded by the shared codec path.
    let o = m
        .request(&ctx(&rec, "r1"), req("ecu", &[0x22, 0xF1, 0x90], 1000))
        .await
        .expect("read");
    assert_eq!(o.row.write_state, WriteState::Confirmed);
    assert_eq!(resp(&o), b"\x62\xF1\x90LOOMFAKE000000001".to_vec());
    let decoded = o.row.decoded.as_ref().expect("decoded");
    assert_eq!(decoded[0].did, "F190");

    // Raw request (any service PDU): exact bytes, DTC parsing.
    let o = m
        .request(&ctx(&rec, "r2"), req("ecu", &[0x19, 0x02, 0xFF], 1000))
        .await
        .expect("raw");
    assert_eq!(resp(&o), vec![0x59, 0x02, 0xFF, 0x12, 0x34, 0x56, 0x2F]);
    let dtcs = o.row.dtcs.as_ref().expect("dtcs");
    assert_eq!((dtcs[0].dtc.as_str(), dtcs[0].status), ("123456", 0x2F));

    // 0x78 → P2*, then the final response.
    let o = m
        .request(&ctx(&rec, "r3"), req("ecu", &[0x22, 0xF1, 0x91], 1000))
        .await
        .expect("pending");
    assert_eq!(o.row.pending_count, 1);
    assert_eq!(resp(&o), vec![0x62, 0xF1, 0x91, 0x01, 0x02]);
    assert!(matches!(
        o.result.as_ref().map(|r| &r.status),
        Some(TxnStatus::Positive)
    ));

    // Read timeout → failed + timeout; the SID is quarantined until the late answer arrives.
    let err = m
        .request(&ctx(&rec, "r4"), req("ecu", &[0x22, 0xF1, 0x92], 300))
        .await;
    let o = err.expect("outcome");
    assert!(o.row.timeout);
    assert_eq!(o.row.write_state, WriteState::Failed);
    assert!(
        m.presend()
            .get("ecu")
            .expect("presend")
            .quarantined
            .contains(&0x22)
    );
    let o = m
        .request(&ctx(&rec, "r5"), req("ecu", &[0x22, 0xF1, 0x90], 1000))
        .await
        .expect("q");
    assert!(o.row.reason.starts_with("quarantine"), "{}", o.row.reason);
    tokio::time::sleep(Duration::from_millis(900)).await; // past the late answer (600 ms after send)
    let o = m
        .request(&ctx(&rec, "r6"), req("ecu", &[0x22, 0xF1, 0x90], 1000))
        .await
        .expect("after late");
    assert_eq!(
        o.row.write_state,
        WriteState::Confirmed,
        "{:?} {:?}",
        o.row.reason,
        o.result.as_ref().map(|r| &r.status)
    );
    let late: Vec<EventDraft> = rec
        .events
        .lock()
        .expect("lock")
        .iter()
        .filter(|e| e.kind == "uds.rx" && e.payload["late"] == true)
        .cloned()
        .collect();
    assert_eq!(late.len(), 1);
    assert_eq!(late[0].request_id.as_deref(), Some("r4"));
    assert_eq!(
        late[0].payload["bytes"]["data"].as_str().map(str::to_lowercase),
        Some("62f192aa".into())
    );

    // Only UDS PDUs crossed the boundary, byte for byte; the quarantined request never went out.
    assert_eq!(
        ctl.sent(),
        vec![
            vec![0x22, 0xF1, 0x90],
            vec![0x19, 0x02, 0xFF],
            vec![0x22, 0xF1, 0x91],
            vec![0x22, 0xF1, 0x92],
            vec![0x22, 0xF1, 0x90],
        ]
    );
    assert_eq!(links.live(), 1);
}

/// Link loss with a write in flight → `outcome_unknown`; failed and wrong-generation reconnect
/// attempts leak nothing and do not advance the generation; nothing is resent on the new link.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_through_connector_keeps_generation_and_never_resends() {
    let rec = Arc::new(Rec::default());
    let m = Arc::new(UdsManager::new());
    let links = FakeLinks::default();
    m.connect_link(&ctx(&rec, "c1"), spec("ecu", &links, true))
        .await
        .expect("connect");
    let (_, ctl1) = links.link(0);
    {
        let mut p = links.plan();
        p.fail_next = 2;
        p.wrong_generation_next = 1;
    }

    let write = [0x2E, 0xF1, 0xA0, 0x02];
    let m2 = m.clone();
    let c = ctx(&rec, "w1");
    let in_flight = tokio::spawn(async move { m2.request(&c, req("ecu", &write, 2000)).await });
    wait_for("write handed off", || ctl1.sent() == vec![write.to_vec()]).await;
    ctl1.close();
    links.signal(
        0,
        LinkSignal::Down {
            reason: "cut by test".into(),
        },
    );

    let o = in_flight.await.expect("join").expect("outcome");
    assert_eq!(o.row.write_state, WriteState::OutcomeUnknown);
    let target = m.target("ecu").expect("target");
    wait_for("reconnected", || target.generation() == 2).await;

    let st = target.state();
    assert_eq!(st.link.state, LinkState::Up);
    // 2 refused + 1 wrong-generation (dropped by the manager) + 1 accepted, all asking for 2.
    assert_eq!(links.plan().attempts, vec![1, 2, 2, 2, 2]);
    assert_eq!(links.plan().links.len(), 3);
    let (generation, ctl2) = links.link(2);
    assert_eq!(generation, 2);
    // Old link and the rejected link are released; only the current link is alive.
    assert_eq!(links.live(), 1);
    // Reconnect cleared quarantine bookkeeping and did not resend the ambiguous write.
    assert!(m.presend().get("ecu").expect("presend").quarantined.is_empty());
    assert!(ctl2.sent().is_empty());

    let o = m
        .request(&ctx(&rec, "r1"), req("ecu", &[0x22, 0xF1, 0x90], 1000))
        .await
        .expect("read");
    assert_eq!(o.row.write_state, WriteState::Confirmed);
    assert_eq!(o.result.as_ref().map(|r| r.generation), Some(2));
    assert_eq!(ctl2.sent(), vec![vec![0x22, 0xF1, 0x90]]);
    assert_eq!(ctl1.sent(), vec![write.to_vec()]);
}

/// A signal source that ends without `Down` takes the target link-down (and then reconnects
/// when allowed); it never leaves the target online.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn link_events_eof_is_link_loss() {
    let rec = Arc::new(Rec::default());
    let m = UdsManager::new();

    let stay_down = FakeLinks::default();
    m.connect_link(&ctx(&rec, "c1"), spec("a", &stay_down, false))
        .await
        .expect("connect a");
    stay_down.end_signals(0);
    let a = m.target("a").expect("a");
    wait_for("a down", || a.state().link.state == LinkState::Down).await;
    assert_eq!(a.state().link.cause.as_deref(), Some(LINK_EVENTS_ENDED));
    assert_eq!(a.generation(), 1);
    assert_eq!(stay_down.plan().attempts, vec![1]);

    let back = FakeLinks::default();
    m.connect_link(&ctx(&rec, "c2"), spec("b", &back, true))
        .await
        .expect("connect b");
    back.end_signals(0);
    let b = m.target("b").expect("b");
    wait_for("b reconnected", || b.generation() == 2).await;
    assert_eq!(b.state().link.state, LinkState::Up);
    assert_eq!(back.plan().attempts, vec![1, 2]);
    assert_eq!(back.live(), 1);
}

/// A failed or wrong-generation first connect registers nothing and leaks nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_first_connect_leaves_nothing() {
    let rec = Arc::new(Rec::default());
    let m = UdsManager::new();
    let links = FakeLinks::default();

    links.plan().fail_next = 1;
    let e = m
        .connect_link(&ctx(&rec, "c1"), spec("ecu", &links, true))
        .await
        .expect_err("fails");
    assert_eq!(e.code, ErrorCode::NotOpen);
    assert!(e.message.contains("fake: no answer"), "{}", e.message);

    links.plan().wrong_generation_next = 1;
    let e = m
        .connect_link(&ctx(&rec, "c2"), spec("ecu", &links, true))
        .await
        .expect_err("wrong gen");
    assert_eq!(e.code, ErrorCode::NotOpen);
    assert!(e.message.contains("generation 6, expected 1"), "{}", e.message);

    assert!(m.targets().is_empty());
    assert_eq!(links.live(), 0);
    assert_eq!(links.plan().attempts, vec![1, 1]);

    m.connect_link(&ctx(&rec, "c3"), spec("ecu", &links, true))
        .await
        .expect("connect");
    assert_eq!(links.live(), 1);
    let again = m
        .connect_link(&ctx(&rec, "c4"), spec("ecu", &links, true))
        .await
        .expect("existing");
    assert_eq!(again["existing"], true);
    assert_eq!(links.plan().attempts, vec![1, 1, 1]);
}
