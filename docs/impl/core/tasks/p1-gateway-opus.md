docs/plan/02-core-principle.md 준수


> **질문 창 금지**(playbook §6): 작업 중 사용자·리드에게 대화형 질문 창(선택지 묻기 등)을 띄우지 않는다. 계약(`docs/contract/`)과 이 지시서로 판단해 진행하고 그 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다.

# P1 M1 — core-opus: 실제 `loomd` gateway (허브 · WS · 디스패처 · 쓰기 파이프라인 v0 · STOP · 마커)

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `cd68bb8` 이후). 근거: `docs/contract/` v1.2(특히 01·02·03·04), `docs/plan/core/plan.md` §0.2·§3.2–3.7·§9.2·§9.5·§10.3·§10.8, F1(기본 모두 자동).
**범위 경계**: 너는 `crates/loom-gateway`(+ 필요하면 `crates/loom-proto` 소폭 추가 — 보고에 명시)만 수정한다. 같은 시기에 리드 쪽 하위 작업자가 **`crates/loom-journal`(저널)과 `crates/loom-worker-host`·`crates/loom-worker-sdk`(worker IPC·감독)** 를 만든다 — 그 crate들은 건드리지 말 것. 루트 `Cargo.toml`도 리드만(필요 의존성 보고). workspace에 이미 있는 것: tokio, axum(ws), futures-util, tungstenite, tracing(+subscriber), clap, serde/json, schemars, sha2, rand, crc32c, tempfile, regex.

## 목표 (M1 걷는 뼈대의 코어 부분)
`loomd`를 replay가 아닌 **실제 모드**로 띄우면: 에이전트(토큰)와 사용자 UI(쿠키)가 같은 WS로 붙고, 등록된 도메인(지금은 `loom-echo`)에 쓰기를 보내면 → 파이프라인(F1 자동) → worker(지금은 인프로세스) → 이벤트가 허브를 지나 **모든 viewer에 먼저 발행된 뒤** 응답이 간다. 사용자는 STOP·입력으로 개입한다.

## 작업 (순서대로, 각 단계 끝에 fmt·clippy·test 통과 + 보고 파일에 진행 기록 — 리드가 중간 커밋)

### G1. 허브 (plan §3.2, C10) — `src/hub/`
- 전용 OS 스레드 시퀀서 1개(tokio 밖): 입력 bounded MPSC(8 MiB/16k건 — 넘치면 producer별 공정 드롭 + `display.gap`; **우선 kind는 예약 용량**으로 실패하지 않게), stream_seq 부여, 링(32 MiB, 채널 ≤ 4 MiB, 오래된 것 제거), 리듀서(Registry의 `Reducer` + 코어 리듀서: latch·env·record_health·worker 상태·진행 중 쓰기), 구독 팬아웃.
- 발행 레코드 JSON은 **한 번만 직렬화**(`Arc<[u8]>`)해 링·구독이 공유.
- 구독: 계약 01 §1.4–1.5 그대로 — 응답 먼저, snapshot(허브 한 턴에서 불변 캡처, 직렬화는 밖, 진행 중 live ≤ 1 MiB/2 s), `after_seq`+`stream_epoch` resume, control 구독 자동(우선 kind = `loom_proto::core_events::is_priority_kind`, control로만 한 번), 적응형 배치, 구독 큐 = 유입률×500 ms(1–4 MiB, 전체 32 MiB) 넘치면 그 구독만 드롭 + gap, lag > 2 s jump_to_live, heartbeat 1 s(`sub_stats`).
- 발행 워터마크: `wait_published(source_ref | stream_seq)` API(publish-before-reply용).
- **replay 모드 코드와 공유**: replay의 hub/conn을 이 허브 위로 옮기거나 공통 부분을 추출해 둘 다 같은 구독 로직을 쓰게 한다(replay 시험 8개는 계속 통과해야 함).
- mapping 로그: 허브가 배치마다 `(stream_epoch, seq 범위, producer 범위)` 레코드를 내보내는 **trait 출구 `MappingSink`**만 두고, 실제 파일 기록은 저널 작업자가 `loom-journal`로 붙인다(지금은 메모리 sink).

### G2. 데몬 수명·연결 (plan §3.4, §4.6 일부, C11)
- `loomd`(인자 없음 또는 `loomd run`): 단일 인스턴스 락(`~/.loom/run/loomd.lock`, std `File::try_lock`), 포트 bind(127.0.0.1, 기본 포트 47123 — 사용 중이면 OS 할당), **`~/.loom/run/daemon.json`**(port, pid, token(256bit 랜덤 hex), version, proto; 권한 0600) 원자적 쓰기. 홈 경로는 `LOOM_HOME` 환경변수로 바꿀 수 있게(시험 격리).
- hello/role: 토큰 = agent, **bootstrap 쿠키 = viewer**. bootstrap 흐름 M1: `loomd`가 시작 시(stdout이 TTY면) `http://localhost:<port>/#bootstrap=<code>`를 출력하고, 별도로 `GET /api/v1/bootstrap/new`(**agent 토큰 필요**, TTY 판정은 CLI 쪽 책임 — CLI가 TTY일 때만 호출)로 일회성 코드(60 s) 발급, 브라우저 `POST /api/v1/bootstrap {code}` → HttpOnly·SameSite=Strict 쿠키(rights operate+approve). 쿠키 없는 WS는 거절. `User-Agent`/`navigator.webdriver` 판정은 M2(지금은 `--rights watch` 쿠키 발급 옵션만).
- 오리진: 01 §1.2(localhost 정본, 127.0.0.1 리디렉트, allowlist, 보안 헤더). 정적 UI는 `--ui-dir <path>`로 디스크 디렉터리 서빙(rust-embed 내장은 M1 후반 — 보고에 남김).
- `core.hello`·`core.time`·`core.status`(데몬·worker·채널·latch·record·viewers)·`core.viewer.presence`·`core.metrics.report`(지표 저장, `core.status`에 요약).

