# loom.columns.v1 vs JSON — 같은 D3 배치 Node 비교

작성: ui-sol, 2026-11-11. 최종 수치·프로파일은 측정 종료 뒤 아래에 기록한다.

## 조건

- 각 프로파일 10,000개 동일 D3 이벤트: CAN Classic8 B, CAN FD64 B, UART1 KiB. D3 필수 필드와 모든 현재 선택 필드를 입력에 포함하고 request_id는 누락/null/문자열을 순환한다. CAN payload에 device_ts/isotp/frame_index도 포함한다.
- 전체 입력의 순서와 값을 유지하고 각 형식이 독립적으로 max200행·전체65,536 B 프레임에 맞춰 greedy 분할한다. columns의 기존4 MiB 확장 예산도 지킨다. 200행이 상한을 넘으면 더 작은 배치가 된다.
- JSON은 실제 JSON-RPC2.0 core.events notification의 params에 EventsBatch를 넣은 UTF-8 텍스트이다. columns는 동일 events와 sub/first/last/lag/hub timestamp/null display_gap/record_health를 담는다. columns 전용 batch_id/hash 등 추가 헤더는 wire 크기에 포함한다. WS framing/TLS/compression은 두 형식 모두 제외한다.
- 각 모드 warmup3회, 측정25회, 순서를 순환한다. median과 nearest-rank p95(25개 중24번째)를 기록한다. encoding/분할/원본 roundtrip 검증은 시간 밖이다. GC는 켜져 있고 강제 GC/호스트 부하 격리/heap 측정은 하지 않는다.
- fields 경로는 seq/mono_ns/wall_ns를 BigInt로 맞추고 id/len/원시 bytes/origin/request_id를 읽는다. JSON bytes는 Node Buffer로 hex/base64를 디코드하고 columns는 borrowed raw getter를 쓴다. UART id는 없음, len은 청크 크기다. checksum은 byte 길이와 양 끝 값을 소비한다(내용 전체 스캔·렌더는 하지 않는다).
- D3 경로는 JSON.parse가 이미 만든 D3 레코드를 반환하거나 columns.decode+materializeAll로 복원한다. 둘 다 행 수와 첫/마지막 cursor를 소비한다. JSON에 불필요한 deep clone을 추가하지 않는다. 별도 columns-decode 경로가 materialize 전 비용을 보여 준다.
- JSON.parse는 스키마·CAN·u64·offset 검증을 수행하지 않는다. columns는 현재 decoder의 모든 검증·사전/sidecar 파싱을 포함한다. 따라서 제품 수신 경계 전체의 대등한 검증 비용 비교로 해석하지 않는다.

**Node만 측정했으며 브라우저·실데몬·장비→사용자 렌더 지연 수치가 아니다.** 전체 payload/메타데이터를 JSON과 columns에서 각각 복원해 deep equality를 확인하고 fields checksum도 같음을 확인한다.

## 재현

작업 디렉터리: /Users/ctmctm/workspace/Loom/.worktrees/ui/ui

`pnpm exec tsx src/codec/columns.compare.perf.ts /Users/ctmctm/workspace/Loom/docs/impl/ui/proposals`

비교 입력/소비자: ui/src/codec/columns.compare.ts, 실행기: columns.compare.perf.ts, 시험: columns.compare.test.ts. 기존 decoder·형식·골든 파일은 수정하지 않는다.

## 결과

환경: Apple M1, darwin/arm64, Node v25.6.1. 측정 종료 2026-11-11T10:29:00.282Z. 준비·검증·25회 측정·CPU 프로파일까지 64.69초.

### 와이어와 분할 (각 10,000행)

| 프로파일 | JSON B/행 | columns B/행 | columns 절감 | 프레임 수 JSON / columns | 최대 행 수 JSON / columns | 최대 B/frame JSON / columns |
|---|---:|---:|---:|---:|---:|---:|
| can-classic-8 | 987.86 | 690.52 | 30.10% | 152 / 157 | 66 / 64 | 65247 / 44188 |
| can-fd-64 | 1100.22 | 746.52 | 32.15% | 170 / 157 | 59 / 64 | 64970 / 47772 |
| uart-1024 | 2116.53 | 1162.21 | 45.09% | 334 / 179 | 31 / 56 | 65533 / 65079 |

### 필드 접근 총시간 (10,000행, ms)

