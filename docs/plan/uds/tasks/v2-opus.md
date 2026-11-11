docs/plan/02-core-principle.md 준수

# v2.0 — uds-opus: UI 패널 명세·통합 테스트·스킬·M1 재정의 (plan.md 수정 금지, 초안 문서만)

먼저 읽기: `docs/plan/00-master-plan.md` §2.3 **C1–C8**(최우선), `docs/plan/04-v2-tasks.md`, `docs/plan/ui/brief.md`, `docs/plan/test/brief.md`, `docs/plan/core/brief.md`, 현재 `docs/plan/uds/plan.md` v1.2(§0, §2.0 E2E-1, §4, §5 뷰어, §6.1 이벤트, §6.5, §10.2–§10.7, §10.9 스킬, §11). `docs/plan/ui/`, `docs/plan/core/`, `docs/plan/test/`에 진행 중 초안이 있으면 참고.

배경: v2에서 사용자 화면의 1차는 TUI가 아니라 **Loom UI 앱(웹, `loomd`가 127.0.0.1로 제공, 같은 WS 프로토콜)** 이다. UI 팀이 셸(작업 공간·STOP·승인 대기열·env 배지·연결/gap·기록 탐색/재생·테스트 실행 화면)·패널 프레임워크·viz를 소유하고, 도메인 팀은 **패널 내용 명세와 데이터 질의**를 소유한다. TUI(`loom uds view`)는 보조. 또 C7: 사람이 자연어·TC 표로 쓴 통합 테스트를 에이전트가 `loom` 명령으로 실행하고, 판정은 이벤트 seq 범위 같은 **증거 인용**으로 하며 Loom이 증거를 검증한다. C8: 도메인 스킬 + 공통 `loom` + `loom-test` 스킬, 본문은 짧게, 상세는 `loom uds help --json`이 정본. 코어와 장치 경로는 Rust(C1)이지만 계약(D2·D3·D4/A14·D5·D6·A1–A16·B4)은 유지(C3).

## 작성할 것 (`docs/plan/uds/v2-opus.md`, 220줄 이하, 한국어)
1. **UDS 콘솔 패널 명세** (UI 팀 전달용 — 그대로 `_xteam/uds-ui.md`로 옮길 수 있게): 패널 목록(트랜잭션 표, 대상 상태 바, 요청 입력·미리보기, 승인 카드 내용(셸 승인 대기열에 넣을 UDS 필드), DID 값/플롯, DTC 표, 대상 목록·연결 대화상자, 복구 송신, 폴링 job), 각 패널의 데이터 원천(이벤트 kind·필드, 질의 RPC), **지연 목표**(응답→UI 렌더 p95/p99), 사용자 입력과 개입 경로(키·버튼, origin=user, 같은 쓰기 파이프라인), 고부하 표시(집계 행·드롭·gap·0x78 카운터), 늦은 합류(snapshot), 에이전트 연동(`others_since_last`), 접근성·키보드만 조작. v1 §5와 viz V-UDS-0~6을 재사용·갱신.
2. **TUI 강등 범위**: 무엇을 TUI에 남기는지(최소), M1 수용에서 TUI가 빠지는지.
3. **M1 걷는 뼈대 재정의 (Rust + 웹 UI)**: E2E-1을 웹 UI 기준으로 다시 쓰기(자동화: 브라우저 자동화 Playwright 등으로 실제 렌더 확인 — 헤드리스 구독자만으론 불합격 유지), 수용 기준 수치, M1에 넣지 않는 것.
4. **통합 테스트 지원 (C7)**: UDS가 제공할 대기·검증 명령(예: `loom uds expect`/`wait-for` 조건 형식, `read-did --expect`, DTC 존재/부재 확인, 세션 상태 확인), **증거로 인용 가능한 이벤트**(request_id·seq 범위·kind, 판정에 필요한 필드), 세션 사전 일괄 승인에서 UDS 위험 요약 제공, 자연어 TC 예시 2개(UDS 단독 1, UART+CAN+UDS 혼합 1 — 예: "UART 콘솔에서 재부팅 명령 → CAN에서 NM 메시지 재개 확인 → UDS로 DTC 없음 확인")와 에이전트가 실행할 명령 열, TEST 팀·CORE에 할 요청 목록.
5. **스킬 개정 (C8)**: `skills/loom-uds/SKILL.md` 새 골격(≤ 50줄), `loom uds help --json` 스키마(명령·인자·등급·종료 코드·예시), `loom-test`와의 경계.
6. **UDS가 CORE에 할 요청** 목록(Rust 코어 기준으로 다시: 쓰기 파이프라인 훅 3개(A2)·잡 API(A1)·승인 digest 필드·safe_state·이벤트 필드·테스트 세션 증거 검증에 필요한 것) — `_xteam/uds-core.md`로 옮길 수 있게.

각 항목에 02 원칙 체크(사용자가 무엇을 언제 보고 어떻게 개입하는가)를 한 줄씩. 끝나면 멈춘다.
