docs/plan/02-core-principle.md 준수
기능/상태: UDS 실제payload→UI 정합수정 / WORKING / HerdrforegroundClaudeOpus 새컨텍스트
기준 main37feca4+UIbrowserWIP보존。TC기능인수main，과거전체대화복제금지。
소유: ui/src/panels/uds/{udsModel.ts,consolePanel.ts,uds.test.ts}(필요새테스트만),보고 reports/p1-uds-ui-contract.md。다른UISDK/store/browserE2E/native/Cargo/index/commit/push 수정금지。main은browserE2E별도소유，메모리30분고정binary진행중(건드리지않음)。
새실제반례(main정적확인): backend crates/loom-uds/src/payload.rs StatePayload는 link:{state,cause}, session:{id,name,confidence}, security.level numeric|null, tester_present:Option, lease canonical필드。UIudsModel은legacy link string/session.value/security.level string/tp/lease.owner_origin,actor만읽어실제상태가?나잠금으로보임。console detail은legacy req_hex/resp_hex만읽어실제bytes/resp_bytes가누락됨。함께실RPCdryrun/rows의canonicalblock/decoded/bytes/risk와비교해필요최소고침。NRC숫자→hex 현재hexByte정상，성공으로거짓표시금지，unknown/secret/raw_ref불명은정직표시。
완료: 실제Rust serdefixtures(loom-uds/fixtures) 또는실gateway로그의정확payload를UI에넣어session/link/security/lease/TP/byte detail이맞게보이는테스트。가능한한실fixtureimport，새mock형태로만테스트하지않음。legacy mock/replay호환은합리적으로유지하되canonical우선。위젯에구현상세노출/브라우저권한회피금지。일반외부web조사불필요，local정본읽기。
운영 Herdrforeground직접/하위agent금지，10분한도，pnpm targeted test/types/lint필요검사+자체리뷰후READY_TO_INTEGRATE보고。브라우저현재자동승인거부로실렌더검사는main이별도，코드unit을browserPASS로보고금지。세션/로그보존，기능반환시소유main。
