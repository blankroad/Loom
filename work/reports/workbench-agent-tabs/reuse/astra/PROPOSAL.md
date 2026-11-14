# Astra — 핵심 관찰 패널 어댑터

TASK-CORE-PANELS와 최신 사용자 지시를 적용한 독립 수정안. 이전 whole-service 최소 너비/높이 정책을 폐기했다. Fable 구현은 읽지 않았다.

통합 보기에는 UART 원본 xterm/HEX와 MCU·AUX 세션 헤더, Ethernet 원본 패킷 목록/상세/hex 및 표시 필터, UDS 원본 요청·응답 기록/응답 순서/decoded/hex만 표시한다. UART의 상태·baud·RX/TX는 원본 헤더가 갱신하며 UDS 헤더는 원본 host의 현재 ECU(BCM 등)·transport를 읽는다. 세션 이름을 가짜 연결 정보로 덮어쓰지 않는다.

`core-panels.css`가 iframe 문서의 `data-astra-core` 속성 아래에서만 적용된다. 서비스 navigation, catalog, connection form, macro, request editor, global toolbar를 숨기고 남은 부모 grid의 named areas와 실제 row/column을 모두 재설정한다. 원본 JS/HTML/CSS 파일은 수정하지 않는다. 프레임 전체 축소나 1080px 최소 너비 없이 사용 가능한 viewport를 나눈다. 원본 목록 자체의 가로 스크롤/열 밀도는 유지한다.

`서비스 열기`는 해당 프레임을 최대화하고 속성을 제거하여 같은 문서의 전체 원본 조작부를 복원한다. `관찰 패널로 돌아가기`는 속성만 복구한다. 탭, 패널 선택, 배치, 최대화, 서비스 전환에서 iframe 이동/교체/src 재설정은 없다. 원본 서비스 내부 navigation이나 브라우저 새로고침은 별도 사용자 navigation이며 상태 보존 계약 밖이다.

상단 compact strip은 DEMO 관측, 원본 현재 UDS target, 대기/취소/완료 단계, 실제로 구독한 SIM 이벤트 시각을 표시한다. 작업 탭에는 기존 확장 timeline/journal이 있다. 성공 응답을 자동 주장하지 않으며 거래 이벤트 조건 만족과 진단 성공은 다르다. 작업 취소는 구독 관측 작업만 끝낸다. STOP은 별도 표시하며 엔진 바인딩이 없어 비활성이다. 자동 연결/송신이나 가짜 에이전트 실행은 추가하지 않았다.

**소스 바인딩:** canonical UART/ETH/UDS 원본 문서와 rich renderer 재사용, UDS `LoomWorkbench.subscribe/context/selectTxn` 연결. **엔진 바인딩:** 원본 로컬 SIM만 존재; 실제 daemon/device/agent/STOP 연결 없음. CAN은 독립 HTML 소스가 없으므로 가짜 whole-app을 추가하지 않았다. native component extraction은 별도 제품 작업이다.

검증은 Node 구문과 HTTP 제공 여부까지. 이번 수정의 렌더링/같은 Document 및 xterm 상태 검증은 root가 Opus5.5 `opus-validation w26` Ego QA로 전달한다. 이전 PNG/JSON은 보존했으며 이번 수정의 증거로 취급하지 않는다.