### G3. 디스패처·도메인 등록·인프로세스 worker (C11, C14 일부)
- JSON-RPC 디스패처: 코어 메서드 표 + `Registry`의 `CommandSpec`. 오류 코드 규격(replay와 같은 코드 재사용).
- 도메인 등록: `loomd`의 `domains.rs`에서 cargo feature `echo`(기본 켜짐)로 `loom_echo::Echo` 등록.
- `CallCtx` 실제 구현(origin·client_id·request_id·deadline, `worker(kind)` 핸들, `publish`, `journal()`(지금은 링 질의만 — 저널 질의는 리드 쪽이 붙임), `approvals()`(F1이라 대부분 no-op)).
- **`WorkerHost` trait 경계**: gateway가 worker를 부르는 인터페이스(`call`, `write`, `job`, `input`, `latch`, 이벤트 수신 스트림). 지금은 **인프로세스 구현**(`loom-worker-sdk::Worker`를 같은 프로세스 tokio 태스크에서 돌림, 이벤트는 Emitter → 허브). 리드 쪽 작업자가 같은 trait의 **IPC 구현**(`loom-worker-host`)을 나중에 붙인다 — trait는 `src/worker_host.rs` 하나에 두고 문서화.

### G4. 쓰기 파이프라인 v0 + STOP + 감사 (plan §10.3·§10.8, 계약 04, C14)
- 요청형 파이프라인: `requested` 행 발행 → 전역 차단(latch·record failed) → `classify` 훅(시간 측정) → 정책 v0(**F1: 기본 auto**, `loom.toml`의 `[policy] deny = ["<method or pattern>"]`만 지원 — confirm/approve는 M2) → idempotency 예약(`(client_id, key)`, pending 대기, TTL 24 h, 32 MiB) → 감사 intent → `accepted` → `presend_check` → worker `write` → 결과 행 → **publish-before-reply**(허브 워터마크, 미발행 시 `E_PARTIAL{published:false}`) → 감사 result → 응답 봉투(`others_since_last` — 클라이언트 전역 커서 + by_channel, `meta.viewers`·`visibility`·`env`·`latch`).
- **`payload.final: true`**를 그 요청의 마지막 행에(성공 종단은 `CommandSpec`에 선언 — 필드가 없으면 loom-domain-api에 추가가 필요하니 보고. 임시로 `sent`=UART류, 그 외 `confirmed`).
- 대화형 입력(`<domain>.input`, viewer operate): 연결 태스크에서 바로 worker로(허브·요청 큐 우회), lease 1회 판정, 감사 `input.batch`(줄/1 s/4 KiB).
- STOP: `core.stop`(watch 포함 모든 viewer·agent), `POST /api/v1/stop`, 순서 = 메모리 latch → worker latch(ack 집계 ≤ 200 ms) → `core.latch` 발행 → `~/.loom/run/latch.json` 영속(실패해도 STOP 유효, `persisted:false`). 재기동 시 on/불명 → latched. `core.stop.release` = viewer operate만. `core.safe_state.list/run`(Registry safe_state, 인터록 검사, `finally_blocked`).
- 감사: `~/.loom/audit/audit-YYYY-MM.jsonl` 해시 체인 `h = sha256(prev ‖ canonical_json(record without h))`(serde_json `Value`의 정렬된 맵 직렬화를 정규형으로 — 정렬이 보장되는지 시험으로 고정), R2+ intent는 송신 전 `sync_data`, 그 외 그룹 ≤ 100 ms. 감사 쓰기 실패 = `record.health=failed` → agent·script R1+ 거부(exit 4). 감사 writer 큐 1 MiB/1024(넘치면 ≤ 200 ms block 후 실패).

### G5. 마커·활동
- `core.mark{label, note}`, `test.step_started/ended` 최소 RPC(`test.step.begin/end` — 세션 엔진 없이 마커만, M1), `core.activity`(agent RPC 1 Hz 집계, 읽은 범위), `core.agent_cursor`(에이전트가 others_since_last를 받은 지점).

## 시험 (plan §9.5 불변식 중 M1 해당분 — `tests/`)
T1 snapshot 이음매(1k ev/s 생산 중 구독 200회, 누락·중복 0) · T2 publish-before-reply(응답 전 최종 행이 viewer에 도착, 허브 정지 주입 시 `E_PARTIAL`) · T3 느린 구독자 격리 · T4 우선 이벤트 보존(폭주 중 STOP·쓰기 행) · T5 origin 위조 무시 · T6 agent의 `stop.release`·watch의 입력 거절 · T8 감사 쓰기 불가 → agent R1+ exit 4, user 입력 허용 · T9 latch 영속·손상 시 latched · T10 idempotency 동시 2요청 = 한 번 송신 · T11 감사 체인 변조 탐지 · echo E2E(agent 토큰으로 `echo.send` → viewer가 행 4개 + rx를 응답보다 먼저 받음 → viewer `echo.input` → agent 다음 결과 `others_since_last.count ≥ 1`). 시험은 `LOOM_HOME` 임시 디렉터리, 포트 0. 전체 시험 2분 이내.

## 품질·보고
workspace 전체 fmt·clippy(`-D warnings`)·test. 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-gateway-opus.md` — **G 단계별로 끝날 때마다 갱신**(완료 항목·시험 수·계약과 다른 점·리드에게 필요한 것: 의존성, loom-domain-api 변경, 저널/IPC 쪽에 기대하는 trait). 화면에는 경로와 3줄 요약만. 10분 넘는 실행 금지.
