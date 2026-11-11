# loom.columns.v1 — UI 소비자 제안 1

작성: ui-sol, 2026-11-11. **CORE 합의 전 제안**이며 서비스 wire 정본이나 배포된 capability가 아니다. 구현은 UI worktree `ui/src/codec/`에 격리했다. net/proto/설정 변경 및 실제 구독 연결은 없다.

근거: [CORE/UI 합의](../../../plan/_xteam/core-ui.md)의 CR-UI-01·CORE 헤더 후보, [UI 계획 §6.2](../../../plan/ui/plan.md), [D3](../../../contract/02-events.md), [CAN 요청 §2](../../requests/can-ui.md). 기존 R1의 24.2 B/행은 축약 스키마 측정이므로 이 full D3 구현의 크기로 인용하지 않는다.

## 1. 적용 범위와 결정

| 결정 | 근거 |
|---|---|
| 협상된 `encoding:"binary"`, codec `loom.columns.v1`인 데이터 구독에만 적용 | 제어·RPC·쓰기 최종 결과의 JSON 경로와 D2 우선순위를 유지한다. |
| `can/can.frame`, `uart/uart.data`를 한 프레임에 혼합 가능 | 공통 D3 메타를 유지하면서 필터 구독을 그대로 전달한다. |
| D3 schema_version=1, stream epoch는 배치 공통 | 다른 epoch는 프레임을 분리해 cursor 의미가 섞이지 않게 한다. |
| 전체 프레임 ≤65,536 B, 행 ≤200, 일반 max40 ms/UART max10 ms·idle 즉시 flush | 이미 합의된 D2 크기·시간 경계를 재사용한다. 이 모듈은 flush를 구현하지 않는다. |
| 숫자·offset·u64 LE, 정렬은 필드 폭 기준 | Rust/TS 사이 endian·정렬 모호성을 없앤다. TS는 DataView로 host endian에 의존하지 않는다. |
| 문자열은 프레임 내 공통 사전, metadata 확장은 JSON sidecar 사전 참조 | session/domain/channel/kind/producer/client 반복을 공유하고 D3의 나머지 필드를 잃지 않는다. |
| 열 접근과 D3 materialization을 별도 API로 제공 | 화면이 필요한 열만 읽을 때 full LoomEvent 객체·u64 문자열·바이트 문자열 생성을 피한다. |
| 미지원 레코드는 원본 JSON fallback; 잘못된 binary는 명시적 오류 | 생략·추정·빈 데이터로 대체하면 source identity와 쓰기 결과가 거짓이 된다. |

`raw_ref`, 비정규 base64(무패딩·URL alphabet 등), 다른 kind/스키마 버전, 이 제안의 JSON 복잡도/확장 예산을 넘는 레코드는 기존 JSON 경로를 쓴다. `can.tx`, gap 이벤트 본문, control 구독은 이 형식에 싣지 않는다. JSON fallback도 D2 전체 크기 상한을 지키며, 큰 UART 청크 분할/단일 레코드 초과 처리는 기존 producer 정책과 CORE가 확정할 사항이다. 실패 프레임을 JSON으로 재해석하지 않는다.

## 2. 고정 헤더 (160 B)

모든 offset은 프레임 시작 기준, 크기는 byte. 숫자는 LE, f64는 IEEE-754 LE이다. 예약 영역은 0이어야 한다.

| offset | 형 | 필드/규칙 |
|---:|---|---|
| 0 | ASCII[8] | magic `LOOMCOL1` |
| 8 | u16 | version=1 |
| 10 | u16 | header_bytes=160 |
| 12 | u32 | frame_bytes, 실제 ArrayBuffer 길이와 같음 |
| 16 | u8[32] | schema_hash, 아래 SHA-256의 raw digest |
| 48 | u32 | sub_id, 문자열 사전 index (`control` 금지, 빈 문자열 금지) |
| 52 | u32 | reserved=0 |
| 56 | u64 | batch_id, 이 디코더는 배치 간 순서를 판정하지 않음 |
| 64 | u64 | stream_epoch → D3 epoch |
| 72 | u64 | first_seq |
| 80 | u64 | last_seq |
| 88 | u64 | hub_publish_mono_ns, flag bit1 없으면 0 |
| 96 | u64 | daemon_wall_ns, flag bit2 없으면 0 |
| 104 | u32 | count 0..200 |
| 108 | u16 | column_count 0..25 (필수 열 존재 검증 별도) |
| 110 | u16 | flags: bit0=seq delta, bit1=hub timestamp 존재, bit2=daemon timestamp 존재, bit3=display_gap 명시적 null; 그 외 0 |
| 112 | u32 | directory_offset=160 |
| 116 | u32 | dictionary_offset |
| 120 | u32 | dictionary_length |
| 124 | u32 | display_gap_offset, 없음/null이면 0 |
| 128 | u32 | display_gap_length, 없음/null이면 0 |
| 132 | u32 | record_health 사전 index, `0xffffffff`=없음, `0xfffffffe`=null |
| 136 | f64 | lag_ms, finite·≥0 |
| 144 | u8[16] | reserved=0 |

