# P0 보고 — `crates/loom-can-dbc` (DBC 파서 + codec)

작성: can-opus, 2026-11-11. 범위: `crates/loom-can-dbc/`만 수정(커밋 안 함).

## 결과
- `cargo fmt --check` ✓ · `cargo clippy --all-targets -- -D warnings` ✓(경고 0) · `cargo test` ✓ **35개 통과**(단위 10, codec 14, load 8, proptest 2×512 cases, doctest 1) + 대용량 측정 1개(`#[ignore]`, 수동 실행).
- `unsafe` 없음, Python 없음. 기대값은 전부 손계산(테스트 주석에 계산 과정).

## 만든 것
| 파일 | 내용 |
|---|---|
| `src/load.rs` | `Database::load(bytes, &LoadOptions) -> Result<(Database, Vec<Warning>), LoadError>`. 파일 상한 → SHA-256 → 텍스트 디코드 → can-dbc 파싱(빠른 경로) → 실패 시 **문장 단위 복구** → compile |
| `src/text.rs` | 인코딩(UTF-8 엄격 / Windows-1252 / Auto), BOM 제거, 최상위 문장 분할기(따옴표·`\"`·`//` 인식, 버퍼 없는 iterator) |
| `src/compile.rs` | AST → 불변 IR. 모든 조회를 한 번에 HashMap으로(파일 크기에 선형). mux 해석·깊이·순환, 겹침 검사, 중복 id/이름, 블록 사유 |
| `src/bits.rs` | Intel/Motorola(sawtooth) 비트 배치. u128 창 하나로 O(1) 추출·삽입 |
| `src/decode.rs` / `src/encode.rs` | runtime codec |
| `src/e2e.rs` | `is_e2e_suspect` |
| `tests/fixtures/*.dbc` | 직접 쓴 fixture 6개(basic, float, mux, ext_mux, fd, broken). 외부 DBC는 쓰지 않음 |

## 공개 API 요약
- **로드:** `LoadOptions { encoding: TextEncoding::{Utf8|Windows1252|Auto}, limits: Limits }`. `Limits` 기본값 = plan §7: 파일 16 MiB, 메시지 50k, 신호 500k, 문자열 64 KiB, mux 깊이 16, IR 32 MiB(+ 문장 2M, 경고 1,000개 보관, 버린 문장 10k). 넘으면 `LoadError::LimitExceeded { what: LimitKind, limit, actual }`. `Auto`가 고르면 `Database::encoding_guessed()` + `EncodingGuessed` 경고. Latin-1은 Windows-1252로 읽는다(인쇄 가능 범위 동일).
- **Database:** `sha256()`/`sha256_hex()`(파일 바이트, `db_sha256`), `messages()`, `message_by_id(CanId)`, `is_ambiguous_id`, `message_by_name`, `signals_named`, `signal_by_path("Msg.Sig" | 유일한 이름)`, `signal(SignalRef)`, `nodes()`, `approx_heap_bytes()`, `decode(&CanFrame)`.
- **Message:** `id()`(`CanId`), `name`, `size`(0..64), `is_fd`(`VFrameFormat`에 FD 또는 길이 > 8), `transmitter`, `cycle_time_ms`(`GenMsgCycleTime`, 0 = None), `comment`, `signals`, `signal_by_name`, `is_multiplexed`, **`encode_blocked()` / `decode_blocked()`**(사유 문자열), `e2e_attributes`, `line()`(소스 줄), `decode`, `decode_payload`, `encode`.
- **Signal:** `start_bit`, `size`(1..64), `byte_order`(`Intel|Motorola`), `kind`(`Unsigned|Signed|Float32|Float64` — `SIG_VALTYPE_`), `factor`, `offset`, `min_max`, `range()`(`[0|0]` = 범위 없음 → `None`), `unit`, `value_table`, `label_for`, `raw_for_label`, `initial_raw_bits`/`initial_physical`/`initial_source`(`GenSigStartValue`, raw 기준, 신호 값 또는 `BA_DEF_DEF_`), `is_multiplexor`, `mux_condition()`(`{multiplexor, ranges}`), `receivers`, `comment`, `raw_value(bits)`, `physical(bits)`.
- **decode:** `DecodedMessage { message, signals: Vec<DecodedSignal>, length_mismatch, fd_mismatch }`. 신호마다 `Value { raw: RawValue::{Unsigned|Signed|Float32|Float64}, physical, label }` 또는 `Unavailable(ShortFrame { needed, actual })`. **활성 mux 분기만** 나온다. multiplexor 자체가 짧아 못 읽으면 그 분기는 생략(어느 분기인지 모름). 패닉 없음(길이 0..8 전부 시험). 오류: `UnknownId`, `AmbiguousId`, `Blocked`, `RemoteFrame`.
- **encode:** `msg.encode(&[(name, SignalInput::{Physical|Raw(i128)|Label})], EncodeBase::{Initial|Payload(&[u8])}, &EncodeOptions { force_range, brs, esi })` → `Encoded { frame, sources: Vec<(idx, ValueSource::{Given|Initial|Base|AutoMultiplexor})>, padded }`. `sources`는 dry-run·IG 미리보기의 "나머지 = DBC 시작값" 표시에 쓴다.
- **E2E:** `is_e2e_suspect(&Message) -> Option<Vec<E2eReason>>`. 이름 패턴(checksum, chksum, crc, chk, counter, alive — 대소문자 무시)과 이름에 e2e/crc/checksum/chksum/dataid/alivecounter가 든 메시지·신호 속성. **추정 근거만 낸다**(차단은 정책 몫).

