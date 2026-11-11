docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6): 대화형 질문 창을 띄우지 않는다. 계약과 이 지시서로 판단해 진행하고 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다.
> **경로**: 항상 `/Users/ctmctm/workspace/Loom/...`. 옛 Desktop 경로 금지(playbook §6). 너는 승인 없는 모드라 작업 폴더 밖이 필요한 명령은 실패한다 — 그런 일은 보고에 적고 넘어간다.

# P1 — core-sol: 저널 채널 상한 · worker 프로토콜 `call_id` · 경계 검사와 CI

작업 위치: **`/Users/ctmctm/workspace/Loom/.worktrees/core`**(브랜치 `core/main`, 기준 `8ab86b2`). 저장소 루트(main 체크아웃)에서는 코드를 고치지 않는다.
먼저 읽기: `/Users/ctmctm/workspace/Loom/docs/impl/00-impl-playbook.md`(§4 품질, §6 경로·질문 금지), 워크트리 `docs/contract/`(05·06·08), `docs/plan/core/plan.md` §3.7(I1)·§9.2(Q2)·§9.8, 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-journal.md`·`p1-worker-ipc.md`(앞서 리드 하위 작업자가 만든 crate 설명).

## 범위 경계
- **너**: `crates/loom-journal`, `crates/loom-proto`(아래 S2만, 추가만 — 파괴적 변경 금지), `crates/loom-worker-sdk`·`crates/loom-worker-host`(S2에 따른 수정), 새 crate `crates/loom-xtask`, `.github/workflows/`, `schema/v1`(재생성).
- **core-opus가 동시에 `crates/loom-gateway`와 `crates/loom-bench`를 고친다** — 둘은 건드리지 않는다. 루트 `Cargo.toml`은 리드만(필요하면 보고).

## S1. 저널 채널별 상한 (plan §9.2 Q2)
- 지금 worker당 저널 하나(seq 순서 유지)라 worker 합계(16 MiB/32k건)만 적용된다. `loom-journal` writer에 **채널(class)별 계정**을 추가해 채널당 8 MiB 상한을 따로 집행한다: 한 채널이 넘치면 그 채널 레코드만 `Full`(record.gap은 그 채널 범위), 다른 채널은 계속. API는 기존 호출이 깨지지 않게 추가(`try_append_class` 등).
- 시험: 채널 A 폭주 + 채널 B 정상 → A만 손실·B 무손실, 합계 상한 동작 유지.

## S2. worker 프로토콜 `call_id` (보고 p1-worker-ipc.md "Job/Input 응답")
- `loom-proto::worker`의 `JobStart/JobUpdate/JobRenew/JobStop`·`Input`에 **선택 필드 `call_id: Option<u64>`** 추가(`#[serde(default, skip_serializing_if = "Option::is_none")]` — 구버전과 호환, minor 증가). `PROTO_MINOR`를 1로 올리고 계약 05에 한 줄(리드가 문서 반영하니 보고에만 적어도 됨).
- worker-sdk·worker-host: `call_id`가 있으면 그것으로 응답을 짝짓고, 없으면 지금의 소켓 순서 번호 방식(하위 호환). 시험 추가.
- `cargo run -p loom-schema -- export`로 `schema/v1` 재생성(drift 시험 통과).

## S3. 경계 검사 `crates/loom-xtask` + CI (plan §3.7 I1, 계약 08 §8.3)
- `loom-xtask`(bin + 시험): `cargo metadata --format-version 1 --no-deps`가 아니라 **의존 그래프 전체**(`cargo metadata --format-version 1`)를 `serde_json`으로 읽어 다음을 검사하는 시험 `boundaries`:
  1. `loom-cli`(bin `loom`)의 의존 닫힘에 장치·worker 쪽 crate가 없다: `loom-gateway`, `loom-worker-host`, `loom-journal`, 그리고 이름이 `loom-*-worker`·`*-sys`인 crate, `pcap`·`serialport`·`socketcan` 같은 장치 외부 crate.
  2. 도메인 crate(`loom-` 접두이고 CORE 목록 밖인 것)는 CORE crate 중 **안정 API 표면**(`loom-proto`, `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`, `loom-testkit`(dev만))에만 의존한다. 예외 목록은 상수로(사유 주석).
  3. workspace의 모든 crate가 `[lints] workspace = true`를 쓴다(manifest 읽기).
  - `cargo` 실행은 시험 안에서 `std::process::Command`(env `CARGO`)로. 오프라인에서도 되게 `--offline` 옵션 고려.
- `.github/workflows/ci.yml`: matrix `ubuntu-latest`, `macos-latest`, `windows-latest` — `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p loom-schema -- check`, `cargo test -p loom-xtask`. Rust 1.93 고정(`dtolnay/rust-toolchain@1.93` 등), 캐시. (원격이 없어 실행은 안 된다 — 문법만 맞게.) Windows에서 Unix 전용 시험은 `cfg`로 이미 갈라져 있는지 확인하고 아니면 보고.
- Windows 타깃 검사: `rustup target list --installed`에 `x86_64-pc-windows-gnu`가 있으면(앞 작업자가 썼다) `cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings`를 돌려 결과를 보고(설치는 하지 말 것).

## 품질·보고
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`(core-opus의 진행 중 수정으로 gateway가 잠시 깨져 있으면 네 crate만 `-p`로 하고 보고). 무한 큐 금지, `unsafe` 금지. 커밋하지 않는다(리드가 함). 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-sol-a.md`(S1–S3 단계별, 시험 수, 경계 검사가 지금 찾아낸 위반, 계약과 다른 점, 필요 의존성). 화면에는 경로와 3줄 요약만. 10분 넘는 실행 금지.
