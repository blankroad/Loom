# P1 보고 — runner 보강 (uds-lead 하위 작업자, Claude) · 커밋 안 함
고친 곳: `crates/loom-uds-runner`(abort.rs 신규, handle.rs 재작성, runner.rs·transport.rs·lib.rs, 시험 2개) + **`crates/loom-uds-doip`(T_TCP 엔티티 타이머만: entity.rs·lib.rs·tests/doip.rs)**. `crates/loom-uds` 손대지 않음. 공개 API는 추가만.
**추가 API (uds-opus용)**
- `Priority { Agent(기본, Script 포함), User, Abort }`(Ord: Abort > User > Agent). `TxnRequest::with_priority(Priority)`, `.with_origin(loom_proto::event::Origin)`(기본 Agent), `.with_request_id(impl Into<String>)`. 새 pub 필드 `priority`·`origin`·`request_id` — 구조체 리터럴 대신 `TxnRequest::read/write/new` + builder를 쓸 것.
- `RunnerHandle::abort(AbortScope) -> AbortSummary`(= `AbortKind::User`), `abort_with(AbortScope, AbortKind) -> AbortSummary`, `queue_depth() -> QueueDepth{abort,user,agent,in_flight}`.
- `AbortScope { All, Origin(Origin), Request(String) }`, `AbortKind { User → E_INTERRUPTED_BY_USER, Stop → E_STOPPED }`(`error_code()`), `AbortSummary { cancelled_waiting, in_flight }`.
- `Runner::execute_abortable(req, &AbortSignal)`(핸들 없이 직접 쓸 때), `AbortSignal::{new, abort(kind), kind()}`, `TxnResult::aborted(req, kind, clock, gen)`.
- **새 enum 변형 `TxnStatus::Aborted { kind }`** — `TxnStatus`를 전수 match하면 이 팔을 추가해야 한다(유일한 비추가성 지점).
- `RunnerConfig.read_retry: bool`(기본 false) + `RunnerConfig::with_read_retry(bool)`, `READ_RETRY_BUSY_DELAY`(50 ms). `BoxFuture`는 이제 `loom_domain_api::BoxFuture` 재수출(같은 타입).
- doip: `DoipEntity::set_tcp_general_inactivity(Option<Duration>)`, `DEFAULT_T_TCP_GENERAL_INACTIVITY`(300 s), `EntityStats.inactivity_closes`.
**abort 의미**: 대기 중 일치 요청은 즉시 `failed` + abort 오류, 미송신(`sent=false`). 진행 중 요청은 신호만 보내고 그 결과는 원래 receiver로 온다 — 송신 전(quarantine 정숙 대기 포함)이면 미송신 `failed`, 송신 후면 읽기 `failed`·쓰기 `outcome_unknown`(`may_have_executed=true`) + **U 등록**(늦은 응답은 D19대로 late, 같은 SID는 `E_QUARANTINE`). `Transport::submit`(소켓 넘김) 자체는 끊지 않는다.
**C2 상한**

| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_PENDING_PER_TARGET`(Agent/Script 대기열) | 32 (+진행 중 1) | `E_BUSY`, 미송신 (기존 값·동작 유지) |
| `MAX_PENDING_USER` | 8 (예약) | `E_BUSY`, 미송신 |
| `MAX_PENDING_ABORT` | 16 (예약) | `E_BUSY`, 미송신 |
| 엔티티 `T_TCP_General_Inactivity` | 기본 off, 설정 시 300 s 권장 | 수신 없음 → 연결 종료 + `inactivity_closes` |

**시험**: runner 44 통과(기존 33 + `priority_abort.rs` 10 + doip_e2e 1), doip 29 통과(기존 27 + T_TCP 2). 5회 반복 동일.
- 정지 시계+Fake: Agent 3 대기 중 User → 진행 중 routine 뒤 User가 먼저, abort(All)이 대기열 비움(Abort 우선순위는 남음), 진행 중 쓰기 abort → `outcome_unknown` + 같은 SID `E_QUARANTINE` + 늦은 `6E` late·해제, 진행 중 읽기 abort(Stop) → `failed`/`E_STOPPED`, 송신 전·정숙 대기 중 abort → 미송신, 우선순위별 상한 넘침 → `E_BUSY`, origin·request id 범위, 핸들 drop 시 대기열 마무리 후 정지, 읽기 0x21 재시도 1회·쓰기/기본/timeout 재시도 없음.
- DoIP 루프백: 0x78을 100 ms마다 보내는 routine 350 ms 시점 abort → `outcome_unknown`, 이어서 같은 SID 거부, 비겹침 요청은 늦은 0x78을 late로 본 뒤 성공.
- **abort 지연(실측, macOS arm64 debug)**: `abort()` 반환 15 µs(대기 56건+진행 중, Fake) / 21–55 µs(DoIP), 진행 중 결과 도착 0.12–0.65 ms(DoIP). 시험 기준: 반환 < 10 ms, DoIP 결과 < 100 ms.
- 관문(두 crate 각각): `cargo fmt --check` OK · `cargo clippy --all-targets -D warnings` 경고 0 · `cargo test` 위 수치.
**결정**
1. `AbortScope::All`·`Origin`은 `Priority::Abort` 요청(안전 상태 복구 등 abort 후속 송신)을 건드리지 않는다. 연속 `!`가 복구를 지우지 않게 하려는 것. 특정 id로만 취소.
2. 우선순위별 대기열을 따로 두고 각자 상한(plan §3.8 "사용자 입력·STOP용 용량 예약"). `MAX_PENDING_PER_TARGET`은 이제 Agent 대기열 상한(총 대기 최대 56).
3. 읽기 재시도(plan §3.4 "읽기만 정책상 1회")는 **NRC 0x21에만, 정책으로 켤 때만**. timeout된 읽기는 U에 들어가 같은 SID 재전송이 늦은 응답과 구별되지 않으므로(D19) 재시도하지 않는다. 0x78 수·late 수는 두 시도를 합산, 관찰자에는 `Sent`가 두 번 간다.
4. T_TCP: 타이머는 ISO 13400-2 엔티티 쪽이며 **테스터로부터 받은 메시지**(진단·alive 응답)만 재설정한다. 시험으로 확인: alive 응답만으로는 엔티티가 alive check를 보낼 때만 유지되고, 실제 엔티티는 유휴 소켓에 묻지 않으므로 **진단 없는 유휴 5분이면 연결이 끊긴다**. 클라이언트 keepalive는 넣지 않았다(TP는 도메인 잡 소관, 끊기면 loom-uds가 재연결 → generation 증가로 U 해제). 엔티티 기본은 off(기존 sim 동작 유지), `T_TCP_Initial_Inactivity`(2 s)는 미구현.
5. abort와 응답 도착이 겹치면 abort가 이긴다(`select! biased`). 재시도 대기 중 abort면 이미 끝난 첫 시도(0x21) 결과를 돌려준다.
**질문**: (1) 유휴 5분 연결 끊김을 막을 클라이언트 keepalive(요청 없는 alive check response 송신)를 doip에 넣을지 — ISO 허용 여부 확인 필요. (2) `TxnResult`에 재시도 횟수 필드를 둘지(지금은 `Sent` 이벤트 2회로만 보임). (3) STOP latch 시 진행 중 `Priority::Abort` 요청도 끊어야 하는지(지금은 id 지정 시에만).
**CORE 추가 요청**: 없음(`loom-domain-api` 의존은 허용된 안정 API).
