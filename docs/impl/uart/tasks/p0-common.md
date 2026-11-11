docs/plan/02-core-principle.md 준수.

# UART P0 공통 규칙 (2026-11-11, uart-lead)

먼저 읽기: `docs/impl/00-impl-playbook.md`, `docs/contract/README.md`·`02-events.md`·`05-worker-protocol.md`, UART 계획 `docs/plan/uart/plan.md`(§3.5–3.7, §3.13 큐 표, §5.4–5.5 정제·자동 응답, §7.2–7.5, §9.1–9.3). 마스터 §2.5 F1(기본 모두 자동)·§2.4 E 판정이 계획보다 우선.

## 작업 공간
- 코드는 **worktree `/Users/ctmctm/workspace/Loom/.worktrees/uart`(브랜치 `uart/main`)에서만**. 저장소 루트 체크아웃은 고치지 않는다.
- 빌드 캐시 충돌을 피하려고 각자 `CARGO_TARGET_DIR=$HOME/.cache/loom-target/<이름>`을 쓴다(프로젝트 밖).
- **배정된 파일만 수정**한다. `Cargo.toml`(crate 것 포함)·`lib.rs`의 모듈 목록은 리드 소유 — 의존성·모듈 추가가 필요하면 보고서에 적는다. 루트 `Cargo.toml`·CORE crate·`ui/`·다른 팀 crate는 읽기만.
- 커밋하지 않는다(리드가 diff 검토 후 커밋). `git stash` 금지.

## 품질 관문 (보고 전에 모두 녹색)
```
cargo fmt -p <crate> -- --check
cargo clippy -p <crate> --all-targets -- -D warnings
cargo test -p <crate>
```
- edition 2024, `unsafe` 금지(workspace lint), `unwrap()` 금지(테스트 코드는 `expect("이유")`), 공개 항목 문서 주석.
- C2: **무한 큐·무한 버퍼 금지.** 모든 큐·버퍼에 바이트·건수 상한과 넘칠 때 정책을 코드 상수로 두고, 상수 옆 주석에 plan 절(예: `§3.13`)을 단다.
- 실HW 없이 통과해야 한다(pty·sim). 시험이 sleep에 기대면 상한 2 s 이내, 전체 `cargo test -p` 60 s 이내.
- 장치 출력·입력은 **바이트**로 다룬다. 원시 바이트와 offset은 절대 바꾸지 않는다(표시용 변환만).

## 보고
- 끝나면 `docs/impl/uart/reports/<작업 이름>.md`(저장소 루트 경로)에: 만든 공개 API 요약, 상수 표(상한·정책), 시험 목록과 결과(명령 출력 끝 몇 줄), 계획과 다르게 한 점과 이유, 리드에게 묻는 것. 첫 줄 `DONE` 또는 `BLOCKED: 이유`.
- 막히면 우회하지 말고 보고서에 `BLOCKED`로 쓴다. 권한 확인은 작업 범위 안의 무해한 것만 진행.
- **대화형 질문 창(선택지 질문, AskUserQuestion 등)을 띄우지 않는다**(playbook §6). 결정이 필요하면 계약·계획에 맞는 쪽으로 스스로 정하고 보고서 "계획과 다르게 한 점"에 적는다. 판단할 수 없으면 보고서 "리드에게 묻는 것"에 질문을 쓰고 작업을 끝낸다.
- 경로는 항상 `/Users/ctmctm/workspace/Loom/...`(또는 상대 경로). 옛 `~/Desktop/01_PROJECT/Loom` 경로 금지(playbook §6).
