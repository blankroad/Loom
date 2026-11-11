# ETH P0 공통 규칙 (두 워커 모두 읽기)

정본 계획: `docs/plan/eth/plan.md`. 결정 기록 X0–X9를 따르고, 이번 P0에서 X10을 추가한다(아래). 최우선 원칙은 `docs/plan/02-core-principle.md`다: 에이전트(CLI)와 사용자(웹 UI)가 같은 패킷을 같은 순간 본다. 이번 P0는 그 토대가 되는 **코어 독립 부품**이다.

## 작업 위치
- 코드는 **worktree `/Users/ctmctm/workspace/Loom/.worktrees/eth`(브랜치 `eth/main`)** 에서만 고친다. 저장소 루트 체크아웃에서는 고치지 않는다.
- 자기에게 배정된 crate 디렉터리만 만든다·고친다. 다른 워커 crate는 읽기만 한다. 루트 `Cargo.toml`은 만들지 않는다(CORE 소유).
- 커밋하지 않는다. 리드가 검토 후 커밋한다.
- 보고는 저장소 루트 `/Users/ctmctm/workspace/Loom/docs/impl/eth/reports/p0-<worker>-<crate>.md`에 쓴다(crate마다 1개, 40줄 이하): 무엇을 만들었나, 공개 API 요약, 상한(C2) 표, 테스트 수·결과, 미해결.

## crate 규칙 (CORE workspace가 main에 들어오기 전까지 독립 crate)
- `crates/<name>/Cargo.toml`: `edition = "2024"`, `rust-version = "1.93"`, `publish = false`, `version = "0.1.0"`.
  - 아래 lints 블록을 그대로 넣는다(CORE workspace와 동일, 나중에 `[lints] workspace = true`로 바꿈):
  ```toml
  [lints.rust]
  unsafe_code = "deny"
  missing_debug_implementations = "warn"
  [lints.clippy]
  dbg_macro = "deny"
  todo = "deny"
  unwrap_used = "deny"
  ```
- crate 디렉터리에 `rustfmt.toml`을 두고 `edition = "2024"`, `max_width = 110`으로 설정한다(CORE와 동일).
- 의존성은 정확한 버전 범위로 적는다. 새 외부 crate는 보고서에 "CORE workspace.dependencies 추가 요청" 목록으로 남긴다. 공통 crate는 CORE와 같은 판을 쓴다: serde 1.0.228(derive), serde_json 1.0.149, thiserror 2.0.18, hex 0.4.3.
- `unsafe` 금지. `unwrap()` 금지(테스트 코드 포함 — `expect("이유")`나 `?`를 쓴다). `todo!()` 금지.
- 모든 큐·버퍼·맵에 **상한과 넘칠 때 정책을 코드 상수로 명시**한다(C2). 상수 이름 앞에 `MAX_`/`CAP_`를 붙이고 doc 주석에 넘칠 때 동작을 쓴다. 무한 `Vec` 누적을 금지한다.
- 입력 파서(pcapng 블록, 패킷 바이트, 필터식)는 **어떤 입력에도 panic 금지**. 길이 검사는 할당 전에 한다. 테스트에 무작위 바이트 반복(결정적 시드 의사난수, 외부 fuzz crate 없이 ≥ 10만 회)을 넣는다.
- Python 금지. 테스트 fixture 생성도 Rust로 한다.

## 빌드 환경 (재개 공지 05)
- 모든 cargo 명령에 다음을 붙인다(대형 캐시는 프로젝트 밖):
  `CARGO_HOME=$HOME/.cache/loom-eth-cargo CARGO_TARGET_DIR=$HOME/.cache/loom-target/eth`
- 관문(crate마다, crate 디렉터리에서):
  `cargo fmt --check` · `cargo clippy --all-targets -- -D warnings` · `cargo test`
- 시간은 명령 1회 10분 이내로 한다. 파일 읽기가 30초 넘게 멈추면 중단하고 보고서에 `BLOCKED: file io`를 쓴다.
- sudo·전역 설치·시스템 설정 변경을 하지 않는다. 이 Mac은 BPF 권한이 없다(라이브 캡처 불가 — 정상).

## X10 (P0 리드 결정): pcapng 안의 Loom 메타데이터
EPB의 `opt_comment`에 **`loom1 ` 접두 + 공백 구분 key=value**를 쓴다. 예: `loom1 seq=41240 dir=tx req=t-000418 org=agent`.
- 이유: custom option(2988/2989)은 IANA PEN이 필요한데 Loom은 PEN이 없다. 주석이면 Wireshark에서 사용자가 그대로 볼 수 있다.
- 읽을 때는 이 접두가 있는 주석만 해석하고, 사용자 주석은 보존한다.
- 키: `seq`(u64), `dir`(`rx|tx`), `req`(문자열, 공백 금지), `org`(`agent|user|script`, `attributed_origin`).

## 질문 대화상자 금지 (플레이북 §6)
대화형 질문 창을 열지 않는다. 계약·계획대로 정하고 보고서에 결정을 적거나, 질문을 보고서에 쓰고 작업을 끝낸다. 상세는 `p1-common.md` 같은 절.
