docs/plan/02-core-principle.md 준수
기능 / 상태: 실제 도메인 P1 스모크 / WORKING
기준: main 1a2e817; 기존 미커밋은 보존. 메인이 index 단독 소유, push 없음.
담당/소유: 메인 UART gateway tests/r1.rs; can_eth_smoke 담당 신규 can_eth_smoke.rs 및 CAN gateway 연결; uds_tc1_smoke 담당 신규 테스트와 UDS/TC-1 보고서. 하위 위임 없음.
완료 조건: 가능한 sim 경로를 실제 RPC+IPC로 실행하여 TX/RX·viewer 스트림·개입/STOP 증거 확보. 실패는 구체 반례로 반환, P1 전체 완료와 구별.
검사: 기능별 smoke + 변경별 fmt/clippy. 기존 demo STOP 해제 금지; 격리 temp HOME만 사용.
환경 제약: ps RSS 조회 operation not permitted. ego-browser launch macOS Connection invalid (exit 4); Full Access 사용 불가, 브라우저 반복 실행/우회 없음.
한도/사용량: 별도 제공 없음. 인수 담당 메인.

통합 체크포인트:
- UART cd6a737, UDS 진단 cae4fa7, CAN lifecycle+CAN/ETH smoke d26735c, UDS runner/DoIP ad6f16c 로컬 커밋. 각 보고서 reports/ 참조. root 통합 자체 리뷰 완료; CAN은 구현 담당과 다른 메인이 write risk/STOP 경로 리뷰.
- 사용자 최신 조건: 브라우저 검증 생략 금지. Ego Lite 우선, 실패시 Playwright 명시 허용. browser_smoke 담당 추가(신규 real-smoke 파일 및 보고서만 소유). 브라우저 launch/접근 차단으로 전체 과제 완료 아님.
- 원인: Ego CLI macOS 서비스 연결 실패(샌드박스 영향 추정), CUA Ego 앱 접근 자동 승인 거부(확정), Playwright Chromium MachPort registration Permission denied/SIGTRAP(확정). UI automationCapped는 실제 서버의 webdriver 클라이언트를 watch-only로 제한(코드 확인).
- 해결 제안: 사용자가 승인 가능한 실행 환경에서 Ego Lite 접근 허용 후 실제 input 검증. Playwright는 실제 UI 표시·STOP 회귀, 사용자 송신은 watch 정책을 우회하지 않음. 현재 세션 workspace-write/approval never는 root가 변경하지 않음. 앱 실행 여부와 에이전트 접근 권한은 별개.
- 공식 문서: https://learn.chatgpt.com/docs/sandboxing (macOS Seatbelt, spawned command inheritance, approvals/sandbox 구별, permissions control). 원인 확증용 외부 사용자 터미널의 동일 최소 probe A/B가 남음. 보안·샌드박스 설정 자동 변경 없음.
