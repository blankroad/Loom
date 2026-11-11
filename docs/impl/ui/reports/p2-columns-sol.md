# P2 columns — ui-sol 완료 보고

2026-11-11. 코드 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui`(`ui/main`), 문서는 저장소 루트. 커밋·stash·네트워크·브라우저 시험·승인 요청 없음.

## 산출물

- [loom.columns.v1 제안](../proposals/loom-columns-v1.md): LE160 B header,32 B directory, 공통 문자열 사전,25개 열, CAN/UART 혼합, presence/validity, offsets+values, u64 delta/절대 seq, gap/health,64 KiB·200행 및 bounded metadata 규칙.
- [TS decoder](../../../../.worktrees/ui/ui/src/codec/columns.ts): `decode(ArrayBuffer)`가 명시적 Result를 반환한다. 모든 영역의 bounds/alignment/겹침, bitmap/offset,4 MiB 확장 예산 preflight 후 열 view를 제공한다. `get()`은 full LoomEvent 없이 읽고, `materialize()`/`materializeAll()`은 D3 JSON 값으로 펼친다.
- [layout/hash](../../../../.worktrees/ui/ui/src/codec/layout.ts), [hash 입력 JSON](../proposals/loom-columns-v1-layout.json): 제안 전용 SHA-256 `2e01eea1b6346a1cb35caeee041ebdbb171c025648d646bd83daf77e004f02c6`.
- [시험](../../../../.worktrees/ui/ui/src/codec/columns.test.ts), [손으로 쓴 시험 전용 writer](../../../../.worktrees/ui/ui/src/codec/columns.test-support.ts), [고정 corpus](../../../../.worktrees/ui/ui/src/codec/golden):4종×hex/JSON. 빈 배치, 가변 DLC·FD64 B·RTR, 혼합 domain, 최대u64·2^53+1, request_id 누락/null/있음, gap 요약, source identity·origin·quality 등 D3 값을 동등 비교한다.
- [Node 성능 메모](../proposals/loom-columns-v1-node-perf.md), [25회 원본 표본](../proposals/loom-columns-v1-node-perf.json), [재현 코드](../../../../.worktrees/ui/ui/src/codec/columns.perf.ts).

## 결정과 근거

- 결정: optional의 누락과 명시적 null에 presence·validity를 각각 사용한다. 근거: 단일 validity로 두 JSON 상태를 모두 보존할 수 없다.
- 결정: producer_id/producer_epoch/source_seq, epoch/seq, origin, request_id, quality를 열에 유지하고 기타 D3·payload 확장은 검증된 JSON sidecar로 복원한다. 근거: 실제 장비 echo의 출처·쓰기 요청·품질이 JSON 경로와 같아야 한다.
- 결정: unknown version/hash, 잘린/겹친 offset, overflow와 예산 초과는 오류로 끝낸다. 근거: 버퍼를 자르거나 값/seq를 추정하면 표시가 거짓이 된다.
- 결정: raw_ref·다른 kind·미지원 프로파일은 JSON fallback이며, 이 decoder를 net/proto에 아직 연결하지 않는다. 근거: 형식 정본은 CORE 소유이며 이 작업은 합의용 제안이다.
- 결정: 버퍼는 view 생존 동안 독점·불변 소유한다. 근거: 복사를 피하는 borrowed raw view의 검증 이후 변경을 막는 호출자 계약이 필요하다.

## 검증

- `pnpm exec vitest run src/codec/columns.test.ts`: **17개 통과**. seeded 무작위 truncation/bit-flip **10,000회**, 결과 반환·성공 변형의 materialize 검증, 크래시/시간초과 없음.
- 추가 부정 시험: unknown version/hash/enum/column, frame/count 상한, detached buffer, 정렬·중복·겹침·잘린/비단조 offset, bitmap 모순, CAN DLC/ID/FD 오류, seq overflow/순서, 잘못된 metadata·device_ts u64·gap·generation, 확장 예산 초과.64 KiB 경계 수용/초과 거부.
- 추가 정상 시험: DLC16코드, UART hex/base64 길이0..7, 선택 열 생략, 독립 metadata 복사, own `__proto__` 확장의 안전한 복원.
- 최종 `pnpm check`: schema drift32종·typecheck·lint 통과, **33파일/358시험 통과**(2026-11-11 19:20 KST 실행). 기존 jsdom canvas `getContext()` 미구현 안내가 출력되었으나 실패0. 브라우저 실행은 하지 않았다.
- Node10k행(50프레임×200행, FD64 B): decode/getter median **41.79 ms**, p95 **128.88 ms**; decode+전체 materialize median **186.62 ms**, p95 **357.36 ms**.34,069 B/프레임. 호스트·GC 미격리로 분산이 크며 **브라우저/실시간 지연 합격 수치가 아니다**.

## CORE에 확인할 것

1. header/directory·사전 방식과 제안 hash 채택 여부, 기존 D3 manifest와 최종 hash를 결합할지.
2. sub_id를 문자열 사전 참조로 유지할지와 batch_id 증가/reset 규칙.
3. Classic RTR의 payload.len 의미: 현재 요청 DLC 길이를 유지하고 raw bytes만0으로 해석했다.
4. raw_ref·미지원 bytes 표기·단일 초과 UART 청크의 fallback/분할 정책, metadata 복잡도·4 MiB accounting 예산.
5. 동일 고정 corpus에 대한 Rust encoder/decoder 왕복·부정 입력·공동 부하 측정 일정. 현재 TS끼리의 corpus 검증만 완료했다.

다른 에이전트가 수정 중인 panels/sdk/shell/store 및 net/proto/패키지·설정은 수정하지 않았다. 작업 범위는 `ui/src/codec/**`, 루트 `docs/impl/ui/proposals/**`와 이 보고서이다.