헤더 timestamp의 존재 비트는 값 0과 누락을 구별한다. `record_health`는 `ok/degraded/failed/disabled`만 허용한다. 근거: 원본 EventsBatch의 optional/null 의미를 보존한다.

현재 hash는 `2e01eea1b6346a1cb35caeee041ebdbb171c025648d646bd83daf77e004f02c6`이다. 입력은 [layout identity JSON](loom-columns-v1-layout.json)의 **파일 byte 그대로**(UTF-8, BOM·개행 없음)이며, `layout.ts`의 `JSON.stringify(LAYOUT_IDENTITY)`와 같다. 모든 해석 규칙 변경은 identity의 `format` revision 또는 내용과 hash를 바꿔야 한다. 테스트가 hash를 다시 계산하고 고정 골든 header를 대조한다. **CORE의 기존 32-schema manifest hash와 다른 제안 전용 hash**이다. 정식 협상에서 D3 manifest와 layout hash를 결합할지는 CORE 확인이 필요하다. 근거: 불일치한 레이아웃을 같은 decoder로 읽지 않는다.

version 또는 hash 불일치는 각각 `version`/`schema` 오류이며 열을 노출하지 않는다. 호출 측은 capability 재협상·JSON 재구독과 명시적 display gap 처리를 해야 한다. 자동 재구독/상태 변경은 이 순수 디코더의 범위 밖이다.

## 3. 열 디렉터리와 바디

디렉터리는 160에서 시작하는 32 B 엔트리 `column_count`개이다. 엔트리 순서는 무관하고 중복 ID·미지 ID/type/flags는 거부한다.

| 엔트리 offset | 형 | 의미 |
|---:|---|---|
| 0 | u16 | column ID |
| 2 | u8 | type: 1=u8, 2=u32, 3=u64, 4=dict-ref(u32), 5=bytes(offsets u32) |
| 3 | u8 | flags: bit0 optional, bit1 nullable; 아래 열 표에 지정된 값과 정확히 같아야 함 |
| 4 | u32 | data_offset |
| 8 | u32 | data_length |
| 12 | u32 | validity_offset, nullable이면 ceil(count/8) B, 아니면 0 |
| 16 | u32 | presence_offset, optional이면 ceil(count/8) B, 아니면 0 |
| 20 | u32 | aux_offset, bytes values만 사용 |
| 24 | u32 | aux_length, bytes values만 사용 |
| 28 | u32 | reserved=0 |

- 필수 열은 디렉터리에 항상 존재한다. optional 열 엔트리 자체를 생략하면 모든 행에서 그 속성이 없다. 근거: 전체 비어 있는 선택 열은 보낼 필요가 없다.
- 비트맵은 행 r → byte `r>>3`, bit `r&7`(LSB first)이다. presence=0은 누락, presence=1·validity=0은 null, 두 비트=1은 값이다. optional·nonnullable는 presence만 사용한다. 미사용 꼬리 비트=0, 누락/null 슬롯의 숫자 바이트=0, presence=0·validity=1은 금지한다. count=0이면 비트맵 offset=0이다. 근거: 단일 validity만으로는 D3 누락과 null을 동시에 복원할 수 없다.
- 고정 열 data_length=`count*width`, data_offset은 폭(1/4/8)에 정렬한다. bytes 열은 `(count+1)*4` B offsets이며 4 B 정렬, aux는 raw byte 값이다. offsets[0]=0, 비감소, 마지막=aux_length, 모두 aux 범위 안이어야 한다. bytes 열은 필수·nonnull이다. 근거: 빈 청크와 가변 DLC를 범위 밖 읽기 없이 표현한다.
- 각 비어 있지 않은 영역(헤더·디렉터리·사전·gap·열 data/bitmap/aux)은 겹치지 않는다. 모든 미참조 padding/예약 byte는 0이다. 0길이 영역 offset도 프레임 내부/끝의 정렬된 위치여야 한다. 근거: aliasing·숨겨진 trailing 데이터·overflow 해석 차이를 막는다.

