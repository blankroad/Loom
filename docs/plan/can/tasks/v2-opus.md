# can-opus v2.0 작업 — 구조·UI 패널 명세·통합 테스트·스킬

먼저 `docs/plan/can/tasks/v2-common.md`를 읽고 따른다. 출력: **`docs/plan/can/v2-opus.md`** (한국어). 리드가 이 파일에서 `plan.md` 개정 절과 `_xteam/can-ui.md`를 만든다.

## 써야 할 것
1. **핵심 루프 표(§0) v2판**: 4요소(직접 접근·읽기·쓰기·사용자 실시간 표시+개입) × (에이전트 명령 / Rust 구성요소 / **UI 앱 패널** / 첫 마일스톤).
2. **구조(§3) v2판**: `loomd`(Rust, CORE 소유) 안의 CAN gateway 모듈(데이터 질의: trace 필터·Fixed·디코더·SignalCache·Graphics용 시계열 질의) vs CAN worker 프로세스(Rust: 드라이버·링·저널 writer·TX 중재·주기 스케줄러·ISO-TP·SimNode). 벤더별 worker 분리안(`can-vxl`, `can-pcan`, `can-socketcan`, `can-sim`?)의 장단 — C2 크래시 격리 관점. Python 어댑터 worker(CANoe, 선택 P4)의 위치. 프로세스 구성도(ASCII).
3. **UI 패널 명세 (C6) — `_xteam/can-ui.md` 초안 재료.** 패널별로: 목적, 보이는 열·요소, 데이터 소스(구독 이벤트 kind / 질의 RPC), **지연 목표**(장치→화면 p95), 고부하 동작(생략·gap 배지·다운샘플 표시), **사용자 개입 입력**(송신·IG 값 바꾸기·takeover·STOP·승인·마커), 에이전트 행동 표시(Activity·AGT 배지). 대상 패널: **Trace**(Fixed/Chrono, ISO-TP 묶음 + UDS 주석, EXT 배지), **Graphics**(신호 시계열, 커서), **IG**(Interactive Generator — 1회·주기, 소유자·lease), **신호/Data·State**, **채널 상태 바**(버스 부하·에러·bus-off·env 배지·드라이버 종류·FD), **Statistics**(후순위). v1.2 §5.3 웹 패널 표와 §5.2 TUI를 출발점으로. 패널 ↔ 코어 데이터 계약에서 CAN이 제공할 RPC/이벤트 목록과 UI 팀에 요구할 것(가상 스크롤 행 수, 바이너리 열 블록 등)을 분리.
   ASCII 와이어프레임 1~2개.
4. **통합 테스트 제공 동작 (C7)**: TEST 기획(`test/brief.md`)에 CAN이 줄 것 — 대기·검증 명령(예: `loom can sig wait-for`, `loom can expect …`, `trace --since mark:` 범위 질의, 주기 측정 검증 `stats --id --period`), **증거로 인용 가능한 이벤트**(kind, seq 범위, 어떤 필드로 일치 검증하는지), 세션 시작 일괄 사전 승인과 CAN 잡의 관계, 테스트 단계 마커가 CAN Trace에 찍히는 모양. 예시 TC 1개(자연어 → 에이전트가 부를 `loom can` 명령 시퀀스 → 인용 증거).
5. **스킬 개정 (C8)**: `loom-can` 스킬 v2 골자(본문 짧게, 상세는 `loom can help --json` 정본), 규칙 변경점(UI 안내: "UI를 열어 달라", 테스트 세션 중 규칙, 증거 인용). `loom-viz`는 UI 팀 소유로 이관됨을 반영.
6. **M1 걷는 뼈대 v2**: Rust worker(sim) + loomd + `loom can` CLI + **웹 UI 최소 CAN 패널**(Trace + 송신 입력 + STOP + 상태 바)로 4요소 시연. 수용 기준(지연 p95, 늦은 참가, 고부하 gap 배지, 사용자 개입 → 에이전트 `others_since_last`). M1에서 뺀 것. 이후 M2~M5 한 줄씩(PCAN FD 첫 실HW Win+Linux, VN5640 CAN FD, ISO-TP, VN5640 이더넷은 UDS와, CANoe 브리지 P4).
7. 작업 분해 표 초안(§12): ID·작업·의존·M — 웹/viz 작업(W*, V*)은 UI 팀으로 넘기고 CAN 쪽은 패널 명세·데이터 질의만.

C2 큐·메모리 수치와 crate 선택은 can-sol이 하므로 자리만 표시한다. 끝나면 파일 저장 후 멈춘다.
