docs/plan/02-core-principle.md 준수
기능 / 상태 / 담당: UART SharedWorker 통합 / WORKING / 메인 직접 수행
기준: main f3b7577; 기존 UART worktree 미커밋을 보존하며 필요한 patch만 이식.
소유: crates/loom-uart{,-worker}, crates/loom-gateway/src/domains.rs
완료 조건: CORE R1 의존성으로 UART 시험·lint 통과, production IPC가 shared 진입점 사용.
비범위: STOP 해제, 기존 데모 송신, 전체 P1 수용 선언, 다른 도메인 통합.
검사: UART 전체 시험/clippy, gateway check; 자체 리뷰. 한도/사용량 미제공.

결과 / 상태: DONE (UART 코드 통합 범위만); 자체 리뷰, 독립 리뷰 없음.
변경: UART 준비 patch와 r1_wire를 main에 이식; 테스트 입력을 R1 conn_id/input_seq로 전환; loomd production UART 분기를 worker::run(shared)으로 변경.
검사: cargo test -p loom-uart-worker -p loom-uart --locked --offline의 성공 테스트 131개, worker_e2e 3건은 구형 FakeWorker.input의 누락 ID로 실패. 해당 테스트를 실제 R1 Input으로 수정 후 --test worker_e2e --test r1_wire 14/14 PASS (앞 성공 4개와 중복). 재실행 로그 /private/tmp/loom-uart-shared-retest.log. cargo clippy -p loom-uart -p loom-uart-worker -p loom-gateway --all-targets --locked --offline -- -D warnings PASS. cargo fmt --all -- --check 및 git diff --check PASS.
소스 기준: f3b7577 + 이 기능 커밋의 crates diff. CARGO_TARGET_DIR=~/.cache/loom-main-target.
한계: 실제 daemon IPC 동시 입력/STOP 및 웹 종단 간 증거는 아직 없음; P1 수용 아님. 데모 STOP 유지. 기존 worktree 원본 및 타 팀 변경 보존.
커밋: 기능별 로컬 커밋; 푸시 안 함. 다음 담당 메인: UDS b23171a 필요한 patch 인수, 이후 실제 도메인 수용/TC-1/RSS.
