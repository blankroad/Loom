# TEST P1 보고 — explore-opus: predicate ↔ 세션 `VerifierEngine` 연결 (2026-11-11)

**완료.** test worktree, 커밋 안 함. 관문 3개 green: `cargo fmt --all --check` · `clippy --workspace --all-targets -D warnings` · `test --workspace` 718 통과(2 ignored, 실패 0 — sol의 기존 loom-test 29개 포함). `Cargo.lock`은 loom-test → loom-test-predicate 내부 의존 1줄만.

## 바꾼 것
- `loom-test/src/wire.rs` **`PredicateEngine: VerifierEngine`**. `TestModule::new()` 기본값이 이것(공통 술어만). CORE는 `PredicateEngine::from_registries(&[&Registry…])`(도메인 `EvidenceVerifier` + 모든 `PointerSpec`)로 `with_services`에 넣는다. `NotWired`는 시험용으로 남김.
- `check`: 등록된 plan 단계의 **모든 expect**를 판정(에이전트 인용과 무관).
  1. 창 = 시작 fence `captured_mono_ns` ~ 끝 fence(없으면 지금).
  2. anchor는 따로 읽음(최근 30분, kind·`test_step`·where 일치 중 창 끝 이전 마지막) → `mono_ns`. 창 ∩ `[anchor, anchor+within_ms]`.
  3. 채널 전체를 시도 시작 1 s 전부터 재조회(64 MiB·2 s·200k, 1만 건씩). gap 이벤트는 같은 채널 스캔에 들어옴(P1-10).
  4. `Coverage{complete = 스캔 완결 && 창 안 gap 없음, causes}` → 공통 술어는 `Citation::from_plan(...).check(specs).evaluate`, 도메인 술어는 Registry 검증기(`Predicate{name, args+filter+ch}`).
  5. 에이전트 REF: 재조회에 없는 seq → `ref_not_in_journal`(mismatch), 창 밖 → `out_of_window`.
  - 결과 `EvidenceCheck`: 상태·witness(device/complete_absence/measurement/own_tx_only)·`matched_refs`(RefString)·digest·**`observed.pinned:false`**(2 KiB 안).
  - 자극 단계: 인용한 쓰기 REF 행의 상태 ≥ confirmed → `stimulus_confirmed`.
- `decide`: `verdict::judge_step`(manual/stimulus/assert 구분). 거절 코드는 predicate `Reject::code()`. viewer의 user_assertion·skip은 그대로 수락.
- 세션 끝 `aggregate`: 엔진의 계획 경로 걷기(`when`·`on_fail goto`·순환)는 유지하고 판정 규칙은 `loom_test_predicate::verdict::aggregate`로 일원화 → 0/11/8(STOP·abort 6은 엔진 그대로).
- predicate crate: `Citation::from_plan`(계획형 `{op: <name>, …}`), `is_common`, `plan_pred_name` 추가.
- `CheckCtx.now_ns`(모듈 시계) 추가 — 시도 시작이 얼마나 전인지 계산해 상대 커서로 스캔하려고.

## 결정
1. 창 소속은 **fence mono와 레코드 `ts.mono_ns`**로만 본다. `host_started_ns`는 모듈 가동 시간이라 레코드와 비교 불가 → 시작 fence가 없으면 `uncertain{window_unknown}`(PASS 불가).
2. 지어낸 REF는 실제 증거가 창에 있어도 거절(`E_EVIDENCE_MISMATCH`, 사유 `ref_not_in_journal`). 정직한 에이전트는 결과 REF를 복사만 한다.
3. 도메인 술어에도 코어 규칙 적용: 창 안 같은 domain의 user 쓰기 → `uncertain{user_intervention}`. 도메인이 창 밖 레코드를 matched로 내면 `out_of_window`.
4. `repeat` > 1은 반복별 창(fence)이 없어 한 창 = 1회 → 엔진이 "not all repeat iterations"로 거절(R1 이후).
5. gap 이벤트는 domain 필터 없이 채널로 스캔한다(현재 worker-sdk gap domain = `source`). 같은 채널 이름을 다른 도메인이 써도 gap은 보수적으로 inconclusive.

## 시험 (`loom-test/tests/wire.rs`, 9개: 가짜 JournalReader·UART/UDS 가짜 검증기·시계 fence)
- TC-1 S1 done(psu 쓰기 confirmed) → S2 `uart.text_contains`(S1 anchor, 30 s) → S3 PowerMode `exists`(S2 `uart.expect_result` anchor, 5 s) → S4 `uds.dtc_absent` → end **0**, 모든 검사 digest·`pinned:false`.
- 지어낸 seq(증거 없음/있음 둘 다) 거절 · 5 s 넘은 신호 `E_OUT_OF_WINDOW` · 창 안 `source.gap` → uncertain, PASS 거절, inconclusive 수락 → **8** · 자기 송신만 → `own_tx_only` 거절 · 스캔 불완결 → uncertain · DTC 있음 → PASS 거절·FAIL 근거 있음 → 복구 S5도 fail → **11** · 복구 S5 pass → **0** · fence 없음 → PASS 불가.

## CORE R1 전 제한 (흉내 내지 않음)
- `CallCtx::wait` 없음(검사는 이미 기록된 창만 봄), **pin 없음 → verified여도 보존 보호 안 됨**(`pinned:false`), worker fence는 CORE `SessionHooks::fence` 몫 — 없으면 PASS 불가.
- R1 요청: `CallCtx::now_mono_ns()`(+ mono 시각 커서)로 상대 커서 대신 fence 시각부터 정확히 스캔, 단계 창 pin API, `proto::canonical`로 digest 이전.

## 질문
- 없음.
