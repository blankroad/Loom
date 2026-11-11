#![allow(clippy::unwrap_used)]

use super::*;
use crate::{BoxFuture, MemorySink, SimClock};
use loom_proto::bytes::Bytes;
use loom_proto::event::Origin;
use loom_proto::worker::{Call, Input, InputLeaseEnd, InputLeaseEndCause};
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct TestWorker {
    active: Arc<AtomicUsize>,
    started: Arc<Notify>,
    input_seen: Arc<Notify>,
    latch_seen: Arc<Notify>,
    end_seen: Arc<Notify>,
    ticks: Arc<AtomicUsize>,
    last_input: Arc<Mutex<Option<Input>>>,
    last_end: Arc<Mutex<Option<InputLeaseEnd>>>,
}
impl SharedWorker for TestWorker {
    const KIND: &'static str = "test";
    fn on_call(&self, _cx: WorkerCtx, _call: Call) -> crate::BoxFuture<'static, Result<Value, WorkerError>> {
        let active = self.active.clone();
        let started = self.started.clone();
        Box::pin(async move {
            active.fetch_add(1, Ordering::SeqCst);
            started.notify_one();
            tokio::time::sleep(Duration::from_secs(5)).await;
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(Value::Null)
        })
    }
    fn on_write(
        &self,
        _cx: WorkerCtx,
        _write: loom_proto::worker::Write,
    ) -> BoxFuture<'static, WriteOutcome> {
        Box::pin(async {
            WriteOutcome {
                state: WriteState::Sent,
                detail: Value::Null,
                error: None,
            }
        })
    }
    fn on_job(&self, _cx: WorkerCtx, _job: JobCmd) -> BoxFuture<'static, Result<Value, WorkerError>> {
        Box::pin(async { Ok(Value::Null) })
    }
    fn on_input(&self, _cx: WorkerCtx, input: Input) -> BoxFuture<'static, Result<(), WorkerError>> {
        let last = self.last_input.clone();
        let seen = self.input_seen.clone();
        Box::pin(async move {
            *lock(&last) = Some(input);
            seen.notify_one();
            Ok(())
        })
    }
    fn on_latch(&self, _on: bool) -> BoxFuture<'static, ()> {
        let seen = self.latch_seen.clone();
        Box::pin(async move {
            seen.notify_one();
        })
    }
    fn on_tick(&self, _cx: WorkerCtx) -> BoxFuture<'static, ()> {
        let ticks = self.ticks.clone();
        Box::pin(async move {
            ticks.fetch_add(1, Ordering::SeqCst);
        })
    }
    fn on_input_lease_end(&self, _cx: WorkerCtx, end: InputLeaseEnd) -> BoxFuture<'static, ()> {
        let last = self.last_end.clone();
        let seen = self.end_seen.clone();
        Box::pin(async move {
            *lock(&last) = Some(end);
            seen.notify_one();
        })
    }
}

