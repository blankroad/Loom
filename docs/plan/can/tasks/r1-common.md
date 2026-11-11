docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선함)

# R1 공통 지시 (can-opus, can-sol 둘 다 읽을 것)

당신은 Loom 프로젝트 **② CAN + 시각화** 컴포넌트의 계획 워커다. 리드는 can-lead.
**계획 단계다. 구현 코드는 쓰지 않는다.** (스파이크 예외는 아래)

## 먼저 읽을 것
1. `docs/plan/00-brief.md` — 공통 브리프 (특히 §3 공통 아키텍처 계약, §4 산출물 15개 절, §5 규칙)
2. `docs/plan/can/brief.md` — CAN 컴포넌트 브리프
3. 참고(읽기만): `docs/plan/uart/brief.md`, `docs/plan/uds/brief.md`, `docs/plan/_xteam/` (있으면), 다른 팀 폴더의 진행 중 초안

## 할 일
`00-brief.md` §4의 **15개 절 전체**를 포함하는 독립 초안을 한국어로 작성한다. 상대 워커와 상의하지 말고 독립적으로 쓴다 (다양성 확보 목적). 다른 워커의 r1 파일은 읽지 말 것.

절 목록 (제목 그대로 번호 유지):
1. 범위 / 비범위
2. 사용 시나리오 (에이전트 관점, 사용자 관점 각각 3개 이상, 구체적으로 — 실제 명령/화면 흐름 수준)
3. 아키텍처 (프로세스 구성도, 데이터 흐름 — ASCII 다이어그램 권장)
4. CLI / API 명세 초안 (명령, 인자, 출력 예시 — `loom can ...`, `loom viz ...`, 데몬 RPC 메서드/이벤트)
5. 사용자 뷰어 UX (터미널/웹, 화면 구성 — 웹 vs 데스크톱(PySide/pyqtgraph) vs TUI 비교 후 결정)
6. 데이터/기록 포맷
7. 라이브러리 선택과 근거 (버전, 라이선스, 플랫폼 지원, 유지보수 상태 — 가능한 한 실제 확인)
8. HW/OS 지원 매트릭스
9. 시뮬레이션 & 테스트 전략 (HW 없는 CI 포함)
10. 안전 / 가드레일
11. 마일스톤 (M0, M1, … 각각 수용 기준)
12. 구현 단계 작업 분해 — 병렬 가능한 단위, 각 작업에 추천 담당(Opus 워커 / Sol 워커)과 의존성
13. 리스크
14. 사용자에게 물어볼 질문 (각 질문에 기본값 제안)
15. 공통 계약 변경 요청 / 타팀 의존성

## 반드시 다룰 핵심 쟁점 (브리프 요약)
- python-can 백엔드(vector, pcan, virtual, socketcan, kvaser…), CAN FD, 다채널, 데이터 비트레이트, HW 타임스탬프. **CANoe와 같은 Vector 채널 공존**(XL 앱 채널 할당, 같은 채널 동시 사용 가능 여부).
- CANoe 자체 제어(COM API / 공식 Python 패키지 / XIL API) — 범위 포함 여부 판단과 근거.
- DB: DBC 필수, ARXML/LDF 범위 판단. 신호 인/디코드, 멀티플렉스, 값 테이블.
- 에이전트 동작: 1회/주기 송신, 신호 단위 송신(주기 메시지 내 신호만 변경 = CANoe IG), 트레이스 조회(filter/tail/since), 신호 조회/대기(`wait-for`), 버스 통계(부하, 에러프레임), 기록 시작/정지. 출력 상한, 무한 스트림 금지.
- 사용자 도구: Trace(심볼릭/원시, 고정/스크롤, 필터), Graphics(다중 신호, 커서, 줌), Data/State, Interactive Generator, Statistics, Replay. herdr 패인용 최소 TUI 트레이스.
- `loom.viz` 범용 뷰어: CSV/JSONL/Parquet/BLF/ASC/MF4/라이브 스트림 → 자동 렌더, `loom viz show data.csv --x ts --y a,b` 한 줄. 수백만 포인트 성능(LTTB 등). UART/UDS 팀 요구 수용 구조(`docs/plan/_xteam/viz-requirements.md`).
- 기록: BLF/ASC/MF4 입출력.
- **CAN-TP 호스팅** (공통 계약 §3-8): CAN 데몬 안에서 ISO-TP 스택 실행, UDS 팀용 API. python-can-isotp 등, STmin/FC/N_Bs/N_Cr 타이밍. 인터페이스는 `docs/plan/_xteam/can-uds-isotp.md`에서 UDS 팀과 합의할 예정 — 초안에 당신이 제안하는 인터페이스를 절 15 또는 별도 소절로 구체적으로 적을 것.
- 시뮬레이션: virtual bus + DBC 기반 가짜 ECU 노드.
- 플랫폼: 개발 macOS, 실HW Windows 가능성 높음. Vector XL Windows 전용, PCAN macOS(PCBUSB/MacCAN) 확인.
- 데몬/IPC: 공통 계약 §3-2,3 — 장치는 데몬 하나만 소유, 로컬 IPC(요청/응답 + 이벤트 구독), 웹 뷰어가 직접 붙을 수 있으면 유리. 구체 프로토콜을 제안할 것. UART/UDS와 데몬을 하나로 할지 도메인별로 나눌지도 의견 제시.

## 규칙
- 파일 쓰기는 `docs/plan/can/` 안과 (스파이크 시) `spikes/can/` 안만. 다른 팀 폴더와 `_xteam/`은 이번 라운드에서 쓰지 말 것 (리드가 함).
- 전역 설치 금지. 필요하면 `spikes/can/.venv` (uv 또는 python -m venv). 사용자 설정 파일(~/.claude, ~/.codex) 수정 금지.
- 모르는 건 추측으로 단정하지 말고 "미확인"이라고 표시. 웹/문서/소스로 확인한 사실은 출처(URL 또는 패키지 버전)를 적는다.
- 분량: 충분히 구체적으로 (대략 600~1200줄 마크다운이 적당). 결론 → 근거 순서.
- 끝나면 파일 마지막 줄에 `<!-- END R1 -->` 를 넣는다. 화면 응답은 "done: <경로>" 한 줄이면 충분.
