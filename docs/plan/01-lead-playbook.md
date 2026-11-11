# 리드(서브 오케스트레이터) 플레이북

당신은 한 컴포넌트의 리드다. 직접 계획서를 처음부터 쓰지 말고, 워커 두 명을 돌려 초안을 받고 **비교·검증·통합**하는 데 집중한다.

먼저 읽을 것: **`docs/plan/02-core-principle.md` (최우선)**, `docs/plan/00-brief.md`, `docs/plan/<comp>/brief.md`, 그리고 `herdr --skill` 출력.

## 환경

- 이미 같은 탭에 워커 두 명이 떠 있다 (메인이 만들어 둠):
  - `<comp>-opus` — Claude Opus 5.5
  - `<comp>-sol` — Codex Sol 6.1
- 확인: `herdr agent list` → 이름과 상태가 보여야 한다.
- 워커를 새로 만들거나 종료하지 말 것. 다른 워크스페이스의 에이전트를 건드리지 말 것.

## herdr 사용 주의 (실전에서 겪은 문제들)

- 프롬프트는 짧게: `herdr agent prompt <name> "Read docs/plan/<comp>/tasks/r1-opus.md and follow it."` 처럼 **지시는 파일로 쓰고 경로만 보낸다**.
- 큰따옴표 프롬프트 안에 백틱이나 `$(...)` 절대 금지 (zsh가 실행해 버림).
- 기다릴 때는 Bash를 **백그라운드**로: `herdr agent wait <name> --timeout 1800000` 또는 `herdr agent prompt <name> "..." --wait --timeout 1800000` 를 run_in_background 로 실행하고 완료 알림을 받는다. 두 워커는 동시에 돌린다.
- 프롬프트 후 상태가 `working`으로 바뀌었는지 `herdr agent get <name>`으로 확인. 안 바뀌었으면 `herdr agent read <name> --source visible`로 화면을 보고 처리.
- `blocked`이면 화면을 읽고 판단. 계획 작업 범위 안의 무해한 승인(파일 쓰기, 웹 검색, 스파이크 venv 설치)이면 승인해도 된다. 그 외(사용자 설정 변경, 전역 설치, 삭제)는 거절하고 대안 지시.
- 워커 응답 본문은 화면 대신 **파일**로 받는다 (아래 경로 규칙).

## 라운드

**R1 — 독립 초안 (병렬)**
- `docs/plan/<comp>/tasks/r1-opus.md`, `r1-sol.md`에 지시를 쓰고 동시에 보낸다.
- 두 워커 모두 `00-brief.md` §4의 15개 절 전체 초안을 쓴다. 관점을 나눠 다양성을 확보:
  - Opus: 아키텍처, 사용자/에이전트 UX, 스킬 설계, 안전 규칙에 무게.
  - Sol: 라이브러리·프로토콜·드라이버 조사(실제 문서/소스 확인), 실현 가능성, 타이밍/성능, 테스트·시뮬레이션에 무게. 필요하면 `spikes/<comp>/`에서 짧은 검증.
- 출력: `docs/plan/<comp>/r1-opus.md`, `r1-sol.md`.

**R2 — 교차 리뷰 (병렬)**
- 각 워커가 상대 초안을 읽고 `r2-opus-review.md`, `r2-sol-review.md` 작성: 틀린 사실, 빠진 것, 더 나은 선택, 합의 가능한 부분.

**R3 — 리드 통합**
- 당신이 `plan.md`를 쓴다. 두 초안과 리뷰에서 근거가 강한 쪽을 고르고, 남은 의견 차이는 "결정 기록" 표에 이유와 함께 남긴다.
- 사실 확인이 필요한 쟁점은 워커에게 짧은 확인 작업을 보내도 된다.

**R4 — 최종 점검 (선택, 권장)**
- Sol(또는 Opus)에게 `plan.md`를 `00-brief.md` §4 체크리스트 기준으로 점검시켜 `r4-check.md`로 받고, 반영.

## 타팀 조율

- CAN ↔ UDS: CAN-TP 경계(브리프 §3-8)를 `docs/plan/_xteam/can-uds-isotp.md`에 합의. 먼저 쓰는 쪽이 초안, 다른 쪽이 `## <comp> 의견` 절을 덧붙인다. 상대 팀 폴더의 진행 중 초안을 읽어도 된다.
- UART/UDS → CAN(viz): viz 요구사항은 `docs/plan/_xteam/viz-requirements.md`에 `## <comp>` 절로 추가.
- 상대 리드에게 직접 herdr 프롬프트를 보내지 말 것 (작업 중인 에이전트 입력을 망가뜨릴 수 있음). 파일로만 조율.

## 완료

- `docs/plan/<comp>/STATUS.md`에 첫 줄 `DONE`, 다음 줄부터 5~10줄 요약 (핵심 결정, 사용자 질문, 공통 계약 변경 요청).
- 그 다음 워커들에게 추가 작업을 보내지 말고 대기. 메인이 통합 후 후속 질문을 보낼 수 있다.
- 진행 상황은 `STATUS.md`에 `IN PROGRESS: R2 리뷰 중` 처럼 라운드마다 갱신.
