docs/plan/02-core-principle.md 준수
# UART P1 실제 IPC 스모크

기능 / 상태 / 담당: UART IPC·live viewer·journal / DONE (스모크 범위) / 메인 직접 수행, 자체 리뷰. 독립 리뷰 없음.
기준: main 1a2e817 + crates/loom-gateway/tests/r1.rs diff. 기존 데모 STOP 유지.
구성: 격리 tempfile HOME, production daemon::start를 시험 프로세스에서 실행, 실제 loomd worker UART subprocess, 실제 TCP WebSocket agent/operate viewer. 브라우저 렌더나 별도 loomd daemon 프로세스 시험은 아님.

검사: CARGO_TARGET_DIR=~/.cache/loom-main-target cargo test -p loom-gateway --test r1 uart_sim_live_input_and_stop_smoke --locked --offline -- --nocapture
결과: 1 PASS, 2.77s. /private/tmp/loom-p1-uart-smoke-final.log
- sim://bootshell open, login prompt expect, agent root 전송, viewer secret 입력으로 shell prompt 응답.
- agent expect가 진행 중일 때 viewer 7-byte 입력 전송 성공; 입력 RPC 942.083 µs (화면 렌더 지연 아님).
- viewer subscription에 uart.data의 agent TX/user TX/device RX 모두 확인.
- viewer STOP 후 agent 송신 E_STOPPED. STOP release 수행 없음.
- 정상 shutdown으로 worker journal seal 후 같은 HOME 재시작. 이전 stream epoch의 query가 source=journal이며 라이브 UART 이벤트가 동일 값으로 모두 복구됨.
- cargo clippy -p loom-gateway --test r1 --locked --offline -- -D warnings PASS. /private/tmp/loom-p1-uart-smoke-lint.log

초기 시험에서 input sent를 batch 수 1로 잘못 기대했으나 실제 byte 수 7임을 확인하여 수정. timeout/worker error를 누락하지 않도록 응답 검사 강화. 종료 시 journal.close만 호출하던 harness는 shutdown_gracefully로 변경해 worker seal 후 TempDir 제거.
브라우저: ego-browser skill v2.2.0 확인. nodejs taskSpace 호출은 exit 4, macOS com.apple.hiservices-xpcservice Connection invalid / Failed to start ego lite. 사용자는 앱 실행 중임을 확인. CLI help에 안내된 범위에 기존 인스턴스 연결 복구 절차 없음. Full Access 사용 불가. 브라우저 화면 검증 미수행.
RSS: ps -o rss= -p PID 자체가 operation not permitted. 정식 30분 측정 판정 불가; echo-only soak 대체 실행 안 함.
남은 항목: 실제 브라우저 렌더/화면 지연, 도메인 혼합 TC-1, 정식 RSS. P1 전체 완료 아님.
커밋/푸시: 메인이 기능 단위 커밋 예정, push 없음. 한도/사용량 미제공. 후속 소유자 메인.
