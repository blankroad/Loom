# P2 columns 동일 D3 비교 — ui-sol

2026-11-11. 기준 ui/main `728a99e`, 코드 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui`. 커밋·stash·네트워크·승인 요청·브라우저 실행 없음.

## 결과

[표·조건·해석·대안](../proposals/loom-columns-v1-vs-json.md), [25회 원본 표본](../proposals/loom-columns-v1-vs-json.json), [CPU profile](../proposals/loom-columns-v1-vs-json.cpuprofile).

각 프로파일 동일10,000행, D3 필수/현재 선택 필드 포함, request_id 누락/null/값, max200행·전체64 KiB로 독립 분할했다. 3 warmup+25회/모드, 인코딩 제외. 측정/CPU profile 전체 실행은64.69초다. 다음 시간은10k행의 **median/p95 ms**이다.

| 프로파일 | B/행 JSON→columns | 프레임 JSON→columns | fields JSON / columns | D3 반환 JSON / columns |
|---|---:|---:|---:|---:|
| CAN Classic8 B | 987.86→690.52 | 152→157 | 38.97/65.76 · 71.65/91.11 | 36.36/49.69 · 118.37/167.63 |
| CAN FD64 B | 1100.22→746.52 | 170→157 | 40.45/84.75 · 63.43/129.59 | 35.56/95.58 · 133.27/391.08 |
| UART1 KiB | 2116.53→1162.21 | 334→179 | 47.73/126.09 · 31.74/63.32 | 41.40/86.21 · 141.27/210.42 |

**결론:** columns는 와이어30–45% 절감. CAN fields CPU 이득은 없고, UART raw fields는 median 약34% 빠르다. full D3 복원은 모두 느리다. JSON.parse는 이미 D3 객체를 만들며 columns의 의미/경계 검증과 같은 검증을 하지 않는 baseline이다. GC/호스트 미격리로 p95 변동이 크다. **Node 수치이며 브라우저·실데몬·렌더 지연 수치가 아니다.**

CAN columns는4 MiB 확장 accounting 때문에 최대64행으로 분할되어 Classic의 프레임 수가 오히려 증가했다. CPU sampler1회(360k행/1822 samples)에서 parseJson self sample 합계43.96%(사전11.09%, sidecar32.88%)가 관측됐다. 단일 sidecar에 공통 metadata와 매행 고유 device_ts가 섞인 구조가 원인 후보이다. 세부 self 비율은 stopwatch phase 시간과 같지 않다.

제안: cache된 sidecar 의미 검증 재사용·typed column reader(형식 유지), metadata 참조 분리/device_ts 명시 열(새 hash), 실제 heap 근거를 둔 확장 accounting 개선, D3 materialize 우회 소비 경로. decoder/형식 최적화는 이번 범위에서 구현하지 않았다.

## 코드와 검증

- `ui/src/codec/columns.compare.ts`: bounded 입력·독립 framing·동등 fields 소비자·D3 equality 검증.
- `ui/src/codec/columns.compare.perf.ts`: 5모드×25회, 순서 순환, Node inspector CPU sampling1회·원본 저장. 실행8분 상한, 고정 최대10k행/프로파일.
- `ui/src/codec/columns.compare.test.ts`: 추가4시험 통과.3프로파일의 lossless 분할/필드 동등성/크기·행 상한과 입력 상한 검증.
- 측정 전 전체30k행의 JSON→D3/columns→D3 deep equality 및 checksum 동등성, 모든 프레임 상한 확인.
- `pnpm exec tsc -p tsconfig.app.json`, `pnpm exec eslint src/codec`: 통과.
- `pnpm check`: schema32종 통과 뒤, 기존 수정 중인 **`e2e/m1/t10-lifecycle.spec.ts`의 Element/document/NodeFilter 타입 오류(TS2304/TS2584)**로 typecheck 단계 실패. 작업 시작부터 변경되어 있던 범위 밖 파일이므로 수정하지 않았다.
- 위 typecheck 실패와 별도로 `pnpm lint` 통과, `pnpm test` **34파일/362시험 통과**. 기존 jsdom canvas 미구현 안내가 있었으나 실패0.

원래 codec/골든/net/proto/설정은 수정하지 않았다. 새 코드3개는 codec 범위, 문서/원본 증거는 proposals, 보고서는 이 파일에만 썼다. 원본 벤치마크·CPU profile은 Node에서1회 실행했고, 더 좋은 표본을 고르는 재실행은 하지 않았다.
