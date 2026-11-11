# P1 R1 — core-opus 보고

작업 지시: `docs/impl/core/tasks/p1-r1-opus.md` · 판정 정본 `docs/impl/requests/core-answers-p1.md` · 워크트리 `.worktrees/core`(기준 29f20c3) · 커밋하지 않음.
수정 crate:
- `loom-gateway`, `loom-domain-api`, `loom-cli`.
- `loom-testkit`: sol의 proto 새 필드에 맞춘 리터럴 2곳만.
- `loom-echo`: 샘플 도메인에 lease 종료·입력 필드 반영 — 다른 도메인이 따라 할 예시.

sol 범위(`loom-proto`·`loom-worker-sdk`·`loom-worker-host`)와 다른 팀 crate는 수정하지 않았다.

## 상태
| 항목 | 상태 |
|---|---|
| 1 도메인 등록 | **완료** — `loomd`: echo·uart·can·uds·eth·psu(기본 전부). `loom`: echo·uart 기본, can·eth·psu opt-in(I1, 아래 결정 5). test는 crate 없음 |
| 2 PerChannel · `worker_for` · `worker_program` · `worker_tick` | **완료** |
| 3 CallCtx 보강 | **완료**: `wait`·`now_mono_ns`·`spawn_blocking`·`config`·`activity`·`_meta` 승격·`@last` 이동. `ScanResult.scanned_through`만 보류(결정 4) |
| 4 대화형 입력 ⓟ | **완료**: `data_hex`, `input_seq`·`conn_id`·`channel`, 0x03 → priority, `Registry::input_events(Owner)`, viewer 종료 → `InputLeaseEnd{conn_closed}`, viewer `Call.conn_id`, PerChannel 라우팅 |
| 5 UI | **완료(ui-assets 제외)**: `#boot=` 딥링크, bootstrap 204/400/410, `ui.focus_request` 2 s, `core.channels` 리듀서·snapshot·`core.channels.list`, `safe_state.list` = `{items:[{kind,…}]}` + emergency OFF 실행, replay C-4·C-7·C-8·C-9·C-10. **`loom_ui_assets` crate가 core/main에 없어 Router merge 미연결** |
| 6 `E_LIMIT` | sol이 추가 → PerChannel 상한 초과에 사용 |

## 1. 도메인 등록
- `loom-gateway` feature = 도메인 이름(`echo`, `uart`(= `loom-uart` + `loom-uart-worker`), `can`, `uds`, `eth`, `psu`). 기본에 모두 넣었다(전부 순수 Rust·sim 가능, eth live 캡처는 꺼져 있음). 루트 workspace에 도메인 crate 항목이 없어 member Cargo.toml에서 path 의존으로 걸었다.
- `domains.rs`:
  - `load(module, [(kind, 인프로세스 생성, 별도 실행 파일)])` 표와 `worker_main` 표(`echo`, `uart`, `can_sim`, `eth`, `psu`).
  - 생성 함수가 없는 `WorkerSpec`은 경고만 하고 넘어간다.
  - uds = gateway 모듈만(worker·cli 없음).
- **test: `loom-test` crate가 core/main에 없다**(test/main에만). 병합되면 `Registry::for_core("test")`로 같은 표에 넣는다.
- `loom` CLI: 결정 5 참고.

## 2. PerChannel (`src/pool.rs`)
- kind별 `WorkerPool`:
  - `Shared`: 시작할 때 1개(id = kind).
  - `PerChannel{max}`: 채널을 처음 쓸 때 기동해 Hello까지 ≤ 5 s 기다린다. id = **`<kind>.<channel>`**.
  - 채널이 없는 호출(열거)은 실행 중인 아무 인스턴스로, 없으면 `<kind>._enum`으로 간다.
  - 상한 초과는 `E_LIMIT{limit}`.
- 라우팅 채널 = `params.channel` → `ch` → **`alias`**(uart `open --as dut`가 `alias`로 보냄 — open이 만드는 채널의 별칭).
  - `CallCtx::worker(kind)`는 호출의 채널로, `CallCtx::worker_for(kind, channel)`은 명시한 채널로 간다.
  - 새 인스턴스는 STOP이 걸려 있으면 바로 latch된다.
- 상태·STOP·종료·list·restart는 모든 인스턴스 대상이다(`acked`·`core.worker.restart`는 instance id 기준).
- `Registry::worker_program(kind, name)`: `loomd` 옆에서 찾고, 없으면 경고 후 `loomd worker <kind>`. `Registry::worker_tick(kind, 10 ms–60 s)`는 `Welcome.config.tick_ms`로 넘긴다(SDK가 읽는 쪽은 sol).

