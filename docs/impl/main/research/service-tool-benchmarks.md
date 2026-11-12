# 서비스별 도구 벤치마크와 화면 분리 결정

상태: **통합 리뷰 완료** · 기준 코드 `dbca34d` 위 서비스 분리 변경 · 조사일 2026-11-12

사용자 요청에 따라 UART, CAN/CAN-TP, Ethernet, UDS/DoIP/EthDiag, PSU를 담당별로 병렬 조사했다. 아래는 최종 요약이며 상세 근거·화면 대안·수용 조건은 각 보고서에 있다. 이번 구현은 서비스 경계 분리이고, 모든 패널을 한꺼번에 재설계하지 않는다.

## 조사 결과와 다음 화면 방향

| 서비스 | 대표 도구와 채택 근거 | 화면 대안 / 추천 | 담당별 보고서 |
| --- | --- | --- | --- |
| UART | Tera Term·PuTTY·CoolTerm. Microchip의 실제 보드·시리얼 설정 가이드가 사용을 안내한다. | 현재 alias별 대화형 터미널 유지 → 같은 포트 Trace 접근성 개선 → 필요할 때 줄·HEX 송신 보조 도구. 직접 키 입력을 없애지 않는다. | [UART](benchmark-uart.md) |
| CAN / CAN-TP | PCAN-View(PEAK 하드웨어 동봉), CANalyzer/CANoe(공급사 산업 사용 주장), SavvyCAN(공개 OSS). | 시간순 Trace / ID별 수신 요약 / 구성형 분석 공간 중, 다음 단계는 **채널별 수신 요약 + 기존 Trace**. 주기 송신은 별도 작업 목록으로 설계한다. | [CAN·CAN-TP](benchmark-can.md) |
| Ethernet | Wireshark(Microchip·NXP 기술 문서의 사용 사례), tcpdump/libpcap(수집 계층), CANalyzer Ethernet. | **패킷 목록 → 상세 → 바이트** 유지. 진단 우선 화면과 NIC별 비교는 후속 대안이다. 캡처 필터·표시 필터·화면 정지를 구분한다. | [Ethernet](benchmark-eth.md) |
| UDS / DoIP / EthDiag | Softing DTS.monaco·Vector 진단 도구. PCAN-UDS는 GUI가 아닌 API 계층 참고다. | **진단 콘솔 중심 + 실제 CAN·Ethernet 관찰 보조**. 상하 비교는 선택 기능, ECU 카드 그리드는 대상 수가 늘 때 검토한다. | [UDS·전송 경로](benchmark-uds.md) |
| PSU | Keysight BenchVue Power Supply, NI InstrumentStudio. 공식 지원 장비 범위와 NI-DCPower 드라이버 동봉이 확인된다. | 현재 게이지 유지 → **채널 요약 목록 + 선택 상세** → 필요 시 다채널 추세 비교. 설정값·실측값·출력·연결을 구분한다. | [PSU](benchmark-psu.md) |

공개 비교 사용자 수·시장 점유율은 확보하지 못했다. 따라서 표는 인기 순위나 1위 목록이 아니다. 공급사 표현, 하드웨어 동봉, 칩 공급사 가이드의 권장 사례, OSS 활동을 구분했다. GitHub 별은 사용자 수로 환산하지 않았다. 상용 제품을 직접 실행하거나 물리 장비를 시험한 결과는 아니다. 공식 문서 열람과 현재 소스·실제 simulator 브라우저 증거를 대조했다.

UART 담당은 초기 조사에서 2차 출처로만 확인했던 CoolTerm 기능을 공식 도움말로 재확인했다. Ethernet 담당은 제조사 기술 문서에서 Wireshark의 실제 적용 사례를 보강했다. 상세 출처와 열람 여부는 각 보고서에 표시했다.

## 확정한 서비스 경계

```text
통합 작업 공간                 여러 도메인의 관찰·배치
인터페이스 서비스
  UART                         실제 열린 UART alias만
  CAN                          실제 열린 CAN 채널만
  Ethernet                     실제 열린 ETH 캡처 채널만
  PSU                          실제 연결된 PSU만 (출력 OFF도 연결됨)
진단
  UDS                          진단 콘솔 + 실제 열린 CAN·ETH 관찰
```

