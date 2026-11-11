# UI workbench / DONE

기준 main ea49d60. 사용자 요청: UI 벤치마킹→계획→병렬 구현→실제 브라우저 검증→커밋. 기능 범위와 파일 소유는 [작업 계획](../tasks/ui-workbench.md).

## 벤치마킹과 적용 결정

| 공식 참고 자료 | 관찰한 사용 패턴 | Loom 적용 |
| --- | --- | --- |
| [VS Code UI](https://code.visualstudio.com/docs/editing/getting-started/userinterface), [Custom Layout](https://code.visualstudio.com/docs/configure/custom-layout) | 탐색과 편집 영역을 분리하고 여러 항목을 탭·그룹으로 배치 | 좌측 도메인별 인터페이스 탐색, 중앙 대상별 도크 탭, 명확한 포커스·활성 상태 |
| [Postman navigation](https://learning.postman.com/docs/getting-started/basics/navigating-postman) | 헤더·사이드바·워크벤치·상태 영역의 역할 분리, 대상별 탭 | 상단 연결·권한·STOP, 검색 가능한 연결 목록, 설정의 점진적 공개 |
| [Saleae Logic navigation](https://www.saleae.com/support/logic-software/viewing-and-analyzing-data/navigating-the-software) | 장치·채널 설정과 데이터 관찰 영역 구분 | 인터페이스 이름·실제 연결 상태·보기 선택 구분, 밀도 높은 데이터 영역 보존 |

상표·에셋 복제 없이 작업 방식만 적용했다. 기능 비교나 완전한 제품 동등성을 주장하지 않는다.

## 구현

- 일관된 색상·타이포·컨트롤·간격, 명시적인 키보드 포커스, 반응형 셸. 최초 기본 화면은 연결 시작 안내. STOP/환경/세션 권한·연결 상태는 유지.
- 인터페이스 검색·도메인 그룹·접기, 이름별 보기. UART terminal/trace 및 CAN/UDS/ETH/PSU의 실제 구현된 패널만 제공. 여러 인터페이스의 탭을 구분하고 같은 대상을 다시 선택하면 기존 탭 활성화.
- 연결 창: UART 시뮬레이터/시리얼 포트·보레이트, CAN 시뮬레이터, UDS DoIP 시뮬레이터/호스트, Ethernet sim/live 캡처. UART 포트 조회·직접 주소 입력. 실제 장치 경로는 제공하되 이번 수용에서 실제 하드웨어에 접근하지 않는다.
- 모든 연결 요청은 기존 ActionGate와 서버 권한·정책을 통과하며 자동 재시도하지 않는다. 보기 선택은 연결·송신을 수행하지 않는다. 실패는 UI에 표시하고 연결 성공을 추정하지 않는다.
- ETH live 및 UDS link object 등 네이티브 상태 해석 보완. 연결 상태를 모르는 경우 unknown 유지; PSU 출력 OFF를 연결 종료로 간주하지 않는다.
- 실제 브라우저에서 발견한 재접속 결함 수정: `core.channels` 및 중첩된 도메인 SnapshotItem을 해석해 패널을 열기 전에도 대상 목록·상태를 복원한다. SDK는 상한 있는 읽기 전용 스냅샷 상태를 이벤트 히스토리와 분리해 제공하고, UDS 콘솔 및 UART 연결·입력권 메타데이터가 이를 복원한다. 오래된 백로그 상태가 현재 스냅샷을 덮어쓰지 않는다.
- 실제 P1 브라우저 회귀에서 발견한 자동 탭 활성화 수정: 새 장치의 패널은 배경으로 추가하여 현재 딥링크 트레이스와 키보드 대상을 유지한다. 사용자의 명시적인 탐색기·추가 동작은 해당 탭을 활성화한다.

## 검증과 제약

최종 UI 통합 `pnpm --dir ui check`: 스키마 33, 타입·린트, 40개 파일 **428 tests PASS**. `pnpm --dir ui build` PASS. JS `index-Ecu5RWJd.js` SHA-256 `8b0c72ae21c1765d80bfd3179e41b1d7ce445512a4420a5cfa96e562c98e1e17`. 로그 `/private/tmp/loom-ui-workbench-final-check.log`, `/private/tmp/loom-ui-workbench-build.log`.

최종 UI를 포함한 `cargo build -p loom-gateway --bin loomd -p loom-cli --bin loom --locked --offline` PASS, 내장 UI 자산 검사 11 PASS. Rust 도메인 구현은 변경하지 않아 전체 workspace 검사를 반복하지 않았다. [브라우저 보고](ui-workbench-browser.md), [셸 인계](ui-workbench-shell.md), [인터페이스 인계](ui-workbench-interfaces.md), [재접속 수정](ui-workbench-latejoin.md).

최종 고정 빌드 `final2` 실제 수용:
- **Ego Lite 61/61 체크 PASS**, RESULT PASS, runner exit 0. UI에서 UART 2개·CAN·UDS·ETH 연결, 채널별 입력·데이터 분리, 검색, 탭 중복 방지, 저장→reload, 두 번째 뷰어 발견·메타데이터 복원·입력, 1440/1024/768/390 CSS px 가로 넘침 없음·STOP 접근·실제 차단을 확인했다.
- 기본 배율과 렌더 완료 후 실제 UART 글꼴 가독성을 main도 캡처로 확인했다. Ego의 RAF cadence 제한은 이전 조사와 같이 구분하며, Ego 성능 PASS로 오인하지 않는다.
- **변경하지 않은 canonical Playwright 실제 시뮬레이터 suite 5/5 PASS**, skipped/unexpected/flaky 0, 19.0초, exit 0. UART·CAN·UDS·ETH 사용자 개입/STOP + powered TC-1 live·원본 journal verdict. CAN 20개 표시 표본 p95 **82.9ms <150ms**, 측정 기준 유지.
- 증거: `../evidence/ui-workbench/final2/final-062029.out`, `../evidence/ui-workbench/p1-suite-final2/pw-results.json` 및 각 스크린샷. 초기 실패를 덮어쓰지 않았다.

처음 검증의 late-join FAIL과 기존 P1 suite UART 1 FAIL/나머지 4 PASS 원본은 보존했다. 이를 전체 수용 PASS로 세지 않는다. 터미널 글꼴 조사는 기본 Ego DPR2에서 정상 13px 표시를 확인했고 DPR1 에뮬레이션에서 WebGL 캔버스와 링크 레이어의 배율이 달라지는 증거를 남겼다. upstream xterm canvas font 공백 가설은 현재 브라우저에서 재현되지 않아 제품 패치를 적용하지 않았다.

레이아웃 지속성은 명시 저장→저장 레이아웃 선택→새로고침이다. 마지막 작업 공간 자동 저장은 추가하지 않았다. 기본 작업 공간 초기 보기는 최대 32개로 제한하고 탐색기 목록은 유지한다. CAN 연결 창은 sim만 제공하며 UDS 연결 창은 DoIP 경로다.

범위: UI와 연결 작업 흐름. UDS over CAN-TP 추가, 실제 하드웨어 수용, 기본 IPC TC-1 fence 프로토콜은 이번 변경 범위가 아니다. 기존 소스 외 dirty 문서는 보존하며 이 커밋에 포함하지 않는다.

## 담당·수명

Herdr foreground: wC/ui-shell Sol, wD/ui-interfaces Sol, wF/ui-latejoin Sol, wE/ui-qa Opus. 각 독립 모듈 전용 workspace, 내장/백그라운드 하위 에이전트 없음. main만 index/commit 소유. wC·wD·wF는 파일 인수 후 Ctrl-D 정상 종료, wE는 Claude `/exit` 정상 종료 후 각각 workspace 제거 확인. 최종 목록에는 MAIN w8만 남았다. Ego TaskSpace 2와 자체 harness/daemon/TC-1/Playwright/control 파일 정리도 확인했다. 사용자 기존 작업과 데몬은 유지했다.

코드·보고서 diff whitespace 검사는 PASS. 시험 stdout·실패 원본은 후행 공백/빈 줄까지 그대로 보존했다. 이 기능은 로컬 커밋 대상으로 완료했으며 이번 요청에서 원격 푸시는 수행하지 않는다.
