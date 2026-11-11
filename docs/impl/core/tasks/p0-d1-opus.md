docs/plan/02-core-principle.md 준수

# P0 Day 1–2 — core-opus: `crates/loom-proto` 구현

작업 위치: **`/Users/ctmctm/workspace/Loom/.worktrees/core`**(브랜치 `core/main`). 저장소 루트(main 체크아웃)에서는 코드를 고치지 않는다.
먼저 읽기: `/Users/ctmctm/workspace/Loom/docs/impl/00-impl-playbook.md`, `docs/plan/core/plan.md`(§3.3, §3.4, §3.9, §4.3–4.5, §6.1, §10.4), 마스터 `docs/plan/00-master-plan.md` **§2.4 E1–E12, §2.5 F1**(기본 모두 자동 — 최우선).

## 목표
도메인 팀과 UI 팀이 첫 주에 `cargo add` 해서 바로 쓸 **계약 v1 타입 crate**. 태그 `core-p0-d2`의 핵심. tokio·I/O 의존 없음. 리드가 동시에 `docs/contract/`(계약 문서 v1)를 쓴다 — 타입과 문서가 다르면 리드에게 보고(파일 끝 "문서와 다른 점" 절).

## 이미 있는 것 (리드가 만듦)
- 루트 `Cargo.toml` workspace(edition 2024, workspace.dependencies: serde, serde_json, serde_bytes, schemars 1.2, rmp-serde(dev), thiserror 2, hex, base64; lints: unsafe_code deny, unwrap_used deny 등). **루트 Cargo.toml은 수정하지 말 것** — 의존성이 더 필요하면 보고에 적고 이유를 쓴다(리드가 추가).
- `crates/loom-proto/src/lib.rs`(모듈 선언·상수)와 빈 모듈 파일 11개. 모듈 파일을 채운다. lib.rs의 모듈 목록을 바꿔야 하면 보고에 적는다.

## 모듈별 내용 (plan.md 절 번호)
1. `num.rs` — `U64`(JSON = 10진 문자열 + schema `{"type":"string","pattern":"^[0-9]{1,20}$"}`, MessagePack 등 비 human-readable = u64 정수). `Serializer::is_human_readable()`로 한 타입에서 분기. 역직렬화는 **fallible**: 음수·소수·공백·선행 `+`·2^64 이상 → 오류(panic 금지). 선행 0 허용 여부를 정하고 문서화.
2. `bytes.rs` — `Bytes`(Vec<u8>): JSON 기본 hex 소문자 문자열, 비 human-readable = bin(`serde_bytes`). 별도 `Base64Bytes`(이벤트 payload 대용량용, JSON = base64 표준). 디코더 fallible: 홀수 길이·비ASCII·비hex·**상한 초과(상수 `MAX_INLINE_BYTES = 64 KiB`)** → 오류. 길이를 먼저 검사한 뒤 할당.
3. `event.rs` (§6.1, E1, E8) —
   - `Ts{mono_ns: U64, wall_ns: U64, clock_id: String, source: TsSource, quality: Option<String>}`, `TsSource{HostReadComplete, Driver, Hardware, Device, Gateway, Synthetic}`.
   - `Dir{Rx, Tx}`, `Origin{Agent, User, Device, Script}`(고정 4종), `Risk{R0..R4}`, `Env{Sim, Bench, Vehicle}`, `WriteState{Requested, Accepted, Sent, Confirmed, EffectiveVerified, EffectUnverified, Failed, OutcomeUnknown}`(A14).
   - `SourceRef{producer_id, producer_epoch: U64, source_seq: U64}`.
   - **`SourceRecord`**(worker가 저널에 쓰는 원본: stream seq·epoch **없음**) / **`PublishedRecord`**(허브가 `seq: U64`, `epoch: U64`(= stream_epoch) 를 붙인 것, `#[serde(flatten)]`로 SourceRecord 포함 또는 공통 필드 구조체 공유 — 와이어 JSON이 평평한 한 객체가 되게).
   - 필드: `schema_version, session_id, domain, channel: Option, kind, producer_id, producer_epoch, source_seq, ts, dir: Option, origin, client_id: Option, actor_id: Option, external_actor: Option, original_origin: Option, request_id: Option, job_id: Option, generation: Option<u32>, purpose: Option, risk: Option, test_session: Option, test_step: Option, env: Option, payload: serde_json::Value(kind별 구조 객체, 기본 빈 객체), metrics: Vec<Metric>(비면 생략), quality: Vec<String>(비면 생략)`. **최상위 `write_state`·`data` 필드 없음** — 쓰기 상태는 `payload.write_state`, 원시 바이트는 `payload.bytes`(E1·E8, plan §6.1).
   - 헬퍼: `PayloadBytes{encoding: Encoding(hex|base64|raw_ref), data: Option<String>, r#ref: Option<RawRef>}` 타입과 `payload_write_state(&Value) -> Option<WriteState>`·`payload_bytes(&Value) -> Result<Option<Vec<u8>>, _>` 같은 읽기 헬퍼.
   - `Metric{name, value: f64, unit: Option}`. 손실 3층: `GapData{seq_range: Option<SeqRange>, producer_range: Option<ProducerRange>, count: GapCount, cause: String, bytes: Option<U64>, detail: Value}`, `GapCount = Exact(U64) | Unknown`(JSON `"123"` | `"unknown"`).
   - kind 상수: `core.latch`, `core.approval_requested`, … `display.gap`, `record.gap`, `record.health`, `source.gap`, `ui.focus_request`, `core.agent_cursor`, `core.activity`(plan §6.1 목록 + §3.4 v0.2 추가). 이름 검사 함수 `validate_event_kind(&str)`(2토큰, `[a-z][a-z0-9_]*`).
