# 구현 플레이북 (리드용, P0부터)

계획 단계는 끝났다. 이제 코드를 만든다. 먼저 읽을 것: `docs/plan/02-core-principle.md`(최우선), `docs/plan/00-master-plan.md` v2.2(특히 **§2.5 F1 = 기본 모두 자동**, F2 = 이더넷 1차, §2.4 E 판정, §3.0 단계), 자기 팀 `docs/plan/<comp>/plan.md`, 그리고 CORE가 내는 `docs/contract/`.

## 1. 저장소와 작업 공간

- 저장소: `/Users/ctmctm/workspace/Loom` (git, 브랜치 `main`, 원격 없음 — push 금지).
- **팀별 worktree**: `/Users/ctmctm/workspace/Loom/.worktrees/<comp>` 브랜치 `<comp>/main`. 팀의 모든 코드 작업은 여기서 한다. `main` 체크아웃(저장소 루트)에서는 코드를 고치지 않는다.
- 저장소 배치(CORE가 Day 1에 확정): 루트 `Cargo.toml` = workspace(`members = ["crates/*"]`), Rust crate는 `crates/<이름>/`, 웹 UI는 `ui/`(pnpm), 스킬 원본은 `skills/`, 계약은 `docs/contract/`.
- **소유 경계**: 루트 `Cargo.toml`·`crates/loom-proto`·`crates/loom-core*`·`docs/contract/`는 CORE만 수정. 각 팀은 자기 crate(`crates/loom-<comp>*`)와 자기 문서만. `ui/`는 UI 팀. 다른 팀 영역 변경이 필요하면 `docs/impl/requests/<from>-<to>.md`에 요청을 쓴다.
- CORE의 workspace가 `main`에 들어오기 전에는 각 팀 crate를 독립 crate로 만들어 `cargo test --manifest-path crates/<x>/Cargo.toml`로 시험한다(루트 `Cargo.toml`을 만들지 말 것).

## 2. 통합 (메인 담당)

- 리드가 팀 브랜치에서 의미 있는 단위(테스트 통과)를 커밋한 뒤 `docs/impl/<comp>/STATUS.md`에 `MERGE READY: <커밋 해시> — <요약>`을 쓴다. 메인이 `main`에 병합하고 STATUS에 `MERGED`를 남긴다.
- 팀은 `main`이 바뀌면 자기 브랜치에 `git merge main`(또는 rebase)으로 따라간다. 다른 팀 브랜치를 직접 병합하지 않는다.
- CORE 태그 `core-p0-d2`(loom-proto + compile-only trait)와 `core-p0-d5`(testkit·fixture-replay)가 `main`에 들어오면 모든 팀이 그 위로 옮긴다.

## 3. 팀 안 역할

- 리드: P0 작업을 쪼개 `docs/impl/<comp>/tasks/*.md`로 워커에게 준다. **두 워커에게 겹치지 않는 파일·crate를 배정**한다(같은 worktree를 공유하므로). diff 검토, 테스트 실행, 커밋은 리드가 한다.
- 워커: 배정된 범위만 수정하고 테스트까지 통과시킨 뒤 보고. 커밋은 하지 않는다(리드가 함).
- 권장 분담: opus = 구조·핵심 경로·UI/UX, sol = 드라이버·FFI·성능 측정·시험 보강. 리드 판단으로 바꿔도 된다.

## 4. 품질 기준 (모든 커밋)

- Rust: stable(현재 1.93), edition 2024, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` 통과. 시뮬레이션 백엔드 시험 필수(실HW 없이 통과).
- Python 금지(C1). 예외는 결정 기록에 근거가 있는 것만.
- C2: 무한 큐 금지, 상한과 넘칠 때 정책을 코드에 명시.
- UI·E2E: Playwright. 브라우저는 프로젝트 밖에 설치(`PLAYWRIGHT_BROWSERS_PATH=~/.cache/ms-playwright`). 에이전트가 쓰는 자동화 브라우저는 watch 권한만.
- 핵심 원칙 관문: "로그만 있고 사용자 화면 없음", "사용자 개입 경로 없음", "증거 없는 PASS"는 불합격.
- 커밋 메시지 끝에 다음 두 줄:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01Hkioe2Dum77VbLisuue7eV
  ```

## 5. 운영 규칙

- herdr는 `herdr --session loom ...`. 지시는 파일로 쓰고 경로만 보낸다(프롬프트에 백틱·`$()` 금지).
- 장시간 작업(soak·벤치마크)은 10분 이내. 대형 다운로드·캐시는 프로젝트 밖(`~/.cache`).
- 파일 읽기가 30초 넘게 멈추면 기다리지 말고 STATUS에 `BLOCKED: file io`.
- 권한 확인이 뜨면 작업 범위 안의 무해한 것만 진행하고, 아니면 우회하지 말고 STATUS에 적는다.
- 진행 상황 파일은 worktree가 아니라 **저장소 루트의 `/Users/ctmctm/workspace/Loom/docs/impl/<comp>/STATUS.md`** 를 고친다(커밋하지 않음, 메인이 정리). 첫 줄을 `IN PROGRESS: <작업>` / `MERGE READY: …` / `BLOCKED: …` / `P0 DONE` 으로 갱신.
- 사용량 한도(5시간)에 걸리면 풀린 뒤 이어서 한다. 한도 직전이면 중간 커밋을 남긴다.

## 6. 경로·권한 주의 (2026-11-11 추가)
- **옛 경로 `/Users/ctmctm/Desktop/01_PROJECT/Loom`을 쓰지 말 것.** 링크라서 작업 폴더 밖 읽기로 판정돼 권한 창이 뜬다. 항상 `/Users/ctmctm/workspace/Loom/...`(또는 상대 경로)를 쓴다.
- 작업 폴더 밖 파일(예: `~/.cargo`, 시스템 경로)이 꼭 필요하면 셸 명령으로 읽거나, 필요 없게 설계한다. 권한 창에 막히면 STATUS에 적는다.
- Codex 워커는 승인을 묻지 않는 모드(`-a never`, 쓰기는 작업 폴더 안만)로 돈다. 샌드박스 밖이 필요한 명령은 실패하므로, 그런 작업은 리드가 맡는다.
- **워커는 대화형 질문 창(선택지 질문)을 띄우지 말 것.** 메인은 다른 에이전트의 창에 응답할 수 없다. 결정이 필요하면 계약·계획에 맞는 쪽으로 스스로 정하고 보고서에 적거나, 판단할 수 없으면 보고서에 질문을 쓰고 작업을 끝낸다. 리드는 워커 지시 파일에 이 규칙을 넣는다.
