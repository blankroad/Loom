# UI P3 선행 — loom-viz 데이터셋 엔진

작성: ui-sol, 2026-11-11. `docs/plan/02-core-principle.md` 준수.
작업 공간: `/Users/ctmctm/workspace/Loom/.worktrees/ui`, 브랜치 `ui/main`.

구현은 `crates/loom-viz/**`에만 추가했다. Cargo가 `Cargo.lock`에 새 workspace package `loom-viz` 항목 8줄을 추가했으며, 기존 외부 crate 버전·루트 Cargo.toml·다른 crate·UI 소스는 바꾸지 않았다. 커밋·stash·네트워크 사용 없음.

## 1. 산출물·역할

| 파일 | 역할 |
|---|---|
| `src/model.rs` | series/sample/value/quality/source·상한·오류·자동 렌더 종류 |
| `src/store.rs` | dataset 생성/제거·원본 수용·byte admission·자동 종류 판정 |
| `src/ingest.rs` | 제한된 스트리밍 JSONL/CSV·시간 열 탐지·오류 집계 |
| `src/timestamp.rs` | 정밀 decimal epoch·RFC3339·mono/wall ns 파싱 |
| `src/pyramid.rs` | 고정 크기 다층 first/last/min/max index cache |
| `src/query.rs` | gap 보존 minmax·원본 summary |
| `src/tests.rs`, `README.md` | 수용 시험·실행 가능한 API 예제·제약 설명 |

장치를 여는 코드가 없다. 에이전트와 사용자가 보는 같은 데이터의 source seq·rx_offset·request_id를 보존하고, 브라우저에 보낼 축소 결과와 원본 통계를 구분하는 역할이다. gateway/RPC/CLI·장치 구독·저널 저장은 이번 범위에 넣지 않았다.

## 2. 공개 API

```rust
DatasetStore::new(Limits) -> Result<DatasetStore>
create_dataset(id) -> Result<()>
remove_dataset(id) -> bool
append(id, SeriesMeta, Sample) -> Result<()>
ingest(id, impl BufRead, IngestOptions) -> Result<IngestReport>
metadata(id) -> Result<Vec<&SeriesMeta>>
memory_usage(id) -> Result<MemoryUsage>
auto_view(id) -> Result<Vec<Presentation>>
query(id, Query { series, t0, t1, px, mode: QueryMode::MinMax }) -> Result<QueryResult>
summary(id, series, t0, t1) -> Result<Summary>
parse_timestamp(&serde_json::Value) -> Result<ParsedTime>
```

- `SeriesMeta`: name/unit/value_type(number|enum|bool|string)/enum_map/time_source/time_label/clock_id/quality/source.
- `Sample`: ts_ns(u64), Value, quality, 샘플별 source, break_before.
- `Source`: domain/channel/seq, 선택 rx_offset/request_id/generation/clock_id. series 초기 source와 샘플별 원문 참조를 따로 보존한다.
- 메타 quality는 초기/선언 값이며, 현재 구간의 품질은 샘플·gap 정보가 정본이다.
- 시간은 내부 u64 ns. Sample·Gap·Summary 시간의 JSON 직렬화는 decimal string으로 2^53 초과를 보존한다.
- 값은 유한 f64·enum 문자열·bool·문자열. 비정상 quality에서 null을 Missing으로 보존한다. 숫자 값 자체는 IEEE754 f64 표현이다.
- 결과 points는 원본 샘플을 빌린다. 값·문자열·source를 대량 복제하지 않고, 결과 수명 동안 Rust borrow가 데이터셋 변경을 막는다.

사용 예제는 crate README와 doc-test에 있다. `IngestReport.error_count`·오류 목록을 확인한 뒤 query를 렌더해야 한다.

## 3. ingest·자동 판정

### 입력 지원

- long JSONL/CSV: `ts,series,value,unit,quality,source`. 선택 `value_type`, `enum_map`, `time_source`도 수용한다.
- wide CSV: 시간 열 이외를 series로 만든다. CSV의 source/enum_map/ns 객체는 JSON을 CSV quoting으로 감싼다.
- 공통 이벤트 JSONL: root `metrics[]` 또는 `payload.metrics[]`의 name/value/unit, 공통 timestamp·source 식별자를 사용한다.
- CSV comma·escaped doubled quote·CRLF·quoted multiline·UTF-8 BOM·빈 레코드를 처리한다.
- 한 record씩 처리한다. 전체 파일을 읽어 적재하지 않는다. CSV Auto의 probe도 최대 두 레코드다.