## 3. CallCtx (domain-api 비파괴 추가 + gateway 구현)
- `wait(WaitSpec{filter, after, deadline, max_events}) -> WaitResult{events, cursor, timed_out, scanned_through}`: 허브 publish마다 갱신되는 high-water watch로 깨어나 링을 읽는다(폴링 없음). **반환 지연: 이벤트 발행 후 약 6 ms**(시험, debug).
- `now_mono_ns()`(허브 시계), `spawn_blocking(f)`, `config()`(병합된 `[<domain>]`, 프로젝트 우선·키 단위), `activity(action, range?, predicate?)` → `core.activity{action, domain, channel, method, range, waiting}`.
- 결과 `_meta{next_cursor, truncated, range}` → 봉투 `meta`로 옮기고, `@last`를 `next_cursor`(없으면 `range.to`)로 옮긴다.
- 새 `CtxServices` 메서드는 **모두 기본 구현**이 있다(can·eth·psu 시험 fake가 직접 구현하므로).
- `Registry`에 추가한 것: `worker_program`·`worker_tick`·`input_events(Owner)` + 조회 함수, `Owner{Core, Domain}`, `MIN/MAX_WORKER_TICK`. `ScanResult`에 `Default`.

## 4. 대화형 입력
- `<domain>.input`:
  - `hex`/`data_hex`/`text`를 받는다.
  - `input_seq`·`conn_id`(`c<n>`)·`channel`을 worker `Input`에 싣는다.
  - bytes에 0x03이 있으면 `priority=true`.
  - lease는 (연결, 인스턴스)별로 둔다.
- `Registry::input_events(Owner::Domain)`이면 core는 `<domain>.input` 이벤트를 내지 않는다(감사 `input.batch`는 계속).
- viewer WS가 끊기면 그 연결의 lease마다 해당 인스턴스에 `InputLeaseEnd{lease_id, conn_id, cause: conn_closed}`를 보낸다(IPC = `IpcHost::input_lease_end`, 인프로세스 = `on_input_lease_end`).
- viewer 연결의 `Call`에 `conn_id`를 싣는다.

## 5. UI
- 딥링크: `loomd`(TTY)·`GET /api/v1/bootstrap/new`(→ `loom ui`) 모두 `http://localhost:<port>/#boot=<code>[&next=<route>]`.
- `POST /api/v1/bootstrap`: **204 + Set-Cookie**, 본문 오류 400, 모르는·쓴·만료 코드 410(정보 없음). 쿠키 없는 브라우저 WS = 401(기존).
- `core.ui.focus`(→ `ui.focus_request{panel, props, at, reason, actor}` + 최상위 `actor_id`): 클라이언트당 2 s 1회, 넘으면 `E_RATE_LIMIT{retry_after_ms}`.
- **`core.channels`**:
  - 허브 코어 리듀서가 도메인의 `<domain>.state`/`<domain>.*_state`(채널 있음)로 `{domain, channel, state, generation, seq, kind}`를 갱신한다. `state`가 `closed`/`removed`면 뺀다.
  - snapshot에 `kind:"core.channels"` 항목으로 들어간다.
  - RPC `core.channels.list`.
- `core.safe_state.list` = `SafeStateList{items}`: recover + **emergency_off**(`kind`).
  - `core.safe_state.run`은 emergency OFF id도 실행한다: spec의 `method`를 고정 인자 `{"targets": targets}`로 부르고, 확인 없이, 인터록은 경고만 한다.
  - emergency OFF만 요청하면 watch viewer도 실행할 수 있다(계약 04 §4.4).
- replay:
  - C-4: gap 이벤트 domain·channel = 영향받은 스트림(`can`/`ch0`).
  - C-5: watch STOP은 이미 허용.
  - C-7: `POST /api/v1/stop`.
  - C-8: `{items}`.
  - C-9: `snapshot:false` + `after_seq` 없음 = snapshot 생략.
  - C-10: `backlog` 선택, 기본 1000건 — README에 문서화.
- **Q21 "연결 후 만료 → `core.closed{auth_expired}`"**: M1 쿠키 세션에 만료가 없어 미구현.

## 시험
- **새 gateway `tests/r1.rs` 4개**(IPC):
  - 입력: `data_hex:"03"` → worker rx `priority:true`, `input_seq:"7"`. 타이핑하던 viewer를 닫으면 → `echo.lease_end{cause:conn_closed, conn_id}`.
  - `wait`: 즉시 반환(약 6 ms 지연), 기한 넘으면 `timed_out` + `scanned_through`.
  - `core.channels`·PerChannel:
    - psu 채널 2개 → 프로세스 2개(`psu.pa`, `psu.pb`).
    - 늦게 붙은 viewer의 snapshot과 `core.channels.list`에 두 채널이 있다.
    - `psu.pa`를 kill -9 → `psu.pb` 측정이 바로 되고, `psu.pa`는 새 pid로 ready.
  - 도메인 RPC smoke: eth sim 캡처 → `eth.pkt`가 허브에 도착, `can.status`(sim worker), `uds.status` = `E_NOT_IMPLEMENTED`(등록됨), `core.status`에 6개 도메인.
