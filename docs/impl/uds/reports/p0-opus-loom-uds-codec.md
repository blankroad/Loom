# P0 보고 — uds-opus · `crates/loom-uds-codec`

**만든 것**: UDS 서비스 코덱. 순수 crate(I/O·tokio·시계 없음). 의존 crate는 serde, serde_json, schemars, thiserror, hex뿐이다.

**공개 API 요약**
- `sid::service(u8) -> Option<&ServiceInfo{sid, name, has_subfunction, min_request_len}>`: ISO 14229-1 요청 서비스 27개(0x10–0x87). 정의되지 않은 SID와 응답 SID는 `None`. `sid::ids::*` 상수.
- `nrc_name(u8) -> Option<&str>`: 2013판 표 + 2020판 인증 NRC(0x34, 0x50–0x5D, 0x94). 0x38–0x4F와 0xF0–0xFE는 범위 이름으로 돌려준다. `NRC_RESPONSE_PENDING = 0x78`.
- **`classify_response(req, resp) -> ResponseKind`** (러너 안정 API): `Positive | Negative{nrc} | Pending | Foreign{why} | Malformed{why}`, `is_final()`.
  - 에코 검증: 서브기능(bit7 제외), 0x22/0x24 첫 DID, 0x2E DID(응답 3바이트), 0x2F DID+제어 파라미터, 0x31 RID, 0x36 블록 카운터, 0x14는 1바이트.
  - 부정 응답은 정확히 3바이트여야 한다. 다른 SID의 7F는 Foreign이다. panic도 할당도 없다.
- `request::*` 빌더: `session_control`, `ecu_reset`, `read_dids`, `write_did`, `read_dtc_by_status_mask`, `clear_dtc`, `tester_present`, `security_request_seed/send_key`, `routine_control(RoutineOp,…)`. SPRMIB는 `suppress` 인자로만 켠다(bit7이 이미 켜진 값을 넘기면 오류).
- `response::*` 파서: `parse_session_response`(P2 ms, P2* 10 ms 단위 → ms, 2바이트 짧은 형식은 timing 없음), `parse_read_did_response(req, resp, &dyn DidLookup)`(마지막 DID를 뺀 나머지는 길이를 알아야 한다), `parse_dtc_by_status_response`(24bit raw + status, `DTC_STATUS_BITS`), `parse_seed_response`(`already_unlocked()`).
- `DidDef{id, name, length: Option<u16>, codec, writable}`, `DidCodec = ascii | hex | uint{endian, scale, offset, unit} | bcd | enum{map:[{value,name}]}`, `decode → DidValue`, `encode(DidInput)`. 길이가 다르면 오류다.
  - 표준표 `standard_did(F180–F19E)`: F190은 ASCII 17바이트, F186은 uint 1바이트, 나머지는 가변 길이 hex.
  - F18F는 이름이 확실하지 않아 넣지 않았다.
- `load_db(bytes, Format) -> LoadedDb{db, hash}`: JSON만 지원. `Format::Yaml`은 `E_DB "not yet"`을 돌려준다. `hash` = 정규화 JSON에 대한 FNV-1a 64(`fnv1a64`, 자체 구현). `DiagDb::json_schema()`. `DidLookup` 구현: `LoadedDb`(사용자 DB 다음 표준표), `StandardDids`.
- **DB는 등급을 바꾸지 못한다**: risk 필드가 없고 `deny_unknown_fields`라서 `"risk"` 키가 있으면 로드가 거부된다(시험 포함).
- `CodecError::code()` → `E_USAGE | E_DB | E_DECODE`.

**C2 상한**
| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_PDU_LEN` | 4095 B | 빌더 `TooLong`, 파서 `TooLong`/`Malformed` (할당 전에 검사) |
| `MAX_DIDS_PER_READ` | 64 | `TooMany` |
| `MAX_DTCS` | 1023 (PDU에서 도출) | PDU 상한이 막는다 |
| `MAX_DB_FILE_BYTES` / `MAX_DB_DIDS` | 1 MiB / 4096 | 파싱 전에 / 검증에서 `E_DB` |
| `MAX_DID_DATA_LEN` / `MAX_NAME_LEN` / `MAX_ENUM_ENTRIES` | 4092 / 128 / 256 | 검증 `E_DB`, 디코드 `TooLong` |

**시험**: 26개 모두 통과. 단위 6, classify 7(다른 DID 에코, 0x78 다음 다른 SID의 긍정, 다른 SID의 7F, 서브기능·RID·블록 에코, Malformed 8종), roundtrip 6, db 4(공백만 달라지면 같은 해시, risk 키 거부, 중복, 한도, 스키마), fuzz 3(분류·파서 10만 회, DID 디코드 10만 회, DB 로더 10만 회, 결정적 xorshift).
관문: `cargo fmt -p loom-uds-codec --check` OK · `cargo clippy -p loom-uds-codec --all-targets -- -D warnings` 경고 0 · `cargo test -p loom-uds-codec` 26 passed.

**임시 정의·통합 시 교체**: 없음.
**CORE 추가 요청**: `serde-saphyr`(YAML, 이미 요청됨). 붙이면 `Format::Yaml`에 같은 `DiagDb` 타입을 연결한다.
**미해결**:
- 2020판 NRC 이름(0x34, 0x50–0x5D, 0x94)과 F180–F19E 이름은 공개 자료 기준이라 ISO 원문과 대조해야 한다.
- 부정 응답 길이 ≠ 3을 Malformed로 판정하는 것은 엄격한 해석이다. 실ECU가 패딩을 붙이면 완화를 검토한다.