### 시간

- epoch 초: 정수·소수·지수. `serde_json/arbitrary_precision`으로 숫자 문자열을 유지하고 ns 계산은 정수 연산으로 한다.
- RFC3339: Z/offset, Gregorian 윤년 검증, 최대 9자리 소수. timezone 없는 local time·윤초·pre-epoch·u64 초과는 오류다.
- `{mono_ns,wall_ns}`: mono 우선, 문자열 u64의 최대값까지 보존한다. source/clock_id를 메타로 유지한다.
- CSV Auto: `ts|time|timestamp|ts_wall` 우선 → 두 레코드에서 ISO/ns 객체 또는 비감소 숫자 열 탐지 → 없으면 RowIndex.
- 이름 없는 숫자 시간축은 `Monotonic` + `csv_monotonic_seconds`로 표시해 wall clock이라고 주장하지 않는다. named epoch 숫자는 epoch 초다.
- probe는 전체 파일의 단조성을 증명하지 않는다. 이후 역행은 위치 있는 오류로 거부한다. 애매한 파일에는 `CsvTime::Column(name)`·`CsvTime::RowIndex`를 지정한다.
- 타입·unit·시간 기준/label/clock_id·enum_map은 정상 값 수용 뒤 고정이다. Missing뿐인 초기 구간은 첫 정상 값에서 타입을 확정할 수 있다.
- generation이 바뀌면 경계, clock_id가 바뀌면 새 series를 요구한다. source가 없는 중간 샘플 때문에 이미 알려진 clock identity를 잊지 않는다.

### 오류·수용 정책

- 오류마다 line/column/code/message. JSON parser 오류 column은 byte 위치, metric/CSV 값 오류는 항목 번호, 레코드 전체 오류는 column 1이다.
- 오류 상세는 상한까지만 보관하지만 총 error_count는 계속 증가한다. 긴 레코드도 경계까지 drain하고 다음 레코드를 처리한다.
- wide/metrics는 샘플별 수용이며 부분 성공이 가능하다. accepted/rejected 집계에 알 수 있는 샘플 slot 수를 반영한다. 완전히 파싱 불가능한 JSON의 샘플 수는 알 수 없고 error_count에 남긴다.
- 불량 레코드의 영향을 정확히 한 series로 한정할 수 없으므로, 오류 뒤 해당 dataset의 다음 수용 샘플부터 보수적으로 discontinuity를 표시한다. 잘못된 줄을 건너뛰고 직선으로 잇지 않는다.
- I/O 실패는 Err이며 이전에 수용된 prefix는 남는다. 호출자가 job 실패를 표시해야 한다.

### 자동 렌더

정수·고유값 ≤16 → step, 기타 숫자 → line, bool·고유값 ≤32의 string/enum → state, 그 외 table. 숫자 series는 이름 정렬 순서의 8개만 기본 표시한다. raw 값의 유한한 cardinality 탐색으로 판단하며 화면 축소본을 보지 않는다.

## 4. minmax·원본 summary

- query 범위는 inclusive `[t0,t1]`, px > 0. series당 points ≤ `min(4*px,20000)`.
- 픽셀별 first/last/min/max를 원본 index 순서로 반환하고 같은 index만 중복 제거한다. 동일 timestamp에 다른 값이 있어도 임의로 합치지 않는다.
- original_count는 선택 구간의 수용한 raw 개수(비정상 quality 포함), returned_count는 실제 그릴 정상 points 개수다. gap 범위는 별도 메타다.
- `quality != valid` 구간은 points로 잇지 않는다. gap 범위·샘플 수와 다음 점의 break_before를 반환한다. 혼합된 비정상 품질은 gap quality=mixed로 표시한다.
- generation/ingest 오류는 discontinuity. renderer는 **points의 break_before와 gaps를 모두** 지켜야 한다.
- 끊긴 segment가 너무 많아 point/gap 상한을 지킬 수 없으면 `query_points_limit`이다. 연결을 발명하거나 손실을 숨기지 않는다.
- bool/enum/string은 raw 전이를 보존하며 cap 초과 시 역시 오류다.
- 피라미드의 bucket이 query와 한 픽셀 경계 안에 완전히 들어오고 clean일 때만 쓴다. 경계·퇴출된 가속 범위·gap 부근은 raw fallback이다.
- summary는 수용한 raw에서 min/max/mean/count/valid_count/numeric_count/구간/gap_intervals를 계산한다. 화면 축소 데이터·피라미드 통계로 대신 계산하지 않는다.
- 서로 다른 신호의 timestamp union·clock 정렬·보간을 만들지 않는다.