| 프로파일 | JSON.parse+fields median / p95 | columns decode+getters median / p95 | columns/JSON median 비 |
|---|---:|---:|---:|
| can-classic-8 | 38.97 / 65.76 | 71.65 / 91.11 | 1.84× |
| can-fd-64 | 40.45 / 84.75 | 63.43 / 129.59 | 1.57× |
| uart-1024 | 47.73 / 126.09 | 31.74 / 63.32 | 0.66× |

### D3 레코드 반환 총시간 (10,000행, ms)

| 프로파일 | JSON.parse→D3 median / p95 | columns decode→D3 median / p95 | columns decode만 median / p95 |
|---|---:|---:|---:|
| can-classic-8 | 36.36 / 49.69 | 118.37 / 167.63 | 62.99 / 86.51 |
| can-fd-64 | 35.56 / 95.58 | 133.27 / 391.08 | 58.43 / 189.16 |
| uart-1024 | 41.40 / 86.21 | 141.27 / 210.42 | 24.44 / 37.53 |

JSON은 parse 과정에서 D3를 이미 만들므로 추가 펼치기 단계가0이다. columns의 decode→D3 총시간과 decode-only 차이에는 객체·decimal u64·bytes 문자열·독립 sidecar 복사가 추가된다. 서로 다른 표본의 median 차이를 정확한 독립 materialize phase 시간으로 해석하지 않는다. min/max·25개 표본은 원본 JSON에 모두 보관한다. GC/호스트 미격리로 p95가 흔들리므로 작은 차이의 확정 순위를 주장하지 않는다.

## 비용 프로파일 1회

측정 종료 뒤 별도로 Node V8 CPU sampler(1 ms)를 한 번 실행했다. 세 프로파일의 columns-fields를12회씩, 합계360,000행 처리했다. 1822개 sample이다. 다음은 호출 노드가 여러 개인 같은 함수의 **self sample 합계**이며 중첩 시간이나 구간 stopwatch 비율이 아니다.

| 항목 | self samples | 전체 비율 |
|---|---:|---:|
| 사전 parseJson (JSON.parse+bounded traversal) | 202 | 11.09% |
| sidecar parseJson (JSON.parse+bounded traversal) | 599 | 32.88% |
| extra (sidecar 의미 검증) | 85 | 4.67% |
| cell (검증/getter 공통 열 읽기) | 169 | 9.28% |
| GC | 22 | 1.21% |

parseJson의 JSON.parse native 비용과 JS depth/node/안전 숫자 순회는 sampler에서 분리할 수 없다. 조상 json() 유무로 dictionary와 sidecar의 parseJson self sample을 구별했다. 나머지 sample에는 decode 본문의 영역/bitmap/offset/row 의미 검증, wrapper/index 검사, UTF-8, checksum 등이 있다. tsx 변환으로 source line이1에 합쳐져 익명 callback을 정확한 검증 세부 단계별 비율로 나누지는 않았다. 원본 cpuprofile을 보관한다.

## 해석과 결론

- **CAN Classic8 B:** columns의 와이어는30.10% 작지만 fields median은1.84배, D3 반환은3.26배 느리다. 이 조건에서 CPU 이득은 없고 대역 이득만 있다. 프레임도 JSON152개보다 많은157개다.
- **CAN FD64 B:** 와이어는32.15% 작고 프레임은170→157개지만 fields median은1.57배, D3 반환은3.75배 느리다. 현 decoder를 CPU 최적화 근거로 채택하기 어렵고 대역/프레임 감소 이득은 있다.
- **UART1 KiB:** 와이어45.09% 감소, 프레임334→179개, fields median은JSON의0.66배로 이득이다. D3로 다시 펼치면3.41배 느려져 이 CPU 이득을 잃는다. raw column 소비자가 있을 때의 이득이다.

CAN은 columns의 최대 프레임이44,188/47,772 B여도64행에서 끊겼다. wire 한도가 아니라 **4 MiB 확장 accounting**이 다음 행을 막았기 때문이다. 한 extra JSON에 반복 record metadata와 매행 변하는 device_ts가 함께 있어 사전 항목이 사실상 매행 고유하며, `128*extra.length` 예산도 매행 누적된다. binary search의 후보 거부 집계는 원본에 있지만 후보 횟수를 실제 드롭/프레임 수로 세지 않는다. UART는 추가 metadata가 반복되고 wire 한도에서56행으로 나뉜다. 마지막 부분 프레임도 모두 바이트/행·프레임 수 계산에 포함했다.

