# TEST P1 보고 — explore-opus: `crates/loom-test-predicate` (2026-11-11)

**완료.** test worktree, 커밋 안 함. 순수 라이브러리(I/O 없음). 이 crate만: fmt OK · `clippy -p loom-test-predicate --all-targets -D warnings` OK · 시험 18개 OK(표 14 = 단언 약 140개, 스파이크 10사례, 무작위 10만 회 + 해시 호환). workspace test는 `crates/loom-test`가 생기기 전 680개 green.
**리드 메모**: 지금 workspace clippy/fmt 빨강은 남의 진행 중 파일 — `crates/loom-test/src/lib.rs`(없는 모듈 7개 선언), `loom-test-format/src/check.rs` fmt diff. 내 crate는 아님. sol 보고서가 짚은 내 `Num::cmp`·fmt 문제는 고쳤다.

## 공개 API
- `ast::Expr::parse(&Value, path) -> Result<Expr, PredError{path, expected, actual, msg}>` · `check(&[PointerSpec], path)` · `eval(&record_json) -> Result<bool, EvalError>` · `to_json()`, `fields()`.
- `pred::Citation::parse(&Value, path)`(계획형 `{filter, pred:{<name>:{…}}}`) · `from_predicate(&Predicate)`(`test.exists` 등) · `check` · `render()`(한국어) · `evaluate(&[PublishedRecord], &Coverage, &mut EvalBudget) -> Check{status, cause, observed, matched(≤64), matched_total, device_evidence, digest, need}` · `Check::verification()`.
- `pred::CommonVerifier{specs}`: CORE `EvidenceVerifier`(`test.exists|absent|count|order|within_ms|hold|measure`).
- `verdict::judge_step(&Proposed, StepKind, &[StepCheck], &[WriteState], RecordHealth) -> Result<StepVerdict, Reject>` · `Reject::code()` · `UserVerdict`(별도 타입) · `aggregate(&[StepState], stopped_by_user) -> SessionResult{outcome, exit_code, recovered_failures, decisive}`.
- `render::expr`, `canonical::{canonical_json, canonical_sha256}`.

## 상한 (`limits.rs`, 넘치면)
| 항목 | 값 | 넘칠 때 |
|---|---|---|
| AST 깊이 / 노드 / `in` 원소 | 32 / 512 / 256 | 파싱 오류(경로 포함) |
| 정규식 길이 / size·DFA limit | 1 KiB / 256 KiB (Rust `regex`) | 파싱 오류 |
| 문자열·bytes 리터럴 / 포인터 | 4 KiB / 512 B | 파싱 오류 |
| 평가 이벤트 | 200k + `EvalBudget`(256개씩 차감) | `Inconclusive{"eval_budget"}` |
| matched REF / observed | 64 / 2 KiB | 개수만 셈 / `{truncated, bytes}` |
| hold 간격 기본 / 시간 상한 | 150 ms / 24 h | — / 파싱 오류 |
| 단계 | 500 | `aggregate` 오류 |