## 5. C2 상한·결정

| 자원 | 기본 상한·정책 |
|---|---|
| dataset / store 전체 retained budget | 32 MiB / 128 MiB, 초과는 reject-new |
| dataset / series / raw sample | dataset 16개, dataset당 series 256개, series당 최대 2,000,000개 |
| raw 메모리 계상 | Vec capacity·소유 String capacity·metadata·map의 여유 inline slot 예약 비용·피라미드 예약 공간 |
| 논리 record | 64 KiB, 설정 최대 1 MiB; CSV token hard cap 4104개 |
| 시간축 probe | 최대 2 CSV records |
| 오류 상세 | 기본 128개, 설정 최대 1024개, 메시지 256문자 |
| 피라미드 | 기본 8층 × 층당 bucket 128개; L0=32 samples, 상층마다 4배 |
| query | 최대 64 series, 결과 구조 예약 예산 16 MiB, series당 최대 20,000 points와 유한 gap 목록 |

- byte 한도는 샘플 수 한도보다 먼저 걸릴 수 있다. raw를 자동 퇴출하지 않으며, 초과 시 기존 원본·summary는 보존한다. dataset 제거 시 비용을 반환한다.
- 가속 bucket만 층별 고정 크기로 퇴출한다. raw index가 정본이므로 cache가 덮지 않는 구간도 정확히 질의할 수 있다.
- 계상 예산은 allocator/RSS나 일시 realloc peak의 실측 보장이 아니다. query의 byte 예산도 **빌린 결과 구조** 예산이며 JSON wire byte 상한이 아니다. 후속 gateway에서 HTTP 분할/응답 byte 제한을 적용해야 한다.
- 10^7점 전체를 현재 기본 메모리 안에 보존한다고 주장하지 않는다. 한도 밖 원본은 CORE 저널·범위 질의 어댑터에 연결해야 한다. 이 단계에서는 명시 거부 정책을 제공한다.
- query는 정확성을 우선하므로 최악에 raw 전체를 스캔할 수 있다. 이번에 대규모 throughput/24 h soak/브라우저 지연 합격을 선언하지 않았다.

## 6. 검증 결과

| 명령 | 결과 |
|---|---|
| `cargo test --offline -p loom-viz` | **단위 시험 22개 + 실행 가능한 문서 예제 1개 통과** |
| `cargo clippy --offline -p loom-viz --all-targets -- -D warnings` | 통과 |
| `cargo fmt -p loom-viz -- --check` | 통과 |
| `cargo tree --offline -p loom-viz --depth 1` | serde 1.0.228·serde_json 1.0.149만 직접 사용 |
| `git diff --check -- crates/loom-viz Cargo.lock` | 통과 |
| `cd ui && pnpm check` | schema/typecheck/lint/Vitest 통과, **31 files / 325 tests** |

시험 범위: 각 입력 형식·CSV escaping/multiline/BOM·시간 열 선택/override·최대 u64와 2^53 초과 ns·정수/문자 cardinality·숫자 기본 8개·단일 spike·원본 픽셀 극값 대조·동일 timestamp·query 경계/빈 범위·gap/generation/clock·불량 입력 뒤 선 끊기·부분 metric 오류 위치·원본 평균·dataset/전체/sample/series/query 상한·String capacity 계상·고정 pyramid 크기·에러 상세 상한·긴 레코드 복구·초기 Missing 뒤 타입 확정.

UI 테스트는 기존 jsdom Canvas getContext 미구현 메시지를 냈지만 모두 통과했다. UI 소스는 이 작업에서 변경하지 않았다.

## 7. 의존성 요청·후속 연결

**새 외부 crate 요청 없음.** 잠금 파일에 CSV/날짜 parser crate가 없어 제한된 parser를 직접 구현했다. 기존 serde_json의 `arbitrary_precision` feature만 켰다(새 registry 항목 없음). 이 feature는 Cargo의 feature unification 대상이므로 CORE 통합 시 workspace 회귀 검사에 포함하면 된다.

다음 단계: CORE 저널/파일 job → 이 ingest/append → query·summary → HTTP/RPC/CLI 연결. CLI/gateway DTO·인증·취소·wire 응답 상한·sidecar/열 선택 UI는 아직 만들지 않았다. BLF/ASC/MF4/TRC 등은 계획대로 도메인 어댑터 책임이다.
