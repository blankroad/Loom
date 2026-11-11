# can-opus M2a(앞당김) — DBC 신호를 gateway에 연결 (K6·G2·K7 일부)

먼저 `docs/impl/can/tasks/p0-common.md`(공통 규칙, **대화형 질문 창 금지** 포함). 검사는 worktree 루트에서 `cargo fmt --all --check`, `cargo clippy -p loom-can --all-targets -- -D warnings`, `cargo test -p loom-can`. 경로는 `/Users/ctmctm/workspace/Loom/...`만.

**범위:** 새 파일 `crates/loom-can/src/signals.rs`(+ `tests/signals.rs`), 그리고 `gateway.rs`·`cli.rs`(네가 만든 파일)에 연결. `lib.rs`·`schema.rs`·`Cargo.toml`·`worker.rs`는 리드 소유 — 필요한 변경(예: `loom-can-dbc` 의존성 추가, `pub mod signals;`, RPC 이름 상수)은 **리드가 먼저 해 둔다**(아래). 보고서: `docs/impl/can/reports/m2-signals.md`.

## 리드가 미리 해 둔 것
- `Cargo.toml`에 `loom-can-dbc = { path = "../loom-can-dbc" }`, `lib.rs`에 `pub mod signals;`와 상수 `RPC_DB_LOAD = "can.db.load"`, `RPC_DB_INFO = "can.db.info"`, `RPC_SIG_GET = "can.sig.get"`, `RPC_SIG_WAIT_FOR = "can.sig.wait_for"`. (`signals.rs`는 빈 파일로 시작)

## 할 일
1. **채널별 DB**: `can.db.load{channel, path}`(R0 — 파일 읽기는 gateway 프로세스 안, 상한은 `loom-can-dbc` `Limits` 기본) → `{db_sha256, messages, signals, warnings[≤20], blocked[]}`, `can.db.info{channel}`. 채널 8개·DB 4개 상한(plan §7 "동시 DB 4"). 바뀌면 `can.db_changed{channel, db_sha256}` publish(새 kind 등록).
2. **trace 행 디코드**: `can.trace.query`·`can.last`에 `decode: true`(기본 false)면 각 프레임 행에 디코드 결과를 붙인다 — 저장된 이벤트는 바꾸지 않고 응답 행에만 `decoded{message, signals:[{name, value, unit, raw, label?}]}`. 디코드는 요청된 행만(상한 작업).
3. **`can.sig.get{channel?, signal("Msg.Sig"|유일 이름), window_ms ≤ 60000}`**: 창 안에서 그 메시지의 마지막 프레임을 찾아 디코드 → `{value, unit, raw, label?, age_ms, message, frame_ref, db_sha256}` + `evidence:[{ref:"can:<ch>@e<epoch>:seq:<n>#/signals/<Sig>", kind:"can.signal", …}]`. 없으면 `E_NO_MATCH`(계약 E2 → exit 3).
4. **`can.sig.wait_for{channel?, expr, after?, within_ms ≤ 3600000}`**: `expr` = 제한 비교식 한 개 `<Sig> <op> <number>`(op: `> >= < <= == !=`) 또는 `<Sig> between <a> <b>`. eval 금지, 손으로 파싱. `can.expect`와 같은 폴링 대기(ctx.wait가 생기면 교체) — **조건이 처음 참이 되는 새 샘플**에서 반환, 증거 ref + 값. 시간 초과 → `E_TIMEOUT` + 본 창 ref·마지막 값(본 것만).
5. **증거 술어**: `CanVerifier`에 `can.signal_range{signal, op|between, value, tol?}` 추가 — 인용 이벤트(프레임)를 세션에 고정된 DB(`db_sha256` 인자)로 디코드해 판정. DB가 다르면 `Inconclusive("db_changed")`.
6. **metrics(선택, 할 수 있으면)**: plan D3 `metrics[]`는 worker/허브 쪽 일이라 이번엔 하지 않는다 — 보고서에 설계 메모만.
7. **CLI**: `loom can db load <path> [-c ch]`, `db info`, `sig get <Sig> [--window MS]`, `sig wait-for "<expr>" [--after C] --timeout S`, `trace --decode`. 사람용 출력: `VehicleSpeed = 37.25 km/h  (ESP_01 0x0FD, age 18 ms)`.
8. **스킬**: `skills/loom-can/SKILL.md`의 M2 목록에서 `sig get/wait-for`를 정식 절로 올린다(짧게).

## 시험
fixture DBC(`crates/loom-can-dbc/tests/fixtures/*.dbc`를 경로로 읽거나 시험 안 문자열)로: load/info·상한·잘못된 파일, decode 붙은 trace, sig get(값·age·없음), wait_for(즉시 아님·새 샘플·between·timeout 증거·식 파싱 오류), verifier signal_range(일치·불일치·DB 변경), CLI 변환·렌더 golden.

커밋하지 않는다. 끝나면 보고서를 쓰고 멈춘다.
