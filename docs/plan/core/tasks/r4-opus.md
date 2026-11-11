docs/plan/02-core-principle.md 준수

# CORE R4 — 최종 점검 (core-opus, 30분 이내 목표)

리드가 통합한 `docs/plan/core/plan.md`를 점검해 `docs/plan/core/r4-check.md`를 쓴다. 한국어. **plan.md는 수정하지 않는다** — 리드가 반영한다. 경로는 `/Users/ctmctm/workspace/Loom`. 스파이크는 하지 않는다(필요하면 질문으로만).

## 점검 기준 (각 항목 통과 / 부분 / 실패 + 위치 + 고칠 문장 제안)
1. `00-brief.md` §4의 15절이 모두 있고 내용이 있는가.
2. `02-core-principle.md` 체크리스트 6개.
3. `core/brief.md` 범위 10항목 각각에 결정·근거가 있는가.
4. 두 R2 리뷰(`r2-sol-review.md`, `r2-opus-review.md`)의 P0/P1 지적 — Sol F1–F15·O-1–O-5, Opus E1–E10·S1–S18 — 이 plan.md에 반영됐는지 하나씩 표로(반영 / 의도적 미반영(이유 있음) / 누락).
5. **내부 모순**: 수치(큐 상한·예산 장부 합·지연 목표·M1 수용 기준), 이름(RPC·이벤트·오류 코드·종료 코드 묶음), 역할(web viewer / agent 권한), 단계(M0/M1/M2 배치)가 절 사이에서 일치하는가. 특히 §9.2 큐 표 ↔ §9.3 gateway 장부 ↔ §0.2 홉 표.
6. 타팀 요청 반영표(§15.2)가 각 `_xteam/*core*.md`, `core-ui.md`, `core-test.md`, `test-domains.md`, `ui-test.md`의 현재 내용과 맞는가(파일이 R1 이후 갱신됐을 수 있음).
7. 마스터 §2 D/A/B/C(C4.1 포함) 위반이 있는가 — 있으면 CR(§15.1)로 올라가 있는가.
8. P0 첫 주 계획이 도메인 팀 관점에서 바로 시작 가능한가(무엇을 언제 import할 수 있는지 명확한가).

## 출력 형식
- 맨 위 판정 한 줄: `PASS` / `PASS with fixes` / `FAIL`.
- 수정 필요 목록(우선순위 P0/P1/P2, 위치, 현재 문장 요지, 제안 문장). 최대 40행.
- 위 1–8 점검 표.
- 끝나면 화면에 파일 경로와 3줄 요약만. 다른 에이전트에게 메시지 금지.
