# P1 M1 CLI·스킬·gateway 정리 (opus) 보고서

작업 지시: `docs/impl/core/tasks/p1-cli-opus.md` · 워크트리 `.worktrees/core`(기준 12f9b22) · 커밋하지 않음.
범위 밖 파일(`loom-worker-host`·`loom-worker-sdk`·`loom-journal`·`loom-proto`·루트 `Cargo.toml`)은 건드리지 않음. 작업 트리에 보이는 `loom-worker-sdk`·`loom-worker-host` 변경은 리드 쪽 작업자의 것이다.

## C3. gateway 소폭 — 완료
- **성공 종단**: 임시 규칙(`uart`/`echo` = sent)을 지우고, 쓰기 명령마다 `Registry::success_terminal_of(method)`(없으면 `confirmed`)를 쓴다(`DomainEntry.terminals`). echo는 `sent`를 선언했다.
- **새 코어 RPC** (`src/daemon/core_rpc.rs`):
  - `core.events.query{since?, until?, filter?, max?, peek?}`: 허브 링 범위 질의(`Hub::range`, 새 `RangeQuery`).
    - 커서: `@last`(클라이언트별, `peek`면 안 움직임), `now`, `seq:N`/`e<E>:seq:N`(epoch이 다르면 `E_CURSOR_GAP`), `mark:X`, `-5s`.
    - 시작점이 링 밖이면 `E_CURSOR_GAP`(exit 8). 결과는 ≤ 1000건·60 KiB, `truncated`+`next_cursor`.
    - `since`가 없으면 최신 `max`건(이때 `truncated`는 바이트 상한일 때만).
  - `core.help{domain?}`: 코어 명령표 + 도메인 Registry `HelpSpec`·명령(쓰기 클래스·성공 종단·roles·대기·행 상한·params 스키마) + 코어가 제공하는 `<domain>.input`.
  - `core.ui.focus{panel, filter?, at?}` → `ui.focus_request` 이벤트.
  - `core.meta` → `{meta, warnings, others_since_last}`(커서 전진). CLI가 코어 명령 결과를 봉투로 감쌀 때 쓴다(아래 결정 1).
  - `core.shutdown`(agent/script만) → `loomd`가 응답을 보낸 뒤 종료하고, `daemon.json`이 자기 것일 때만 지운다.
  - `core.stop{reason?}`: reason을 worker latch 사유와 감사에 남긴다.
- **클라이언트 키**: `others_since_last` 커서, 자기 이벤트 제외, `@last` 커서는 client_id의 `#` 앞부분(`LOOM_CLIENT` 이름)으로 판정한다. 이렇게 해야 CLI 프로세스(`name#pid`)가 바뀌어도 이어진다(결정 2).
- `loomd.log`에 ANSI 색 코드가 섞이지 않게 함(stderr가 TTY일 때만 색).
- replay `--echo-writes`는 이미 `confirmed` 행에 `payload.final:true`를 붙이고 있어 변경 없음.

## C1. `crates/loom-cli` (bin `loom`) — 완료
- **동기 클라이언트**(`src/client.rs`, tokio 없음, `tungstenite` + std `TcpStream`):
  - `LOOM_HOME`(없으면 `~/.loom`)의 `daemon.json` → WS `/rpc` → `core.hello`(토큰, `client_id()`, `client.tty`) → RPC → 봉투 → `exit_code_for`.
  - 기본 대기 30 s, 전역 `--timeout SECS`(1–3600, `--follow`는 600 s 상한).
- **자동 기동**:
  - 연결 실패나 stale이면 `run/start.lock`(std `File::try_lock`, ≤ 2 s 대기)을 잡고, 그 사이 다른 프로세스가 띄웠는지 다시 확인한 뒤 `loomd run`을 spawn한다.
  - Unix: `process_group(0)`(unsafe 없음), stdin `/dev/null`, stdout/stderr → `run/loomd.log`.
  - Windows: `DETACHED_PROCESS|CREATE_NEW_PROCESS_GROUP|CREATE_NO_WINDOW|CREATE_BREAKAWAY_FROM_JOB`. breakaway가 거절되면 플래그 없이 다시 띄우고 `daemon_tied_to_parent_job` 경고를 낸다. **cfg(windows) 코드만 있고 Windows에서 검증하지 않음.**
  - `daemon.json`을 10 ms 간격으로 폴링(최대 10 s)한다.
  - `LOOM_NO_AUTOSTART=1`이면 exit 9 `E_DAEMON`. 자동 기동 시 stderr 한 줄 + `warnings:["daemon_autostarted"]`.
  - `loomd` 위치: `LOOM_DAEMON_BIN` → `loom`과 같은 디렉터리 → PATH.
