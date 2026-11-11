# M2a 보고 — DBC 신호를 gateway에 연결 (K6·G2·K7 일부)

작성: can-opus, 2026-11-11. 과제 `docs/impl/can/tasks/m2-signals.md`. 커밋 안 함.

## 결과 (워크스페이스 루트에서)
- `cargo fmt --all --check` ✓ · `cargo clippy -p loom-can --all-targets -- -D warnings` ✓ · `cargo test -p loom-can` ✓ — **55개 통과**(새 signals 11, cli 6→9, gateway 13, worker 17, lib 5). signals 시험 0.15 s.
- `unsafe` 없음, Python 없음(제품 코드), 새 의존성 없음(리드가 넣은 `loom-can-dbc`만).
- `lib.rs`·`schema.rs`·`Cargo.toml`·`worker.rs` 안 건드림.

## 바뀐 파일
| 파일 | 내용 |
|---|---|
| `src/signals.rs`(새로 채움) | DB 저장소, `can.db.load`/`can.db.info`, 행 디코드, `can.sig.get`, `can.sig.wait_for`, 조건식 파서, `can.signal_range` 판정, 등록 함수 |
| `src/gateway.rs` | `trace`/`last`에 `decode`, `CanVerifier`가 `can.signal_range`도 처리, `register`가 `signals::register` 호출, help 예시·notes. signals가 쓰는 helper 몇 개를 `pub(crate)`로(공개 API 변화 없음) |
| `src/cli.rs` | `db load/info`, `sig get/wait-for`, `trace --decode`, `last --decode`, 사람용 출력 |
| `tests/signals.rs`(새), `tests/cli.rs` | 시험 |
| `tests/common/mod.rs`(새), `tests/gateway.rs` | gateway 시험의 가짜 journal/worker/services를 공용 모듈로 옮김(두 시험 파일이 같이 씀). 가짜 journal이 `wall_ns`도 찍게 함(`age_ms` 시험용). 동작 변화 없음 |
| `skills/loom-can/SKILL.md` | "Signals (DBC)" 절 추가, M2 목록을 M2b로 줄임. 40줄 |

