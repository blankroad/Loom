docs/plan/02-core-principle.md 준수

# UDS P0 — uds-opus (codec → sim → 도메인 골격)

먼저 `docs/impl/uds/tasks/p0-common.md`를 읽는다. **대화형 질문 창을 띄우지 않는다** — 스스로 정해 보고서에 적거나, 보고서에 질문을 쓰고 끝낸다(공통 규칙 "질문 금지"). 순서대로 3개 crate. **1번을 먼저 끝내라** — uds-sol의 러너가 응답 분류에 쓴다.

## 1. `crates/loom-uds-codec` (plan U-03 + 서비스 코덱)
순수 crate(I/O·tokio 없음). 의존: serde, serde_json, schemars, thiserror, hex.
- **SID 표**: ISO 14229-1 서비스 전부(0x10 11 14 19 22 23 24 27 28 29 2A 2C 2E 2F 31 34 35 36 37 38 3D 3E 83 84 85 86 87) — 이름, 서브기능 유무(SPRMIB 비트 0x80 적용 가능 여부), 최소 요청 길이. 정의 안 된 SID는 `None`.
- **요청 빌더**(M1 범위): 0x10 세션, 0x11 리셋, 0x22 읽기(DID 여러 개), 0x2E 쓰기, 0x19 02 마스크, 0x14 삭제, 0x3E(SPRMIB), 0x27 seed/key, 0x31 start/stop/result. 빌더는 길이 상한 검사.
- **응답 분류** `classify_response(req: &[u8], resp: &[u8]) -> ResponseKind`: `Positive` / `Negative{nrc}` / `Pending`(7F sid 78) / `Foreign`(이 요청의 응답이 아님 — SID·서브기능·DID 에코 불일치) / `Malformed`. 긍정 응답의 에코 검증(서브기능, DID) 포함. 러너가 이 함수 하나만 쓰므로 안정 API로 둔다.
- **응답 파서**: 0x50(P2 ms, P2* 10 ms 단위 → ms), 0x62(DB 길이로 다중 DID 분할, 길이 모르면 단일 DID만), 0x59 02(DTC 3바이트 + 상태), 0x67 seed.
- **NRC 표**: ISO 14229-1 NRC 전부 `nrc_name(u8) -> Option<&'static str>`(예 0x31 `requestOutOfRange`). UI·CLI·스킬이 같은 원천을 쓴다.
- **진단 DB 스키마**(serde + schemars JSON Schema): `DidDef{id, name, length, codec, writable}`, codec = `ascii | hex | uint{endian, scale, offset, unit} | bcd | enum{map}`. encode/decode, 길이 불일치는 오류. 표준 F1xx 내장표(F180–F19E ISO 이름, 예 F190 VIN 17 ASCII).
- **로더**: 지금은 JSON(serde_json). 파일 크기 상한·DID 개수 상한·문자열 길이 상한(C2 상수). YAML은 CORE가 `serde-saphyr`를 추가하면 같은 타입으로 붙인다(지금은 `Format::Yaml`이 `E_DB` 성격 오류 + "not yet" 반환). DB 식별 hash = 정규화 JSON에 대한 FNV-1a 64(자체 구현, 결정적).
- **DB는 등급을 바꾸지 못한다**(plan "DB 하향 무시"): DB 타입에 risk 필드를 두지 않는다.
- 시험: 빌더↔파서 왕복, `classify_response` 반례(다른 DID 에코, 0x78 다음 다른 SID의 긍정, 7F 다른 SID), 무작위 바이트 10만 회 panic 없음.

## 2. `crates/loom-uds-sim` (plan U-05의 순수 부분, §9.1)
의존: `loom-uds-codec`. tokio·I/O 없음 — DoIP 엔티티 어댑터는 uds-sol이 `loom-uds-doip`에 만든다(아래 인터페이스로 붙는다).
- `SimEcu::handle(&mut self, now_mono: Duration, req: &[u8]) -> Replies` — `Replies`는 상한 있는 `(delay, bytes)` 목록(0x78 다음 최종 응답 같은 순서열). 시간은 인자로만 받는다(결정적).
- 기본 동작: 세션 01/03(S3 5 s 만료 시 01로), 0x22(F190 VIN `LOOMSIM0000000001`, F1A0 1바이트 기본 0x01 등), 0x2E(쓰기 가능 DID만, 기본 세션이면 NRC 0x7F `serviceNotSupportedInActiveSession`), 0x19 02, 0x14, 0x11(리셋 후 세션 01·보안 잠금), 0x3E(SPRMIB면 무응답), 0x27(seed 4바이트, key = seed XOR 상수, 틀리면 0x35, 3회 0x36), 그 외 0x11 `serviceNotSupported`. 0x2A/0x86도 0x11.
- **결함 주입**(결정적, 대상 SID·DID별): 응답 지연, NRC 강제, 0x78 n회 후 최종, 응답 버림, P2 지난 늦은 응답, 잘못된 에코(다른 DID). 주입 계획 개수 상한.
- `SimNetwork`: 논리 주소(u16) → `SimEcu`, 상한 `MAX_SIM_ECUS = 64`. 기능 주소 요청은 해당 ECU 모두에 분배.
- 시험: 각 서비스 긍정·부정, S3 만료, 결함 주입별 응답 순서, 무작위 요청 10만 회 panic 없음.

## 3. `crates/loom-uds` (plan U-01·U-02 + TargetSession 모델 + 이벤트 payload)
의존: `loom-proto`, `loom-domain-api`, codec, sim, (있으면) `loom-uds-doip`·`loom-uds-runner`. 견본: `crates/loom-echo`, 시험 도구: `crates/loom-testkit`(RegistryProbe·HookHarness).
- `UdsModule: DomainModule`(NAME `uds`). `register`: M1 명령의 `CommandSpec`(connect/status/read-did/write-did/session/dtc list/raw/trace/abort/help — 이름·인자·RoleSet·Bounds), 이벤트 kind(`uds.tx`, `uds.rx`, `uds.state`, `uds.result`) + payload 스키마 + 고정 JSON Pointer, risk table, `WriteHooks`, capability, help. 핸들러는 P1 전까지 `DomainError::not_implemented`(가짜 성공 금지).
- **`classify` 훅**(plan §10.3 표를 데이터로): 실제 바이트 기준 R0–R4, SPRMIB 비트를 지운 서브기능으로 판정, 기능 주소면 reason에 표시. env 보정 없음(B4). F1: `block`은 0x2A·0x86만 `ImmutableDeny`, 나머지는 `None`(사용자 deny 설정은 코어 정책이 처리). SID 256개 전수 시험(panic 없음, 정의된 SID는 모두 행 존재), ≤ 1 ms.
- **TargetSession 순수 모델**(plan §3.3): generation, link 상태, 세션(id·이름·확신도 `confirmed|assumed|stale`), 보안 레벨, TP, P2/P2*, lease 소유자(origin·actor_id). 전이: 0x50 긍정 → confirmed + P2/P2* 갱신, S3 경과 추정 → stale, 재연결 → generation+1·세션 assumed(default)·보안 잠금, 0x11 긍정 → default·잠금.
- **이벤트 payload 타입**(serde + schemars): UI 요청 `docs/impl/requests/ui-uds.md`와 그 아래 **UDS 답변** 절을 정본으로 한다(리드가 씀). `payload.bytes{encoding:"hex", data}`, `payload.write_state`, `request_id` 필수(E8).
- 시험: RegistryProbe로 등록 검증, HookHarness로 classify 시간·결과, TargetSession 전이 표 시험.

보고서 3개(crate마다). 끝나면 멈춘다.
