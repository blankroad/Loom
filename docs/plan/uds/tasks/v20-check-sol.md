docs/plan/02-core-principle.md 준수

# v2.0 점검 — uds-sol (plan.md 수정 금지, 보고서만, 짧게)

리드가 `docs/plan/uds/plan.md`를 v2.0으로 개정했다(마스터 `00-master-plan.md` §2.3 C1–C8, 변경 목록 §16.3, 적용 규칙 §0.1). 반영 전 원본: `/private/tmp/claude-501/-Users-ctmctm-Desktop-01-PROJECT-Loom/1a6cf52f-186a-4b7e-9e7c-f3cbd6fc304f/scratchpad/plan-pre-v20.md`. 근거: `v2-sol.md`(네 보고서), `v2-opus.md`, `_xteam/uds-ui.md`, `_xteam/uds-core.md`, `_xteam/test-domains.md` `## uds 의견`.

확인할 것:
1. 02 체크리스트 6개 항목이 **웹 UI 기준**으로 성립하는가(§0, §5.0, §11 M1, §3.8).
2. v2.0 절(§0.1, §3.1, §3.7, §3.8, §5.0, §6.6, §7.0, §8.2, §10.8, §10.10, §11, §12, D26–D32)과 남은 v1 본문 사이에 **구현자를 오도할 모순**이 있는가(예: 아직 Python·TUI를 1차로 읽히게 하는 문장, M2c 잔재, 종료 코드 충돌, `canoe` 기본 경로, vehicle 규칙). §0.1 표가 대체한다고 선언한 v1 표현은 모순으로 세지 않는다.
3. 네 보고서(`v2-sol.md`)의 사실·수치가 plan에 정확히 옮겨졌는가(crate 판단, VN5640 경로, seed-key ABI, C2 표).
4. 새로 생긴 결함(예: auto_reconnect와 quarantine U 해제, 패턴 범위 승인과 approval_binding, check exit 3).

출력: `docs/plan/uds/v20-check.md`, 40줄 이하, 한국어. 첫 줄 `V20 PASS` 또는 `V20 FIX`, 이어서 P1(반드시 고칠 것)·P2(권장) 목록, 각 항목에 plan 절 번호와 고칠 문장 제안. 끝나면 멈춘다.
