docs/plan/02-core-principle.md 준수

# UDS P0 공통 규칙 (두 워커 모두 먼저 읽기)

최우선: `docs/plan/02-core-principle.md` — 에이전트의 UDS 요청·ECU 응답이 같은 순간 사용자 웹 UI(UDS 콘솔)에 보이고, 사용자가 개입할 수 있어야 한다. P0는 그 토대가 되는 **코어 독립 부품**이다(마스터 §3.0 P0 UDS 칸: 서비스 코덱, tokio DoIP, 가상 ECU).

읽을 것(필요한 절만): `docs/plan/uds/plan.md`(§3.3–3.4 TargetSession·요청 흐름, §3.8 큐 예산, §9.1 가상 ECU, §10.3 등급표, §12 작업 표), `docs/plan/uds/v2-sol.md`(crate·DoIP 조사), 계약 `docs/contract/`(02 이벤트·04 안전·08 도메인 통합), 마스터 `docs/plan/00-master-plan.md` §2.4 E·§2.5 F1.

## 결정 (P0 리드)
- **F1 = 기본 모두 자동.** 등급(R0–R4) 분류는 그대로 하고 화면·기록에 표시하지만, 기본 정책은 R0–R4 전부 `auto`. 확인 토큰·뷰어 승인은 기본 꺼짐. `immutable_deny`는 Loom이 구현하지 않은 서비스만(UDS 0x2A ReadDataByPeriodicIdentifier, 0x86 ResponseOnEvent). 0x34/0x36/0x31/0x27 등은 차단하지 않는다(사용자가 `loom.toml`로 막을 수 있게만).
- plan.md의 R2 `token_or_viewer`·sim 화이트리스트 같은 v1.2 승인 흐름은 **"설정으로 켰을 때"** 경로로만 남긴다. P0에서는 구현하지 않는다.
- Python 금지. 테스트 fixture도 Rust.

## crate 배치와 소유 (겹치지 않게)
| crate (`/Users/ctmctm/workspace/Loom/.worktrees/uds/crates/…`) | 소유 | 내용 |
|---|---|---|
| `loom-uds-codec` | uds-opus | UDS 서비스 코덱·NRC 표·응답 분류·DID 코덱·진단 DB 스키마 (순수, I/O 없음) |
| `loom-uds-sim` | uds-opus | 가상 ECU 상태 모델 `handle()` (순수) + 결함 주입 |
| `loom-uds` | uds-opus | 도메인 모듈(`DomainModule`), `classify` 훅·등급표, TargetSession, 이벤트 payload 타입 |
| `loom-uds-doip` | uds-sol | DoIP 프레이밍 + tokio 클라이언트 전송 + 테스트·sim용 엔티티(서버) 측 |
| `loom-uds-runner` | uds-sol | `Transport` trait + 트랜잭션 러너(P2/0x78/deadline/epoch/quarantine) + `write_state` 매핑 |

- 자기 crate만 만들고 고친다. 다른 워커 crate는 **읽기만**. 의존 방향: `runner → codec`(응답 분류만), `sim → codec`, `loom-uds → 전부`. `doip`는 codec에 의존하지 않는다(UDS 바이트를 불투명 payload로 다룸).
- 상대 crate가 아직 없거나 필요한 API가 없으면 기다리지 말고 **자기 crate 안에 최소 임시 정의**를 두고 보고서 "통합 시 교체" 목록에 적는다. 상대 crate를 직접 고치지 않는다.

## 저장소 규칙 (구현 플레이북 `docs/impl/00-impl-playbook.md`)
- 코드는 worktree `/Users/ctmctm/workspace/Loom/.worktrees/uds`(브랜치 `uds/main`)에서만. 저장소 루트 체크아웃은 고치지 않는다. **옛 Desktop 경로를 쓰지 않는다.**
- 루트 `Cargo.toml`·`crates/loom-proto`·`loom-domain-api`·`loom-worker-sdk`·`loom-cli-kit`·`loom-testkit`·`docs/contract/`는 CORE 소유 — 고치지 않는다. 의존 가능한 CORE crate는 이 5개뿐(`loom-gateway` 등 직접 의존 금지).
- crate는 workspace member로 자동 포함된다(`members = ["crates/*"]`). `Cargo.toml`은 `version.workspace = true`, `edition.workspace = true`, `rust-version.workspace = true`, `publish = false`, `[lints] workspace = true`, 의존은 `serde = { workspace = true }` 형태. dev-dependency에서 workspace 의존에 feature 추가(예: tokio `test-util`)는 허용.
- 루트 `[workspace.dependencies]`에 없는 외부 crate가 필요하면 쓰지 말고 보고서에 "CORE 추가 요청"으로 적는다(리드가 `docs/impl/requests/uds-core.md`로 요청). 이미 요청한 것: `serde-saphyr`(YAML).
- 커밋하지 않는다(리드가 검토 후 커밋).
- `unsafe` 금지, `unwrap()` 금지(테스트 포함 — `expect("이유")`나 `?`), `todo!()` 금지.
- **C2**: 모든 큐·버퍼·맵에 상한과 넘칠 때 정책을 코드 상수로(`MAX_`/`CAP_` 접두, doc 주석에 넘칠 때 동작). 무한 `Vec` 누적·무한 채널 금지(`mpsc::unbounded_channel` 금지).
- 입력 파서(DoIP 헤더, UDS 응답, DB 파일)는 **어떤 입력에도 panic 금지**, 길이 검사는 할당 전에. 결정적 시드 의사난수로 무작위 바이트 ≥ 10만 회 시험(외부 fuzz crate 없이).

## 빌드
- cargo 명령에 붙인다: `CARGO_HOME=$HOME/.cache/loom-uds-cargo CARGO_TARGET_DIR=$HOME/.cache/loom-target/uds` (리드가 의존성을 미리 받아 둠, 필요하면 `--offline`).
  - Codex 샌드박스가 이 경로 쓰기를 막으면, 대신 worktree 안 `CARGO_HOME=.cargo-home CARGO_TARGET_DIR=target`(둘 다 gitignore됨)을 쓰고 보고서에 적는다. 네트워크가 막혀 새 crate를 못 받으면 멈추고 보고한다.
- 관문(자기 crate마다, worktree 루트에서): `cargo fmt -p <crate> --check` · `cargo clippy -p <crate> --all-targets -- -D warnings` · `cargo test -p <crate>`.
- 명령 1회 10분 이내. 파일 읽기가 30초 넘게 멈추면 중단하고 보고서에 `BLOCKED: file io`.
- sudo·전역 설치·시스템 설정 변경 금지.

## 질문 금지 (플레이북 §6)
- **대화형 질문 창(선택지 질문, AskUserQuestion 등)을 띄우지 않는다.** 메인·리드는 다른 에이전트의 창에 응답할 수 없다.
- 결정이 필요하면 계약(`docs/contract/`)·계획(`plan.md`)·이 지시에 맞는 쪽으로 스스로 정하고 보고서 "결정" 목록에 근거와 함께 적는다.
- 판단할 수 없으면 보고서 "질문" 목록에 쓰고, 그 부분을 건너뛴 채 나머지를 끝낸다.

## 보고
crate마다 `/Users/ctmctm/workspace/Loom/docs/impl/uds/reports/p0-<worker>-<crate>.md` (40줄 이하): 만든 것, 공개 API 요약, C2 상한 표, 테스트 수·결과(관문 3개 출력 요약), 임시 정의·통합 시 교체 목록, CORE 추가 요청, 미해결. 모든 배정 crate를 끝내면 멈춘다.
