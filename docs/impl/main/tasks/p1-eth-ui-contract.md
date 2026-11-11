docs/plan/02-core-principle.md 준수
기능 ETH canonical payload→UI 정합 / WORKING / Herdr foreground Claude Opus, 새문맥
목적 P1 실제UI 반례 최소수정. 소유 ui/src/panels/eth/{ethModel.ts,ethPanel.ts,eth.test.ts} 및 reports/p1-eth-ui-contract.md. 다른소스/Cargo/lock/index/commit/push 금지. root는browserE2E, tc1-live는UDS UI별도소유. 정식30분soak 실행binary 손대지말것.
실제계약 crates/loom-eth/src/model.rs 확인: capture_state backend:'sim'와 buffer_bytes_actual/requested U64문자열(현재UI는sim:boolean/buffer_bytes만읽음). eth.stats pps/bps/decode_lag_ms U64문자열인데 현재ethPanel은number만읽어통계0. canonical우선+합리적legacycompat. kernel_drop/if_drop unknown과total도비교하여거짓0표시방지. byte/U64 safe경계정직표시. 같은화면다른필드불일치도필요최소확인.
완료 실제Rust schema와일치payload를통한UI회귀테스트/targeted vitest+types+lint/자체리뷰. fixture존재하면재사용. browser권한거부해결안됐으므로jsdom을실브라우저PASS로기록금지. 구현상세문구제품노출금지. 10분한도, 하위agent금지. 준비후보고파일경로한번반환, 소유main. role반환은프로세스/로그삭제아님.