문자열 사전은 strict UTF-8 JSON `string[]`로 한 번 읽고, 최대4096개 index를 0부터 쓴다. 중복 문자열은 허용하지만 writer는 intern을 권장한다. 사전이 비어 있으면 필수 sub 참조 검사에서 거부한다. metadata JSON도 문자열로 사전에 저장한다. 근거: 문자열 offset 테이블보다 초기 상호운용 구현이 단순하고, 후속 최적화는 새 hash로 명시할 수 있다.

## 4. 열 정의

P=presence(optional), V=validity(nullable). `—`는 필수·nonnull. dict는 위 문자열 사전 index이다.

| ID | 이름 | type | bitmap | D3 매핑 |
|---:|---|---|---|---|
| 1 | session_id | dict | — | session_id |
| 2 | domain | dict | — | can 또는 uart |
| 3 | kind | dict | — | can.frame 또는 uart.data |
| 4 | producer_id | dict | — | producer_id |
| 5 | producer_epoch | u64 | — | producer_epoch |
| 6 | source_seq | u64 | — | source_seq |
| 7 | seq | u64 | — | stream seq 또는 first_seq 기준 delta |
| 8 | mono_ns | u64 | — | ts.mono_ns |
| 9 | wall_ns | u64 | — | ts.wall_ns |
| 10 | clock_id | dict | — | ts.clock_id |
| 11 | ts_source | u8 | — | 0 host_read_complete, 1 driver, 2 hardware, 3 device, 4 gateway, 5 synthetic |
| 12 | origin | u8 | — | 0 agent, 1 user, 2 device, 3 script |
| 13 | channel | dict | P,V | channel |
| 14 | client_id | dict | P,V | client_id |
| 15 | extra | dict(JSON) | — | 아래 metadata envelope |
| 16 | encoding | u8 | — | payload.bytes.encoding: 0 hex, 1 base64 |
| 17 | raw | bytes | — | payload.bytes.data의 실제 bytes |
| 18 | text | dict | P | 비정규 대소문자 hex 원문 override; 일반 행은 생략 |
| 19 | can_id | u32 | P | payload.id |
| 20 | can_flags | u8 | P | bit0 ext, bit1 fd, bit2 brs, bit3 esi, bit4 rtr; 그 외 0 |
| 21 | dlc | u8 | P | payload.dlc: DLC **코드** |
| 22 | len | u8 | P | payload.len |
| 23 | dir | u8 | P,V | 0 rx, 1 tx |
| 24 | request_id | dict | P,V | request_id |
| 25 | quality | dict(JSON string[]) | P | quality; 빈 배열도 보존 |

열을 나눈 근거: source identity·시간·origin·request_id·quality를 명시적으로 유지하면서 CAN hot path와 UART raw 청크를 직접 읽게 한다. 기타 필드는 아래 lossless 확장에 둔다. ts_source·origin·dir·encoding의 미지 enum은 오류다. session_id/producer_id/clock_id는 빈 문자열을 거부한다.

CAN 행은 19..22 네 선택 열이 모두 존재해야 하고 UART 행에서는 모두 누락이어야 한다. CAN ID≤0x7ff(standard) 또는0x1fffffff(extended), dlc0..15, len0..64를 검사한다. FD byte length 표는 `[0,1,2,3,4,5,6,7,8,12,16,20,24,32,48,64]`, RTR 불가. Classic은 dlc≤8, len=dlc, brs/esi=false이며 RTR이면 raw 길이=0, 아니면 raw 길이=len이다. **RTR의 len=dlc(요청 길이) 해석은 제안 결정**이며 CORE/CAN의 확인이 필요하다. 근거: data가 빈 원격 요청의 DLC도 잃지 않는다.

## 5. seq·u64·gap

seq delta는 이전 행 기준이 아니라 **header.first_seq 기준 u64 차이**이다. bit0=0이면 절대 seq이다. 덧셈은 BigInt에서 하고 결과≤2^64−1을 검사한다. 각 행 seq는 엄격 증가, 첫/마지막은 헤더와 일치해야 한다. 필터로 생긴 불연속을 연속 seq로 복원하거나 display loss로 추정하지 않는다. 근거: 큰 필터 간격에도 정확하며 32비트 delta overflow가 없다.

빈 배치는 count=0, first_seq=last_seq=0, 필수 열 엔트리와 bytes offsets `[0]`을 유지한다. stream_epoch는 배치 자체 값이다. 근거: gap/health만 전달하는 빈 배치도 해석 가능하다.

