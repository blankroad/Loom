# P1 core-sol S1–S3 작업 보고

S1–S3 구현을 완료했다. 담당 5개 crate의 시험 100개와 macOS·Windows 담당 crate clippy, schema drift 검사를 통과했다. 전체 workspace 품질 검사는 범위 밖 Input 생성자와 Windows enum 경고 때문에 통과하지 못했다. 커밋하지 않았다.

## 작업 범위

- 작업·보고 위치는 `/Users/ctmctm/workspace/Loom/.worktrees/core`로 한정했다. 사용자 지시를 우선하여 루트 체크아웃 보고 경로에는 쓰지 않았다. 이 보고의 루트 복사는 리드가 수행해야 한다.
- `loom-gateway`, `loom-bench`, 루트 Cargo.toml과 다른 팀 crate는 수정하지 않았다. 기존 변경을 보존했다.
- 실행 환경은 Rust/Cargo 1.93.1, macOS aarch64다. 기존에 설치된 `x86_64-pc-windows-gnu`만 사용했으며 target을 설치하지 않았다. 모든 실행은 10분 미만이었다.

## S1. 저널 채널별 상한

- 하나의 producer 저널과 seq 순서를 유지하며 입력 큐에 class별 바이트 계정을 추가했다. 기본 class 상한은 8 MiB다. worker 전체 16 MiB/32k건 상한과 공유 QueueBudget은 유지한다. class 예약·반납은 전체 예약·반납과 같은 mutex 안에서 수행하고 사용량이 0인 class는 제거한다.
- 기존 `try_append`는 SourceRecord.channel을 자동 분류한다. `try_append_class`, `try_append_raw_class`, `class_queue_bytes`를 추가했다. 기존 raw 호출은 class 미지정으로 전체 상한을 적용한다. 채널 없는 제어 레코드도 전체 상한을 적용한다.
- class 이름 복제 비용을 큐 바이트 계정에 포함했다. 이름은 UTF-8 256바이트 이하로 제한하며 초과하면 기존 Encode 오류로 거절하고 seq를 소비하지 않는다. 큐와 손실 목록 모두 유한하다.
- 큐 포화 손실을 class별로 모으고 GapReport.class와 GapData.detail.class에 보존한다. SDK는 해당 class를 channel로 가진 `record.gap`을 ctl로 발행한다. 디스크·복구 손실처럼 채널을 확정하지 못하면 class는 None이다. 최대 손실 목록을 넘어서 여러 class를 합쳐야 할 때에는 class=None/count=unknown/cause=multiple로 보수적으로 보고한다.
- 기본 8 MiB class/16 MiB worker 계정의 독립 집행과 반납을 단위 시험했다. 실제 writer를 정지시킨 통합 시험에서는 작은 상한으로 A를 폭주시켜 A만 Full, B 무손실, gap의 A 범위·정확한 손실 수, 수락된 seq 순서와 큐 계정 반납을 확인했다.

## S2. worker call_id

- JobStart/JobUpdate/JobRenew/JobStop/Input에 `call_id: Option<u64>`와 `serde(default, skip_serializing_if = "Option::is_none")`를 추가하고 WORKER_PROTO_MINOR를 1로 올렸다. 필드가 없는 기존 JSON/MessagePack map과 0·9123·u64::MAX의 왕복을 검사했다.
- SDK·host는 minor 1에서 명시 ID를 응답 연결에 우선 사용한다. ID가 없으면 소켓 요청 순번을 사용한다. 명시 ID가 있는 요청도 물리 요청 순번을 한 번 증가시켜 다음 생략 요청과 맞춘다. priority Input도 같은 규칙을 따른다.
- minor 0 협상 시 host는 새 선택 ID를 보내지 않고 SDK는 기존 순번으로 응답한다. host가 기존 Call/Write에 순번을 부여하는 API 동작은 유지한다. 아직 처리 중인 ID가 충돌하면 전송 전에 WorkerBusy로 거절하여 기존 응답 대기자를 덮어쓰지 않는다.
- 실제 host↔worker 시험에서 네 Job 종류의 명시 ID, 0/u64::MAX, 일반·priority Input의 명시/생략 ID와 기존 Call을 섞어 확인했다. 가짜 gateway↔실제 SDK 시험에서는 minor 0/1 각각의 wire Reply ID와 명시 ID 다음 생략 ID의 순번을 검증했다.
- 스키마 32개를 재생성했다. 실제 diff는 worker_msg.schema.json의 선택 call_id 5개뿐이며 schema check가 통과했다.

