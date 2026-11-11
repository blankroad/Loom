//! Worker piece: owns the (simulated) device. DOMAIN: open your device here, read it on your own
//! threads with `Emitter::rx`, and write in `on_write`.

use base64::Engine as _;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Origin, TsSource, WriteState};
use loom_proto::worker::{Call, Input, InputLeaseEnd, WorkerError, Write};
use loom_worker_sdk::{BoxFuture, EventDraft, JobCmd, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};

use crate::{KIND_DATA, KIND_LEASE_END, KIND_WRITE, WORKER_KIND};

/// Loopback worker.
#[derive(Debug, Default)]
pub struct EchoWorker {
    /// Lines sent so far (stands in for device state).
    pub sent: u64,
}

fn err(code: ErrorCode, message: &str) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail: Value::Null,
    }
}

fn rx_payload(text: &str) -> Value {
    json!({"text": text, "bytes": {"encoding": "base64", "data": base64::engine::general_purpose::STANDARD.encode(text)}})
}

impl Worker for EchoWorker {
    const KIND: &'static str = WORKER_KIND;

    fn on_call<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        call: Call,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move {
            match call.method.as_str() {
                "echo.status" => Ok(json!({"open": true, "sent": self.sent})),
                m => Err(err(ErrorCode::NotImplemented, &format!("{m} not implemented"))),
            }
        })
    }

    fn on_write<'a>(&'a mut self, cx: &'a WorkerCtx, w: Write) -> BoxFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let em = cx.emitter("loop");
            // Agent/script units are refused while STOP is latched (A1-②, A3).
            if cx.latch().is_on() && w.origin != Origin::User {
                return WriteOutcome {
                    state: WriteState::Failed,
                    detail: Value::Null,
                    error: Some(err(ErrorCode::Stopped, "STOP latched")),
                };
            }
            let text =
                w.op.get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
            // Simulated device latency (`delay_ms`, ≤ 2 s) so tests can catch a write in flight.
            // DOMAIN: real drivers block on their own threads, never in this callback.
            if let Some(ms) = w.op.get("delay_ms").and_then(Value::as_u64) {
                std::thread::sleep(std::time::Duration::from_millis(ms.min(2000)));
            }
            // DOMAIN: hand bytes to the driver here; `sent` means "driver accepted", not "on the wire".
            self.sent += 1;
            let row = EventDraft {
                kind: KIND_WRITE.into(),
                ts: cx.clock().ts(TsSource::Driver),
                dir: Some(Dir::Tx),
                origin: w.origin,
                client_id: w.client_id.clone(),
                request_id: Some(w.request_id.clone()),
                job_id: None,
                generation: Some(1),
                purpose: None,
                risk: None,
                env: None,
                payload: json!({"write_state": "sent", "written_bytes": text.len()}),
                metrics: vec![],
                quality: vec![],
            };
            // Emit failures are counted, never block (can-core W7); the seq hole shows the loss.
            let _ = em.priority(row);
            // The loopback "device" answers immediately.
            let _ = em.rx(
                KIND_DATA,
                cx.clock().ts(TsSource::HostReadComplete),
                rx_payload(&text),
            );
            WriteOutcome {
                state: WriteState::Sent,
                detail: json!({"written_bytes": text.len()}),
                error: None,
            }
        })
    }

    fn on_job<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _job: JobCmd,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async { Err(err(ErrorCode::NotImplemented, "echo has no jobs")) })
    }

    fn on_input<'a>(&'a mut self, cx: &'a WorkerCtx, input: Input) -> BoxFuture<'a, Result<(), WorkerError>> {
        Box::pin(async move {
            let text = String::from_utf8_lossy(input.bytes.as_slice()).into_owned();
            // DOMAIN: `priority` (Ctrl-C …) jumps your TX queue; `input_seq` orders keystrokes.
            let mut p = rx_payload(&text);
            p["priority"] = json!(input.priority);
            if let Some(n) = input.input_seq {
                p["input_seq"] = json!(n);
            }
            let _ = cx
                .emitter("loop")
                .rx(KIND_DATA, cx.clock().ts(TsSource::HostReadComplete), p);
            Ok(())
        })
    }

    fn on_input_lease_end<'a>(&'a mut self, cx: &'a WorkerCtx, end: InputLeaseEnd) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // DOMAIN: the viewer typing into this channel went away — stop echoing for it, release
            // anything it held (uart: input owner).
            let p = json!({"lease_id": end.lease_id, "conn_id": end.conn_id, "cause": end.cause});
            let _ = cx
                .emitter("loop")
                .rx(KIND_LEASE_END, cx.clock().ts(TsSource::HostReadComplete), p);
        })
    }
}