u64는 0..18446744073709551615의 LE 8 B이고 헤더·열 getter는 BigInt를 반환한다. materialize할 때만 canonical decimal string으로 변환한다. JSON sidecar의 명시적 u64(device_ts.value_ns, gap count/range)도 canonical·최댓값을 검사한다. generation은 별개의 nullable **u32 숫자**이다. 근거: stream epoch·producer epoch·device generation을 혼동하거나 2^53에서 정밀도를 잃지 않는다.

`display_gap`은 독립 strict UTF-8 JSON GapSummary이다. count는 canonical u64 문자열 또는 `unknown`, causes는 optional string[], seq_range는 optional/null 또는 `{from:u64,to:u64}`(from≤to)이다. 없음과 null은 헤더 bit3으로 구분한다. 다른 gap 이벤트의 source/record/display 구분을 이 요약으로 대체하지 않는다. 근거: 배치 드롭을 행 seq 간격으로 임의 계산하지 않고 기존 D2 의미를 유지한다.

## 6. D3 lossless 왕복

metadata envelope는 JSON object `{record?:{},ts?:{},payload?:{},bytes?:{}}`이고 다른 최상위 키는 거부한다. 각 부분은 원본 D3에서 해당 열이 소유하는 키를 제외한 나머지 객체이다. 빈 부분은 생략해도 된다.

- record: actor_id, external_actor, job_id, generation, purpose, env, risk, original_origin, test_session, test_step, metrics 및 확장 키.
- ts: quality와 확장 키.
- payload: device_ts, isotp, frame_index 및 기타 확장 키. CAN 열의 id/ext/fd/brs/esi/rtr/dlc/len은 CAN sidecar에 중복 금지다. UART의 같은 이름 확장 키는 보존한다.
- bytes: encoding/data 이외의 키(예: ref:null).

record의 schema_version/session_id/domain/kind/producer_id/producer_epoch/source_seq/epoch/seq/ts/origin/payload/channel/client_id/dir/request_id/quality, ts의 clock_id/mono_ns/wall_ns/source, 모든 payload의 bytes, bytes의 encoding/data는 sidecar에 중복 금지다. 근거: 열과 sidecar의 충돌 우선순위가 source identity를 덮어쓰지 못하게 한다.

D3 선택 필드 타입과 enum, generation u32, metrics, ts.quality, CAN device_ts는 추가 검증한다. JSON은 최대 depth16·8192 nodes이며 finite 숫자·안전 정수만 허용하고 -0은 거부한다. JSON property 순서/공백/숫자의 원문 표기까지 보존하는 문자열 프로토콜은 아니다. 이 프로파일 밖의 값은 JSON fallback 대상이다. 근거: bounded parser/clone을 보장하면서 JSON 값의 의미를 보존한다.

raw는 실제 bytes, materialize된 hex는 기본 소문자, base64는 표준 alphabet+padding이다. 원본 hex가 대소문자를 섞으면 text 열로 정확한 문자열을 싣고 raw와 일치하는지 검증한다. text 열에 base64를 싣는 경우도 canonical raw 표현과 일치해야 한다. 근거: 바이트 비교와 원본 D3 `bytes.data` 문자열 동등성을 함께 만족한다.

복원은 새 객체에 알려진 열과 검증된 sidecar를 합치며 null/누락/빈 배열·빈 문자열을 그대로 구분한다. source identity 세 값(producer_id, producer_epoch, source_seq), stream cursor(epoch,seq), origin, original_origin/external_actor, request_id, quality/ts.quality를 추정하거나 삭제하지 않는다. 별도 materialize 호출 사이 중첩 metadata 객체는 공유하지 않는다. `__proto__` 같은 확장 키도 own property로 안전하게 복사한다. 근거: 화면과 원본 기록이 같은 출처·쓰기 요청·품질을 가리켜야 한다.

## 7. 디코더와 자원 경계

`decode(ArrayBuffer): Result<ColumnsView>`는 throw 대신 `{ok:false,error:{code,message}}`를 반환한다. 성공 시 `{header,get(column,row),materialize(row),materializeAll()}`가 있다. getter도 범위 오류를 Result로 반환한다. `get('seq',r)`는 delta를 적용한 BigInt, `get('raw',r)`는 borrowed Uint8Array이며 dir/origin 등 enum 열은 숫자 값이다. 이 함수는 wire 경계를 검증하는 제안 decoder이며 generated D3 전체 JSON Schema validator를 대체하지 않는다.

검증 순서: 전체 길이·magic/version/hash → count/flags → directory와 모든 영역의 subtraction 기반 bounds·alignment·겹침/padding → bounded dictionary → bitmap/가변 offsets/누락 슬롯 → 확장 예산 preflight → row 값/enum/seq/CAN/metadata → 접근자 노출. raw column view 생성은 offsets·확장 예산 검증 뒤에만 한다.