### 계약 05에 반영할 문장

`loom-worker/1.1`에서 JobStart/JobUpdate/JobRenew/JobStop/Input은 선택적 정수 call_id를 지원하며, 협상 minor가 1 이상이고 필드가 있으면 Reply.call_id로 이를 반영하고, 필드가 없거나 minor가 0이면 기존 소켓 요청 순번으로 연결한다. 모든 응답 대상 요청은 명시 ID 유무와 관계없이 순번을 소비한다.

문서 계약은 지시대로 직접 고치지 않고 여기 보고한다. wire 형식은 필드 생략 시 하위 호환이다. Rust struct literal 호출자는 추가 필드를 명시해야 한다. GapReport에도 선택 class 필드를 추가했으므로 이를 직접 생성하는 외부 Rust 호출자는 class: None을 보강해야 한다. 기존 메서드 이름·인수는 유지했다.

## S3. 경계 검사와 CI

- 새 `loom-xtask` library/bin과 `boundaries` 통합 시험을 추가했다. env CARGO를 사용하는 std::process::Command로 `cargo metadata --format-version 1 --offline`의 전체 resolve 그래프를 읽는다. `--no-deps`는 사용하지 않는다.
- CLI의 일반·build 의존 닫힘을 순회해 장치/worker crate를 금지한다. dev 전용 간선은 CLI 실행 파일에 들어가지 않아 제외한다. 오류에는 전이 의존 경로를 표시한다.
- 전체 workspace metadata에서는 gateway/echo의 feature 통합으로 CLI에 worker 의존성이 있는 것처럼 보일 수 있다. 따라서 CLI만 의존하는 임시 독립 workspace의 전체 metadata를 별도로 읽는다. 실제 Cargo.lock을 probe에 복사하여 잠금 버전을 우선 사용하고 probe만 갱신한다. probe는 워크트리 target 아래 고유 디렉터리에 만들고 종료 시 정리한다.
- 도메인의 직접 CORE 의존은 안정 API 4개와 dev 전용 testkit만 허용한다. 안정 SDK 내부의 journal 같은 전이 구현은 허용한다. CORE 목록·안정 API·예외는 상수로 선언했다.
- 모든 workspace member manifest를 TOML로 해석해 실제 `[lints] workspace = true`를 검사한다. 주석 문자열이나 직접 lints 선언은 상속으로 취급하지 않는다.
- CLI 예외는 anstyle-query→windows-sys, anstyle-wincon→windows-sys 두 간선뿐이다. Windows 콘솔 색·터미널 탐지는 장치 백엔드가 아니므로 사유를 코드 주석에 적었다. 다른 caller의 windows-sys는 금지하며 단위 시험했다. 도메인 비안정 CORE 예외는 없다.
- 경계 단위 시험 7개는 전이/build 금지, dev 제외·순환, 좁은 Windows 예외, 안정 API·dev testkit 허용/금지, resolve 누락, lint TOML 판별을 검증한다. 실제 workspace 통합 시험 1개도 통과했다. **최종 시점에 발견한 경계 위반은 0건**이다. 작업 초기에 보였던 ETH의 직접 lints 선언은 최종 manifest에서는 상속으로 정리되어 있었으며 이 작업자는 수정하지 않았다.
- CI는 ubuntu-latest/macos-latest/windows-latest, Rust 1.93, 캐시와 10분 job 상한을 설정했다. fmt, workspace all-target clippy, workspace 시험, schema check, xtask 시험을 실행한다. 먼저 `cargo fetch --locked`로 타 OS 의존성까지 준비하여 오프라인 metadata가 새 CI 캐시에서도 작동하게 했다. PyYAML로 YAML 구문·OS matrix·step 수를 확인했다. 원격 CI는 실행하지 않았다.
- Unix IPC 시험은 SDK transport와 host frames/ipc/journal 파일의 `#![cfg(unix)]`로 이미 분리되어 있다. 추가 IPC 시험도 해당 파일에 넣었다. Windows는 cross clippy 결과이며 Windows 런타임 시험 결과는 아니다.

