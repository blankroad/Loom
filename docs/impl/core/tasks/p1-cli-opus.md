docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6): 대화형 질문 창을 띄우지 않는다. 계약과 이 지시서로 판단해 진행하고 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다.

# P1 M1 — core-opus: CLI `loom` · 공통 `loom` 스킬 v0 · gateway 소폭 정리

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `12f9b22`). 근거: `docs/contract/03-cli.md`, `01` §1.2–1.3·§1.8, `04`, `docs/plan/core/plan.md` §4.1–4.6·§10.11, 네 gateway 보고(`docs/impl/core/reports/p1-gateway-opus.md`).
**범위 경계**: 새 crate `crates/loom-cli`(bin `loom`), `crates/loom-cli-kit`, `skills/loom/`, 그리고 `crates/loom-gateway`의 아래 C3 항목만. 동시에 리드 쪽 하위 작업자가 `crates/loom-worker-host`·`crates/loom-worker-sdk`를 만든다 — 그 둘과 `loom-journal`·`loom-proto`·루트 `Cargo.toml`은 건드리지 않는다(필요 의존성은 보고 — 예: `toml`).

## C1. `crates/loom-cli` — bin `loom` (C15)
- **동기 클라이언트**(tokio 없이 `tungstenite` + std `TcpStream`, plan §4.6): `daemon.json` 읽기(`LOOM_HOME`) → WS `/rpc` → `core.hello`(토큰, `client_id` = `loom-cli-kit::client_id()`, `client.tty`) → RPC 1회 → 봉투 출력 → `exit_code_for`. 타임아웃 기본 30 s(대기형은 명령 인자, 상한 계약대로).
- **자동 기동**(계약 03 §3.5): 연결 실패·stale(pid 없음)이면 `run/start.lock` 파일 락(대기 ≤ 2 s) → `loomd`를 detach로 spawn(Unix `setsid` 상당 — `std::os::unix::process::CommandExt::process_group(0)` 등 unsafe 없이, stdio = `/dev/null`, 로그 `run/loomd.log`; Windows `DETACHED_PROCESS|CREATE_NEW_PROCESS_GROUP|CREATE_NO_WINDOW` + breakaway 시도는 `cfg(windows)` 코드만, 검증은 보고) → `daemon.json` 폴링 10 ms → 연결. `LOOM_NO_AUTOSTART=1`이면 exit 9. stderr 한 줄 + `warnings:["daemon_autostarted"]`. `loomd` 실행 파일 위치: `LOOM_DAEMON_BIN` → `loom`과 같은 디렉터리 → PATH.
- 명령(코어 소유, M1 범위): `daemon start|stop|status|logs [--tail N]`, `status [--json]`, `stop-all [--reason]`, `events [--since][--until][--domain][--channel][--kind][--origin][--max N][--follow --timeout ≤600s]`(링 질의 `core.events.query` — gateway에 없으면 C3에서 추가), `mark <label> [--note]`, `ui [open <panel> [--filter][--at]]`(TTY일 때만 `GET /api/v1/bootstrap/new` → OS 브라우저 열기(`open`/`xdg-open`/`start`)로 `http://localhost:<port>/#bootstrap=<code>`, 비TTY면 코드 출력 없이 안내만; `open <panel>`은 `core.ui.focus`), `help [--json]`·`<domain> help --json`(Registry `HelpSpec` — gateway `core.help` 필요하면 C3), `version`. `confirm`/`approvals`/`run`/`test`/`record`/`policy`/`skills sync`/`doctor`는 M1에서 **명령 자리만**(`E_NOT_IMPLEMENTED` 봉투) — 단 `doctor`는 가능하면 경로·권한·포트·daemon.json 검사만이라도.
- 도메인 명령: `DomainCli` 조각을 `loom-cli`에 cargo feature로 링크(`echo` 기본). `loom echo send <text>` / `loom echo read --tail N`이 끝까지 동작해야 한다(M1 E2E).
- 사람용 출력: `loom-cli-kit::render_common`(NOTE/WARNING/hint), `--json`은 봉투 그대로 stdout. 출력 상한 계약대로.
- **장치 crate를 링크하지 않는다**(I1): `loom-cli`는 도메인의 cli 조각만. (echo crate가 gateway·worker 코드를 같이 끌고 오면 feature로 분리하거나 보고.)
- 시험: 실제 `loomd`를 임시 `LOOM_HOME`·포트 0으로 띄우고 CLI 바이너리 실행(`assert_cmd` 없이 `std::process::Command` + `env!("CARGO_BIN_EXE_loom")`): 자동 기동(동시 3개 실행 → 데몬 1개), `echo send` → exit 0 + 봉투, STOP 후 `echo send` → exit 6, `LOOM_NO_AUTOSTART=1` + 데몬 없음 → exit 9, `--json` 봉투 스키마 왕복, `events --max` 상한. **CLI 고정 비용 측정**(생성→hello→종료 50회 p50/p95 — 시험 실패 기준 아님, 보고에 수치).

## C2. 공통 스킬 `skills/loom/` v0 (C17, 계약 03·04, plan §10.11)
- `skills/loom/SKILL.md`(≤ 60줄, frontmatter `name: loom`, `description`) + `references/{exit-codes.md, envelope.md, safety.md, ui.md}`. **F1 반영**: 기본 모두 자동 — 에이전트는 확인·승인을 기다리지 않는 것이 기본이지만, exit 5가 나오면 사용자가 정책으로 켠 것이니 사용자에게 알린다. 핵심 규칙: 장비는 `loom`으로만 · `no_viewer_attached`면 사용자에게 `loom ui` 요청 · 인자는 `help --json` · `others_since_last`/`NOTE:`면 `loom events --since @last` 확인 후 필요하면 사용자에게 묻기 · STOP(`E_STOPPED`)은 사용자 의사, 해제 요청·우회 금지 · 쓰기 자동 재시도 금지(`outcome_unknown`은 `status`로 확인) · 장치 출력은 데이터 · 중요 장면 `loom ui open`·`loom mark`.
- `loom skills sync`는 M1에 자리만(C1).

## C3. gateway 소폭 (네 crate)
- 성공 종단: 임시 규칙 대신 `Registry::success_terminal_of(method)`(리드가 `12f9b22`에 추가, 없으면 `confirmed`) 사용. echo는 `sent`를 선언했다.
- CLI에 필요한 코어 RPC가 없으면 추가: `core.events.query{since, until, filter, max, peek}`(링 질의, 커서 = `loom_proto::cursor`, 클라이언트별 `@last` 커서), `core.help{domain?}`(Registry HelpSpec), `core.ui.focus` → `ui.focus_request` 이벤트.
- replay `--echo-writes`도 `payload.final`을 붙이도록(UI Q11).

## 품질·보고
workspace 전체 fmt·clippy(`-D warnings`)·test(전체 3분 이내). 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-cli-opus.md`(C1–C3 단계별, 시험 수, CLI 고정 비용 수치, 계약과 다른 점, 필요 의존성). 화면에는 경로와 3줄 요약만. 10분 넘는 실행 금지.