## 구현 요약
1. **채널별 DB** — `can.db.load{channel?, path, encoding?}`: 경로는 **절대 경로만**(daemon 작업 폴더가 다르므로; CLI가 상대 경로를 절대로 바꿔 보냄). 파일 크기는 metadata로 먼저 거르고(16 MiB), 읽기도 `take(limit+1)`로 제한. 읽기·컴파일은 **별도 스레드**에서(허브 스레드를 막지 않음), 동시에 하나만(`E_BUSY`). 결과 `{db_sha256, messages, signals, nodes, encoding, encoding_guessed, warnings[≤20], warnings_total, blocked[≤50], blocked_total, changed, previous_sha256}`. 오류는 `E_DB`(exit 2) + detail(`limit/max/actual`, `line`, `offset`). sha가 바뀌면(첫 load 포함) `can.db_changed{channel, db_sha256, previous_sha256, path}` publish — 같은 파일 재로드는 publish 안 함. `can.db.info{channel?}` → `{channels:[…]}`(채널 없으면 전체).
2. **상한**: DB 있는 채널 8개, 서로 다른 파일 4개(같은 sha는 한 번만 셈). 채널의 DB 교체는 그 채널 몫을 빼고 센다. 넘치면 `E_DB`. 메모리 최악 4 × 48 MiB(IR 상한).
3. **trace/last 디코드**(`decode: true`): 응답 행에만 `decoded{message, signals:[{name, value, unit, raw, label?}]}`, 짧은 프레임 신호는 `{unavailable:"short_frame"}`, 길이·FD 불일치 표시, 모르는 id는 필드 없음, 모호한 id·blocked는 `{error}`. **필터를 통과해 반환되는 행만** 디코드하고, 디코드 포함 크기로 64 KiB 상한을 잰다. 결과에 `db_sha256`. 저장된 이벤트는 그대로(시험으로 확인).
4. **`can.sig.get{channel?, signal, window_ms ≤ 60000(기본 10000), origin?}`**: 최근 250 ms부터 4배씩 넓혀 창까지 앞으로 스캔해 가장 새 표본을 찾는다(바쁜 버스에서도 최신 값을 놓치지 않게). 결과 `{value, unit, raw, label, age_ms, signal:"Msg.Sig", name, message, id, origin, seq, frame_ref, db_sha256, complete, evidence:[can.signal], refs:["can:ch0@e1:seq:n#/signals/Sig"]}`. 없으면 `E_NO_MATCH`(exit 3). mux 신호는 그 분기가 활성인 프레임만 표본.
5. **`can.sig.wait_for{channel?, expr, after?(기본 now), within_ms ≤ 3600000, origin?(기본 device), any_origin?}`**: `can.expect`와 같은 10 ms 폴링. `after` 뒤 **새 표본 중 조건이 처음 참인 것**에서 반환 — `{value…, expr, samples_before, previous, predicate:{name:"can.signal_range", args}, evidence, refs}`. 시간 초과 `E_TIMEOUT` + 본 표본만 인용(`samples`, `last{value, seq, ref}`, 표본 첫..마지막 범위 ref, `can.signal_wait_timeout` evidence). 표본이 없으면 evidence 비움. 대기 시작 시 `core.activity{action:"wait", waiting:{predicate:"can.signal_range", args}}`.
6. **조건식**: 손으로 쓴 파서(eval 없음). `<Sig> <op> <number>`(op `> >= < <= == !=`, 공백 없어도 됨), `<Sig> between <a> [and] <b>`. 숫자는 10진·지수·`0x` 16진, NaN/inf 거절, 256바이트 상한, 남는 토큰 거절(`A > 1; rm -rf /` 등 시험).
7. **`can.signal_range{signal, op+value | between:[a,b], tol?, db_sha256, origin?}`**: 인용 프레임을 **그 sha의 DB로** 디코드해 판정. 그 sha DB가 지금 로드돼 있지 않으면 `Inconclusive("db_changed")`. 없음+완전한 창 → `Mismatch`, 불완전한 창 → `Inconclusive`, 인자 오류 → `Inconclusive`, 예산 초과 → `Inconclusive("eval_budget")`. `wait_for` 결과의 `predicate.args`를 그대로 넘기면 재검증된다.
8. **CLI**: `loom can db load <path> [-c ch] [--encoding utf8|windows1252|auto]`, `db info [-c]`, `sig get <Sig> [--window MS] [--origin O]`, `sig wait-for "<expr>" [--after C] --timeout S [--origin O | --any-origin]`(S: `10`, `1.5`, `500ms`, `30s`, `2m`, ≤ 1 h; 식은 보내기 전에 검사 → exit 2), `trace --decode`, `last --decode`. 출력 예: `VehicleSpeed = 37.25 km/h  (ESP_01 0x0FD, age 18 ms)` + `evidence: …`; trace는 프레임 줄 아래 `            EngineData: EngRPM=3200 rpm, Gear=2 "N"`; 시간 초과는 `scanned:` + `last: EngRPM = 1200 rpm  seq:2`. 표시 숫자는 소수 6자리까지(JSON은 원래 값).

