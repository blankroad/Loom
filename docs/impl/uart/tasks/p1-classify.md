docs/plan/02-core-principle.md 준수.
질문 창 금지: 대화형 질문을 띄우지 말고, 스스로 정해 보고서에 적거나 보고서에 질문을 쓰고 끝낸다(playbook §6, p0-common.md).

# P1 uart-sol: UART 위험 분류 규칙 (T10 일부)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. **단, Codex 무승인 모드는 작업 폴더 밖에 쓸 수 없으므로 `CARGO_TARGET_DIR`를 지정하지 말고 worktree 기본 `target/`(gitignore됨)을 쓴다.** 경로는 항상 `/Users/ctmctm/workspace/Loom/...`(옛 Desktop 경로 금지 — playbook §6).
보고서: `/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-classify.md`.

## 수정 가능한 파일 (이것만)
- `crates/loom-uart/src/classify.rs` (하위 모듈 `src/classify/` 허용)
- `crates/loom-uart/tests/classify_*.rs`

`hooks.rs`·`module.rs`·`commands.rs`·`kinds.rs`는 리드 소유(읽기만). 지금 `classify.rs`에는 인터페이스(`Verdict`, `Rule`, `classify_text`, `warm`, `BASE_RULES`)와 기본 규칙 6개가 있다. **공개 시그니처는 유지**하고(리드의 `hooks.rs`·`module.rs`가 쓴다), 확장은 함수 추가로.

## 할 일 (plan §10.1–10.2, F1)
F1: 기본 정책은 R0–R4 모두 자동 실행이다. 분류는 **화면·감사에 등급을 표시**하고, 사용자가 `loom.toml`에서 규칙 id별로 confirm/approve/deny를 켤 수 있게 하는 근거다. 막는 코드는 여기 없다.
1. **프로필별 규칙 묶음**: `generic`(기본), `linux`, `uboot`, `esp-idf`, `zephyr`. 새 함수 `classify_text_with(profile: &str, line_state: &[u8], bytes: &[u8]) -> Verdict` — 모르는 프로필은 `generic`. plan §10.1 표의 예를 모두 덮고, 각 규칙에 id(`<profile>.<name>`) 고정. R4 `default_blocked` 후보(efuse/OTP/fuse·보안부트 키·락비트)도 등급만 R4로(F1: 기본 차단 없음).
2. **줄 재조립**: 직전 미완 줄(`line_state`, 호출자가 넘김 — P1 리드가 worker 미러에서 공급) + 현재 payload를 이어 붙여 줄 단위 판정. 줄 사이 BS(0x08/0x7F)·^U(0x15) 편집을 적용한 결과로 판정(`sav\x7f\x7fsaveenv`류 우회 방지 — 오탐 쪽으로 기운다). 줄당 상한 4 KiB, 전체 32 KiB(넘으면 앞부분만 판정 + reason에 `truncated`).
3. **hex/바이너리**: 비출력 비율이 높아도 텍스트로 디코딩해 판정(latin-1 매핑). 
4. **성능**: 규칙은 프로필별 `RegexSet` 한 번 컴파일(`warm()`이 전부 미리 컴파일). 32 KiB 입력에서 `classify_text_with` p99 < 1 ms(release가 아닌 test 빌드 기준 수치도 보고서에). 훅 예산 1 ms(contract 08 §8.2).
5. **시험**: 규칙마다 양성·음성 예 1개 이상, 편집 키 우회, 줄 재조립(앞 호출에서 `sf er` + 이번 `ase 0 100\r`), 대소문자·공백 변형, 오탐 허용 목록(예: `echo "reboot later"`는 R1이어야 하는가 — 보고서에 판단 근거), proptest(임의 바이트에서 panic 없음, 같은 줄을 몇 조각으로 나눠도 등급 동일).
6. 보고서에 규칙 표(id·등급·패턴·예)를 넣는다 — 리드가 `help --json`과 스킬 references에 옮긴다.

## 끝나면
`cargo fmt -p loom-uart -- --check`, `cargo clippy -p loom-uart --all-targets -- -D warnings`, `cargo test -p loom-uart` 녹색 확인 후 보고서 작성, 대기. 커밋 금지.
