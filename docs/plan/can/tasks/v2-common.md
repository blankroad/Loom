# CAN v2.0 재계획 — 공통 맥락 (can-opus, can-sol 모두 읽기)

## 먼저 읽을 것
1. `docs/plan/02-core-principle.md` (최우선 — 에이전트가 장비에 직접 읽고 쓰고, 사용자는 같은 데이터를 같은 순간 **UI 앱**에서 보고 개입)
2. `docs/plan/00-master-plan.md` **§2.3 C1–C8** (C > B > A > D). 특히 C1 Rust, C2 메모리·큐 상한·24h soak, C4 VN5640(Vector XL FFI crate = CAN 소유), C5 PCAN-USB FD, C6 UI 1급(UI 팀이 웹·viz 소유, CAN은 패널 명세 + 데이터 질의 소유), C7 통합 테스트, C8 스킬
3. `docs/plan/04-v2-tasks.md` (CAN 절)
4. 현재 계획 `docs/plan/can/plan.md` v1.2 — 이번은 **v2.0 개정**이다. 처음부터 다시 쓰지 말고 바뀌는 절을 중심으로 쓴다.
5. 참고: `core/brief.md`, `ui/brief.md`, `test/brief.md`. 이 팀들이 동시에 계획 중이다(`core/`, `ui/`, `test/` 폴더의 진행 중 초안이 생기면 읽어도 된다).

## 바뀌지 않는 것 (C3)
D2(JSON-RPC over WS, role·origin), D3 레코드, D4/A14 쓰기 상태, D5 CLI 봉투·종료 코드·커서, D6 R0–R4·승인·감사, D7 기록, A1–A16, B1(Windows x64 + Linux x64 실HW, macOS 개발), B4(vehicle = bench 규칙), `can-uds-isotp.md` 합의(이름 `can.isotp.*`). 이것들은 언어와 무관하므로 v1.2 내용을 그대로 쓴다.

## 바뀌는 것 (요약)
- 코어·CLI·장치 경로 = Rust. Python 제거(python-can, can-isotp, cantools, asammdf …). Python이 남는 곳은 "대안이 없다"는 근거와 함께만 (CANoe COM 브리지 → **선택 기능 P4, Python 어댑터 worker**로 강등).
- Vector: **VN5640 직접 접근이 1급**(CAN FD + 차량 이더넷), `vxlapi64.dll` 자체 Rust FFI 바인딩 crate를 CAN 팀이 소유. pyvxl은 참고·교차 검증.
- PCAN-USB FD: CAN FD를 M2 첫 실HW부터. Windows = PEAK 공식 Rust FFI(PCAN-Basic 5.1+), Linux = SocketCAN FD.
- 웹 UI·`loom.viz` 소유는 UI 팀으로 이관. CAN = Trace/Graphics/IG/신호 **패널 명세**(`_xteam/can-ui.md`로 전달) + gateway 쪽 CAN 데이터 질의 모듈.
- 통합 테스트(C7): CAN이 제공할 대기·검증 명령, 증거로 인용 가능한 이벤트. 스킬 개정(C8).
- M1 걷는 뼈대 = Rust + 웹 UI 기준으로 재정의.

## 규칙
- 한국어로 쓴다. 수치는 **실측 / 추정 / 목표 / 미확인**을 구분한다. 확인한 사실은 출처(URL, crate 버전, 소스 파일:줄)를 단다.
- 쓰기는 `docs/plan/can/` 아래 지정된 출력 파일과, 스파이크는 `spikes/can/` 아래만. 전역 설치 금지(`cargo install`, `pip --user`, `brew` 금지). 스파이크용 `cargo build`가 crate를 `~/.cargo/registry`에 받는 것은 허용.
- 사용자 설정(`~/.claude`, `~/.codex`) 수정 금지. 다른 팀 폴더·`_xteam` 파일은 읽기만(리드가 씀).
- 분량: 바뀌는 절 중심, 표 위주, 군더더기 없이.
