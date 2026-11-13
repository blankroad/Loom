# UDS 워크벤치 · DID / Raw

[목업 열기](http://127.0.0.1:4180/uds-read-did/app/) · [설계 결정](DESIGN.md) · [브라우저 검증](qa/NATIVE.md)

Sol과 Opus가 Vector 진단 콘솔과 Softing 진단 워크벤치를 각각 조사하고, 선택한 Ethernet Opus의 Bench Light 셸 안에 **Opus가 한 가지 Read DID 화면**을 구현했습니다. 요청 대상·경로·바이트·응답 순서·확정값을 함께 보는 사용자 중심 구성입니다.

1. ECU와 DoIP / CAN-TP / EthDiag 경로 선택 후 **연결**.
2. 왼쪽 DID 선택 또는 4자리 hex 입력. 선택만으로 전송되지 않으며 실제 요청 PDU를 미리 확인할 수 있습니다.
3. **읽기** 또는 Enter. 기록에서 요청을 선택하고 응답 순서·해석·원시 바이트를 비교합니다.
4. SIM 옵션에서 Pending·NRC·타임아웃·연결 끊김을 확인합니다. 확정값에는 출처 요청·시각·경로가 남습니다.

왼쪽 목록과 확정값 패널을 접을 수 있고 기록/상세 경계는 드래그·키보드로 조절합니다. JSON·CSV는 현재 기록을 실제 파일로 내보냅니다. 설정만 브라우저에 저장되며 다시 열면 자동 연결·전송하지 않습니다.

## ECU별 연결 설정

상단 **연결 설정**에서 프리셋을 복제하거나 편집해 사용자 프로필로 저장합니다. ECU·경로별로 프로필을 보관하며, 적용은 연결 해제 상태에서 저장한 다음 **연결**할 때 이루어집니다.

- DoIP: Ethernet 채널, IPv4/호스트 이름, TCP 포트, Tester·ECU 논리 주소.
- CAN-TP: CAN 채널, 비트레이트 메타데이터, 11/29비트 Tx·Rx CAN ID. 주소 지정은 normal physical입니다.
- EthDiag: Ethernet 채널, 벤더 어댑터 이름, 해석하지 않는 엔드포인트 메모.
- 공통: 실제 타임아웃 판정에 쓰는 P2/P2* (ms), 이름·복제·되돌리기·프리셋 적용·사용자 프로필 삭제.

설정 JSON 내보내기·가져오기는 트랜잭션 내보내기와 별개입니다. 가져온 프로필은 검증 후 추가되고 자동 적용·연결되지 않습니다. 요청 기록에는 사용 당시 프로필 버전·주소·타이밍이 보존됩니다. IPv6와 실제 버스/소켓 연결은 현재 목업 범위에 포함되지 않습니다.

[설정 변경 인계·누락 점검](CONFIG-HANDOFF.md) · [설정 브라우저 검증](qa/config/NATIVE.md)

## 범위

로컬 시뮬레이터를 사용하는 인터랙티브 목업입니다. 실제 ECU·CAN·Ethernet 또는 native daemon에 연결하지 않습니다. CAN-TP/DoIP/EthDiag는 UDS 응답을 제공하는 모의 경로이며 바이트 패널은 UDS PDU입니다. EthDiag는 ISO 13400 DoIP와 구별한 벤더 어댑터 모형으로 프레이밍을 정의하지 않습니다. 이전 UART/Ethernet 목업은 독립 페이지입니다.

## 근거와 검증

- [Sol 리서치·설계 리뷰](research/sol.md), [Opus 리서치](research/opus.md), [그림 출처와 접근 기록](qa/SOURCES.md)
- [현재 Raw 인계·검사](RAW-HANDOFF.md), [설정 인계](CONFIG-HANDOFF.md), [초기 목업 인계](app/HANDOFF.md), [현재 소스 해시](app/selfcheck/source-sha256-freeze-raw.txt)
- [직접 받은 JSON](qa/native-export.json), [CSV](qa/native-export.csv)

소스 검사와 실제 브라우저 검증은 별도로 기록합니다. 최종 수용 상태는 qa/NATIVE.md 및 RUN.md를 참조하십시오.


## Raw UDS PDU 전송

요청 편집기의 **Raw**를 선택하고 SID를 포함한 hex 바이트 전체를 입력합니다. `22 F1 90`, `22F190`, `0x22 0xF1 0x90` 형식을 지원하며 공백·탭·줄바꿈으로 구분할 수 있습니다. 미리보기의 바이트와 길이를 확인한 다음 **전송** 또는 Ctrl/⌘+Enter로 보냅니다. Enter는 줄바꿈입니다. DID/Raw 입력은 각각 유지되며 모드 전환이나 재열기만으로 전송하지 않습니다.

SID 자동 추가·패딩 없이 입력한 바이트를 그대로 보냅니다. 빈 값·홀수 자릿수·비hex·쉼표·모호한0x 표기는 전체 거부합니다. 목업 입력 상한은4095B입니다. 기록·응답 순서·원시 바이트·JSON/CSV에 모드·SID·길이가 남습니다.

모의 ECU는 정확한 단일 DID 요청 `22 hi lo`만 기존 fixture로 응답합니다. 그 외 SID는 `7F <SID> 11` 모의 미지원 응답, 미모델링0x22형식은0x13으로 표시합니다. 다른 서비스의 실제 ECU 동작을 구현한 것은 아닙니다. 원시 요청도 선택한 ECU·경로·프로필·대기시간을 사용하며 당시 설정을 기록에 보존합니다.

[Raw 브라우저 검증](qa/raw/NATIVE.md) · [최종 JSON](qa/raw/final-trace.json) · [최종 CSV](qa/raw/final-trace.csv)

## Native UDS 코어와 전송 확장 구조

실제 Rust 엔진의 연결 경계를 분리했습니다. `UdsManager → Connector/Link → Transport → 전송 어댑터` 구조이며, UDS PDU 처리·P2/P2*·Pending·취소·우선순위·늦은 응답 격리는 공통 코어가 담당합니다. 기존 DoIP와 loopback SIM은 `DoipConnector`를 사용하고, 새 어댑터는 `Transport`와 `Connector`를 구현해 `UdsManager::connect_link`로 연결합니다. DoIP를 제외한 runner 빌드도 검증했습니다.

CAN-TP 분할·흐름 제어와 EthDiag 벤더 프레이밍은 향후 해당 어댑터가 구현해야 합니다. 재연결 시 이전 응답 격리를 입증하는 계약도 필요합니다. 현재 브라우저 목업은 여전히 독립된 로컬 시뮬레이터입니다.

[Native 전송 경계 인계·어댑터 구현 절차](TRANSPORT-HANDOFF.md) · [Rust 검증·리뷰](qa/transport/REVIEW.md)

## Integrated workbench features

The local SIM workbench includes saved commands/recent requests, Watch charts, sequential macros, trace filters/pause, IndexedDB recording and offline log import, resizable/named layouts, editable ECU/DID catalogues, session/TesterPresent/DTC controls, and project export/import with rollback. Final source and native browser acceptance: [qa/team/REVIEW.md](qa/team/REVIEW.md) (625 source checks; 1440/768/390px).

The browser's Connect button currently operates the local JavaScript simulator. It does not open a TCP socket to a real DoIP server. Native Rust DoIP support and its loopback verification are separate; an HTTP/WebSocket bridge is still needed to use them from this workbench.