4. `error.rs` (§4.4, E2·E3) — `ErrorCode` enum(SCREAMING_SNAKE serde, `E_` 접두는 serde rename로) 전체 목록과 `const fn exit_code(self) -> u8`. 묶음: 1 INTERNAL·NOT_IMPLEMENTED / 2 USAGE·AMBIGUOUS_CHANNEL·CONFIG·DB·UNKNOWN_METHOD·SCHEMA / **3 TIMEOUT·NO_MATCH·UNEXPECTED_MATCH·COUNT_OUT_OF_RANGE**(E2) / 4 DENIED·UNSUPPORTED·RATE_LIMIT·BUSY·RECORDING_FAILED·EVIDENCE_MISSING·EVIDENCE_MISMATCH·OUT_OF_WINDOW·PLAN_MISMATCH / 5 CONFIRM_REQUIRED·APPROVAL_REQUIRED·NO_APPROVER·APPROVAL_DENIED·APPROVAL_EXPIRED / 6 USER_ACTIVE·LOCKED·LOCK_BROKEN·HELD_BY_USER·INTERRUPTED_BY_USER·QUARANTINE·STOPPED·LEASE·SESSION_ACTIVE·SESSION_PAUSED / 7 NOT_OPEN·DISCONNECTED·CONNECTION_CHANGED·BUS_OFF·NO_RESPONSE·PORT_BUSY·PERMISSION·NOT_FOUND·WORKER_DOWN·WORKER_BUSY / 8 CURSOR_GAP·SCAN_LIMIT·PARTIAL / 9 DAEMON·PROTO_MISMATCH / 10 NRC / **11 TEST_FAILED**(E3). 시험: 모든 변형의 exit_code가 표와 같음(표를 시험 코드에 한 번 더 적어 교차 검증), JSON 이름 왕복.
5. `envelope.rs` (§4.3) — `Envelope<T>{schema_version, ok, cmd, data: Option<T>, meta: Meta, warnings: Vec<Warning>, others_since_last: OthersSinceLast, error: Option<ErrorBody>}`, `Meta{truncated, next_cursor: Option<Cursor>, range: Option<SeqRange>, display_gap: Option<GapSummary>, record_health: RecordHealth, env: Env, latch: bool, viewers: Viewers{subscribed, visible}, stream_epoch: U64, visibility: Option<Visibility>}`, `RecordHealth{Ok, Degraded, Failed, Disabled}`, `OthersSinceLast{count, origins, from_cursor, kinds, by_channel}`, `ErrorBody{code, message, hint, retryable, request_id, detail}`, `Warning{code, detail}` + 경고 코드 상수(`no_viewer_attached`, `no_visible_viewer`, `daemon_autostarted`, `record_degraded`, `env_vehicle`). `data.evidence[]` DTO는 `evidence.rs`.
6. `evidence.rs` (§4.3, test-domains) — `EvidenceRef{stream_epoch: U64, seq_range: SeqRange, source_refs: Vec<SourceRef>}`, `Evidence{r#ref, kind, pointer: Option<String>, predicate_id, observed, expected, request_id, complete: bool}`, 사람용 REF 문자열 `<domain>:<channel>@e<epoch>:seq:<a>[..<b>][#<json-pointer>]` **파서/포매터**(epoch 생략형도 파싱해서 `epoch: None`). JSON Pointer는 RFC 6901 이스케이프(~0 ~1) 검사.
7. `cursor.rs` (§4.5) — `Cursor` enum: `Last`(`@last`), `Now`, `Seq{epoch: Option<u64>, seq}`, `Mark(String)`, `Relative(Duration 음수 초, "-5s"|"-250ms"|"-2m")`, `Clock(HH:MM:SS(.mmm))`, `Iso8601(String — 형식만 검사)`, `Domain{prefix, value}`(예: `off:N`, `send:last` — 공통 문법과 겹치지 않는 접두어만). `FromStr`/`Display` 왕복, 잘못된 입력은 오류. 와이어 직렬화는 문자열.
8. `rpc.rs` (§3.4, §4.7) — JSON-RPC 2.0 기본(`Request{jsonrpc, id: Option<Id>, method, params}`, `Response`, `RpcError{code, message, data}`, `Id = Number(i64) | String`), 규격 오류 상수(-32700/-32600/-32601/-32602/-32603). `Hello{proto, token: Option, client_id, want_role, client{kind, version, tty}}`, `HelloResult{role, rights, conn_id, stream_epoch, server, env, latch, viewers, capabilities}`, `Role{Agent, Viewer, Script}`, **`Rights{Watch, Operate, Approve}`**(E9). `validate_method(&str)`(1–3토큰 `<domain>.<verb>`/`<domain>.<noun>.<verb>`), 예약 domain 상수 8개(`core, record, display, source, run, test, ui, adapter`). `Subscribe{filter{domains, channels, kinds, origins}, after_seq: Option<U64>, snapshot: bool, backlog, max_batch_ms, encoding(json|binary), coalesce_by: Option}` 정도까지(구독 결과·이벤트 배치 타입 `EventsBatch{sub, first, last, events: Vec<PublishedRecord>, display_gap, lag_ms}` 포함).
9. `policy.rs` (§10, **F1**) — `PolicyAction{Auto, Confirm, Approve, Deny}`. **기본값 = 모든 등급 `Auto`**(F1). `BlockClass{DefaultBlocked, HardDeny, ImmutableDeny}` — 단 F1에 따라 `DefaultBlocked`는 기본 비어 있음, `ImmutableDeny`는 "Loom이 구현하지 않은 서비스"만. `ApprovalScope{Once, Session{max_uses, until}, TestSession, Run}`, `Presence{method: Code|Webauthn|Oob, proof}`(E9: 켰을 때만), `ApprovalKind{Write, JobStart, PolicyRelax, Unlock, TestPlan, RunPlan}`, `WriteClass{Request, Job, Interactive}`. 정책 판정 로직은 넣지 않는다(타입만).
10. `worker.rs` (§3.3) — `loom-worker/1` 메시지 enum(`Hello`, `Welcome`, `Reject`, `Call`, `Write`, `JobStart/Update/Renew/Stop`, `InputLease`, `Input`, `Latch`, `Credit`, `Reply{call_id, result, covers: Option<SourceRef>}`, `PriorityEvents`, `Health`, `LatchAck`, `Segment`, `Events{first_source_seq, count, records}`), lane 구분(`Lane{Ctl, Data}`), 프레임 헤더 `FrameHeader{len: u32, msg_type: u8, flags: u8}` 인코딩/디코딩 함수(순수 함수, `MAX_FRAME = 64 KiB`, 길이를 할당 전에 검사, 잘린 입력·상한 초과·모르는 msg_type → 오류). 직렬화 형식은 MessagePack map(rmp-serde)을 쓰는 쪽(worker-sdk)이 정하고 여기선 serde 타입만.
11. `schema.rs` (§7, C3) — `pub fn export_all() -> BTreeMap<&'static str, schemars::Schema>`(최소: PublishedRecord, SourceRecord, Envelope<Value>, ErrorCode, Cursor(문자열), Hello, HelloResult, EventsBatch, Evidence, worker 메시지). draft 2020-12 확인(`$schema` 값 시험).

