docs/plan/02-core-principle.md 준수
기능 / 상태 / 담당: UDS 실제 domain 연결 인수 / WORKING / Herdr Claude opus, 메인 통합
기준: main 302a5d5, /Users/ctmctm/workspace/Loom. 실행전 HEAD/status 재확인. 공유 index는 메인 단독 소유, worker commit/push 금지.
목적: 현재 E_NOT_IMPLEMENTED인 UDS connect/request/status를 기존 .worktrees/uds 준비 domain 코드로 실제 sim에 연결하고 RPC 스모크로 검증.
먼저 읽기: docs/impl/main/reports/uds-tc1-smoke.md, docs/impl/uds/reports/p1-runner-doip-sol-direct.md; .worktrees/uds status/diff의 필요한 domain 파일만. runner/DoIP는 ad6f16c에 이미 통합되어 두 crate 최종 tree는 b23171a와 일치.
소유: crates/loom-uds 및 신규 UDS gateway smoke 테스트, docs/impl/main/reports/uds-domain-foreground.md. gateway domains.rs/Cargo manifest 공유 변경은 필요한 최소 diff를 별도 파일에 제안하고 메인에게 요청. 다른 domain/TC-1/UI/runner/DoIP/기존 미커밋 문서 변경 금지.
방식: Herdr 패인 내 대화형 포어그라운드, 내장/추가 하위 에이전트 금지. git branch merge/cherry-pick/index/reset/clean 금지. 필요한 patch만 이식. 미커밋 보존. 기존 demo/STOP/실하드웨어 접근 금지.
완료: 실제 sim connect→request→정확한 응답·이벤트, 거짓 성공 없음, 영향 검사·lint·자체 리뷰 증거와 남은 문제를 READY_TO_INTEGRATE로 반환. 의존성 실패/권한/사용량 차단은 보고하고 임의 모델 전환/우회 금지. CARGO_TARGET_DIR=~/.cache/loom-main-target. 브라우저 접근 거부/권한 제한을 worker로 우회하지 말 것.
한도: 별도 시간/토큰 한도 없음, 제공 사용량 미상. 반환 보고서에 SHA/diff파일/명령/결과/다음행동 기록. 알림은 herdr agent prompt main-orch가 busy면 억지 전송하지 않고 보고서 READY 기록으로 충분. 역할 반환 후 추가 기능 시작 금지.

메인 통합 메모: 준비 UdsModule::new(clock)는 gateway와 같은 clock 원점을 요구한다. daemon::start는 cfg clock을 만든 뒤 domains::builtin()을 호출한다. 메인이 builtin_with_clock(clock) 인터페이스와 daemon callsite를 소유하여 생성자 변경 통합 시 clock을 공유할 계획. worker는 UdsModule 생성자 그대로 유지하고 공유 gateway 파일 수정하지 않는다.
メイン統合済み（作業中）: Cargo.lockはmanifestに応じた既存5dependency追記済み。domains::builtin_with_clock(clock) + daemon callsite + UdsModule::new(clock)変更済み。共有ファイルはworker提案で上書きせずこのmain変更を利用して検証すること。
정정: 담당자가 unit UdsModule 유지 + HubClock::from_ctx로 개선한 것을 확인. main의 builtin_with_clock/new 변경은 불필요하여 제거. 원래 builtin/UdsModule 등록 유지. main gateway 스모크를 ctx 시계 구성으로 실행.
