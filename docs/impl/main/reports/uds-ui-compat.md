docs/plan/02-core-principle.md 준수
기능/상태/담당: UDS 콘솔 실제 RPC 필드 호환 / READY_TO_INTEGRATE / 메인 직접 수행·자체 리뷰(독립 리뷰 없음)
기준: main 302a5d5. 소유 ui/src/panels/uds/{consolePanel.ts,uds.test.ts}.
발견: 실제 uds.request API의 인자는 text|bytes_hex인데 UI가 hex를 보내 mock-only 시험을 통과했다. 실제 gateway 송신은 Usage 오류가 된다.
변경: preview와 send 모두 bytes_hex 사용, 기존 시험 기대값 보강. 제품 권한/자동화 watch 제한 변경 없음.
검증: pnpm --dir ui exec vitest run src/panels/uds/uds.test.ts 11 PASS; eslint 지정 2파일 PASS; pnpm --dir ui typecheck PASS. /private/tmp/loom-uds-ui-compat-{test,lint,typecheck}.log.
실제 gateway bytes_hex 요청 확인: /private/tmp/loom-uds-domain-rpc.log에서 sim 확정 응답·live viewer·STOP·journal 복구 PASS. 이 evidence는 main의 미커밋 UDS 통합과 별도이며 본 UI DOM browser 검증을 뜻하지 않는다.
완료 범위: 필드 호환 수정과 단위검사. 실제 화면 검증/브라우저 접근 차단 해소는 남음. 후속 메인, 기능별 로컬 커밋·push 없음. 한도/사용량 미제공.
