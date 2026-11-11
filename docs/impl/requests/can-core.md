# CAN → CORE 요청 (구현, 2026-11-11, can-lead)

docs/plan/02-core-principle.md 준수. 기준: 계약 v1, `loom-domain-api` @ core-p0-d5, 설계 요구는 `docs/plan/_xteam/can-core.md` W1–W10.

1. **대기 기본 기능 `CallCtx::wait`(또는 `subscribe`)** — `can.expect`·`can.sig.wait-for`(그리고 UART `expect`, UDS 응답 대기)는 "조건 이벤트가 기한 안에 오면 즉시 반환"이어야 한다(지연 목표 trace 가시성 p95 < 50 ms, `wait-for` 반환 < 100 ms). 지금 `CallCtx`에는 `journal().scan`만 있어 M1은 **짧은 간격 저널 폴링**(10 ms, 기한까지)으로 만든다. 제안: `ctx.wait(filter: SubFilter, after: Cursor, deadline: Instant, max_events) -> BoxFuture<Result<WaitResult{events, cursor, timed_out}, DomainError>>` — 허브 구독 기반, 상한 있음, 대기 중 `core.activity{action:"wait", predicate}`는 도메인이 publish. 도입되면 CAN이 폴링을 걷어낸다.
2. **workspace 의존성 추가**(루트 `[workspace.dependencies]`, CORE 소유): `can-dbc = { version = "10", default-features = false, features = ["encodings"] }`, `toml = { version = "1", default-features = false, features = ["parse", "serde", "std"] }`, `sha2 = "0.11"`, `pest = "2"`, dev `proptest = "1"`. 지금은 CAN crate 안 버전 지정으로 쓰고 있다(동작 영향 없음). 받아 주면 `workspace = true`로 바꾼다.
3. **`can.frame` payload의 `dlc` 정의를 계약에 명시**: CAN은 **DLC 코드(0..15)**, 바이트 수는 별도 `len`으로 낸다(`docs/impl/requests/can-ui.md` §2). fixture `event_can_frame.json`(`dlc:8`, 8바이트)은 두 해석 모두 맞아 모호하다 — FD 12바이트(DLC 9) fixture를 하나 추가해 주면 좋다.
4. **데이터 레인용 전체 이벤트 emit**: `Emitter::rx`는 origin=device·dir=rx 고정이라, Loom이 보낸 프레임의 tx 에코(origin agent/user/script, top `request_id`, `payload.frame_index`)를 지금은 `priority`(제어 레인)로 낸다. 고빈도(주기 송신·burst)에는 데이터 레인이 맞다 — `Emitter::data(draft: EventDraft)` 같은 메서드 요청.
5. **`source_gap`에 detail/범위**: sim observer 넘침처럼 원인 쪽 범위(첫/마지막 seq)를 아는 경우 `GapData.producer_range`·`detail`을 채울 수 있게 인자 추가 요청.
6. **응답 뒤 쓰기 진행 행**: gap 있는 `can.send` burst는 worker를 막지 않으려고 응답을 `write_state: accepted`로 먼저 돌려주고, 남은 프레임 에코와 최종 `can.tx{sent}`는 응답 뒤에 낸다. E7(publish-before-reply)은 "응답이 말하는 상태까지의 행"에 적용된다고 해석했다 — 맞는지 확인, 아니면 SDK에 지연 응답(deferred reply) 요청.
7. **`range` 모양 불일치**: 계약 02의 `core.activity`는 `range{seq_a, seq_b}`, `loom-proto`의 `SeqRange`는 `{from, to}`. CAN gateway는 지금 proto 타입(`{from,to}`)을 따른다. 정본을 하나로 정해 계약·UI(can-ui.md §4)에 알려 달라. 같은 맥락에서: 도메인이 `core.activity`를 publish해도 되는지, payload 정본(proto `ActivityPayload`에 `action/domain/channel` 추가 여부).
8. **cursor·봉투**: (a) `JournalQuery.from`이 포함(inclusive)인지 계약에 명시, (b) `@last` 커서 이동 주체(core가 응답 `next_cursor`로 옮기는지 — 지금 CAN `peek`은 그대로 돌려주기만 함), (c) 도메인 결과의 `next_cursor`·`truncated`·`range`를 core가 D5 봉투 `meta`로 올리는지, (d) `ScanResult`에 스캔한 high-water seq(일치 없음 timeout의 증거 창용).
9. **CallCtx 보강(신호 작업에서)**: (a) 허브 단조 시계 또는 `ScanResult.now_mono_ns`(`sig.get`의 `age_ms`를 wall clock 없이), (b) 차단 작업 실행(`spawn_blocking` 같은 것 — DB 로드는 지금 스레드 + 2 ms 폴링), (c) TEST 세션 시작 시 채널 DB sha를 고정할 자리(G2 — 지금 `can.signal_range`는 인자 `db_sha256`과 로드된 DB를 비교만 한다).
10. **`loomd`에 CAN 도메인 연결 (M1, P1 관문 전제)** — `crates/loom-gateway/src/domains.rs`(CORE 소유, core/main 기준)에 echo와 같은 방식으로 feature `can` 추가 요청:
    ```rust
    #[cfg(feature = "can")]
    match register(&loom_can::gateway::Can::default()) {
        Ok(registry) => {
            let w = crate::worker_host::InProcessHost::spawn(
                {   // 실시간 pump 스레드는 첫 can.open에 시작
                    let w = loom_can::worker::CanSimWorker::new();
                    let _ = loom_can::uds_sim::register(&w); // UDS 가상 ECU를 ISO-TP 서버 모델 "uds_sim"으로
                    w
                },
                loom_can::DOMAIN,
                sink.clone(),
                clock.clone(),
            );
            out.push(Loaded { registry, workers: vec![w] });
        }
        Err(e) => tracing::error!("domain can disabled: {e}"),
    }
    ```
    `Cargo.toml`: `loom-can = { workspace = true, optional = true }`(루트 `[workspace.dependencies]`에 `loom-can = { path = "crates/loom-can" }` 추가), `[features] default = ["echo", "can"]`, `can = ["dep:loom-can"]`. worker kind는 `can_sim`(Registry 규칙). CAN 쪽 준비: `Can`은 `Default`, `CanSimWorker::new()`는 실시계·기본 한도. 붙이고 나면 CAN이 `loom can demo`(sim 채널 열기 + 가짜 BCM) 경로와 웹 CAN Trace E2E(Playwright)·§2.0 시험을 맡는다. InProcessHost 대신 별도 프로세스 worker(`loomd worker can_sim`)로 갈 때도 같은 `CanSimWorker`를 쓴다.
