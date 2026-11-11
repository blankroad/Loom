docs/plan/02-core-principle.md 준수
기능/상태: TEST 실제 Registry command/risk adapter 인수 / WORKING / Herdr Claude opus. 기준 main 302a5d5 + UDS worker 병렬 변경(타 소스 읽을 때 변경 중임 주의).
목적: TestModule::new() commands=None으로 TC-1이 첫 단계부터 E_NOT_IMPLEMENTED인 문제를 해결할 실제 Registry command/risk 어댑터를 구현·검증한다. 단순 가짜 명령표/기본 R0 위험 금지.
소유: 신규 crates/loom-gateway/src/test_services.rs 및 필요 신규 TEST 서비스 시험, docs/impl/main/reports/test-services-foreground.md. gateway domains.rs/daemon.rs/Cargo.toml 등 기존 공통 파일은 메인 소유이므로 필요한 변경은 별도 patch로 제안만. crates/loom-test 수정 필요하면 먼저 보고. UDS/다른 도메인 소스 수정 금지.
먼저 읽기: reports/uds-tc1-smoke.md, crates/loom-test/src/{engine,seams,wire}.rs 필요한 부분, loom-test-format CommandTable/RiskProvider, 실제 Registry/hook/risk 계약. PredicateEngine::from_registries 재사용.
완료 조건: 실제 명령 등록/인자 검증/도메인 위험 분류를 사용해 TEST에 주입 가능. 누락 source fence/recording health/approval/finally 등은 명시적으로 unavailable 유지하여 거짓 PASS를 만들지 않는다. 가능한 통합 범위와 남은 hooks 후속을 정확히 보고. 공유 파일 통합 전 검증 불가면 필요한 patch·명령으로 반환.
운영: Herdr 패인 대화형 포어그라운드; 추가 하위/내장 에이전트 금지. index/commit/push/reset/clean 금지. 기존 미커밋/데모 STOP 보존. 브라우저/보안 권한 우회 금지. CARGO_TARGET_DIR=~/.cache/loom-main-target. 자체 리뷰·검사 후 READY_TO_INTEGRATE, 차단은 BLOCKED. 토큰/시간한도 미제공, 사용량 미상. 보고서 반환 후 추가 기능 시작 금지.
