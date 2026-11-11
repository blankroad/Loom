//! 공유 worker의 유한 동시 실행. 일반 56 + 입력 6 + lease 종료 1 + latch 1 = 최대 64 콜백.
//! 초과 요청은 E_WORKER_BUSY, tick은 중복 생략, latch는 최신 상태만 보류한다.

use super::*;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[cfg(test)]
mod tests;

pub(super) const NORMAL_SLOTS: usize = 56;
const INPUT_SLOTS: usize = 6;

fn send(out: &mpsc::Sender<WorkerMsg>, message: WorkerMsg) {
    match out.try_send(message) {
        Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            // 응답을 잃은 채 계속 실행하지 않는다. 호스트가 worker 종료를 관찰하고 복구한다.
            eprintln!("loom-worker-sdk: shared 응답 큐 포화; worker 종료");
            std::process::abort();
        }
    }
}

pub(super) fn dispatcher<W: SharedWorker>(worker: Arc<W>, args: Dispatch) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap_or_else(|e| {
            eprintln!("loom-worker-sdk: shared runtime: {e}");
            std::process::abort()
        });
    runtime.block_on(serve(worker, args));
}

async fn serve<W: SharedWorker>(worker: Arc<W>, args: Dispatch) {
    let Dispatch {
        cx,
        shared,
        mut req_rx,
        mut pin_rx,
        mut latch_rx,
        out_tx,
        beat_every,
        tick,
    } = args;
    // tick 설정 시 일반 슬롯 하나를 tick 전용으로 예약하여 긴 RPC 폭주에도 주기를 유지한다.
    let normal = Arc::new(Semaphore::new(NORMAL_SLOTS - usize::from(tick.is_some())));
    let input = Arc::new(Semaphore::new(INPUT_SLOTS));
    let latch = Arc::new(Semaphore::new(1));
    let lease = Arc::new(Semaphore::new(1));
    let ticking = Arc::new(Semaphore::new(1));
    let mut jobs = JoinSet::new();
    let mut pending_latch = None;
    let mut pending_end = VecDeque::new();
    let mut beat = tokio::time::interval(beat_every);
    beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ticks = tick.map(|period| {
        let mut t = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        t
    });
    loop {
        while let Some(result) = jobs.try_join_next() {
            if let Err(e) = result {
                eprintln!("loom-worker-sdk: shared 콜백 종료: {e}");
                std::process::abort();
            }
        }
        if !pending_end.is_empty()
            && let Ok(permit) = lease.clone().try_acquire_owned()
        {
            let end = pending_end.pop_front().expect("대기 lease 종료");
            let worker = worker.clone();
            let cx = cx.clone();
            jobs.spawn(async move {
                let _permit = permit;
                worker.on_input_lease_end(cx, end).await;
            });
        }
        if pending_latch.is_some()
            && let Ok(permit) = latch.clone().try_acquire_owned()
        {
            let on = pending_latch.take().expect("대기 중 latch");
            let worker = worker.clone();
            jobs.spawn(async move {
                let _permit = permit;
                worker.on_latch(on).await;
            });
        }
        let request = tokio::select! {
            biased;
            Some(on)=latch_rx.recv()=>{pending_latch=Some(on);continue;},
            Some(req)=pin_rx.recv()=>Some(req),
            _=beat.tick()=>{shared.beat_ms.store(shared.now_ms(),Ordering::Release);continue;},
            result=jobs.join_next(),if !jobs.is_empty()=>{
                if let Some(Err(e))=result {eprintln!("loom-worker-sdk: shared 콜백 종료: {e}");std::process::abort();}
                continue;
            },
            _=async {match &mut ticks {Some(t)=>{t.tick().await;},None=>std::future::pending::<()>().await}}=>{
                if let Ok(one)=ticking.clone().try_acquire_owned() {
                    let worker=worker.clone();let cx=cx.clone();
                    jobs.spawn(async move {let _one=one;worker.on_tick(cx).await;});
                }
                continue;
            },
            req=req_rx.recv()=>req,
        };
        let Some(request) = request else {
            break;
        };
        if let ReqKind::LeaseEnd(end) = request.kind {
            if pending_end.len() >= 16 {
                eprintln!("loom-worker-sdk: lease 종료 통지 큐 포화; worker 종료");
                std::process::abort();
            }
            pending_end.push_back(end);
            continue;
        }
        let control = matches!(&request.kind, ReqKind::Input(_));
        let pool = if control { &input } else { &normal };
        let permit = match pool.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                send(
                    &out_tx,
                    reply(
                        request.id,
                        busy(ErrorCode::WorkerBusy, "shared 동시 실행 상한"),
                        None,
                    ),
                );
                continue;
            }
        };
        let worker = worker.clone();
        let cx = cx.clone();
        let shared = shared.clone();
        let out = out_tx.clone();
        jobs.spawn(async move {
            let _permit = permit;
            let before = lock(&shared.prio).pushed;
            let result = match request.kind {
                ReqKind::Call(call) => worker
                    .on_call(cx, call)
                    .await
                    .map(ReplyResult::Ok)
                    .unwrap_or_else(ReplyResult::Err),
                ReqKind::Write(write) => {
                    ReplyResult::Ok(encode_write_outcome(&worker.on_write(cx, write).await))
                }
                ReqKind::Job(job) => worker
                    .on_job(cx, job)
                    .await
                    .map(ReplyResult::Ok)
                    .unwrap_or_else(ReplyResult::Err),
                ReqKind::Input(input) => worker
                    .on_input(cx, input)
                    .await
                    .map(|()| ReplyResult::Ok(Value::Null))
                    .unwrap_or_else(ReplyResult::Err),
                ReqKind::Lease => ReplyResult::Ok(Value::Null),
                ReqKind::LeaseEnd(end) => {
                    worker.on_input_lease_end(cx, end).await;
                    return;
                }
            };
            let covers = {
                let p = lock(&shared.prio);
                if p.pushed != before { p.last.clone() } else { None }
            };
            send(&out, reply(request.id, result, covers));
        });
    }
    jobs.abort_all();
}
