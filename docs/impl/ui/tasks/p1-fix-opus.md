docs/plan/02-core-principle.md 준수.

# UI P1 M1 — ui-opus: 수용 시험이 찾은 결함 수정 (U-1–U-5 + 실패 시험)

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md` 그대로(커밋 안 함, **질문 창 금지**, 경로 `/Users/ctmctm/workspace/Loom/...`). 기준 ui/main `3e0f2ab`(main 병합 + WIP d58fcd4: M1 수용 스위트 `ui/e2e/m1/**`, 설정 `ui/e2e/m1.config.ts`).

근거: `docs/impl/ui/reports/p1-m1-accept.md` §5(U-1–U-5)와 §3(결과). 수용 스위트 실행: `cd ui && pnpm e2e:mock --workers=1`(약 4분) / 하나만: `pnpm exec playwright test -c e2e/m1.config.ts --project=mock e2e/m1/<file>`.

## 고칠 것
1. **U-1 (실제 결함)**: `.vt-row{display:grid}`가 `[hidden]`을 덮어 VirtualTable이 숨긴 행이 보인다 → eth 목록 배지 "2행"인데 6행 표시, UDS 한 요청이 옛 단계 행과 두 줄. 수정 + 회귀 시험. 실패 시험: `e2e/m1/eth.spec.ts` 수용 2.
2. **T08 R2 카드 실패**: `e2e/m1/t08-rights.spec.ts:35` "policy on → approval card → approve → the write runs"가 실패(`toHaveCount`). 원인을 찾아 UI 쪽이면 고치고, 시험·mock 쪽이면 보고서에 근거와 함께(시험 파일은 수정 금지 — 리드가 반영).
3. **U-2**: UART 상태줄에 마지막 송신의 `data-request-id`(UART 상태줄 관련 시험이 실행마다 흔들림 — 같이 확인).
4. **U-3**: STALLED 중 CAN 송신 입력칸 비활성(쓰기 잠금과 일치).
5. **U-4**: eth 목록이 STOP 패널 등 레이아웃 변화로 생긴 스크롤을 사용자 스크롤백으로 오인해 완전 모드로 바뀜 → 사용자 입력(휠·키·드래그)으로 생긴 스크롤만 스크롤백으로.
6. **U-5 결정**: CORE fixture-replay 연결에서 자동화 브라우저(webdriver) 강등을 예외로 할지. 계약·V5 기준으로 정하고 보고서에 "결정: …(근거)". 기본안: 실제 데몬과 같은 규칙(강등 유지), 시험은 replay에서 사용자 쓰기를 mock으로만 검증.

끝나면 `pnpm check` + `pnpm e2e:mock --workers=1` 전체 결과를 보고서에.

## 파일 범위
`ui/src/**`(net·proto 제외). **수정 금지**: `ui/src/net/**`, `ui/src/proto/**`, `ui/tools/**`, `ui/e2e/**`, `ui/fixtures/**`, 리드 소유 설정, Rust crate.

## 보고
`docs/impl/ui/reports/p1-fix-opus.md`. 끝나면 "완료: <경로>"만.

## 추가 (리드, 같은 작업에 포함)
7. **C-6**: 셸 `core.activity` 어댑터를 **schema/v1 정본**(`src/proto/gen/activity_payload.ts`: `{client_id, method, summary, range{from,to}, waiting?}`)으로 읽게 고친다. CAN 답(`action:"read"`, `range{seq_a,seq_b}`)과 기존 가정(`actor`, `verb`)도 계속 받되 정본 우선.
8. **C-8**: `core.safe_state.list` 결과는 replay가 **맨 배열** `[{id, description, blocked:bool, method, …}]`, run 결과 `{items[{id, outcome:'sent'|'skipped'|'failed', finally_blocked, reason, request_id}]}`(생성 타입 `safe_state_item.ts`·`safe_state_run_result.ts`). 어댑터를 생성 타입 기준으로, 기존 `{items}`·`results[{ok}]` 모양도 받게. `kind`(recover/emergency_off)가 없으면 비상 OFF 경로는 `method`로 판별하거나 "경로 없음" 표시.
근거: `docs/impl/ui/reports/p1-m1-accept.md` §4.
