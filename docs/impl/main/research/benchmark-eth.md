# Ethernet 관찰 서비스 벤치마크 — READY_FOR_REVIEW

- 검토일: 2026-11-12. 범위: Ethernet 서비스의 관찰 작업 흐름과 서비스 분리 이후의 표시 방식.
- 기준: `docs/impl/main/tasks/benchmark-eth.md`, `service-separation.md`, 기존 종합 초안의 Ethernet 절.
- 실제 근거: `ui/src/panels/eth/{ethPanel,ethModel,filterBar,dialogs}.ts`, `ui/src/shell/routes.ts`, 아래 시뮬레이터 화면.
- 외부 제품은 공식 문서만 검토했다. **상용 소프트웨어를 실행하지 않았고 물리 Ethernet 하드웨어도 시험하지 않았다.**
- 인기도 1위나 사용자 수 순위는 확인되지 않았다. 제조사 표현과 저장소 별 수는 그런 순위의 근거로 쓰지 않는다.

## 후보와 사용 근거

| 후보 | 실제 확인한 공식 근거 및 채택 정황 | 이 서비스에 유효한 비교점 | 한계 |
| --- | --- | --- | --- |
| [Wireshark 사용자 가이드](https://www.wireshark.org/docs/wsug_html_chunked/ChUseMainWindowSection.html) | 패킷 목록·상세·바이트 화면의 선택 연동과 표시 필터를 명시. [Microchip ENC624J600 가이드](https://ww1.microchip.com/downloads/en/DeviceDoc/ENC624J600-Quick-Start-Guide-50002901A.pdf)는 NIC 선택, 캡처, 필터, 패킷 상세 확인을 단계별로 지시하고 [NXP S32G 응용 노트](https://www.nxp.com/docs/en/application-note/AN13423.pdf)는 Ethernet 라우팅 패킷 확인에 Wireshark를 권한다. | 행 선택 → 트리·hex, 표시 필터, 캡처와 조회의 분리 | 제조사 매뉴얼의 권장은 실제 사용자 수 측정이 아니다. |
| [tcpdump 프로젝트](https://github.com/the-tcpdump-group/tcpdump) / libpcap | 프로젝트 README는 tcpdump를 네트워크 감시·데이터 수집 도구로 설명하고 libpcap 의존성을 명시한다. 여러 OS의 기본 패키지·포트를 열거한다. | BPF 캡처 필터, NIC에서 기록까지의 수집 계층 | CLI/라이브러리 기준이며 Loom 화면 레이아웃 표본은 아니다. 설치 가능성은 사용량 통계가 아니다. |
| [Vector CANalyzer](https://www.vector.com/en/product/canalyzer/) | Vector는 Ethernet 옵션, 실시간 네트워크 관찰, 필터·로깅·재생, 진단 창을 명시하고 자동차 산업에서 세계적으로 쓴다고 설명한다. | 자동차 Ethernet 관찰과 진단을 같은 도구 안에서 다루되 전송 경계를 유지 | 채택 표현은 판매사 주장이다. 실행·라이선스 검증 및 정량 사용자 수 확인은 없다. |

- 출처 열람일은 모두 **2026-11-12**. 위 링크는 검색 결과가 아닌 실제 열람한 1차 자료다.
- [Microchip 개발자 도움말](https://developerhelp.microchip.com/xwiki/bin/view/applications/tcp-ip/network-tools/)도 Wireshark를 패킷 분석의 사실상 표준이라 부른다. 이는 외부 제조사의 평가이지 계측된 점유율은 아니다.
- [Wireshark DoIP 필드 참조](https://www.wireshark.org/docs/dfref/d/doip.html)는 `doip`, `doip.source_address`, `doip.target_address`, `doip.type`을 명시한다.
- [Wireshark 필터 가이드](https://www.wireshark.org/docs/wsug_html_chunked/ChWorkDisplayFilterSection.html)는 표시 필터가 캡처 파일 내용은 바꾸지 않는다고 설명한다.
- [Wireshark FAQ](https://www.wireshark.org/faq)는 libpcap의 캡처 필터와 표시 필터 문법이 다르다고 설명한다.

## 실제 Loom 상태와 증거 경계

- [ETH 서비스 화면](../evidence/ui-workbench/service-split-final/eth-all-connected.png)은 daemon 시뮬레이터의 `#/s/eth` 화면이다. Ethernet 별도 서비스, `mirror` 한 채널, DoIP·SOME/IP-SD 패킷 6행, 송신 접힘 영역, UDS 이동 링크가 보인다.
- 이 화면은 표시 필터 입력 예시를 보여 준다. 필터 **적용 결과**를 증명하지는 않는다.
- 패킷을 선택하지 않아 상세 트리·hex가 열린 모습, pcapng 파일 결과, 손실 행, 다중 ETH NIC의 동시 캡처는 이 화면에서 확인되지 않는다.
- 화면의 `[SIM]`, `[SIM tap]`, `synthetic` 표시는 물리 회선 증거가 아니라는 점을 분명히 한다.
- 실제 구현에는 상태·pps/bps·드롭·기록·시각 정밀도, 시작/정지, pcapng 내보내기, 필터 검사·저장·이력, 패킷 목록·상세 트리·hex, 송신 목록이 있다 (`ethPanel.ts`, `filterBar.ts`).
- 행은 `seq/시간/출발지/목적지/프로토콜/길이/정보/UDS/출처`를 표시한다. 선택 시 `eth.show`에서 상세와 바이트를 가져온다 (`ethPanel.ts:173–238, 314–353`).
- 상세 필드와 바이트 선택 연동은 `TreeHex`에 맡긴다. 위 스크린샷만으로 실제 연동의 성공을 단정하지 않는다.
- 시작 대화상자는 NIC, BPF, snaplen, promiscuous, 버퍼를 받으며 BPF가 기록에서 패킷을 제외한다고 안내한다 (`dialogs.ts:154–199`).
- 표시 필터는 서버의 `eth.filter.check` 후 적용한다. 적용 시 스트림을 재구독하고 저널 범위를 `eth.query`로 조회한다 (`filterBar.ts`, `ethPanel.ts:254–306`).
- Space의 일시정지 또는 과거 스크롤은 **캡처 중지**가 아니라 저널의 `COMPLETE` 범위 조회다. 최대 2,000행이면 잘림을 표시한다 (`ethPanel.ts:271–315, 541–558`).
- 라이브 행은 샘플일 수 있다. `samplingText`는 표시 비율과 기록 상태를 구분하며, source/record/display gap 행과 알 수 없는 드롭 수를 별도로 표시한다 (`ethPanel.ts:417–470, 519–553`).
- `pcapng` 버튼은 `eth.export` 작업을 시작한다. 성공 토스트는 완료 파일 검증이 아니라 작업 시작의 증거다 (`ethPanel.ts:361–366`).
- `↔UDS`와 `↔UDS?`는 `eth.link`의 `confidence`를 구별한다. 누르면 UDS 요청 참조로 이동한다 (`ethPanel.ts:195–228, 425–437`).
- `eth.pkt` 또는 DoIP 해독만으로 UDS 연결 성공, 특정 ECU 대응, 현재 Loom 진단 세션과의 동일성을 추론할 수 없다.

## 서비스 안의 작업 흐름 대안

| 대안 | 흐름과 배치 | 장점 | 비용·주의 |
| --- | --- | --- | --- |
| A. **현재 행 → 상세 → 바이트 유지** | 채널 탭 → 상태·캡처 제어 → 표시 필터 → 시간순 행 → 선택한 행의 트리·hex. UDS 참조는 해당 행에서 이동. | Wireshark의 검증된 정보 순서를 따른다. 현 구현을 재사용하므로 서비스 분리를 흐리지 않는다. | 좁은 화면에서는 상세가 접히거나 스크롤된다. 선택하지 않으면 하단이 비어 보인다. |
| B. 진단 우선 ETH 보기 | DoIP/UDS 필터와 관련 행을 위에 두고 일반 패킷은 보조 목록으로 배치. | DoIP 문제를 빨리 찾는다. | 원시 캡처가 진단 연결처럼 보일 수 있다. SOME/IP 등 일반 Ethernet 관찰을 밀어낸다. 현재 목표보다 큰 재설계다. |
| C. NIC별 분리 비교 | 복수 ETH 채널을 좌우 표로 놓고 각각 독립 필터·시각 기준·상태를 둔다. | 양쪽 링크의 시간차를 비교하기 쉽다. | 화면 폭과 시각 동기화·드롭 해석 비용이 크다. 실제 다중 NIC 운영 증거도 아직 없다. |

**권고: A.** 서비스 분리의 첫 단계에서는 현재 패킷 중심 흐름과 채널별 패널을 유지한다.
DoIP를 빠르게 찾는 저장 필터/프리셋과 정확한 UDS 참조 이동만 후속 소범위 개선으로 검토한다.
이때 필터 예시는 Loom 서버 문법으로 검증해야 한다. Wireshark 식을 그대로 복사해 동등하다고 가정하지 않는다.
B와 C는 진단 연결 메타데이터 또는 다중 NIC 실제 수요·테스트가 있을 때 다시 판단한다.

## 복수 인터페이스와 진단 경계

- `routes.ts:56–74`의 `eth` 서비스 프리셋은 **열린 ETH alias 각각**에 기본 패널 slot을 만든다. 한 NIC에 여러 서비스가 자동 연결된다는 뜻은 아니다.
- 현재 UI는 각 채널의 `ch`, 필터, 선택, 일시정지, 기록 상태를 별도 패널 인스턴스로 가진다. 서비스 이동은 장치를 새로 열거나 캡처를 시작하지 않는다.
- 통합 작업 공간은 여러 도메인을 함께 볼 수 있다. UDS 진단 경로는 열린 CAN·ETH 관찰을 함께 보여 주지만, 패널 병치만으로 ECU 연결을 표기하면 안 된다 (`routes.ts:65–77`).
- 캡처 BPF는 수집 전에 적용되어 저장 내용을 바꾼다. 표시 필터와 패널 일시정지는 저장된 데이터의 **조회 방식**이다. 이 세 상태를 같은 “정지/필터”로 합치지 않는다.
- pcapng는 인터페이스 식별·타임스탬프·메타데이터의 교환 형식이다. [Wireshark 파일 속성 가이드](https://www.wireshark.org/docs/wsug_html_chunked/ChStatSummary.html)는 pcapng의 캡처 환경·인터페이스 정보를 설명한다.
- DoIP는 ISO 13400 전송의 해독·필터 대상으로 ETH에서 관찰한다. 진단 연결 상태는 UDS manager의 실제 연결 정보가 기준이다.
- Hyundai/Kia **EthDiag는 사용자 제공 맥락에서 DoIP와 별개인 전송**이다. 공개된 공식 사양을 확보하지 못했다. 포트·패킷 형식·활성화 절차·디코더를 추정하지 않는다.

## 다음 구현 단계의 수용 확인

1. `#/s/eth` 직접 진입: ETH alias만 기본 탭에 나타나고, 비어 있으면 연결 안내가 보이며, 다른 서비스 이벤트가 패널을 추가하지 않는다.
2. ETH alias가 둘 이상일 때 각 탭의 채널명·캡처 상태·필터·선택·일시정지·내보내기 대상이 서로 섞이지 않는다. 통합 화면으로 이동해도 캡처가 재시작되지 않는다.
3. 캡처 BPF와 표시 필터의 차이를 대화상자에서 확인한다. 잘못된 표시 필터는 서버 오류 위치가 보이고, 유효한 적용은 저널 조회 범위와 결과 수를 명시한다.
4. 라이브 샘플·기록 상태·완전 조회·2,000행 잘림·커널/인터페이스 드롭 미확인을 각각 사실대로 표시한다. Space는 화면 조회 모드만 바꾸고 캡처를 정지시키지 않는다.
5. 행 선택은 해당 채널의 상세 트리와 hex를 열고 선택 범위를 맞춘다. snaplen 절단 또는 큰 프레임의 원본 접근을 표시한다.
6. pcapng 내보내기는 시작 표시뿐 아니라 작업 완료와 파일 열기/메타데이터 확인까지 별도 검증한다. 물리 NIC 시험 전에는 시뮬레이터 결과로만 표시한다.
7. `↔UDS` 정확 연결과 `↔UDS?` 추정 연결을 구분하고, 없는 연결은 비워 둔다. ETH 패킷이나 DoIP 표시만으로 ECU 링크 상태를 초록색 성공으로 표시하지 않는다.
8. DoIP 필터 예시는 Loom 문법 검사·실제 결과로 확인한다. EthDiag 지원은 공식 사양과 backend 증거가 생길 때까지 주장하지 않는다.

남은 불확실성: 물리 NIC의 권한·드롭 계수·타임스탬프 정확도, 상용 CANalyzer 실제 화면, 복수 NIC의 운영성, EthDiag 공식 사양.
