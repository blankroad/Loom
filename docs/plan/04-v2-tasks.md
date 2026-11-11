# v2.0 재계획 지시 — 기존 팀 리드용 (UART · CAN · UDS · EXPLORE)

먼저 `00-master-plan.md` §2.3 **C1–C8**을 읽는다 (C > B > A > D). 최우선 원칙은 그대로다: **에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자는 같은 데이터를 같은 순간 UI에서 보고 개입한다.** v2의 사용자 화면은 TUI가 아니라 **Loom UI 앱(웹)** 이 1차다(C6).

## 진행 방식
- 이번은 계획서 **v2.0 개정**이다. 플레이북 라운드를 가볍게: 워커 둘에게 나눠 맡기고(예: opus = 구조·UX·UI 패널 명세·스킬 / sol = Rust crate·드라이버·FFI 조사와 실현성), 리드가 `plan.md`에 통합. 필요하면 `spikes/<comp>/`에서 Rust 짧은 검증.
- 새 팀: **CORE(w6)** `core/brief.md`, **UI(w7)** `ui/brief.md`가 동시에 계획 중이다. 그들의 진행 중 초안을 읽어도 되고, 요청은 `_xteam/<from>-<to>.md` 파일로만(예: `_xteam/can-core.md`, `_xteam/uart-ui.md`).
- 사용량 한도 주의: 큰 재작성 대신 바뀌는 절 중심으로. 결정 기록에 "v2.0 방향 전환" 행.

## 공통 (모든 팀)
1. 구현 언어 Rust로 이식: 사용할 crate·FFI·직접 구현 범위, Python 의존(python-can, udsoncan, pyserial 등) 제거 계획. **Python이 남는 곳은 대안이 없음을 근거로** 표시(C1).
2. C2: 도메인 내부 큐·버퍼 상한과 넘칠 때 정책, worker 메모리 예산, soak 시험에 넣을 부하 패턴.
3. C6: 사용자 화면을 **UI 앱 패널 명세**로 다시 쓴다(무엇을, 어떤 지연으로, 사용자가 어떤 입력으로 개입). TUI는 보조. 패널 명세는 `_xteam/<comp>-ui.md`로 UI 팀에 전달.
4. C7·C8: 통합 테스트에서 이 도메인이 제공할 동작(대기·검증 명령, 증거로 인용 가능한 이벤트)과 스킬 개정.
5. M1 걷는 뼈대를 Rust + 웹 UI 기준으로 다시 정의.

## UART
- Rust 시리얼(serialport/tokio-serial 등), pty 가짜 장치, `loom/console` 엔진(A4)을 Rust crate로. 터미널 패널(xterm.js 등)로 원시 키 입력·Ctrl-C 경로, 입력 중재(InputArbiter)를 UI 경유로.

## CAN
- **C4**: Vector XL FFI 바인딩 crate(`vxlapi64.dll`, CAN FD + 이더넷 원시 프레임 API) 소유 — VN5640. pyvxl은 참고·교차 검증. VN5640 위 DoIP용 이더넷 경로 (a)/(b)/(c) 조사를 UDS와 `_xteam/vn5640-eth.md`에서(CAN 초안).
- **C5**: PCAN-USB FD — Windows PEAK 공식 Rust FFI(PCAN-Basic 5.1+), Linux SocketCAN FD. FD를 M2 첫 실HW부터.
- ISO-TP Rust 구현(FC 타이밍 전용 스레드), DBC 파서 crate 선택.
- 웹 UI·`loom.viz` 소유는 **UI 팀으로 이관** — CAN은 Trace/Graphics/IG/신호 패널 명세와 데이터 질의(gateway 쪽 CAN 모듈)를 소유.
- CANoe 브리지는 선택 기능(P4, Python 어댑터)으로 강등, Vector 채널 공존 실측은 유지.

## UDS
- Rust UDS 클라이언트·DoIP·CAN-TP 사용(이 팀이 직접 구현 범위 결정, 기존 Rust crate 조사).
- **C4**: 진단 기본 경로 = 자체 스택 over **VN5640(CAN FD·이더넷)** 과 PCAN FD. VN5640 이더넷 위 DoIP 경로 결정(`_xteam/vn5640-eth.md`에 `## uds 의견`).
- `canoe` 백엔드(B3)는 선택 기능으로 강등(P4, Python 어댑터) — `_xteam/canoe-bridge.md` 합의는 보존.
- seed-key: CANoe 없이 쓸 방법(Windows DLL 직접 로드 FFI, 사용자 스크립트) 우선순위 재정리.

## EXPLORE
- **주 작업 = TEST 기획**: `test/brief.md`를 따라 `docs/plan/test/plan.md` 작성(같은 라운드 방식).
- 부 작업: `explore/plan.md`를 C1에 맞게 가볍게 v2(DLT·SSH Rust, Saleae는 gRPC proto로 Rust 가능성 먼저 검토, 안 되면 Python 어댑터 근거).

## 완료
- `plan.md` v2.0, `STATUS.md` 첫 줄 `DONE v2.0` + 바뀐 점 5~8줄 (EXPLORE는 `test/STATUS.md`도).

## 추가 (C4.1, 사용자 확인 — CAN·UDS 필독)
- DoIP는 **PC 랜카드 + 미디어 컨버터**로 이미 쓰고 있다 → OS TCP/IP 소켓 위 DoIP(`_xteam/vn5640-eth.md`의 경로 c). **VN5640은 CAN FD 전용**으로만 쓰고 Vector XL 이더넷 API·`smoltcp`(경로 b)와 OS 어댑터 노출(경로 a)은 범위 밖. pyvxl은 CAN 전용.
- Vector XL 바인딩 crate는 CAN/CAN FD API만. `_xteam/vn5640-eth.md`의 CAN 질문 1·3은 닫힘.
