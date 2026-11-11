# p1-ui-query-envelope — READY (owner: main)

docs/plan/02-core-principle.md 준수. 기준 main 26f3053 + 브라우저 WIP. 커밋/push/index/build 없음.

## 진행
1. 정본 확인: `loom-gateway/src/daemon.rs::envelope()` → `loom_proto::Envelope` 를 JSON-RPC **result**로 반환
   (도메인 `<domain>.<cmd>` 경로 read/write_request/denied 모두). 실패도 result(`ok:false`, `error:ErrorBody`), 
   `write_request` partial 은 `ok:false` + `data`. 항상 직렬화: `schema_version, ok, cmd, meta, others_since_last`.
   UI `RpcClient.handleResponse` 는 result 를 그대로 resolve → `PanelContext.query` 가 envelope 를 caller 에 넘김
   (eth.show `res.bytes` 없음, eth.filter.check 가 envelope `ok:true` 를 보고 항상 ✓ 였던 원인).
2. 수정 `ui/src/sdk/context.ts`:
   - `query<R = unknown>(rpc, params, signal?): Promise<R>` 명시, `unwrapDomainEnvelope<R>` 한 곳에서 R 변환 (리뷰 반영, `as Promise<never>` 제거).
   - envelope 판별: plain object && `schema_version` number && `ok` boolean && `cmd` string && `meta` object && `others_since_last` object. 하나라도 없으면 bare 결과 그대로(동일 객체) 반환.
   - `ok:true` & error 없음 → `data` 반환. 그 외(`ok:false`, partial data 포함, error 본문 없음) → `DomainQueryError` reject
     (`code`(없으면 `internal`), `message="<rpc>: <msg>"`, `hint`, `retryable`, `requestId`, `detail`, `data`=partial).
   - 원본 결과 비변경. 선언 검사·dry-run 관문은 동기 throw 그대로, abort/NetError 는 net.call 거부 그대로 전파.
3. 테스트 `ui/src/sdk/sdk.test.ts` (+4): 실 Envelope 성공→data(원본 불변, null data), 실패 reject(not_found 필드,
   partial+data, error 없는 ok:false), bare 유지(eth.filter.check `{ok:false,pos}`, ok/data/cmd/schema_version 유사 payload, core.time, 비객체),
   undeclared/non-dry-run 동기 throw·NetError(disconnected) 동일 객체·aborted 보존·signal 전달.
   수정 전 코드로 되돌리면 envelope 성공/실패 2건 FAIL 확인.

## 검증
- `pnpm check` (schema + tsc + eslint + vitest): 33 files / 391 tests PASS.
- 실제 브라우저 PASS 대체 아님. UI build 는 메인 통합 후 실행 → 브라우저 담당(tc1-live) 신호 필요.

## 호출자 raw 의존 (읽기만, 변경 없음)
- `eth/ethPanel.ts` eth.show: bare `bytes/hex/tree/raw_url/truncated` 기대 → 이번 수정으로 정상 수신.
- `eth/ethPanel.ts` eth.query: `rows/range/truncated` 는 도메인 data 안에 있음(handlers.rs, `_meta` 아님) → OK.
- `eth/filterBar.ts` eth.filter.check: 업무 `{ok,pos,expected,message}` → 이제 실제 판정 표시.
- `eth/dialogs.ts` eth.send dry-run / eth.ifaces: bare 기대 → OK.
- `can/canTrace.ts` `dataOf()`: `o.data` 있으면 내려감. can.send dry-run data 에 `data` 키 없음 → OK (중복 unwrap 위험 없음).
- `uds/udsModel.ts previewText`: `'dry_run' in r` 가드 → bare/envelope 모두 OK.

## 확장 (메인 승인: actionGate.ts + 공통 helper)
- 새 `ui/src/sdk/rpcEnvelope.ts`: `isDomainEnvelope`(5필드 판별), `DomainRpcError`(구 DomainQueryError, 미커밋 신규라 alias 없음),
  `rejectDomainFailure`(실패만 throw, 그 외 동일 객체), `unwrapDomainResult`(query: 성공→data). context.ts 의 복사본 제거.
- `ActionGate.run`: net.call 1회 후 `rejectDomainFailure` — 성공 응답(envelope/bare)은 그대로 반환, `ok:false`(partial data 포함)는 throw.
  사전권한/재확인/STOP 안전방향/confirm/no-retry 로직 불변, 위젯 변경 없음.
- 효과: `act()` 실패 envelope 가 `WriteHandle` 로 resolve 되지 않음. `ui.mark`, TestScreen `gate.run(...).catch` 도 실패를 표시.
  `canTrace` 오류 분기는 `dataCode ?? code` → 이제 `stop_latched` 등 ErrorBody code 를 받음.
- 테스트 +2: native partial 실패 → reject(code/requestId/data 보존), WriteHandle 없음, RPC 1회 / stop_latched reject;
  성공 envelope 는 gate 에서 동일 객체·act requestId=data.request_id, bare 업무 `{ok:false}` 는 resolve.
  ActionGate 수정을 되돌리면 act 실패 테스트 FAIL 확인.
- `pnpm check`: 33 files / 393 tests PASS.

## 기타
- 작업 중 `ui/src/panels/eth/eth.test.ts` 가 타인 WIP 로 수정 상태 — 건드리지 않음.

## 변경 파일
`ui/src/sdk/{context.ts,actionGate.ts,rpcEnvelope.ts(신규),sdk.test.ts}`, 본 보고서.

## 메인 최종 통합 확인

새 dist `index-BExAuFCo.js`로 실제 브라우저 재검증 완료. Ego UART/ETH 기능 검사와 Playwright 실제5개 전체 PASS(18.5초,skipped0), CAN20샘플 p95 83.40125ms<150ms. 최종 UI393/schema/type/lint/build PASS. 실행 구성/캡처/정리: [최종 브라우저 보고](p1-browser-final.md). 미완료였던 실제 화면 검증을 이 결과로 갱신한다.
