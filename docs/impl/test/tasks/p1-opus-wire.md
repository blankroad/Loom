# TEST P1 — explore-opus: predicate를 세션 모듈 `VerifierEngine`에 연결

먼저 `p1-common.md`와 다음을 읽는다:
- 리드 커밋 `1b48995`(세 crate)·`6c39f77`(main 병합)
- 보고서 `docs/impl/test/reports/p1-sol-session.md`·`p1-opus-predicate.md`
- CORE 답변 `docs/impl/requests/core-answers-p1.md`(TEST 경계 동의, R1 예정 API)

배정 범위: `crates/loom-test/`(세션 모듈 — 이번에는 너만 고친다, explore-sol은 손대지 않음)와 `crates/loom-test-predicate/`.

## 할 일
1. **`PredicateEngine: VerifierEngine`**(loom-test 안). 기본값 `NotWired`는 시험용으로만 남긴다.
   - `check`:
     - 단계 `expect` 인용 → `Citation::parse` → 등록된 `PointerSpec`(Registry 이벤트 kind 표)로 `check`
     - 창 계산(anchor + `within_ms`, 재시도마다 attempt 창)
     - **`ctx.journal().scan`으로 filter·창 전체 재조회**(상한: 스캔 64 MiB·2 s·200k 이벤트)
     - `Coverage` 구성: 결과 `complete`, 창 안 gap 이벤트, 스캔 한도. 계약 07·CORE 답변 P1-10에 따라 gap 이벤트는 영향받은 채널로 오므로 같은 스캔에 포함된다.
     - → `evaluate` → `test.evidence_checked`
   - 도메인 의미 술어(`uart.text_contains`, `eth.match`, `psu.current_lt` …)는 Registry의 도메인 `EvidenceVerifier`로 위임한다. 공통 술어(`test.*`)는 `CommonVerifier`.
   - `decide`: `judge_step`. 세션 끝에는 `aggregate` → 종료 코드.
   - 에이전트가 낸 REF는 witness 후보일 뿐이다. 판정은 재조회 결과로만 한다.
2. **anchor**: 다른 단계 이벤트를 anchor로 쓰면 창 밖일 수 있다. 엔진이 anchor 이벤트를 따로 읽어 `mono_ns`로 넘긴다(네 보고서 권장).
3. **CORE R1 미도착 부분**:
   - `CallCtx::wait`, pin, worker fence는 아직 없다. 흉내 내지 말고 TODO(R1)와 명시 제한으로 둔다.
   - pin이 없으므로 verified여도 보존 보호 안 됨 → 결과 `pinned:false`를 표시한다.
4. **시험**(가짜 JournalReader + 가짜 도메인 verifier):
   - TC-1 S2(uart login 30 s 창)·S3(PowerMode within 5 s)·S4(`uds.dtc_absent`) 흐름
   - 지어낸 seq로 PASS 시도 → 거절
   - 창 밖 → `E_OUT_OF_WINDOW`
   - gap → inconclusive
   - 자기 송신만 → 거절
   - 스캔 한도 → inconclusive
   - 위 결과가 `aggregate`로 0/11/8 종료 코드까지 이어지는지
5. 관문(`p1-common.md` 3개). 보고서 `/Users/ctmctm/workspace/Loom/docs/impl/test/reports/p1-opus-wire.md`(40줄 이하). 커밋하지 않는다.
