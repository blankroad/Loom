docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6). 경로는 항상 `/Users/ctmctm/workspace/Loom/...`.

# P1 R1 — core-opus: 도메인 합류를 위한 gateway · domain-api 보강

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `29f20c3`). **판정 정본: `/Users/ctmctm/workspace/Loom/docs/impl/requests/core-answers-p1.md`** — 그 표의 **[R1]** 항목 중 아래 배정분을 구현한다. 원 요청은 같은 폴더의 `uart-core.md`, `can-core.md`, `eth-core.md`, `explore-core.md`, `test-core.md`, `ui-core.md`.
**범위 경계**: 너 = `crates/loom-gateway`, `crates/loom-domain-api`, `crates/loom-cli`(도메인 feature 등록), `crates/loom-testkit`(필요한 보조). **core-sol = `crates/loom-proto`·`crates/loom-worker-sdk`·`crates/loom-worker-host`** — 건드리지 말 것. sol이 먼저 proto에 `Input{conn_id?, channel?, input_seq?}`·`InputLease{conn_id?}`·`InputLeaseEnd`·`Call{conn_id?}`·`ErrorCode::E_LIMIT`·`ActivityPayload{action?, domain?, channel?}`·`SafeStateItem.kind`·`canonical` 모듈을 넣는다. **proto에 기대는 항목(아래 ⓟ)은 sol 보고 `p1-r1-sol.md`의 "proto 완료" 줄을 확인한 뒤** 한다. 그 전에는 ⓟ 없는 항목부터. 다른 팀 crate는 수정 금지(그들 crate가 깨지면 보고).

## 순서 (ⓟ = sol proto 필요)
1. **도메인 등록**: `loomd`·`loom`에 feature `uart`, `can`, `uds`, `eth`, `psu`, `test`(각 도메인 crate의 gateway/worker/cli 조각 — 이름·구성은 그 crate의 Cargo.toml·lib.rs를 읽고 맞춘다. 조각이 없거나 API가 맞지 않으면 그 도메인은 등록하지 않고 보고). 기본 feature에 넣을지 판단해 보고(빌드 시간·sim 가능 여부). `Registry::for_core("test")` 허용.
2. **PerChannel 인스턴스** + `CallCtx::worker_for(kind, channel)`(판정표 그대로), `Registry::worker_program(kind, name)`, `Registry::worker_tick(kind, period)`(gateway가 spec을 worker에 전달 — tick 호출은 sol의 SDK).
3. **`CallCtx` 보강**(domain-api API + gateway 구현): `wait(WaitSpec) -> WaitResult`(허브 구독 기반, 상한), `now_mono_ns()`, `spawn_blocking`, `config()`(병합된 `[<domain>]` 섹션), `activity(action, range?, predicate?)`(→ `core.activity` ⓟ). 결과 `_meta` 승격·`@last` 이동·`ScanResult.scanned_through`.
4. **대화형 입력 ⓟ**: `data_hex` 별칭, `input_seq`·`conn_id`·`channel` 전달, 0x03 자동 priority, `Registry::input_events(Owner::Domain)`, viewer 끊김 시 `InputLeaseEnd`, viewer 연결 Call에 `conn_id`, PerChannel 라우팅.
5. **UI**: `loom_ui_assets::router()` merge(ui/main crate — 없으면 보고), bootstrap 딥링크 `#boot=<code>&next=`(loomd 출력·`loom ui`), `POST /api/v1/bootstrap` → 204, 쿠키 없는 WS = 401, `ui.focus_request` 클라이언트당 2 s 1회, **`core.channels` 리듀서 + `core.channels.list`**, `core.safe_state.list` 결과 `{items:[{id, kind, …}]}` ⓟ, replay 정합(C-4·C-5·C-7·C-9·C-10).
6. `ErrorCode::E_LIMIT` 사용처 없음 — sol이 추가만. `WriteState::Confirmed` 문서는 리드가.

## 시험
- 각 도메인이 등록된 `loomd`로 그 도메인의 sim 명령 하나 이상이 CLI(`loom <domain> …`) → worker(IPC) → 이벤트 → 응답까지 도는 smoke(도메인 crate의 sim이 준비된 것만; 안 되는 도메인은 이유를 보고).
- PerChannel: 채널 2개 open → worker 2개, 하나 kill → 다른 채널 영향 없음.
- wait: 조건 이벤트가 기한 안에 오면 즉시 반환(반환 지연 측정), 기한 넘으면 `timed_out` + `scanned_through`.
- 입력 ⓟ: viewer 끊김 → worker가 `InputLeaseEnd` 받음, `data_hex`·0x03 priority.
- channels 리듀서: 늦게 붙은 viewer snapshot에 열린 채널 목록.

## 품질·보고
workspace 전체 fmt·clippy(`-D warnings`)·test(`--exclude loom-uart-worker`는 uart 팀이 `call_id` 리터럴을 고치기 전까지 허용, eth 1건 실패도 같은 사유). 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-r1-opus.md` — **단계별로 갱신**(리드가 중간 커밋). 화면에는 경로와 3줄 요약만. 10분 넘는 실행 금지.