11. **`ErrorCode::Conflict`(`E_CONFLICT`) 추가** — `can.periodic.update`·`can.sig.set`의 revision CAS 충돌(IG 개입 경합). 계약에는 있으나 `loom-proto` enum에 없어 지금은 `E_LOCKED`(exit 6) + `detail{code:"E_CONFLICT", current_rev}`로 낸다. 종료 코드는 6 유지 제안. 추가되면 CAN이 매핑을 바꾼다. 같은 맥락: live worker IPC가 `Write{class: Job, op.method: "can.periodic.*"}` 경로를 그대로 전달하는지 통합 때 확인 요청.
12. **시각 cursor**: `mono:<ns>` cursor(또는 시각 → stream seq 찾기) — Graphics 질의(`can.signal.query`의 `t0`)와 커서 값(`can.signal.at`)이 지금은 seq/상대 cursor로 스캔 범위를 잡고 `ts.mono_ns`로 화면 축을 나눈다. 대용량 응답 `raw_ref`(HTTP GET, A5) 경로도 필요.
13. **STOP 중 정지 제어 write 허용**: `can.isotp.cancel`·`can.periodic.stop`은 write(`Request`)로 worker에 가지만 **멈추는 방향의 제어**라 STOP latch 중에도 통과해야 한다(A3: STOP은 새 송신을 막는 것이지 정지를 막는 것이 아님, `safe_state` 성격). CORE 파이프라인이 latch 중 agent/script write를 일괄 거절한다면 이런 메서드를 예외로 둘 표시(예: `CommandSpec.stop_control: true` 또는 `safe_state` 등록)를 요청한다.