## 시험 (`cargo test -p loom-proto`, `crates/loom-proto/tests/`)
- `tests/fixtures/*.json` 골든: 이벤트(UART rx, CAN 프레임, UDS 요청/응답 행 갱신 = 같은 request_id의 write_state 단계들, `display.gap`·`record.gap`·`source.gap`(count unknown 포함)), 봉투(성공·오류·truncated·others_since_last), hello. 각 fixture가 역직렬화 → 재직렬화 동일(키 순서 무관 비교), 그리고 JSON Schema로 검증되는지 — **jsonschema 검증 crate가 필요하면 리드에게 요청**(우선은 schemars 생성 성공 + 역직렬화 왕복으로 충분).
- JSON ↔ MessagePack(rmp-serde, `to_vec_named`) 왕복: U64 최대값, None 필드, 빈 bytes, Unicode, 모르는 필드 무시.
- 비정상 corpus(plan T15): 홀수 hex, 비hex, 비ASCII, 음수/소수/2^64 U64 문자열, 상한 초과 bytes, 잘린 msgpack, 모르는 enum 값, 잘린 프레임 헤더 → 전부 `Err`, panic 0.
- 커서·REF 파서 정상/비정상, 메서드·이벤트 이름 검사.

## 품질
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과. `unwrap()`는 lint로 금지(시험 코드는 `#[allow(clippy::unwrap_used)]` 모듈 단위 허용). 공개 타입은 `Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema`. 모든 공개 항목에 한 줄 doc 주석(계약 문서 절 번호 참조 가능).

## 보고
- 커밋하지 않는다(리드가 검토 후 커밋). 끝나면 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p0-d1-opus.md`에: 만든 것, 시험 결과(개수), 계약 문서와 다른 점·결정한 세부(선행 0 등), 리드에게 필요한 것(의존성 추가 등). 화면에는 경로와 3줄 요약만.
- 10분 넘는 빌드·시험 금지. 파일 읽기가 30초 넘게 멈추면 보고에 적고 멈춘다.
