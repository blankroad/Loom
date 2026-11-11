docs/plan/02-core-principle.md 준수.

# UI M2 선행 — ui-sol: JSON vs `loom.columns.v1` 같은 배치 비교 (Node)

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md`(커밋 안 함, 질문 창 금지, 경로 `/Users/ctmctm/workspace/Loom/...`, `-a never`). 기준 ui/main `728a99e`(당신의 codec 커밋). 파일 범위: `ui/src/codec/**`, `docs/impl/ui/proposals/**`만.

## 왜
당신 측정(10k 행 FD 64 B: decode median 41.8 ms, 펼치기 186.6 ms)은 UI 스파이크의 단순 열 블록(0.02 ms/200행)·JSON(0.065 ms/200행)과 조건이 달라 비교가 안 된다. CORE가 M2에서 binary를 채택할지 판단할 수 있도록 **같은 D3 배치**로 비교한다.

## 할 것 (10분 이내, Node만)
1. 같은 내용의 배치를 두 형식으로: (a) `core.events` JSON 텍스트(실제 D3 필드 전부, 200행/배치, 64 KiB 상한) (b) columns 프레임. 프로파일: CAN Classic 8 B, CAN FD 64 B, UART 청크 1 KiB.
2. 측정(각 25회 이상, median·p95, GC 영향 명시): 와이어 바이트/행, `JSON.parse` + 패널이 실제로 읽는 필드 접근(seq·ts·id·len·bytes·origin·request_id) vs columns `decode` + 같은 필드 getter, 그리고 둘 다 "D3 레코드로 펼치기" 비용. 프레임 수(64 KiB 제한 때문에 생기는 배치 수 차이 포함).
3. columns 디코더의 비용이 어디서 오는지(검증·사전·sidecar 등) 프로파일 1회 — 줄일 수 있는 부분 제안(형식 변경이 필요하면 제안서에 대안으로).
4. 결과를 `docs/impl/ui/proposals/loom-columns-v1-vs-json.md`에 표로 + 결론("columns가 이 조건에서 이득/무이득, 이유"). 브라우저·실데몬 수치가 아님을 명시.

## 보고
`docs/impl/ui/reports/p2-columns-bench-sol.md`. 끝나면 "완료: <경로>"만.
