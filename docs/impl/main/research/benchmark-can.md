# CAN / CAN-TP 서비스 벤치마크 — READY_FOR_REVIEW

검토일: 2026-11-12. 범위: CAN 서비스 분리의 다음 화면 결정. 제품 코드 변경 없음.
근거: 공식 제품 문서와 현재 소스, 실제 daemon 시뮬레이터 브라우저 캡처.
실물 CAN 하드웨어는 시험하지 않았고 PCAN-View·CANalyzer·CANoe 상용 프로그램은 실행하지 않았다.
따라서 실제 사용자 수 순위, 하드웨어 오류 표시 동작, 상용 앱의 세부 조작감은 판단하지 않는다.

## 후보·출처·채택 근거

| 도구 | 공식 근거(모두 2026-11-12 열람) | 채택 근거의 강도 / Loom에 쓸 점 |
| --- | --- | --- |
| [PCAN-View](https://www.peak-system.com/products/software/analysis-software/pcan-view/) | PEAK의 모든 PCAN PC 하드웨어에 동봉. 수신·송신 정렬 목록, 수동·주기 송신, trace 기록, 버스 오류·하드웨어 오버플로 표시를 공식 명시 | **실제 배포 경로**가 확인된다. 사용자 수는 알 수 없다. 작은 채널별 모니터 화면의 가장 직접적인 기준 |
| [Vector CANalyzer](https://www.vector.com/en/product/canalyzer/) / [CANoe](https://www.vector.com/en/product/canoe/) | Vector는 CANalyzer의 산업 전반 사용을 주장하며 분석 창·필터·생성기·재생·진단을 설명. CANoe는 분석·자극·테스트·진단 영역을 명시 | **제조사 주장**, 계측된 설치 수는 없음. 시간순 분석과 진단 병치의 범위 참고; 대형 측정 구성은 이 단계에 과함 |
| [SavvyCAN](https://www.savvycan.com/docs/) / [원저자 저장소](https://github.com/collin80/SavvyCAN) | 공식 문서에 메인 프레임 목록, Sniffer, Flow, ISO-TP Decoder, Custom Sender. 공개 저장소의 포크·별은 커뮤니티 활동 지표 | **실사용자 수 증거 아님**. 원시 프레임과 파생 분석을 별도 보기로 두는 근거 |
| [Linux SocketCAN ISO-TP](https://www.kernel.org/doc/html/latest/networking/iso15765-2.html) | ISO 15765-2를 CAN 진단용 전송 프로토콜로 설명하며 단일·분할 프레임과 흐름 제어를 구별 | GUI 인기도 자료가 아니라 계층 구분의 기술 근거 |

가장 널리 쓰이는 1위를 정할 공개 계측 근거는 확보하지 못했다.
PCAN-View 추천은 하드웨어 동봉과 Loom의 좁은 CAN 관찰 범위가 일치하기 때문이지 인기도 순위가 아니다.

## 현재 실제 상태

- [`can.trace`](../../../../ui/src/panels/can/canTrace.ts)는 채널 하나를 받아 CAN 이벤트를 시간순 링에 적재한다.
- 화면에는 ID·방향·출처 필터, 일시정지, 원시 시간순 행, 단발 송신 입력, dry-run 위험 표시, 수신 fps·화면 행수 표시가 있다.
- 일시정지는 렌더만 멈춘다. 이력은 유한 링이며 유실·화면 생략을 별도 행 또는 계수로 표시한다.
- [`traceModel.ts`](../../../../ui/src/panels/can/traceModel.ts)는 요청/echo를 합치고 FD·DLC·RTR를 표시하나 ID별 마지막 수신 상태 표는 만들지 않는다.
- [`sendLine.ts`](../../../../ui/src/panels/can/sendLine.ts)는 단발 classic/FD 입력을 검증한다. 현 UI에 주기 송신 작업 목록은 없다.
- [`canTrace.ts`](../../../../ui/src/panels/can/canTrace.ts)의 하단 status는 화면 행수·fps 등 **화면 상태**다. 버스 오류·bus-off의 전용 상태줄로 읽으면 안 된다.
- CAN worker에는 [`periodic.rs`](../../../../crates/loom-can/src/periodic.rs)의 주기 작업과 목록 질의가 있다. UI 연결과 소유권 표현은 별도 설계가 필요하다.
- CAN worker에는 [`isotp.rs`](../../../../crates/loom-can/src/isotp.rs)의 ISO-TP endpoint가 있고 CLI는 이를 전송 디버깅으로 설명한다.
- UDS manager의 실제 연결 경로는 [`manager.rs`](../../../../crates/loom-uds/src/manager.rs)의 DoIP/sim이다. `cantp` 스키마 표기는 연결 구현 증거가 아니다.
- [`udsModel.ts`](../../../../ui/src/panels/uds/udsModel.ts)의 대상 막대 **TP는 TesterPresent**다(`tester_present`, 옛 `tp.on`). ISO-TP 상태가 아니다.
- 현재 UDS 보조 관찰에 쓰는 `can.trace`도 단발 송신 조작을 유지한다. ActionGate의 `operate` 권한·dry-run 검사를 거치며 읽기 전용으로 강제되지 않는다.
- [실제 시뮬레이터 캡처](../evidence/ui-workbench/service-split-final/can-all-connected.png)는 CAN 서비스에 `pt` trace 한 장, 연속 rx `0x310`, 단발 송신 입력을 보여 준다.
- 캡처는 다른 도메인도 연결된 상태에서 서비스 화면이 CAN만 투영됨을 보여 주지만, 물리 버스 오류나 주기 송신 UI의 증거는 아니다.
- [`routes.ts`](../../../../ui/src/shell/routes.ts)는 열린 CAN 채널마다 패널 slot을 만든다. UDS 서비스는 열린 UDS·CAN·ETH 관찰을 병치하되 ECU 경로를 추론하지 않는다.

## 서비스 안의 워크플로 대안

### A. 현재 시간순 trace 한 장 유지

채널 선택 → raw rx/tx 시간순 조사 → ID 필터 → 필요 시 단발 송신.
장점: 지금 구현·검증된 경로 그대로이고 이벤트 순서·echo·gap 판단에 강하다.
단점: 반복 ID의 최신값·주기를 찾기 어렵고 주기 작업·버스 상태가 보이지 않는다.
분리 1단계에서는 안전한 기준선이나 장기 CAN 서비스 기본 화면으로는 부족하다.

### B. 채널별 수신 요약 + 시간순 trace + 송신 작업 (권장)

상단에서 채널/실제 연결 상태 확인 → ID별 마지막 수신값·횟수·최종 시각으로 활성을 훑음 → 해당 ID의 시간순 trace로 이동 → 송신 영역에서 단발·주기 작업을 구분.
PCAN-View의 수신·송신 목록을 참고하되 Loom의 기존 raw trace와 gap 정직성은 유지한다.
장점: 반복 트래픽 탐색이 빨라지고 관찰·조작·상태의 경계가 분명하다.
단점: 요약의 키(ID만인지, ID+채널+프레임 형식인지), 갱신률, 오래된 값 표기가 필요하다.
초기 구현은 **서비스 분리 뒤의 작은 후속 단계**로 한 채널 요약을 trace의 보조 보기로 추가하는 편이 적절하다.
주기 작업 표는 worker 목록 질의와 STOP/owner/lease 수명주기를 확인한 뒤 붙인다.
버스 상태는 실제 daemon 이벤트·질의에 근거한 값만 표시하고 화면 fps를 bus health로 바꾸어 부르지 않는다.

### C. CANalyzer식 구성형 분석 작업 공간

사용자가 필터·생성기·재생·진단 창을 조합해 여러 측정 시나리오를 저장한다.
장점: 복합 분석과 장기 시험에서는 확장성이 크다.
단점: 이번 작업의 서비스 탐색 분리보다 범위가 크고 새 설정·재생 의미를 정의해야 한다.
현재 단계에서 채택하지 않는다.

## CAN-TP 및 여러 인터페이스의 경계

CAN-TP는 CAN 위의 전송 계층이며 별도 물리 서비스 메뉴가 아니다.
원시 trace는 CAN 프레임을 그대로 보존한다. 향후 ISO-TP 재조립은 명시적으로 켠 파생 보기와 endpoint/주소 쌍으로 표시한다.
분할 PDU를 원시 프레임 한 행으로 대체하거나 TesterPresent `TP` 배지로 재조립 성공을 암시하지 않는다.
UDS 진단은 실제 CAN-TP manager 연결이 생기기 전까지 CAN 채널을 UDS ECU의 전송 경로라고 표시하지 않는다.
UDS 화면의 동반 CAN trace는 현재 송신 가능 패널이므로 권한·경로·위험 표시를 그대로 명확히 유지한다.
CAN 채널 둘 이상이면 수신 요약과 송신 작업의 키에 채널을 포함하고, 각 채널의 버스 상태를 독립적으로 표시한다.
서비스 이동은 이미 열린 채널을 다시 열거나 닫지 않는다. 통합 화면은 여러 도메인을 함께 볼 수 있다.
PCAN-View 공식 설명의 단일 CAN 채널 연결을 Loom 다중 채널 합치기의 근거로 삼지 않는다.

## 다음 구현 단계의 수용 검사

1. CAN 서비스에 열린 CAN 채널만 보이고 직접 진입·빈 상태가 유용하다. 다른 서비스 이벤트가 CAN 패널을 추가하지 않는다.
2. CAN 채널 둘의 동일 ID는 수신 요약에서 섞이지 않고 각 trace의 순서·gap·echo가 유지된다.
3. 요약의 최신값·횟수는 raw trace와 대조 가능하며 일시정지·링 퇴출 시의 의미를 명시한다.
4. 단발 송신과 주기 작업은 분리 표시한다. 주기 작업은 실제 worker 목록, 소유자, STOP 상태와 일치한다.
5. bus-off/오류/오버플로 표시를 제안할 때는 실제 이벤트 계약과 sim 주입 검사를 먼저 확보한다. 미지원 값은 `알 수 없음`으로 남긴다.
6. UDS 서비스의 동반 CAN trace는 현재 gated 송신 제어를 노출한다. 읽기 전용을 원하면 명시적 별도 결정·검사가 필요하다.
7. ISO-TP 파생 보기는 raw 프레임과 분리되고 endpoint·주소·재조립 실패를 표시한다. UDS CAN-TP 경로는 실제 연결 전까지 비활성이다.
8. 서비스 전환 중 연결 generation, 수집, 공통 STOP/기록 상태가 유지되고 통합 작업 공간의 혼합 도메인 보기도 유지된다.

남은 불확실성: 실물 어댑터의 버스 오류 이벤트 범위, PCAN/Vector 설치 수, 복수 채널 요약의 키·오래된 값 정책, 주기 작업 UI의 소유권 조작 정책.
