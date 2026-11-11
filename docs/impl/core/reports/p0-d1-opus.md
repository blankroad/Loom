# P0 D1 보고 — core-opus: `crates/loom-proto`

작업 위치: `.worktrees/core`(브랜치 `core/main`). **커밋하지 않음**(리드 검토 후 커밋).

## 만든 것
- `src/` 11개 모듈 전부 구현(lib.rs 모듈 목록은 변경 없음): `num`(U64), `bytes`(Bytes·Base64Bytes, 64 KiB 상한), `event`(Ts·Origin·Risk·Env·WriteState·SourceRef·**SourceRecord/PublishedRecord**·PayloadBytes·GapData·GapCount·kind 상수·`validate_event_kind`), `error`(ErrorCode 표 매크로 + `exit_code` + `ProtoError`), `envelope`, `evidence`(DTO + REF 문자열 파서/포매터 + RFC 6901 검사), `cursor`, `rpc`(JSON-RPC 2.0·Hello·Rights·Subscribe·EventsBatch·`validate_method`·예약 domain 8개), `policy`(F1: 기본 전부 Auto), `worker`(WorkerMsg 19종·Lane·FrameHeader·`split_frame`), `schema`(`export_all` 16개, draft 2020-12 고정).
- 시험 `tests/` 4파일 + fixture 12개: **31개 전부 통과**(corpus 10, golden 4, msgpack 6, names 11). `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `RUSTDOCFLAGS=-D warnings cargo doc` 통과. 빌드·시험 각 1분 이내.

## 결정한 세부
- **U64 선행 0 거부**(`"0"`만 예외). 같은 숫자 = 같은 문자열이어야 JCS digest(승인·감사)가 안정적. schema pattern은 `^(0|[1-9][0-9]{0,19})$`(지시서의 `^[0-9]{1,20}$`보다 엄격). JSON 숫자 입력도 거부.
- Bytes JSON 입력은 대소문자 hex 모두 허용, 출력은 소문자. Base64는 표준 패딩형만.
- **`#[serde(flatten)]`·내부 태그·untagged 미사용**: serde 버퍼링이 `is_human_readable()`을 잃어 U64의 MessagePack 형식이 깨진다. 원본/발행 레코드는 매크로로 같은 필드를 생성하고 `publish()`/`into_source()`로 변환. WorkerMsg는 외부 태그(`{"hello":{…}}`).
- 선택 필드는 `None`·빈 배열이면 출력 생략(입력의 `null`·`[]`는 그대로 읽음). **`payload`는 항상 출력**(빈 객체 `{}`), 없으면 `{}`로 읽음(계약 02 §2.2).
- `validate_method`는 **2–3토큰**(지시서의 "1–3토큰"은 예시 형식과 맞지 않아 2–3으로 함).
- 커서 도메인 확장 값에는 `:` 금지(`e:seq:1` 같은 모호성 제거). 상대 커서는 `(amount, unit)`로 보존해 문자열 왕복이 정확.
- `payload_write_state`는 `Result<Option<_>>`(값이 있는데 잘못되면 오류). `raw_ref`의 `payload_bytes`는 `ProtoError::NotInline`.
- 프레임: `len` = 페이로드 길이, 헤더 포함 ≤ 64 KiB, v1은 flags·reserved 모두 0이어야 함, 모르는 msg_type 오류.
- `HelloResult.rights`는 단일 수준 `Option<Rights>`(watch < operate < approve, approve가 operate 포함).
- `ApprovalScope::Session{max_uses, until_mono_ns}`(지시서 `until`을 단위 명시 이름으로).

## 계약 문서와 다른 점 (리드 확인 요청)
1. `03-cli.md` §3.2 봉투 예시는 `null`·`[]`·`{}`를 모두 적었지만 타입은 생략해서 출력한다(읽기는 둘 다 됨). 예시를 생략형으로 바꾸거나 "생략 가능" 한 줄 권장.
2. `Meta.visibility`(visible|not_visible|no_viewer)를 선택 필드로 추가 — 문서 예시에 없음.
3. 사후 생성물 REF `la:<dev>@capture:<n>#row:<a>..<b>`(07 §7.2)는 아직 파서 미구현.
4. `core.snapshot_part/snapshot_end` 페이로드 타입은 미정의(문서에도 형식 없음).
5. 경고 코드 `daemon_tied_to_parent_job`는 문서에 맞춰 추가함(차이 해소).

## 리드에게 필요한 것
- 의존성 추가 요청 없음. jsonschema 검증 crate가 있으면 fixture를 스키마로도 검증 가능(지금은 schemars 생성 + 역직렬화 왕복).
- 루트 `Cargo.toml`에 `clap` 추가 diff가 있는데 **제 변경이 아님**(다른 작업자로 보임).
- fixture JSON 문법 확인에 python 한 번 사용(제품 코드 아님).
