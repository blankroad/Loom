docs/plan/02-core-principle.md 준수
기능 CAN predicate source scope independent review / WORKING / HerdrforegroundClaudeOpus 새문맥
읽기전용 검토. root구현중인 crates/loom-can/src/{gateway,signals}.rs 및 tests/{gateway,signals}.rs 변경에대한독립리뷰. 소스/index/commit/push 변경금지. report reports/p1-can-predicate-scope.md 만소유. 기존UI UDS기능은main인수.
발견 TC1보고의frame_match는TEST주입filter/ch가deny_unknown으로거부됨. 단순제거하면filter.kinds/origins/request_id가적용되지않는것도확인(TEST는domain/channel만창을줄임). root는PredicateScope typed parser로domaincan/channel/ch합치/kinds/origins/request_id를frame_match와signal_range에적용하고filter/ch는술어키parse전제거. origin직접인자는기존대로추가교집합. 완전창제외는Mismatch/incomplete는Inconclusive. 원본devicewitness경계변경금지.
불필요범위확대없이거짓PASS/타도메인허용/예산상한/기존호출회귀검토. root검사로그 /private/tmp/loom-p1-can-scope-final.log 및 /private/tmp/loom-p1-can-signal-scope.log(실행중). 재검사필요시기존sharedtarget만CARGO_INCREMENTAL=0사용. 메모리soakbinary실행/권한제약손대지말것. 7분한도,하위agent금지,보고경로한번반환/role해제/세션로그유지.
