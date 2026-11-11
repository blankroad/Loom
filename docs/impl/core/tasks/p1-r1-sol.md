docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6). 경로는 항상 `/Users/ctmctm/workspace/Loom/...`. 보고는 저장소 루트 `docs/impl/core/reports/`에.

# P1 R1 — core-sol: proto · worker SDK 보강 (soak 작업 B1·B2를 끝낸 뒤)

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`. **판정 정본: `/Users/ctmctm/workspace/Loom/docs/impl/requests/core-answers-p1.md`**의 **[R1]** 중 아래 배정분.
**범위 경계**: 너 = `crates/loom-proto`, `crates/loom-worker-sdk`, `crates/loom-worker-host`, `schema/v1`(재생성). **core-opus = `crates/loom-gateway`·`crates/loom-domain-api`·`crates/loom-cli`·`crates/loom-testkit`** — 건드리지 말 것(네 변경으로 그쪽 컴파일이 깨지면 고칠 위치를 보고에 정확히 적는다 — opus가 고친다). 다른 팀 crate 수정 금지.

## 1. proto (먼저, 짧게 — 끝나면 보고 파일 맨 위에 `proto 완료: <요약>` 한 줄을 바로 써라. opus가 그 줄을 기다린다)
- `worker`: `Input{conn_id: Option<String>, channel: Option<String>, input_seq: Option<U64>}`, `InputLease{conn_id: Option<String>}`, 새 G→W `InputLeaseEnd{lease_id, conn_id, cause}`(cause enum `conn_closed|released`), `Call{conn_id: Option<String>}`. 모두 `serde(default, skip_serializing_if)`. `WORKER_PROTO_MINOR` 2.
- `error`: `E_LIMIT`(exit 4).
- `core_events`: `ActivityPayload`에 선택 `action, domain, channel`; `SafeStateItem.kind: SafeStateKind{Recover, EmergencyOff}`(기본 Recover); `SafeStateList{items}` 결과 타입.
- 새 모듈 `canonical`: `canonical_json(&Value) -> String`(키 정렬·공백 없음 — serde_json 기본 Map이 BTreeMap임을 시험으로 고정), `sha256_hex(&[u8]) -> String`. (`sha2` 의존 추가 — workspace에 있음.)
- `schema` 재생성, msgpack/golden 시험 보강.

## 2. worker SDK
- **`SharedWorker`** trait(`&self`, 반환 future `Send + 'static`): `on_call/on_write/on_job/on_input/on_latch/on_tick`(on_tick 기본 no-op). **`run_shared_worker(Arc<W>)`**: 콜백을 동시에 띄운다 — `on_input`·`on_latch`는 긴 `on_call`/`on_write` 뒤에 줄 서지 않는다. 동시 실행 상한(예: 64)과 넘칠 때 정책(`E_WORKER_BUSY` 응답)을 명시. 기존 `Worker`·`run_worker`는 그대로(하위 호환).
- `InputLeaseEnd` 디스패치(`on_input_lease_end` 기본 no-op), `Input`의 새 필드 전달.
- **tick**: Welcome config의 `tick_ms`가 있으면 그 주기로 `on_tick`(SharedWorker만).
- **`Emitter`**: `data(EventDraft)`(data lane, dir·origin·purpose·quality·metrics·request_id 지정), `source_gap`에 `producer_range`·`detail` 인자 버전, 표준 gap 발행기(`display_gap/record_gap/source_gap` — domain·channel = 영향받은 스트림), **seq 예약** `reserve_seq()` + `rx_with_seq`/`data_with_seq`(예약 seq가 이미 발행된 seq보다 작으면 오류 반환 — 저널 순서 보호).
- 시험: 긴 `on_call`(5 s 대기) 중 `on_input`·`on_latch`가 10 ms 안에 처리됨, 동시 상한 초과 시 BUSY, tick 주기, InputLeaseEnd 전달, Emitter::data 필드 보존, seq 예약 순서 오류, gap 발행기 domain.

## 품질·보고
`cargo fmt --all --check`, `cargo clippy -p loom-proto -p loom-worker-sdk -p loom-worker-host -p loom-schema --all-targets -- -D warnings`, 그 crate들 test, `cargo run -p loom-schema -- check`. workspace 전체가 opus 진행으로 잠시 깨질 수 있다 — 네 crate 기준으로 보고. 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-r1-sol.md`(맨 위 `proto 완료` 줄, 이후 단계별). 화면에는 경로와 3줄 요약만.