- 서비스 이동은 UI 경로만 바꾼다. 기존 엔진·이벤트·상태·RPC는 공유하고, 장치를 자동으로 열거나 닫거나 캡처·송신하지 않는다. 이번 변경은 별도 프로세스로 서비스를 배포하는 변경이 아니다.
- UDS는 CAN 또는 Ethernet과 같은 층의 물리 인터페이스가 아니다. CAN-TP는 CAN 위의 전송이고, DoIP와 사용자가 설명한 현대·기아 EthDiag는 Ethernet 진단 전송으로 구분한다. EthDiag는 DoIP의 별칭으로 취급하지 않는다.
- UDS 화면에 열린 CAN·ETH 관찰을 병치해도 특정 ECU와의 대응 관계를 증명하지 않는다. 현재 manager에는 CAN 채널/캡처 인터페이스를 연결하는 식별 정보가 없다. 임의 매핑과 자동 캡처는 하지 않는다.
- 현재 UDS manager 연결은 DoIP/sim이다. CAN 도메인에 ISO-TP worker가 있다는 사실과 UDS manager가 CAN-TP를 사용할 수 있다는 사실은 다르다. EthDiag의 공개 1차 사양은 확보하지 못했으며, 이 지원을 구현했다고 주장하지 않는다.
- 기존 UDS `TP` 배지는 **TesterPresent**이며 CAN-TP 전송 상태가 아니다.
- 현재 동반 CAN·ETH 패널은 기존 권한·ActionGate를 따르는 송신 제어도 유지한다. 강제 읽기 전용 모드는 아직 구현하지 않았다.
- 전역 STOP은 agent·script 쓰기와 잡을 멈추며 PSU 출력을 모두 OFF로 바꾸는 기능이 아니다. 채널 비상 OFF, 일반 출력 토글, 연결 해제, 기록 제어를 같은 동작으로 표현하지 않는다.

## 이번 단계의 구현과 검증

- `#/s/uart`, `#/s/can`, `#/s/eth`, `#/s/psu`, `#/s/uds`와 별도 통합 진입점. 활성 도메인만 사이드바에 표시하고 alias 수를 실제 상태에서 계산한다.
- UDS는 별도 진단 그룹이며 CAN·Ethernet 서비스와 상호 이동한다. UDS 기본 탭에는 실제 활성 UDS·CAN·ETH 관찰만 들어간다. 기존 `/l` 레이아웃과 `/p` 패널 링크를 유지한다.
- 최종 UI 검사 **474개 /42파일 PASS**, schema33·타입·lint·build PASS, embedded native build PASS, UI asset 검사11 PASS.
- 실제 Ego Lite **105항목 PASS /0 FAIL**, canonical 브라우저 주요 시나리오 **5/5 PASS**. 실제 native simulator 기준이며 물리 장비 검증은 아니다.
- [구현·검증 인계](../tasks/service-separation.md), [UART 실제 화면](../evidence/ui-workbench/service-split-final/uart-all-connected.png), [UDS 실제 화면](../evidence/ui-workbench/service-split-final/uds-all-connected.png).
- 기존 dock의 경로 전환 수명주기를 유지한다. 서비스 이동 시 연결은 유지하지만 패널 인스턴스는 다시 만들어지므로 미전송 로컬 입력 보존은 후속 과제다. 현재 PSU는 CLI/RPC로 연 연결을 보여 주며, 연결 대화상자 지원 범위는 확장하지 않았다.

## 다음 순서

1. **UART 한 서비스의 화면부터** 사용 흐름을 확정하고 작은 변경으로 구현·브라우저 검증한다. 다중 alias, VT 콘솔과 Trace, 선택형 송신 보조의 역할을 분리한다.
2. CAN은 최신값 요약과 시간순 Trace를 구분한다. 주기 송신·버스 상태·ISO-TP는 실제 데이터 계약과 소유권을 확인하며 별도 단계로 나눈다.
3. UDS는 서비스/NRC 의미 표시와 진단 대상·전송 경로 모델을 다듬는다. CAN-TP manager 연계와 EthDiag 지원은 별도 설계·사양 작업이다.
4. PSU는 다채널 요약·상세, Ethernet은 기존 3단 관찰과 정확한 UDS 연계를 서비스별 근거에 맞춰 순차 개선한다.

다섯 서비스 리서치는 병렬로 했지만, 다섯 패널 재설계를 동시에 진행하지 않는다. 각 서비스의 자세한 선택지는 위 개별 보고서에서 검토한다.
