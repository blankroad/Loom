//! 실제 SharedWorker IPC 시험용 프로세스. 보관 상태와 출력은 유한하다.

use loom_proto::event::WriteState;
use loom_proto::worker::{Call, Input, InputLeaseEnd, WorkerError, Write};
use loom_worker_sdk::{BoxFuture, JobCmd, SharedWorker, WorkerCtx, WriteOutcome, run_shared_worker};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

#[derive(Debug, Default)]
struct State {
    ticks: AtomicU64,
    latched: AtomicBool,
    input: Mutex<Value>,
    end: Mutex<Value>,
}
#[derive(Debug, Default)]
struct TestWorker(Arc<State>);
impl SharedWorker for TestWorker {
    const KIND: &'static str = "test";
    fn on_call(&self, cx: WorkerCtx, call: Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        let state = self.0.clone();
        Box::pin(async move {
            if call.method == "sleep" {
                let _ = cx.emitter("ch0").system("test.shared_started", json!({}));
                tokio::time::sleep(Duration::from_secs(5)).await;
                return Ok(json!({"slept":5}));
            }
            Ok(
                json!({"conn_id":call.conn_id,"ticks":state.ticks.load(Ordering::SeqCst),"latched":state.latched.load(Ordering::SeqCst),
                "input":state.input.lock().unwrap_or_else(PoisonError::into_inner).clone(),"end":state.end.lock().unwrap_or_else(PoisonError::into_inner).clone()}),
            )
        })
    }
    fn on_write(&self, _cx: WorkerCtx, _write: Write) -> BoxFuture<'static, WriteOutcome> {
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
    fn on_input(&self, cx: WorkerCtx, input: Input) -> BoxFuture<'static, Result<(), WorkerError>> {
        let state = self.0.clone();
        Box::pin(async move {
            let data = serde_json::to_value(input).expect("Input 직렬화");
            *state.input.lock().unwrap_or_else(PoisonError::into_inner) = data.clone();
            let _ = cx.emitter("ch0").system("test.shared_input", data);
            Ok(())
        })
    }
    fn on_latch(&self, on: bool) -> BoxFuture<'static, ()> {
        let state = self.0.clone();
        Box::pin(async move {
            state.latched.store(on, Ordering::SeqCst);
        })
    }
    fn on_tick(&self, _cx: WorkerCtx) -> BoxFuture<'static, ()> {
        let state = self.0.clone();
        Box::pin(async move {
            state.ticks.fetch_add(1, Ordering::SeqCst);
        })
    }
    fn on_input_lease_end(&self, cx: WorkerCtx, end: InputLeaseEnd) -> BoxFuture<'static, ()> {
        let state = self.0.clone();
        Box::pin(async move {
            let data = serde_json::to_value(end).expect("InputLeaseEnd 직렬화");
            *state.end.lock().unwrap_or_else(PoisonError::into_inner) = data.clone();
            let _ = cx.emitter("ch0").system("test.shared_end", data);
        })
    }
}

fn main() {
    run_shared_worker(Arc::new(TestWorker::default()));
}
