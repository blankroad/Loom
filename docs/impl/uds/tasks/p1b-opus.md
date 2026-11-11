docs/plan/02-core-principle.md 준수

# UDS P1-b — uds-opus (명령 핸들러를 runner·sim·CallCtx에 연결, plan U-07·U-08 + `connect --sim`)

공통 규칙은 `docs/impl/uds/tasks/p0-common.md` 그대로(worktree, 질문 금지, 커밋 금지, 빌드, 보고). P0 다섯 crate는 커밋됐다(5da92f0, fb28cf4). 이번에는 `crates/loom-uds`만 고친다. `loom-uds-doip`·`loom-uds-runner`는 읽기 전용 — 고칠 점은 보고서 "runner/doip 변경 요청"으로.

먼저 읽기: 보고서 `docs/impl/uds/reports/p0-sol-loom-uds-doip.md`·`p0-sol-loom-uds-runner.md`(특히 "통합 시 교체" 목록), `crates/loom-domain-api/src/ctx.rs`(CallCtx·CtxServices·EventDraft·Handler), `crates/loom-echo`(핸들러 견본), 계약 02·04·08.

## 할 일
1. **연결 관리자**(gateway 안, `ModuleRuntime::Dedicated`): 대상별 `TargetSession` + `Runner`. `uds.connect{sim:true}` = 데몬 안에서 `DoipEntity`(127.0.0.1 임의 포트) + `SimNetwork` 어댑터(엔티티 핸들러 trait 구현 — sim·doip를 잇는 코드는 여기)를 띄우고, 실제 DoIP 클라이언트로 접속한다(sim도 같은 전송 경로 — 화면·지연이 실HW와 같게). 실HW `uds.connect{host, port, la, tester_la, activation}`도 같은 경로. 대상 수 상한(C2).
2. **runner 통합 교체 목록** 처리: `BoxFuture` → `loom_domain_api::BoxFuture`로 맞추는 것은 runner 쪽이라 요청만, `MonoClock` 원점을 worker-sdk `Clock`(또는 CORE가 주는 mono)과 맞춤, 재연결 시 `ClientConfig.generation` 증가 + `Runner::replace_transport`, `RunnerObserver` → **상한 있는 큐**로 이벤트 변환.
3. **이벤트 발행**(E7 publish-before-reply): `uds.tx`(송신 순간), `uds.rx`(0x78 포함 각 응답, 늦은 응답 `late:true`), `uds.result`(같은 `request_id`, `write_state` 단계 갱신), `uds.state`(TargetSession 변할 때). 결과 응답은 해당 이벤트가 `ctx.publish` 된 뒤 돌려준다. 페이로드는 P1-a에서 맞춘 타입·fixture 그대로.
4. **핸들러**: connect/status/read-did/write-did(+ 되읽기 → `effective_verified`/`effect_unverified`)/session/dtc list/request(raw)/abort/trace(저널 질의 `ctx.journal()`). 오류 코드는 runner의 매핑을 그대로 내보낸다. `others_since_last`(마지막 호출 뒤 다른 origin의 쓰기 수)는 결과 `data`에.
5. **lease 최소판**(plan U-08): 대상별 소유자(origin·actor·client). 사용자(user origin)가 쓰면 lease가 user로 넘어가고, 그동안 agent 쓰기는 `E_HELD_BY_USER`(exit 6), 사용자가 반환하면 다시 허용. STOP latch 중 agent·script 쓰기는 코어가 막는다(UDS는 latch 상태를 다시 판정하지 않음).
6. **`presend_check`**: quarantine(runner 상태 조회), lease, generation 변경 → `PresendError`. ≤ 1 ms, I/O 없음 — 공유 상태는 lock-free 스냅샷이나 짧은 Mutex.
7. **policy_section 스키마**(`[uds]` 최소: `auto_reconnect`, 대상 목록, `policy` 표 키 형식 `"0x31:*"`) 등록. 승인 켬 경로(F1 기본 꺼짐)는 구현하지 않는다.

## 시험 (gateway 없이)
- 가짜 `CtxServices`(테스트 안에서 구현 — 발행 이벤트를 상한 있는 Vec에 모음)로 핸들러를 직접 호출: connect --sim → read F190 → write F1A0 + 되읽기 effective_verified → NRC → 0x78 → timeout → 늦은 응답 → user write 후 agent `E_HELD_BY_USER` → 반환 후 성공 → 재연결 generation+1. 이벤트 순서(`uds.tx` < `uds.rx` < `uds.result`)와 publish-before-reply 순서를 단언.
- 응답 → `uds.result` 발행까지 지연 p95를 시험 출력으로(루프백, 1,000회, 1분 이내).

## 보고
`/Users/ctmctm/workspace/Loom/docs/impl/uds/reports/p1b-opus.md`(40줄 이하): 구조, 상한 표, 시험 결과, 지연, CORE gateway에 붙일 때 필요한 것(계약 차이), runner/doip 변경 요청, 결정·질문. 끝나면 멈춘다.