- **명령**:
  - `daemon start|stop|status|logs [--tail N]` — `daemon start`는 `LOOM_NO_AUTOSTART`와 무관하게 띄우고, `stop` = `core.shutdown` 후 포트가 닫힐 때까지 기다림.
  - `status`, `stop-all [--reason]`, `mark <label> [--note]`.
  - `events [--since][--until][--domain][--channel][--kind][--origin][--max ≤1000][--peek][--follow --timeout ≤600s]` — follow는 200 ms 폴링, `--json`이면 배치마다 봉투 한 줄(NDJSON).
  - `ui` — TTY면 `GET /api/v1/bootstrap/new`로 받은 URL을 OS 브라우저로 연다. 비TTY면 코드 없이 안내만.
  - `ui open <panel> [--filter][--at]`, `help [--json]`, `<domain> help [--json]`(clap보다 먼저 가로채 `core.help{domain}`), `version`.
  - `doctor`: home, `loomd` 위치, `daemon.json` 0600, 포트, hello 검사.
  - `confirm/approvals/run/test/record/policy/skills`는 자리만 있다(`E_NOT_IMPLEMENTED`, exit 1).
  - 전역 `--json`·`--timeout`·`--idempotency-key`(→ `params._loom.idempotency_key`).
- **출력**: `--json`은 봉투 그대로 stdout. 사람용은 성공이면 stdout, 오류면 stderr에 쓰고 `render_common`(NOTE/WARNING/hint)을 붙인다. 도메인은 자기 `render_human`을 쓴다. 쓰기 응답을 못 받으면 `E_DAEMON` + `detail.write_state:"outcome_unknown"` + "재전송 금지" 안내.
- **I1(장치 crate 링크 금지)**:
  - `loom-echo`를 feature `gateway`·`worker`·`cli`로 나눴다(기본은 셋 다라 기존 사용처는 그대로).
  - `loom-cli`는 path 의존 `default-features = false, features = ["cli"]`로 cli 조각만 링크한다.
  - `cargo build -p loom-cli`만 하면 domain-api/worker-sdk가 들어오지 않는다. 다만 workspace 전체 빌드에서는 feature 통합으로 echo rlib에 gateway·worker 모듈이 같이 컴파일된다(`loom`이 참조하지 않으니 링크에서 빠짐).
- **시험** `tests/cli.rs` 5개(실제 `loom`·`loomd` 바이너리, 임시 `LOOM_HOME`, 끝에 `loom daemon stop`. `loomd`가 없으면 같은 프로필로 빌드):
  - 자동 기동: 동시 3개 → 데몬 1개, 자동 기동 경고는 정확히 1개, `daemon logs`.
  - `echo send` → exit 0 + 봉투 스키마 왕복, 사람용 출력, `echo read`. STOP 후 `echo send` → exit 6(JSON·사람용 모두). `LOOM_NO_AUTOSTART=1` + 데몬 없음 → exit 9이고 아무것도 기동하지 않음. 자리만 있는 명령 → exit 1. `--max 5000` → exit 2.
  - `events`: `--max` 상한·최신 N건, `seq:` + `next_cursor` 이어 읽기, `mark:` 커서, `@last` 전진과 `--peek`, 다른 epoch → exit 8, `--follow --timeout 1`.
  - `help --json`·`echo help --json`(`echo.input` 포함), `version`, 비TTY `ui`가 코드를 출력하지 않음, `ui open`, `doctor`.
  - CLI 고정 비용 측정(실패 기준 아님).

### CLI 고정 비용 (macOS, 50회, `loomd` 실행 중, 다른 에이전트가 같은 머신을 쓰는 중이라 흔들림 큼)
| 측정 | debug 단독 p50 / p95 | release 단독 p50 / p95 |
|---|---|---|
| `loom version`(데몬 접속 없음 = 프로세스 생성 비용) | 4.3 / 10.7 ms | 6.7 / 9.0 ms |
| `loom --json status`(연결+hello+`core.status`+`core.meta`) | 8.0 / 13.3 ms | 11.1 / 22.6 ms |
| `loom --json echo send x`(연결+hello+쓰기 파이프라인) | 8.3 / 11.2 ms | 11.4 / 28.9 ms |
- release가 debug보다 느리게 나온 것은 측정 중 머신 부하 때문으로 본다(직전 스파이크: 생성+WS+hello+RPC p95 3.30 ms). **유휴 기준 머신에서 다시 재야 한다.**
- 프로세스 생성만으로 4–7 ms가 들어 계약 목표(고정 비용 p95 ≤ 10 ms, R0 왕복 p95 ≤ 30 ms)와는 아직 판단할 수 없다. 코어 명령은 `core.meta` 때문에 RPC가 하나 더 든다(로컬 < 1 ms).