struct Harness {
    req: mpsc::Sender<Req>,
    pin: mpsc::Sender<Req>,
    latch: mpsc::Sender<bool>,
    out: mpsc::Receiver<WorkerMsg>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn start(worker: Arc<TestWorker>, tick: Option<Duration>) -> Harness {
    let shared = Arc::new(Shared {
        latch: LatchFlag::new(),
        prio: Mutex::new(Prio::default()),
        mirror: Mutex::new(Mirror::default()),
        ctl_wake: Notify::new(),
        data_wake: Notify::new(),
        credit: AtomicI64::new(0),
        credit_wake: Notify::new(),
        beat_ms: AtomicU64::new(0),
        start: Instant::now(),
        on_record: None,
        journal: None,
    });
    let clock = Arc::new(SimClock::new(0));
    let sink = Arc::new(MemorySink::new(100));
    let base = Emitter::new("s", "test", "test", U64(1), 1, "", sink, clock.clone());
    let cx = WorkerCtx::new(
        shared.latch.clone(),
        clock,
        Arc::new(move |ch| base.for_channel(ch)),
    );
    let (req, req_rx) = mpsc::channel(REQUEST_QUEUE);
    let (pin, pin_rx) = mpsc::channel(1);
    let (latch, latch_rx) = mpsc::channel(16);
    let (out_tx, out) = mpsc::channel(CTL_OUT_QUEUE);
    let task = tokio::spawn(serve(
        worker,
        Dispatch {
            cx,
            shared,
            req_rx,
            pin_rx,
            latch_rx,
            out_tx,
            beat_every: Duration::from_millis(5),
            tick,
        },
    ));
    Harness {
        req,
        pin,
        latch,
        out,
        task,
    }
}
fn call(id: u64) -> Req {
    Req {
        id,
        kind: ReqKind::Call(Call {
            conn_id: Some("c1".into()),
            call_id: U64(id),
            method: "wait".into(),
            args: Value::Null,
            origin: Origin::Agent,
            client_id: None,
            actor_id: None,
            request_id: None,
            deadline_mono_ns: U64(0),
            approval_ref: None,
        }),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_and_latch_finish_within_10ms_while_call_waits_five_seconds() {
    let worker = Arc::new(TestWorker::default());
    let mut h = start(worker.clone(), None);
    h.req.send(call(1)).await.unwrap();
    worker.started.notified().await;
    let input = Input {
        conn_id: Some("c1".into()),
        channel: Some("dut".into()),
        input_seq: Some(U64(7)),
        call_id: Some(90),
        lease_id: "l".into(),
        bytes: Bytes(vec![3]),
        priority: true,
    };
    let begin = Instant::now();
    h.pin
        .send(Req {
            id: 90,
            kind: ReqKind::Input(input.clone()),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(10), worker.input_seen.notified())
        .await
        .unwrap();
    assert!(begin.elapsed() < Duration::from_millis(10));
    assert_eq!(*lock(&worker.last_input), Some(input));
    let begin = Instant::now();
    h.latch.send(true).await.unwrap();
    tokio::time::timeout(Duration::from_millis(10), worker.latch_seen.notified())
        .await
        .unwrap();
    assert!(begin.elapsed() < Duration::from_millis(10));
    assert_eq!(
        worker.active.load(Ordering::SeqCst),
        1,
        "긴 호출이 아직 실행 중이어야 한다"
    );
    loop {
        let reply = tokio::time::timeout(Duration::from_secs(6), h.out.recv())
            .await
            .unwrap()
            .unwrap();
        if matches!(reply,WorkerMsg::Reply(r) if r.call_id==U64(1)) {
            break;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn normal_limit_returns_worker_busy_and_keeps_control_slots() {
    let worker = Arc::new(TestWorker::default());
    let mut h = start(worker.clone(), None);
    for id in 1..=NORMAL_SLOTS as u64 {
        h.req.send(call(id)).await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while worker.active.load(Ordering::SeqCst) != NORMAL_SLOTS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    h.req.send(call(1000)).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), h.out.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(result,WorkerMsg::Reply(r) if r.call_id==U64(1000)&&matches!(&r.result,ReplyResult::Err(e) if e.code==ErrorCode::WorkerBusy))
    );
    let begin = Instant::now();
    h.latch.send(true).await.unwrap();
    tokio::time::timeout(Duration::from_millis(10), worker.latch_seen.notified())
        .await
        .unwrap();
    assert!(begin.elapsed() < Duration::from_millis(10));
    assert_eq!(worker.active.load(Ordering::SeqCst), NORMAL_SLOTS);
    let input = Input {
        conn_id: None,
        channel: None,
        input_seq: None,
        call_id: None,
        lease_id: "l".into(),
        bytes: Bytes(vec![3]),
        priority: true,
    };
    h.pin
        .send(Req {
            id: 1001,
            kind: ReqKind::Input(input),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(10), worker.input_seen.notified())
        .await
        .unwrap();
    h.req
        .send(Req {
            id: 0,
            kind: ReqKind::LeaseEnd(InputLeaseEnd {
                lease_id: "l".into(),
                conn_id: None,
                cause: InputLeaseEndCause::Released,
            }),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(10), worker.end_seen.notified())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tick_has_a_reserved_slot_when_all_normal_callbacks_wait() {
    let worker = Arc::new(TestWorker::default());
    let h = start(worker.clone(), Some(Duration::from_millis(10)));
    for id in 1..NORMAL_SLOTS as u64 {
        h.req.send(call(id)).await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while worker.active.load(Ordering::SeqCst) != NORMAL_SLOTS - 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_millis(200), async {
        while worker.ticks.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(worker.active.load(Ordering::SeqCst), NORMAL_SLOTS - 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tick_is_opt_in_and_lease_end_is_delivered_without_reply() {
    let no_tick = Arc::new(TestWorker::default());
    let _disabled = start(no_tick.clone(), None);
    let worker = Arc::new(TestWorker::default());
    let mut h = start(worker.clone(), Some(Duration::from_millis(10)));
    let end = InputLeaseEnd {
        lease_id: "l".into(),
        conn_id: Some("c1".into()),
        cause: InputLeaseEndCause::ConnClosed,
    };
    h.req
        .send(Req {
            id: 0,
            kind: ReqKind::LeaseEnd(end.clone()),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), worker.end_seen.notified())
        .await
        .unwrap();
    assert_eq!(*lock(&worker.last_end), Some(end));
    tokio::time::timeout(Duration::from_millis(200), async {
        while worker.ticks.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(no_tick.ticks.load(Ordering::SeqCst), 0);
    assert!(
        h.out.try_recv().is_err(),
        "tick·lease 종료는 Reply를 만들지 않는다"
    );
}