- **CLI smoke** `loom-cli::domains_smoke_via_cli`: `loom uart open sim://bootshell --as dut` → `send` → `read`가 `uart.dut` IPC worker를 거쳐 돈다.
- 수동 확인(같은 바이너리): psu connect·meas, eth capture·stats, uart open·send·read·이벤트 모두 IPC worker로 동작.
- 기존 시험은 가정만 고쳤다:
  - 도메인이 늘어 `workers[0]`·`acked`가 echo 하나라는 가정을 바꿈.
  - bootstrap 200 → 204.
  - C-9에 맞춰 `snapshot:false`는 snapshot을 기다리지 않음.
  - hub 시험은 `snapshot:true`를 명시.
  - CLI help 시험 순서.
  - echo 새 kind에 포인터를 추가(`RegistryProbe` 경고).
- **품질**:
  - `cargo fmt --all --check` 통과.
  - `cargo clippy --workspace --all-targets --exclude loom-uart-worker --exclude loom-psu --exclude loom-eth -- -D warnings` 통과. `loom-psu`·`loom-eth`·`loom-uart-worker`는 `--lib --bins` clippy만 통과(시험 코드는 아래 R-4 때문에 컴파일 불가).
  - `cargo test --workspace --exclude loom-uart-worker --exclude loom-psu --exclude loom-eth --no-fail-fast`: **614 통과, 1 실패**(R-11, sol 범위). `loom-xtask` 경계 검사 포함 통과. 364 s(부하 평균 최대 241의 극심한 부하에서 측정).

## 결정 · 다른 팀에 알릴 것
1. **instance id = `<kind>.<channel>`**(채널 별칭 그대로가 아님). instance id는 producer id·저널 디렉터리·소켓 이름이라, 도메인 사이에 같은 채널 이름(`bench` 등)이 겹치면 충돌한다. 채널 이름 = `[A-Za-z0-9_-]{1,32}`.
2. 라우팅 키에 `alias`를 넣었다(uart open). 도메인은 가능하면 `ctx.worker_for(kind, ch)`로 명시하라(기본 채널을 도메인이 정하는 경우 gateway는 그 채널을 모른다).
3. **C-9는 replay와 실 gateway에 함께 적용**된다(허브 공유). **`loom-bench`(sol)의 viewer는 `snapshot:false` 뒤 `snapshot_end`를 기다리므로 멈출 수 있다** → sol: `snapshot:true`로 바꾸거나 기다리지 않게.
4. **`ScanResult.scanned_through` 보류**: can·eth·psu 시험이 `ScanResult`를 필드 전체 리터럴로 만들어 필드를 추가하면 깨진다. `Default`를 붙였으니 각 팀이 `..ScanResult::default()`로 바꾼 뒤 추가한다. 그동안은 `WaitResult.scanned_through`.
5. **`loom` 기본 feature = `echo`·`uart`**. can·eth·psu의 CLI 조각은 같은 crate에 worker 코드(`loom-worker-sdk` → tokio·`loom-journal`, can은 pest/stacker→`windows-sys`)가 있어 **`loom-xtask` 경계 검사(I1)가 거절**한다. 그래서 opt-in feature로만 둔다. 각 팀은 echo처럼 `gateway/worker/cli` feature를 나누고 CLI가 `default-features = false, features = ["cli"]`로 링크할 수 있게 해 달라. uart는 이미 나뉘어 있어 통과.
6. **can**: worker에 `can.open` 호출이 있지만 gateway에 RPC가 등록돼 있지 않아 에이전트·CLI가 채널을 열 수 없다(sim 송신 `E_NOT_OPEN`). CAN 팀 확인.
7. **uds**: 명령이 `not_yet`(E_NOT_IMPLEMENTED)이고 CLI 조각이 없다.
8. **uart**: help 예시는 `--alias`, 실제 CLI 플래그는 `--as`(파라미터 `alias`).
9. **R-4 (sol 보고와 같음)**: proto 새 필드(`Call.conn_id` 등) 때문에 `loom-psu/tests/support/mod.rs`·`loom-eth/src/worker/tests.rs`(+ uart-worker 기존 건)가 컴파일되지 않는다. 각 팀이 `conn_id: None` 등을 넣어야 한다.
10. **`loom-test`·`loom-ui-assets` crate가 core/main에 없다** → test 도메인 등록·UI 자산 Router merge는 병합 뒤.
11. **R-11**: `loom-worker-host/tests/journal.rs::credit_starved_flood_is_fully_journaled_and_orphan_seals`가 2회 연속 실패한다(59행 `call("rx")`가 Err). sol이 SDK를 고치는 중이었고 부하 평균이 241이었다. 제 변경 범위 밖.
12. 샘플 echo: `echo.lease_end` 이벤트(`on_input_lease_end` 예시), rx에 `priority`·`input_seq`.
13. `_meta` 승격은 아직 `_meta`를 내는 도메인이 없어 통합 시험이 없다(코드 경로만).