## 결정
1. 리터럴: 숫자·`U64` 10진 문자열 = 숫자(정수는 i128로 정확 비교), `"5 s"` 같은 수량은 필드 `PointerSpec.unit`으로 환산(차원 다르면 오류, 단위 없는 필드에 수량 = 오류). 텍스트 필드와 비교하면 문자열 그대로.
2. 누락 필드·타입 불일치는 `!=`·`not` 안에서도 오류 → `Status::Error`(검증기 결과로는 `Inconclusive{"error: seq=N …"}`).
3. coverage: Positive = exists·count(max 없음), Complete = absent·count(max)·order·hold, Measure = within_ms·measure. Complete/Measure는 `Coverage.complete=false`, 창 안 같은 채널(또는 채널 없는) `source.gap`/`record.gap`, stream epoch 변경 중 하나라도 있으면 Inconclusive. Positive는 일치가 있으면 verified, 없고 불완전이면 Inconclusive.
4. 역할: origins 기본 `[device]`. eth `payload.attributed_origin`은 그 origin으로 본다(선상 자기 프레임). 자기 송신만 맞으면 `Mismatch{agent_tx_only}`, 다른 request_id만 맞으면 `Mismatch{request_id_mismatch}`. 같은 domain·channel(·`payload.target`)의 user 쓰기 행 → `Inconclusive{user_intervention}`(모든 술어).
5. within_ms anchor·measure from은 **모든 origin**의 창 전체에서 찾는다(자극=agent 쓰기가 기준점일 수 있음). 결과 쪽(where/to)은 역할 검사 대상. `tolerance_ms`(LA 시각 오차) 안에 경계가 들면 `clock_uncertainty`.
6. hold = 같은 producer(id·epoch)·generation, 시간 증가, 간격 ≤ max_gap; 끊김은 런 재시작 + 실패 시 Inconclusive(`generation_changed|source_changed|stale_sample`). 스파이크의 이벤트 epoch는 generation으로 옮겼다(stream epoch 변경은 3번 규칙).
7. 판정: pass는 인용 ≥ 1·전부 verified·(일치가 있으면) device 증거 ≥ 1. 부재만 verified인 단계도 pass. mismatch/not_found → `E_EVIDENCE_MISMATCH`, inconclusive/error/근거 없음/manual/agent tx only → `E_EVIDENCE_MISSING`, 기록 불능 → `E_RECORDING_FAILED`, 계획 불일치·계획 밖 skip → `E_PLAN_MISMATCH`, 창 밖 → `E_OUT_OF_WINDOW`. 자극 단계 = write ≥ confirmed면 `done`(pass 제안도 done). 근거 없는 fail = `unsupported:true`.
8. 집계 순위 fail > blocked > incomplete > inconclusive > ok. blocked·user skip·inconclusive → inconclusive(8), 미판정 → incomplete(8), fail → 11, STOP → 6. `on_fail goto`는 복구 단계가 전부 필수·통과일 때만 대체하고 `recovered_failures`에 원 fail을 남긴다.
9. digest: sol 규약(`canonical_json` = 키 정렬·serde_json 직렬화·공백 없음, sha256 소문자 hex64). 일치 = `{kind:"match", citation, matched[EventRef]}`, 그 외 = `{kind:<status>, citation, window, coverage{complete, causes}, cause}`. 함수는 몇 줄이라 복제했고(CORE가 YAML 파서 없이 쓰게), 시험이 `loom-test-format`과 바이트 동일을 확인.

## CORE가 쓰는 방법
1. 계획 승인 때: `Citation::parse` → 도메인이 등록한 `PointerSpec` 전체로 `check`(모르는 필드·타입 = 계획 오류) → `render()`를 승인 화면에.
2. 검사 때: 세션 엔진이 filter로 창 **전체**를 저널에서 다시 읽고 `Coverage`(gap·보존 삭제·스캔 한도 원인)와 함께 `evaluate` → `test.evidence_checked{status, observed, matched_refs, digest}`. 또는 Registry에 `CommonVerifier{specs}`를 `evidence_verifier`로 등록.
3. 판정 때: 단계별 `judge_step`, 세션 끝 `aggregate`의 `exit_code`를 `loom test end`·`run --test`에.

## 요청
- CORE workspace.dependencies: `regex = "=1.13.1"`, `sha2 = "=0.11.1"`(format과 같은 판). 지금은 crate에 exact로 둠.
- 스캔 원인(`Coverage.causes`)에 보존 삭제·`--no-record`·스캔 한도를 넣어 주면 그대로 원인 문자열이 된다.
- 다른 단계의 anchor 이벤트는 창 밖일 수 있으니 엔진이 `anchor:{mono_ns}`로 넘기는 것을 권장.

## 질문
- 없음.