## 결정 (리드 확인 바람)
1. **DB 로드는 R0, `RoleSet::READ`**(과제대로). viewer도 로드할 수 있다. 파일 읽기 권한은 daemon 사용자 권한 그대로.
2. **기본 encoding `auto`**: UTF-8이면 그대로, 아니면 Windows-1252(결과에 `encoding_guessed`와 경고). `loom-can-dbc` 기본(strict UTF-8)과 다르다 — 실제 차량 DBC는 cp1252가 흔해서.
3. **`sig get` 기본 origin = 아무거나**(버스의 현재 값), **`wait_for` 기본 = device**(`can.expect`와 같게: 자기가 보낸 프레임이 조건을 채우지 않게). `--any-origin`으로 바꿀 수 있다.
4. **"처음 참이 되는 새 샘플"** = `after` 뒤 첫 참 표본. 거짓→참 전이를 요구하지는 않는다(이미 참인 상태에서 새 표본이 오면 바로 반환). 전이가 필요하면 M2b에 `--edge` 추가 제안.
5. **`tol`** 은 조건을 참 쪽으로 넓힌다: `>`는 `v > x − tol`, `==`는 `|v − x| ≤ tol`, `!=`는 `|v − x| > tol`, between은 양쪽 tol.
6. **`decode: true`인데 DB가 없으면 `E_DB`**(exit 2, detail에 `loom can db load …` 힌트). 조용히 디코드 없이 주지 않는다.
7. **`age_ms`는 wall clock 기준**(`ts.wall_ns` vs gateway의 `SystemTime::now`, 같은 호스트). `CallCtx`에 허브 단조 시계가 없어서다. `wall_ns`가 0이면 `null`.
8. **unload 명령 없음**: 4개 파일 상한에 걸리면 이미 DB가 있는 채널에 교체 로드해야 한다(오류 메시지에 안내).
9. **`can.db_changed`·`PRED_SIGNAL_RANGE` 상수는 `signals.rs`에 둠**(`lib.rs`가 리드 소유).

## 요청 (리드 / CORE)
1. `lib.rs`로 옮길 상수: `KIND_DB_CHANGED = "can.db_changed"`(필요하면 `RPC_DB_UNLOAD = "can.db.unload"`도 — 추가해 주면 구현한다).
2. **CORE `ctx.wait`**(can-core.md 1번) — `sig.wait_for`도 폴링을 걷어낸다. 1시간 대기는 지금 10 ms마다 빈 스캔을 한다(스캔 하나는 작지만 대기가 많으면 낭비).
3. **`CallCtx`에 허브 단조 시계(또는 `ScanResult.now_mono_ns`)** — `age_ms`를 wall clock 없이 계산하려고.
4. **차단 작업 실행 기능(`spawn_blocking` 같은 것)** — 지금 DB 로드는 스레드 + 2 ms 폴링으로 기다린다.
5. **세션 DB 고정(G2)**: TEST 세션이 시작할 때 채널 DB sha를 고정하는 자리. 지금 `can.signal_range`는 인자 `db_sha256`과 "지금 로드된 DB 중 같은 sha"만 비교한다 — 교체된 DB로 만든 증거는 `db_changed`로 판정 보류가 된다(틀린 판정은 안 함).
6. **UI**: `can.db_changed` 이벤트, 응답 행의 `decoded` 형식(위 3번)을 `docs/impl/requests/can-ui.md`에 반영해야 한다(파일 소유는 리드/UI라 여기서 안 고침).

## metrics 설계 메모 (plan D3, 이번엔 구현 안 함)
- 위치: worker가 프레임을 낼 때가 아니라 **허브 쪽 reducer/encoder**에서, 채널에 로드된 DB로 디코드해 `metrics[]`(`{name:"EngineData.EngRPM", v, unit}`)를 붙인다. 그래야 DB 교체가 worker 재시작 없이 반영되고, worker(R0 수집)는 DBC를 몰라도 된다.
- 비용 상한: 프레임당 신호 수 상한(예: 64), 채널당 "UI가 구독한 신호"만(구독이 없으면 0). DB 교체는 `can.db_changed` 이후 이벤트부터 적용, 이전 이벤트는 그대로(재해석하지 않음).
- 이벤트 `metrics`는 증거가 아니다: 판정은 지금처럼 원 프레임을 sha 고정 DB로 다시 디코드(`can.signal_range`).

## 알려진 한계
- 폴링 대기: 응답 지연 최대 약 10 ms 추가. `after` 기본 `now`는 ms 단위라 호출 직전 1 ms 안의 표본이 "새 표본"으로 셀 수 있다(`can.expect`와 같음).
- `sig.get`이 창 단계 하나에서 스캔 예산(16 × 1,000 이벤트)을 다 쓰면 `complete:false`로 표시하고 그때까지의 최신 값을 준다(사람용 출력에 경고 줄). 250 ms 첫 창에서 그러려면 64k fps가 필요하다.
- E2E(CRC/카운터) 의심 신호 표시, 값표 라벨로 비교(`Gear == "D"`), 신호 송신(`sig set`)은 M2b.