## 규칙 (코드 doc과 같음)
- **반올림:** `raw = round_half_away_from_zero((physical - offset) / factor)`(`f64::round`). float 신호는 반올림 없이 그대로 저장.
- **범위:** 주어진 물리값이 `[min|max]` 밖이면 `OutOfRange`, 허용오차 1e-9 × max(1, |min|, |max|). `[0|0]`은 검사 안 함. `force_range`는 이 검사만 끈다. **비트 폭 초과(`RawOverflow`)는 언제나 오류.** 라벨(`VAL_`) 입력은 min/max 검사를 하지 않는다(예: 0xFF="SNA"가 max 밖인 DBC가 흔함) — 리드 확인 요망.
- **기준값(D17):** `Initial` = 활성인데 안 준 신호는 DBC 시작값, 시작값이 없으면 `MissingSignal`. 쓰지 않는 비트는 0. `Payload(bytes)` = 길이는 DBC 길이와 같아야 함(`BaseLengthMismatch`). 기준과 결과 모두에서 활성인 신호는 기준 비트를 유지한다. 새로 선택된 mux 분기는 시작값을 쓰거나 값을 줘야 하고, 비활성이 된 분기의 비트는 지운다.
- **mux:** 비활성 분기 신호에 값을 주면 `InactiveMuxSignal`. multiplexor를 안 줬으면 분기가 허용하는 값이 하나일 때 자동 설정하고 중첩도 따라 올라간다. 범위(`SG_MUL_VAL_ 0-2`)면 `AmbiguousMultiplexor`, 두 신호가 다른 값을 요구하면 `ConflictingMultiplexor`.
- **FD:** 길이가 유효한 FD 길이가 아니면 `fd_padded_len`까지 0으로 채움(`padded = true`). BRS 기본 켬, ESI 기본 끔.

## fail closed (요구 7)
can-dbc 문법은 **모르는 줄이 하나만 있어도 파일 전체를 거부**한다(`SGTYPE_`, `SIG_TYPE_REF_` 등. can-dbc-pest 0.8.1 `dbc.pest` `line` 규칙 확인). 그래서:
1. 먼저 파일 전체를 파싱하고, 실패하면 최상위 문장마다 따로 파싱해 실패한 문장만 빼고 다시 파싱한다. 뺀 문장은 `StatementDropped` 경고(줄 번호 = pest 오류 위치, 첫 줄 80자 요약).
2. 뺀 문장의 영향:

| 문장 | 결과 |
|---|---|
| `BO_` 블록 | 신호 없는 placeholder 메시지로 남긴다(id·이름·줄). decode·encode 모두 막고 사유에 줄 번호. 조회 시 "unknown"이 아니라 "파싱 불가"로 보인다 |
| `SG_MUL_VAL_`, `SIG_VALTYPE_` | 해당 메시지 decode·encode 모두 막음(레이아웃 불명) |
| `BA_ "GenSigStartValue"` / `"VFrameFormat"` | 해당 메시지 encode만 막음 |
| 그 밖(`CM_`, `VAL_`, 다른 `BA_` …) | 경고만 |

3. 그 밖에 막는 경우: 신호 레이아웃 무효·64바이트 초과, float 크기 불일치, 중복 신호 이름, mux 해석 불가(multiplexor가 여럿인데 `SG_MUL_VAL_` 없음, 모르는 multiplexor, 순환), 동시에 활성일 수 있는 신호끼리 겹침(encode만), 메시지 길이보다 긴 신호(encode만), factor 0(encode만), 중복 id(둘 다 막고 `AmbiguousId`).
4. 건너뛰는 경우(`MessageSkipped`): `VECTOR__INDEPENDENT_SIG_MSG` 같은 의사 메시지(id 비트 29/30), 0x7FF보다 큰 표준 id.
5. 쓰지 않는 섹션(`EV_`, `SIG_GROUP_`, `VAL_TABLE_`, `BA_REL_` 등)은 종류마다 경고 한 줄(개수·첫 줄).