확장 예산은 다음 보수적 accounting 합계를 **4 MiB 이하**로 제한한다. UTF-16 문자열 length 기준이며 실제 VM heap/RSS 상한 측정치는 아니다.

```text
frame_bytes + Σdictionary(2*string.length + 32)
+ Σrows(1024 + 128*extra_json.length + 128*quality_json.length + 8*raw_byte_length)
```

JSON character에 큰 계수를 부여해 작은 객체/키가 많은 반복 sidecar를 제한한다. 원본 frame64 KiB·200행·dictionary4096·JSON depth/nodes 제한도 함께 적용한다. 초과 시 `limit`으로 거부하며 큐·무제한 배열·잘라서 수용하는 경로는 없다. materializeAll은 호출당 최대200행이고 반복 호출의 보관/해제는 호출자 책임이다. 근거: 작은 wire payload의 반복 metadata가 거대한 행 객체로 확장되는 것을 방지한다.

ArrayBuffer는 view 생존 동안 호출자가 독점·불변 소유해야 한다. raw getter 결과를 쓰거나 buffer를 transfer/detach/재사용하지 않는다. SharedArrayBuffer는 허용하지 않는다. 검증/JSON sidecar cache 할당이 있으므로 zero-allocation decoder라고 부르지 않는다. 근거: 검증 후 외부 변경으로 조건이 깨지는 TOCTOU와 데이터 복사 비용을 명시한다.

## 8. 골든·퍼즈·Node 측정

코드 위치(모두 UI worktree 상대 경로):

- `ui/src/codec/columns.ts`: 런타임 디코더와 D3 materialization.
- `ui/src/codec/layout.ts`: 열 ID/type/enum/limits와 제안 hash identity.
- `ui/src/codec/columns.test-support.ts`: 손으로 쓴 시험 전용 TS writer(런타임 import 금지).
- `ui/src/codec/golden/{empty,can-dlc,mixed-max,absolute-uart}.{hex,json}`: 고정 raw hex와 원본 D3 JSON.
- `ui/src/codec/columns.test.ts`: writer→고정 hex 비교→decoder→고정 JSON 동등성, 부정 입력, seeded10,000회 truncation/bit-flip. 유효 변형도 성공 결과로 허용하고 materialize까지 검사한다.
- `ui/src/codec/columns.perf.ts`: Node 전용 10k행 비교, encoding 시간 제외, 50프레임×200행, FD64 B full D3, warmup5회·paired25회. 각 경로 모두 검증을 포함한다.

골든 4종은 빈 배치/null health·gap, variable DLC/FD64 B/RTR, 혼합 domain/u64 최댓값·필터 간격·request_id 없음/null/있음·quality/ts.quality/source identity와 기타 metadata, 절대 seq/빈 UART를 포함한다. 별도 시험은 DLC16코드·UART hex/base64 remainder와 선택 열 생략도 검증한다. Rust reader/writer는 아직 없고 CORE가 같은 corpus를 읽고 독립 생성해야 최종 상호운용 관문이 된다.

[Node 원본 수치](loom-columns-v1-node-perf.json)와 [성능 메모](loom-columns-v1-node-perf.md)를 참조한다. **브라우저 수치·실시간 렌더 지연·Rust 성능이 아니다.** full D3 프레임은34,069 B/200행(170.345 B/행)으로 이 fixture에서64 KiB를 지켰다. metadata cardinality/길이가 커지면 writer가 더 일찍 분할해야 한다.

## 9. CORE 확인 요청

1. 이 header/directory와 hash identity를 채택할지, 기존 D3 manifest hash와 결합한 최종 schema_hash 산식을 정할지.
2. 실제 subscribe sub_id 문자열을 프레임 사전 참조로 둘지, 협상된 숫자 ID로 바꿀지와 batch_id 증가/reset 규칙.
3. Classic RTR의 `len`은 요청 DLC 길이인가 실제 raw 길이0인가. 현재 제안은 전자를 적용한다.
4. raw_ref·비정규 bytes text·크기 초과 단일 UART 청크·다른 kind는 JSON fallback이라는 프로파일을 허용할지.
5. JSON metadata 사전/4 MiB accounting·JSON limits가 Rust producer의 분할 정책에도 적절한지. 정본 전에는 net에 연결하지 않는다.
6. 동일 corpus에 대한 Rust encode/decode·오류 corpus와 실제 CAN FD/UART 부하에서 공동 측정을 수행할 담당/시점.