## 검증 결과

명령은 `/Users/ctmctm/workspace/Loom/.worktrees/core`를 workdir로 하고 Cargo manifest·schema 디렉터리를 절대 경로로 지정했다. 아래 명령 표의 Cargo 옵션은 모두 동일한 manifest를 사용했다.

| 검사 | 결과 |
| --- | --- |
| cargo fmt --all --check | 통과 |
| 담당 crate cargo test (offline) | **100개 통과**, 실패·무시 0 |
| 담당 crate cargo clippy --all-targets -- -D warnings (offline) | 통과 |
| 최종 lock probe 보강 후 cargo test -p loom-xtask | 8개 통과 |
| 최종 lock probe 보강 후 xtask all-target clippy | 통과 |
| loom-schema export 및 check | 32개 export, drift 없음 |
| cargo clippy --workspace --all-targets -- -D warnings | 범위 밖 testkit Input 생성자 컴파일 오류 |
| cargo test --workspace | 같은 testkit 컴파일 오류로 시험 실행 전 중단 |
| cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings | 범위 밖 console large_enum_variant 오류 |
| 담당 crate Windows target clippy -- -D warnings | 통과 |
| git diff --check | 통과 |
| CI YAML | 구문·matrix 확인, 원격 실행 없음 |

담당 crate별 통과 수는 journal 25, proto 34, worker-sdk 17, worker-host 16, xtask 8로 합계 100개다. 이 변경으로 추가한 시험은 journal 2, proto 1, worker-host 2, xtask 8로 13개다. 최종 xtask 8개 재실행은 합계에 중복 계산하지 않았다.

검증 로그는 워크트리 target 아래 p1-sol-selected-final.log, p1-sol-selected-clippy.log, p1-sol-workspace-test.log, p1-sol-workspace-clippy.log, p1-sol-windows-clippy.log, p1-sol-selected-windows-clippy.log, p1-sol-xtask-final.log에 있다.

## 리드/소유자 통합 조치

1. `/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-testkit/src/fake_worker.rs:144`의 Input literal에 `call_id: None`을 추가해야 한다. 전체 native test/clippy를 실제로 막은 E0063이다.
2. `/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-gateway/src/daemon.rs:1637`의 Input literal에도 `call_id: None`이 필요하다. 앞선 오류로 전체 빌드가 멈춰 이 위치의 컴파일 진단은 수집되지 않았지만, 최종 소스를 읽어 필드 누락을 확인했다. 동시 작업 금지 영역이므로 수정하지 않았다.
3. `/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-console/src/matcher.rs:194`의 Compiled::Literal(Finder)는 Windows에서 288바이트이고 Regex variant는 32바이트다. Windows 전체 clippy의 large_enum_variant를 소유자가 해결해야 한다.
4. 위 조치를 합친 뒤 전체 workspace test/clippy와 실제 3개 OS CI를 재실행해야 한다. 현재 전체 품질 통과로 보고하지 않는다.

필요 의존성은 xtask의 serde/serde_json/toml뿐이며 모두 기존 workspace dependency를 상속한다. 새 외부 패키지나 루트 Cargo.toml 수정은 필요 없다. wildcard workspace member 등록으로 xtask가 포함되며 Cargo.lock에는 xtask package 항목만 추가되었다. unsafe와 무한 큐는 추가하지 않았다.
