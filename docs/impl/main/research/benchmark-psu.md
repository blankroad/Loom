# PSU 서비스 벤치마크 — READY_FOR_REVIEW

- 범위: PSU 전용 작업 공간의 다음 설계 판단. 현재 서비스 분리 구현을 기준으로 한다.
- 기준: `docs/impl/main/tasks/benchmark-psu.md`, `service-separation.md`, 실제 simulator 브라우저 캡처, 관련 UI 및 PSU gateway 코드.
- 자료 열람일: **2026-11-12**. 아래 상용 제품은 문서만 조사했다. **상용 소프트웨어 실행 및 물리 전원 장치 시험은 하지 않았다.**
- 채택 증거는 배포 경로와 지원 장비 범위이다. 설치 수·실사용자 수 자료는 발견하지 못했으므로 인기 순위나 “가장 많이 사용” 판단을 하지 않는다.

## 1. 대표 도구와 검증된 근거

| 도구 | 공식 자료에서 확인한 작업 흐름 | 실제 채택을 뒷받침하는 범위와 한계 |
| --- | --- | --- |
| Keysight PathWave BenchVue Power Supply BV0003B | 전원 선택·연결, 장비 읽기 및 설정, 시간별 데이터 로깅, 저장 로그 조회와 내보내기. [제품 설명](https://www.keysight.com/zz/en/product/BV0003B/pathwave-benchvue-power-supply-app.html), [공식 도움말 개요](https://helpfiles.keysight.com/Standalone_BenchVueSoftware_PW_HelpFiles/PWPSApp/Content/General/Welcome.htm) (2026-11-12) | Keysight가 상용 제품으로 제공하고 [기술 개요](https://www.keysight.com/content/dam/keysight/en/doc/ungate/technical-overviews/PathWave-BenchVue-Software.pdf)에 여러 지원 모델 계열을 열거한다 (2026-11-12). 이는 호환 범위이지 사용자 수는 아니다. |
| NI InstrumentStudio + NI-DCPower | 감지된 장비 패널, 채널별 출력 레벨·전류 제한·출력 ON, 다중 장비/패널, 측정 캡처. [NI 측정 절차](https://knowledge.ni.com/KnowledgeArticleDetails?id=kA00Z000000P8tPSAS&l=en-US), [제품 설명](https://www.ni.com/en/shop/electronic-test-instrumentation/application-software-for-electronic-test-and-instrumentation-category/instrumentstudio.html) (2026-11-12) | [NI-DCPower 21.3.1 릴리스 노트](https://www.ni.com/pdf/manuals/sw-ni-dcpower-21.3.1.html)는 21.0부터 InstrumentStudio가 이전 soft front panel을 대체하고 드라이버 설치 시 자동 설치된다고 명시한다 (2026-11-12). 배포 경로 증거이며 실제 이용률은 아니다. |

- BenchVue [Instrument Settings](https://helpfiles.keysight.com/BenchVueSoftware_HDML5HelpFiles/PSApp/English/Content/GUI/Instrument%20Settings%20Tab.htm)는 채널별 전압 출력·전류 제한, OVP/OCP, 표시할 V/I trace를 설명한다 (2026-11-12).
- 같은 제품의 [Data Logger](https://helpfiles.keysight.com/BenchVueSoftware_HDML5HelpFiles/PSApp/English/Content/GUI/Data%20Logger%20Mode.htm)는 측정 시작·종료·간격과 저장 로그를 별도 동작으로 둔다 (2026-11-12).
- NI [2020 릴리스 노트](https://www.ni.com/pdf/manuals/377562f.html)는 SMU/Power Supply 패널의 다채널 세션과 독립 채널을 명시한다 (2026-11-12).
- NI [InstrumentStudio 탐색 문서](https://www.ni.com/docs/en-US/bundle/instrumentstudio/page/interface.html)는 패널 추가, 레이아웃, 데이터 캡처, 전체 출력 정지 메뉴를 구분한다 (2026-11-12).
- 이 근거는 전원 작업의 패턴만 제시한다. NI의 SMU 특화 기능을 Loom의 일반 PSU 기능으로 간주하지 않는다.

### 벤치마크에서 채택할 정보 구조

- 장비 목록/연결 선택과 출력 제어를 분리한다. BenchVue 도움말에서도 장비 선택은 Instrument View와 다른 진입점이다.
- 전압 **설정치**와 전류 **제한치**는 목표값이고, 측정 V/I는 관측값이다. 한 줄에 있어도 라벨을 다르게 한다.
- 보호 상태 OVP/OCP와 CV/CC 동작 모드는 서로 다른 상태로 표시한다. 특정 장비가 주지 않는 상태를 계산해 채우지 않는다.
- 측정 trace는 시간·결측을 포함한 관측 화면으로 두고, 로그 파일과 내보내기는 저장 동작으로 분리한다.
- NI의 다채널 세션은 한 장비의 복수 채널 또는 복수 장비 채널을 모두 허용한다. Loom에서는 실제 PSU 별칭/채널 식별을 먼저 정의해야 한다.
- 두 제품의 export 지원을 근거로 Loom에 파일 내보내기가 이미 있다고 주장하지 않는다.

## 2. Loom의 실제 상태와 증거 경계

- `ui/src/shell/routes.ts`의 `#/s/psu`는 현재 OPEN PSU 채널의 `psu.gauge`만 배치한다. `ui/src/store/channels.ts`는 출력 `out`으로 연결을 추정하지 않고 `connected` 등 명시 상태를 읽는다.
- [실제 브라우저 캡처](../evidence/ui-workbench/service-split-final/psu-all-connected.png)는 daemon simulator의 PSU 서비스에 연결된 `rail` 하나와 **출력 OFF**를 동시에 보여 준다. 타 서비스도 연결돼 있으나 PSU 화면에는 해당 PSU 패널 하나가 나타난다.
- 같은 캡처에는 설정 `0.00 V / 1.00 A`, 측정 `—`, 60초 창 `0점`, 보호 `ok`, 일반 출력 ON, R3, 설정 R2, 비상 OFF가 보인다. 측정 누락을 0으로 바꾸지 않은 상태이다.
- `ui/src/panels/psu/psuModel.ts`는 설정 V/I와 측정 V/I/P를 다른 필드에서 읽고, `mode`와 `prot`, 측정 age, `source.gap`의 단절점을 유지한다.
- `ui/src/panels/psu/gaugePanel.ts`는 60초 V/I 추세를 표시하며 결측 구간을 잇지 않는다. 측정 손실 횟수를 표시하지만 기록 시작/내보내기 UI는 없다.
- 일반 `출력 ON/OFF`는 `operate` 권한과 쓰기 잠금 규칙을 따른다. 상태가 미확인일 때 일반 출력 버튼을 비활성화한다.
- `! 비상 OFF`는 별도 action으로 `watch` 권한, 확인 없음, 안전 방향을 선언하고 쓰기 잠금 중에도 버튼이 활성이다. 일반 OFF와 정책을 합치면 안 된다.
- PSU gateway의 `psu.connect`는 현재 sim만 허용한다 (`crates/loom-psu/src/gateway/handlers.rs`). 실물 PSU용 SCPI/드라이버 UI를 현행 기능처럼 적지 않는다.
- PSU gateway는 채널 없는 `psu.out{state:"off"}`를 알려진 전원 전체에 적용할 수 있지만, 현재 패널 비상 OFF는 채널을 명시한다. 이 둘을 같은 범위의 버튼처럼 표시하면 안 된다.
- 기존 [통합 초안](service-tool-benchmarks.md)은 방향 제시용이다. 이 보고서의 화면 관찰과 gateway 범위 검토를 근거로 PSU 결정만 좁힌다.

### 화면 해석의 주의점

- `rail` 행의 `연결됨`은 장치 세션 상태이며 패널의 `출력 OFF`는 전기 출력 상태다. 동시에 참일 수 있다.
- `실측 —`와 `0점`은 캡처 시점에 이 패널에 측정 샘플이 없었다는 뜻이다. 센서 고장 또는 장치 사용 불가라고 단정하지 않는다.
- 캡처의 권한 표시는 `operate` 하나다. `watch` 비상 OFF 정책은 패널 manifest와 action gate 설계에서 확인했으며, 이 캡처만으로 watch 클릭 결과를 입증하지 않는다.
- 캡처의 toast는 다른 서비스가 다시 열린 기록이다. 그 toast만으로 PSU 이동이 장치를 재연결했다고 해석하지 않는다.

## 3. 이 서비스 안의 작업 흐름 대안

| 대안 | 사용자 흐름과 배치 | 장점 | 비용·위험 |
| --- | --- | --- | --- |
| A. 현행 단일 채널 게이지 유지 | OPEN `rail` 탭에서 출력 상태→설정·측정→60초 그래프를 한 화면에 본다. | 서비스 분리 범위가 작고 현재 캡처 및 코드와 일치한다. | 다중 전원 비교가 탭 전환에 의존하며 기록/export의 진입점이 없다. |
| B. **채널 요약 목록 + 선택 상세** | PSU 서비스 상단에 채널별 연결/출력/설정/측정/보호/age를 요약하고 한 채널의 현행 게이지를 상세로 연다. | 여러 전원을 구별하면서 `out`과 연결을 혼동하지 않는다. 현재 패널과 route를 재사용할 수 있다. | 좁은 화면에서 요약 열이 많고 상태 동기화 검증이 필요하다. |
| C. 다채널 카드/그래프 동시 배치 | 채널마다 설정·측정·출력 카드와 추세를 나란히 두고 기록/export 제어를 둔다. | NI의 다중 패널, BenchVue의 채널 설정·로깅 흐름에 가깝다. | 채널 수가 늘면 밀도가 높고, 기록/export는 별도 데이터 범위와 파일 동작 정의가 필요하다. |

### 권고

- 이번 **서비스 분리 단계는 A를 유지**한다. 사용자의 우선순위가 도메인 경계이고 실제 simulator 화면에서 PSU 단독 라우트가 확인됐다.
- 다음 PSU 전용 설계 단계에서 **B를 먼저 설계**한다. 현재 카드와 행동을 재사용하며 다중 전원 상태를 드러낼 수 있다.
- 채널별 추세 비교 또는 장기 기록 요구가 실제로 생기면 C의 그래프·export 부분을 별도 단계로 검토한다. 지금 60초 메모리 창을 저장 로그라고 표시하지 않는다.
- B에서 상세 패널의 비상 OFF는 선택 채널에만 적용한다. 기존 `core.stop{scope:"all"}`은 새 agent·script 쓰기와 잡을 멈추는 전역 자동화 STOP이며, PSU 출력을 모두 끄지 않는다. 미래의 전체 출력 비상 OFF가 필요하다면 별도 동작으로 정의해야 한다.
- 채널 요약의 기본 정렬은 실제 별칭으로 고정해 상태 갱신 때 카드가 움직이지 않게 한다. 이는 제안이며 구현된 기능은 아니다.
- 요약에는 `연결됨/해제됨/미확인`과 `출력 ON/OFF/미확인`을 별도 열로 둔다. CV/CC는 보고된 경우에만 표시하고 보호 상태도 별개로 둔다.
- 상세에는 `설정 V / 제한 A`와 `실측 V / 실측 A / P`를 묶음별로 표시한다. 측정 없음과 stale age는 명확히 표시한다.
- 기록/Trace 일시 정지, 일반 출력 토글, 채널 비상 OFF, 연결 해제는 서로 다른 동작이다. 서비스 이동과 탭 닫기는 이 동작들을 실행하지 않는다.
- `STOP 전체`는 agent·script 쓰기와 잡의 정지, 채널 비상 OFF는 해당 PSU 출력 OFF로 문구를 분리한다. STOP 후에도 출력이 유지될 수 있으며 기존 STOP 패널은 이를 `출력 유지 중`으로 보여 준다. 사용자 입력을 모두 막는다고 쓰지 않는다.

## 4. 다중 인터페이스 동작과 다음 단계 수용 점검

- PSU 서비스는 OPEN PSU 별칭만 투영한다. UART/CAN/Ethernet/UDS 이벤트는 PSU 패널이나 선택 채널을 바꾸지 않는다.
- 통합 작업 공간에는 PSU와 다른 도메인을 함께 둘 수 있다. 이때도 장비 연결·출력은 route와 독립인 공유 상태를 사용한다.
- PSU 별칭 둘 이상일 때 각각의 `connected`, `out`, `mode`, `prot`, 측정 age와 gap을 별도로 표시하고 한 채널의 조작이 다른 카드에 반영되지 않는지 확인한다.
- 서비스 간 이동, reload, 탭 닫기 뒤에도 연결과 출력이 그대로인지 확인한다. 출력 OFF인데 연결됨인 채널은 목록과 서비스 route에 남아야 한다.
- 새 측정이 없으면 `—`와 경과 시간을 유지하며 그래프를 임의 보간하지 않는다. `source.gap`은 끊긴 선과 손실 표시로 검증한다.
- 설정값 적용은 명시 버튼과 R2로 유지한다. 입력 변경만으로 RPC가 나가지 않으며 실패 메시지를 확인한다.
- 일반 ON/OFF는 권한·쓰기 잠금을 확인하고 비상 OFF는 watch에서도 즉시 가능해야 한다. 전역 STOP 후 출력 유지 표시, agent·script 쓰기/잡 정지, 채널 비상 OFF의 실제 출력 변경을 각각 시험한다.
- 연결 미확인 상태에서 `out`만으로 OPEN을 만들지 않고, 출력 상태 미확인에서 일반 토글을 막는다.
- 기록/export를 추가할 때는 데이터 소스, 기간, gap/시간 정보, 파일 형식을 먼저 정의하고 60초 플롯과 저장 기록을 구별한다.
- 검증은 simulator daemon과 실제 브라우저를 사용하되, 실물 장비 동작·SCPI 지연·상용 UI의 실제 사용성은 별도 검증 항목으로 남긴다.

## 남은 불확실성

- 실제 장비별 채널 구조·보호 코드·측정 주기·출력 유지 동작은 sim 결과만으로 확정할 수 없다.
- BenchVue와 InstrumentStudio의 실사용자 수 또는 시장 순위에 관한 1차 계량 자료는 확인하지 못했다.
- 기록/export가 Loom의 어느 기존 기록 API와 연결될지는 이 서비스 분리 범위에서 결정하지 않았다.