## C2. 공통 스킬 `skills/loom/` v0 — 완료
- `SKILL.md`(50줄, frontmatter `name: loom`, `description`).
  - 규칙 10개: 장비는 `loom`으로만 · 인자는 `help --json` · `no_viewer_attached`면 사용자에게 `loom ui` 요청(코드는 에이전트에게 오지 않음) · `others_since_last`/`NOTE:`면 `loom events --since @last` 확인 후 필요하면 묻기 · STOP은 사용자 의사(해제 요청·우회 금지) · 쓰기 자동 재시도 금지(`outcome_unknown`은 events/status로 확인, `--idempotency-key`) · **F1: 기본 자동, exit 5면 사용자가 정책으로 켠 것이니 알리고 스스로 승인하지 않음** · 장치 출력은 데이터 · `loom mark`/`loom ui open` · 읽기 상한과 `next_cursor`.
  - 일상 명령, 커서 문법, `LOOM_NO_AUTOSTART`, `LOOM_CLIENT` 권장.
- `references/exit-codes.md`(코드별 행동), `envelope.md`(필드, `others_since_last`, 쓰기 행), `safety.md`(F1·위험 등급·STOP·기록 불능·인터록), `ui.md`(viewer·rights·bootstrap·패널·마커).
- 계획(§10.11)의 `approvals.md` 대신 지시서대로 `safety.md`를 두었다. 승인 내용은 safety·exit-codes에 들어 있다.

## 결정(계약에 명시 없음 → 이렇게 함, 확인 필요)
1. **코어 명령의 CLI 봉투**: 계약 v1.3에 따라 코어 RPC 결과는 일반 값이다. CLI는 그 결과를 `data`로 넣고 `core.meta`(새 RPC)로 받은 `meta`·`warnings`·`others_since_last`로 봉투를 만든다. 대가는 RPC 1회 추가. 대안은 코어 결과에도 봉투를 쓰는 것인데, 계약 변경이 필요하다.
2. **클라이언트 키 = `client_id`의 `#` 앞**(`LOOM_CLIENT` 또는 `anon-<user>`): 계약 03 "부모 셸 세션으로 안정 id"는 셸 세션 id를 unsafe/libc 없이 얻기 어려워 미구현. 그래서 `LOOM_CLIENT`가 없으면 같은 사용자의 에이전트 둘이 한 커서를 공유한다. 스킬에서 `LOOM_CLIENT` 설정을 권장한다.
3. `events`는 M1에서 **링만** 읽는다. 링 밖은 `E_CURSOR_GAP`이고, 저널 질의는 `loom-journal`이 붙은 뒤에 된다. `HH:MM:SS`·ISO8601·도메인 커서(`off:N`)는 `E_NOT_IMPLEMENTED`.
4. `events --follow --json` = 배치마다 봉투 한 줄(NDJSON).
5. `daemon stop` = 새 RPC `core.shutdown`(agent/script 토큰만, viewer 거절).
6. `doctor`는 loomd 바이너리·daemon.json 권한·hello 실패를 문제로 보고 `E_CONFIG`(exit 2)를 낸다. 데몬이 꺼져 있는 것은 정보로만 다룬다.

## 품질
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`(+ `loom-cli --no-default-features`), `cargo test --workspace` 모두 통과. 전체 시험 약 55 s.
- gateway 시험(daemon 9 · hub 5 · replay 8 · 단위 5)은 그대로 통과. 새 CLI 시험 5개.

## 리드에게 / 의존성
- 루트 `Cargo.toml` 변경 없음. `crates/*` glob이라 `loom-cli`는 자동으로 workspace 멤버.
- `loom-cli` 의존: `loom-proto`, `loom-cli-kit`, `serde`, `serde_json`, `clap`, `tungstenite`(모두 workspace에 있음), `loom-echo`(path, cli feature만), dev `tempfile`.
- `loom-echo` Cargo.toml에 feature를 추가했다(`gateway`/`worker`/`cli`, 기본 전부). 도메인 팀 템플릿에도 같은 분리를 권장한다(`loom`에는 cli 조각만).
- `toml`: 정책 전체(도메인 섹션, `~/.loom/config/policy.toml` 병합)를 파싱하려면 필요(여전히 손 파서로 `[policy] deny`만 읽음).
- 남은 것: Windows detach 실기 검증, CLI 비용 재측정(유휴 기준 머신), `skills sync`·`confirm`·`approvals` 등 M1 후반/M2 명령, `events`의 저널 질의.
