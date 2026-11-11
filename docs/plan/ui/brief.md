# UI 팀 브리프 (v2.0 신규) — Loom UI 앱

먼저 읽기: `02-core-principle.md`(최우선), `00-master-plan.md`(§2.3 **C6** 중심, D2·D3·D9·A3·A5), `00-brief.md`, `01-lead-playbook.md`, `_xteam/viz-requirements.md`, 각 팀 `plan.md`의 뷰어·TUI·웹 절(특히 `can/plan.md`의 웹 Trace/Graphics/IG·loom.viz, `uart/plan.md`의 터미널 모드, `uds/plan.md`의 뷰, `explore/plan.md`의 LA·DLT·러너 패널).

## 핵심 원칙
Loom은 스킬 기반이지만 **사용자는 UI로 모든 것을 본다.** 에이전트가 장치에 쓰고 읽는 바로 그 데이터가 같은 순간 UI에 보이고, 사용자는 UI에서 직접 송신·입력·승인·STOP으로 개입한다. 에이전트만 보는 숨은 데이터가 없어야 한다. CANoe 수준의 트레이스·그래프와 터미널 수준의 시리얼 화면이 목표 품질.

## 범위
1. 앱 형태: `loomd`가 127.0.0.1로 제공하는 웹 UI + 선택적 데스크톱 셸(Tauri 등). 원격(벤치 PC의 UI를 다른 PC에서 보기) 시 인증.
2. 기술 선택 근거: 프레임워크, 고속 렌더링(트레이스 수만 행/s 가상 스크롤, 플롯 WebGL/uPlot 등), 터미널(xterm.js), 상태 관리, 번들 크기.
3. 공통 셸: 작업 공간 레이아웃(패널 분할·저장), 전역 STOP·비상 OFF, 승인 대기열(R2/R3/R4 — 에이전트가 자기 승인 못 하게 뷰어 전용), env 배지(`vehicle`), 연결·gap·LAG 상태, 기록 탐색·재생, 마커, **테스트 실행 화면**(현재 단계, 증거, 판정 — TEST 기획과 합의).
4. 패널 프레임워크: 도메인 패널 등록 방식, 패널 ↔ 코어 데이터 계약(구독·질의·쓰기). 도메인 팀이 패널 내용 명세를 소유, UI 팀이 프레임워크와 구현 소유.
5. 1차 패널: UART 터미널, CAN Trace/Graphics/IG/신호, UDS 콘솔, LA 레인(캡처 후 표시 — Saleae 제약), DLT, `loom.viz`(데이터만 주면 렌더링).
6. 성능 목표: 장치→화면 p95(UART 100 ms, CAN 200 ms, UDS 100 ms), 고부하 시 드롭은 gap 배지로 정직하게.
7. CORE와의 합의: 고속 데이터 전송 형식(`_xteam/core-ui.md`), 스키마 → TS 타입 생성.
8. M1 = 시뮬레이션 위에서 셸 + 최소 패널(UART·CAN·UDS)로 "에이전트 송신 → 화면 → 사용자 개입" 시연.

## 산출물
`docs/plan/ui/plan.md` (00-brief §4의 15절 형식, 맨 위 핵심 루프 표, 주요 화면 와이어프레임은 ASCII로). 스파이크는 `spikes/ui/`(예: 10k 프레임/s 트레이스 렌더링 측정).
