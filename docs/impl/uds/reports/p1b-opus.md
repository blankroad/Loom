# P1-b 보고 — uds-opus (핸들러 ↔ runner·DoIP·sim·CallCtx, runner 2633aaa 우선순위·abort 반영)

**구조** (`crates/loom-uds`만 수정, 커밋 안 함)
- `manager.rs` `UdsManager` → 대상별 `Target`.
  - 구성: `TargetSession`, runner(`RunnerHandle`, abort용 `ctl_handle` 복제), 우선순위 게이트(`lanes.rs`), 데이터 잠금(이벤트 큐·DoipClient), lease, 늦은 응답 귀속 ring, 백그라운드 발행자(connect한 CallCtx 복제).
  - `connect{sim}`: 데몬 안 `DoipEntity`(127.0.0.1 임의 포트) + `SimNetwork` 어댑터를 띄우고 **실제 `DoipClient`로 접속**한다. 실HW `connect{host,port,la,tester_la,activation}`도 같은 경로다.
- 요청 순서: `uds.tx` requested → 차단 → lease(사용자 R1+ 쓰기가 가져감, 그동안 agent·script R1+는 `E_HELD_BY_USER` 미송신, R0 허용, `uds.lease.release`로 반환) → accepted → 게이트 → runner.
  - 실행 중 발행: `uds.tx` sent, `uds.rx`(0x78·늦은 응답 `late:true` → 같은 SID의 timeout request_id), `uds.pending`.
  - 결과 뒤: 최종 `uds.rx` → `uds.state`(바뀐 경우) → `uds.result` → 그 다음 응답(publish-before-reply).
  - write_did는 되읽기 행 `<rid>.verify`를 거쳐 `effective_verified`/`effect_unverified`를 재발행한다.
- runner 요청 = `TxnRequest::new(..).with_priority().with_origin().with_request_id()`. 사용자 콘솔 `User`, safe_state 복구(`recovery:true`, 뷰어 전용 — `uds.session_default`가 설정) `Abort`, agent·script `Agent`.
- `PriorityGate`(Control > Abort > User > Agent, 레인 안 FIFO, 상한 = runner `MAX_PENDING_*` → `E_BUSY`): `RunnerEvent`에 request 태그가 없어 우리 요청을 runner에 하나씩만 넣는다. 태그가 생기면 제거한다.
- `uds.abort`: 뷰어는 `All`(기본) 또는 `scope:"agent"`, agent·script는 자기 origin만. 대기자는 `E_INTERRUPTED_BY_USER` 미송신, 실행 중인 요청은 `RunnerHandle::abort`(읽기 failed, 쓰기 outcome_unknown + U). Abort 레인은 건드리지 않는다.
- `UdsManager::stop_latch()`: agent·script 대기자와 실행 중 요청을 `abort_with(.., Stop)`로 끊는다(`E_STOPPED`). 사용자·복구는 유지한다.
- 0x50 P2/P2* 변경 시, 그리고 재연결 시(Control 레인, `ClientConfig.generation` +1 → `replace_transport` → `uds.state` reconnected, 재전송 없음) runner를 멈췄다 다시 띄운다(U 보존).
- `MonoClock::new(Instant::now(), clock.mono_ns())`, clock은 `UdsModule::new(Arc<dyn Clock>)`(기본 `SystemClock`).
- `presend_check`: generation 불일치 → EpochChanged, user lease → Locked, timeout된 SID → Quarantine.
- `[uds]` policy 스키마를 등록했다(`auto_reconnect`, `targets[]`, `policy{"0x31:*": ..}`). 승인 경로는 미구현(F1).

**C2 상한**: `MAX_TARGETS` 16(초과 시 `E_BUSY`) · `CAP_RUNNER_EVENTS` 256/대상(버리고 `dropped_events`로 셈, 최종 행은 TxnResult로 만듦) · 게이트 레인 32/8/16/4(`E_BUSY`) · `MAX_TIMED_OUT_TRACKED` 16 · `MAX_WRITE_LOG` 1024 / `MAX_CLIENT_CURSORS` 256(오래된 것부터 버림) · 재연결 20 × 250 ms 후 포기 · 요청 timeout 읽기 2 s / 쓰기 10 s / 최대 30 s.

**시험**: `cargo test -p loom-uds` **17개 통과**, e2e 4회 반복 안정. fmt·clippy `-D warnings` 경고 0.
- e2e 시나리오: sim DoIP 위에서 F190(행 순서·publish-before-reply) → session → 쓰기 + 되읽기 → NRC 0x31 → 0x78×2 → timeout + 늦은 응답 귀속 → 사용자 쓰기 → agent `E_HELD_BY_USER` → release → 재연결 gen 2 → 0x2A 차단.
- 그 밖: presend(lease·generation, ≤ 1 ms), 사용자 요청이 대기 중인 agent 요청을 앞지름(실행 중 요청은 선점 안 함), abort 키(실행 중 + 대기, 반환 < 10 ms), STOP latch(agent `E_STOPPED`, 사용자는 수행), lanes 단위 시험.

**지연** (응답 `t_final_rx_mono_ns` → `uds.result` publish, 루프백 sim, n=1000, macOS arm64): p50 59–79 µs, **p95 87–467 µs**(병렬 시험 부하에 따라 변동), p99 0.15–3.6 ms.
- 1000회에 9–13 s가 걸렸다. 시험 harness의 이벤트 전체 복제(O(n²)) 탓으로 **추정**한다. 양쪽 소켓 모두 TCP_NODELAY.

**CORE 요청 (계약 차이)**
1. `EventDraft` origin override(`uds.rx` = device).
2. 호출과 무관한 domain publisher(링크 이벤트가 지금은 connect 호출자 출처로 찍힘).
3. STOP latch 도메인 훅(`Registry::on_stop` 등). 지금은 gateway가 `stop_latch()`를 불러야 한다.
4. `others_since_last` 정본(지금은 도메인 쓰기 로그 근사, 커서 `w:N`).
5. `PresendError::HeldByUser`(presend 경로는 지금 `E_LOCKED`로 나감).

**runner/doip 요청**: `RunnerEvent` request 태그(게이트 제거용), `RunnerHandle::{set_timing, replace_transport, unresolved 스냅샷}`(지금은 task를 멈췄다 다시 띄움), `MonoClock::from_clock`.
**결정**: ECU로 바이트를 보내는 명령은 모두 `WriteClass::Request`로 둔다. lease는 R1+에만 적용하고, agent는 lease가 비어 있으면 가져가되 다른 agent를 막지 않는다. 늦은 응답은 같은 SID의 최근 timeout 요청에 귀속한다. **질문**: 없음.