따라서 기존 축약 열 스파이크를 full D3의 CPU 이득으로 일반화할 수 없다. 이 입력에서 현 제안은 **CAN 대역 절감, UART 직접 raw 소비의 대역·CPU 절감**에는 이득이 있고, **full D3 재구성 경로의 CPU 절감에는 이득이 없다**. JSON.parse만 잰 baseline에는 schema/u64/CAN 의미 검증이 없다는 비대칭도 있으므로 이 결과만으로 최종 codec을 폐기/확정하지 않는다. M2의 기본 전환 결정에는 실제 수신 검증·row store/패널·브라우저·daemon writer를 넣은 공동 측정이 필요하다.

## 비용 절감 대안 (이번 작업에서는 구현하지 않음)

1. **형식 유지:** 사전 JSON 문자열의 syntax/depth 결과는 이미 cache하지만 `extra()`의 의미 검증은 매행 반복한다. `(extra 사전 index, CAN/UART)`별 검증 결과와 quality 배열 검증을 cache한다. dictionary는 구조가 flat string[]임을 이용해 generic JSON object traversal의 stack 객체 할당을 줄인다. depth/node/안전 숫자·범위 검증을 생략하는 대안은 아니다.
2. **형식 유지:** 지금 getter마다 index 검사·COLUMNS.some·Map lookup·Result wrapper가 반복된다. 검증된 열별 접근자와 한 번 검증한 row cursor API를 제안한다. 반환 view의 불변 소유·경계 검사를 유지해야 한다. 현재 cell self sample9.28%에는 decode 내부 읽기도 섞여 있으므로 이를 전부 getter 절감 여지로 계산하지 않는다.
3. **형식 변경 대안:** 단일 extra 참조를 `record_extra/ts_extra/payload_extra/bytes_extra` 참조로 나누거나, device_ts.value_ns/source/clock_id·frame_index를 명시 열로 승격한다. 공통 record metadata가 매행 고유한 device_ts 때문에 반복 직렬화·파싱되는 것을 줄일 수 있다. dictionary wire 크기와 parseJson self sample을 함께 줄일 후보이며 새 schema_hash/CORE 합의가 필요하다.
4. **자원 정책 변경 대안:** 현재 보수적인 문자수×128 예산을 검증된 JSON node/문자열 수를 반영한 accounting으로 바꾸는 실험을 한다. 실제 heap을 먼저 측정하고 최대 노드·확장 상한과 overflow 거부는 유지한다. 제한을 무조건 올리는 제안은 아니며 합의된 semantics/hash revision이 필요하다.
5. **소비 경로:** raw column을 쓰는 패널/저장소에서 전 행 D3 materialize를 피한다. D3가 반드시 필요한 경로라면 metadata clone·bytesText를 별도 프로파일하고 bounded 복사/문자열 변환을 최적화한다. JSON이 이미 제공하는 객체를 다시 만드는 비용은 binary가 자동으로 없애 주지 않는다.

## 보관한 증거

- [전체25회 표본·환경·D3 예시·분할 집계](loom-columns-v1-vs-json.json)
- [V8 CPU profile 원본](loom-columns-v1-vs-json.cpuprofile)
- [CPU self sample 요약](loom-columns-v1-vs-json-profile-summary.json)

전체 입력30,000행의 두 형식→D3 deep equality, 같은 fields checksum, 모든 프레임≤64 KiB·≤200행을 측정 전에 검증했다. Node용 writer/benchmark만 추가했고 기존 decoder/형식/net/proto/골든은 수정하지 않았다. 브라우저/실데몬 성능 또는 제품 합격을 주장하지 않는다.

검증: 새 비교 시험4개 통과, 앱 typecheck·전체 lint 통과, Node Vitest34파일/362시험 통과. 전체 pnpm check는 별도 수정 중인 e2e/m1/t10-lifecycle.spec.ts의 DOM 타입 오류로 typecheck에서 중단되었다. JSON.parse 입력은 이미 문자열이며 transport의 UTF-8 문자열 변환 비용은 측정하지 않았다(사전 UTF-8 해석은 columns decoder 내부 비용에 포함).
