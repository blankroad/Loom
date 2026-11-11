# M2a — 주기 송신을 worker·gateway에 연결 (K9 연결 + IG `sig set`)

두 작업자가 **파일을 나눠** 동시에 한다. 공통 규칙 `docs/impl/can/tasks/p0-common.md`(대화형 질문 금지, 검사는 worktree 루트에서 `cargo fmt --all --check`, `cargo clippy -p loom-can --all-targets -- -D warnings`, `cargo test -p loom-can`, 경로 `/Users/ctmctm/workspace/Loom/...`만, 커밋 안 함). 리드가 준비함: `loom-can-periodic` 의존성, `lib.rs` 상수 `RPC_PERIODIC_{START,UPDATE,STOP,RENEW,TAKEOVER,LIST}`, `RPC_SIG_SET`, `KIND_JOB_STATE`.

## 둘 사이 계약 (이대로 — 바꿔야 하면 보고서에 적고 기본안 유지)
worker 호출(gateway → worker). 시작·갱신·정지·갱신·인수는 **worker write**(A1 잡형: core가 시작/변경/갱신/정지를 게이트), 목록은 call.
- `can.periodic.start{channel, frame: FramePayload, period_ms (1..=60000), offset_ms?, count?, lease_ms?, max_duration_ms? (기본 3600000), owner?}` → `WriteOutcome{state: accepted, detail:{job, revision:0}}`. origin = 요청 origin. 같은 (channel, ext, id) 중복 → `E_LOCKED`, 예약 ISO-TP tx_id → `E_DENIED{reason:"tx_reserved"}`, STOP 중(agent·script·user 모두 새 periodic 거절 — 스케줄러 결정 2) → `E_STOPPED`.
- `can.periodic.update{job, expected_rev, patch: {data: hex} | {mask: hex, bytes: hex}}` → `{job, revision}`, 충돌 → `E_CONFLICT`(detail `current_rev`).
- `can.periodic.stop{job | all_mine:true}`, `can.periodic.renew{job}`, `can.periodic.takeover{job}`(새 소유 = 요청 client/origin).
- `can.periodic.list{channel?}` → `{jobs:[{job, channel, id, ext, fd, period_ms, owner, origin, revision, count_left?, lease_left_ms?, sent, missed, lateness_p99_us, state}]}`.
- 이벤트 `can.job_state{job, channel, id, state: "started"|"patched"|"renewed"|"paused"|"stopped", reason?, revision, owner, origin}`(priority). 주기 프레임 자체는 일반 `can.frame`(tx, origin = 잡 origin, top-level `job_id`, `purpose:"periodic"`) — 데이터 레인 메서드가 생기기 전까지는 worker가 지금 쓰는 방식 그대로.

## can-sol — worker 쪽
**범위:** `crates/loom-can/src/worker.rs`, 새 `crates/loom-can/src/periodic.rs`(worker 안 어댑터), 새 `crates/loom-can/tests/periodic.rs`, 보고서 `docs/impl/can/reports/m2-periodic-wire-worker.md`.
- `loom_can_periodic::Scheduler`를 채널 공용으로(또는 채널별 — 결정·이유 기록) 두고 pump에서 `poll_at`/`on_timer`/`pop_due`/`pop_event` 구동, 프레임은 잡 origin의 sender endpoint로 sim 버스에, `on_dispatch_submitted`·실패 보고. 스케줄러 보고서의 통합 요청(1–7)을 지킨다: **worker 최종 TX latch를 먼저** 세우고 scheduler latch, `is_due_current`로 최종 enqueue 직전 재검사, DispatchId 버전 콜백.
- 위 계약의 write/call 처리, `can.job_state` 발행, `can.status`에 잡 수. ISO-TP 예약 tx_id·raw 송신과의 중재(같은 id에 주기 잡이 있으면 raw 1회 송신은 사용자만 override — plan §10.2 "중재": 사용자 1회 송신은 producer가 아니므로 허용 + `NOTE`용 표시 `payload.override_of: job`).
- 시험(FakeWorker+ManualClock): 10 ms 잡 1 s=100프레임, patch CAS·다음 주기 반영, count 완료, lease 만료·renew, STOP(모든 잡 정지·해제 후 재개 없음, 사용자 1회 송신은 통과), 중복 `E_LOCKED`·takeover, 예약 tx_id, 채널 close 시 잡 정지, list 통계.

## can-opus — gateway·CLI·DBC 쪽
**범위:** `crates/loom-can/src/gateway.rs`, `src/cli.rs`, `src/signals.rs`, 시험 `tests/gateway.rs`·`tests/cli.rs`·`tests/signals.rs`, `skills/loom-can/SKILL.md`, 보고서 `docs/impl/can/reports/m2-periodic-wire-gateway.md`. (`worker.rs`·`periodic.rs`는 can-sol 것)
- RPC 등록: `can.periodic.*`(start/update/stop/renew/takeover = write `Job`(A1 잡형) 클래스가 있으면 그것, 없으면 `Request` + 보고서에 메모; list = R0), classify: sim R1, 실HW는 R3 "periodic" 표시(F1 자동, 등급만 표시). `can.job_state` kind 등록(포인터 `/payload/job`, `/payload/state`).
- **`can.periodic.start`에 DBC 메시지 형태**: `{channel?, msg: "EngineData", signals: {EngRPM: 3000, …}, period_ms?(기본 DBC `GenMsgCycleTime`)}` → `loom-can-dbc` encode(기준 = DBC 초기값, 범위 검사) → 위 계약의 `frame`으로 worker 호출. raw 형태(`frame` 텍스트)도 허용.
- **`can.sig.set{channel?, signal("Msg.Sig"), value, job?}`**(IG): 그 메시지의 실행 중 잡을 찾고(없으면 `E_NOT_FOUND` + hint "periodic start 먼저"), 현재 revision과 **마지막 payload를 기준(`EncodeBase::Payload`)**으로 인코드 → `can.periodic.update{expected_rev}`. 충돌이면 1회 재시도 후 `E_CONFLICT`. 결과에 `revision`, 바뀐 바이트, `risk`.
- CLI: `loom can periodic start <frame>|--msg M [--set Sig=V ...] --period MS [--count N] [--lease S]`, `periodic update|stop [--mine]|renew|takeover|list`, `loom can sig set Msg.Sig=V`. 사람용 출력 한 줄.
- 스킬: "Periodic / IG" 짧은 절(시작하면 job id를 기억, 끝에 `periodic stop --mine`, 사용자가 IG로 바꾸면 `others_since_last`).
- 시험은 가짜 worker로(계약 모양 그대로 응답).

끝나면 각자 보고서를 쓰고 멈춘다.
