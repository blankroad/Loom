docs/plan/02-core-principle.md 준수
기능 TEST anchor.where kind별schema 검증 / WORKING / HerdrforegroundClaudeOpus 새문맥
소유 crates/loom-test/src/wire.rs 및 해당회귀tests, reports/p1-anchor-contract.md. 다른모듈/Cargo/lock/index/commit/push 금지. 공유CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=$HOME/.cache/loom-main-target 사용. 정식30분soak binary불변.
실제TC1보고 finding3: anchor_ts의Expr.check(&common.specs)는모든도메인같은포인터중처음schema만취함. anchor.kind=psu.write의 /payload/state 'on'이UARTstate enum(closed/opening/...)로거부된다. anchor의kind/도메인범위로맞는schema체크; kind생략/다중kind의ambiguity는정직failclosed하며기존valid/unknownfield검증지키기. 정본schema확인필수. 필터조건일반검증에도같은문제가있으면보고하되소유범위최소변경. test/kind간다른enum반례회귀+관련existing tests+fmt/clippy,자체리뷰. browserPASS주장금지. 하위agent금지. 10분한도,완료보고경로한번반환/소유main/세션로그유지.