## 시험 목록
- **단위(10):** 비트 배치(무효 레이아웃, Intel 바이트 경계, Motorola sawtooth, 이웃 비트 보존, 점유 집합, 짧은 데이터), 문장 분할(여러 줄 `CM_`, `\"`, `//`), `BO_`/참조 헤더, 인코딩(UTF-8 오류, Auto→1252, BOM).
- **codec(14):** Intel/Motorola 손계산 8바이트 벡터(`E0 2E F6 07 AB C5 05 68`), 음수·offset Motorola(`D9 86`), 시작값 채우기(`80 0C F6 04 00 00 00 96`), 반올림(12000.4→12000, 12000.5→12001, 149.5→150), 64비트 Intel/Motorola·signed 최소값, 29비트 id, float32/double(LE·BE), 단순 mux, mux 오류, 확장 mux(범위·중첩·자동 multiplexor·분기별 decode), CAN FD 64바이트·29비트·패딩(18→20)·`VFrameFormat`·fd_mismatch, `--base last`(값 유지, mux 분기 전환 시 지우기, 길이 오류), 범위·force_range·RawOverflow·라벨·이름·중복·NaN·MissingSignal, 짧은 프레임·remote.
- **load(8 + 1 ignored):** 조회·속성·주석·sha256 안정성, **broken.dbc 줄 번호 10/19/20/21/24**와 메시지별 차단 결과, 잘못된 DBC, 상한 8종(파일 16 MiB 실제 크기 포함, 메시지, 신호, 문자열, IR, 문장, mux 깊이, 버린 문장), 경고 상한(10개 + `WarningsTruncated "40 more"`), 인코딩 3종, 중복 id/이름/신호·겹침·의사 메시지·큰 표준 id, E2E 추정.
- **proptest(2×512):** 무작위 Intel/Motorola·signed·크기 1..64·시작 비트 → encode→decode 왕복, **신호 비트 외에는 0**(0xFF 기준이면 신호 비트만 바뀜), 물리값 왕복 오차 ≤ 반 단계.

## 측정 [실측, macOS arm64, release]
`large_file_at_limits`(`cargo test --release --test load -- --ignored large_file --nocapture`): 메시지 50k × 신호 8 = 40만, 파일 15.8 MB → **로드 6.2 s, IR 추정 142 MB, 시험 프로세스 최대 RSS 547 MB**(텍스트·can-dbc AST·기본 상한으로 한 번 거부된 로드 포함). 로드 시간 중 can-dbc(pest)와 compile의 비중은 미측정.

## 계획과 다르게 한 점 / 리드 결정 요청
1. **plan §7 상한끼리 맞지 않는다 [실측]:** (a) 짧게 쓴 `SG_` 줄도 약 36 B라 50만 신호는 16 MiB 파일에 들어가지 않는다(이름을 제대로 붙이면 50만 신호 ≈ 25 MB). (b) 신호당 IR이 약 350 B라 IR 32 MiB면 신호 약 10만 개가 한계다. 지금은 기본값을 plan 그대로 두었고, 큰 DB는 **IR 한도에서 명시 오류**로 거부된다. 제안: IR 상한을 신호 수에 맞추거나(예: 50만 → 256 MiB), 신호 상한을 10만으로 낮추거나, IR을 줄인다(`String`→`Box<str>`, 신호 이름 전역 인덱스 제거 등 — 절반 정도 기대, 미측정). "동시 DB 4개"는 gateway 쪽 일이다.
2. 의존성: `can-dbc 10.0.0`(`default-features = false, features = ["encodings"]` — serde 제외, encoding_rs만), **`pest 2`**(can-dbc가 이미 쓰는 같은 crate. 파싱 오류의 줄 번호를 읽으려고 추가), **`sha2 0.11`**(db_sha256), dev `proptest 1`. 전부 MIT/Apache-2.0.
3. can-dbc 결함 우회: 확장 비트 없는 id를 `u16`으로 자른다(`message_id.rs`, `value as u16`). 그래서 `BO_` 헤더의 id를 텍스트에서 따로 읽어 교차 확인한다. 표준 id > 0x7FF는 건너뛰고 경고한다.
4. 라벨 입력은 min/max 검사 예외(위 "범위"). plan에 명시가 없어 내가 정했다.
5. 빈 파일은 오류가 아니라 메시지 0개인 DB로 로드된다(can-dbc가 빈 입력을 받아들임). 오류로 바꿀지 결정 요청.
6. 겹침 검사는 메시지당 신호 4,096개까지만 쌍대 비교한다(C2, 넘으면 경고 후 생략).

## 남은 것 (이번 범위 밖)
gateway 연동·SignalCache·시계열·RPC(K6/G2), ARXML, 신호 그룹 의미, CRC/카운터 계산(E2E 플러그인), 공개 DBC(opendbc 등) 차등 시험은 안 했다(필요하면 MIT fixture 일부로 추가 가능). `loom-can-frame` 변경 요청은 없다.
