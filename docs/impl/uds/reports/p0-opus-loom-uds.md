# P0 보고 — uds-opus · `crates/loom-uds`

**만든 것**: UDS 도메인 gateway 조각. 내용은 등록, `classify` 훅과 데이터 위험표, TargetSession 순수 모델, 이벤트 payload 타입, 요청 텍스트 파서, 순수 dry-run이다. 의존은 loom-proto, loom-domain-api, codec, sim이고, dev 의존으로 loom-testkit을 쓴다.

**공개 API 요약**
- `UdsModule: DomainModule`(NAME `uds`). `register` 등록 내용:
  - 명령 10개: `uds.connect`, `uds.status`, `uds.read_did`, `uds.write_did`, `uds.session`, `uds.dtc.list`, `uds.request`(CLI `raw`와 UI 명령줄), `uds.trace`, `uds.abort`, `uds.help`. 파라미터 스키마는 schemars 구조체에서 만든다.
  - ECU로 바이트를 보내는 명령은 모두 `WriteClass::Request`(R0 읽기 포함, 단일 쓰기 경로)이고 RoleSet은 WRITE다.
  - 이벤트 kind 4개(`uds.tx/rx/result/state`) + payload 스키마 + 고정 Pointer. 위험표, `UdsHooks`, `ModuleRuntime::Dedicated{1}`, `StateReducer`, safe_state `uds.session_default`, capability `doip/sim/dry_run`, help.
- 핸들러: 장치 명령은 `E_NOT_IMPLEMENTED`를 돌려준다(가짜 성공 없음). **실동작**은 두 개뿐이다: `uds.help`(help, 위험 규칙, NRC 표)와 `uds.request{dry_run:true}`(순수 함수, 이벤트·감사 없음).
- `risk::RULES`(plan §10.3 표를 데이터로 둠, 먼저 일치한 행 적용), `classify_pdu(bytes, functional)`(SPRMIB 제외 서브기능, 기능 주소면 0x3E/0x22/0x19 외 한 단계 상향, env 보정 없음), `risk_table()`(규칙마다 `uds.request` 행 + 타입 명령 행).
  - **F1**: `block`은 0x2A·0x86의 `ImmutableDeny`뿐이다. FF00, 0x34–0x3D, 미정의 SID는 R4로 표시만 하고 막지 않는다. `default_blocked`는 비어 있다.
- `UdsHooks`: 바이트는 `req.bytes`를 쓰고, 없으면 타입 파라미터를 `text::encode_typed`로 인코딩한다(핸들러와 같은 인코더). 인코딩할 수 없으면 R4로 표시한다.
  - `approval_binding`: 대상, generation, 요청 FNV 해시, display.
  - `presend_check`: generation 0이면 `EpochChanged`.
- `session::TargetSession`: generation(0 = 미연결, link_up마다 +1), link(cause connected/reconnected), 세션·보안 확신도.
  - 전이: 0x50 → confirmed + P2/P2*, 0x11 → default·잠금(assumed), 0x67 sendKey → 보안 confirmed, 7F 7E/7F → stale, 7F 33 → 잠금, S3 경과(TP 없음) → stale, link_down → stale, SPRMIB 성공 → assumed.
  - `state_payload()`는 ui-uds 답변 #3 모양을 따른다.
- `payload::*`: `TxPayload`, `RxPayload`(kind·nrc·late), `ResultPayload`(write_state·nrc·nrc_name·timeout·ok_suppressed·block{class,reason}·policy_action·mono ns 세 개), `StatePayload`, `DryRunResult`(답변 #1). 바이트는 `PayloadBytes::hex`, mono 시각은 `U64` 문자열이다.
- `text::parse_text`: hex / `read` / `write` / `session` / `tp`. 오류는 `{at, message}`로 돌려준다.

**C2 상한**
| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_TEXT_LEN` | 8192 B | 파싱 전에 `parse_error` |
| 요청 PDU | `MAX_PDU_LEN` 4095 | `parse_error` / 인코딩 오류 |
| `MAX_SNAPSHOT_TARGETS` (reducer) | 64 | 가장 오래 갱신되지 않은 대상을 버린다 |

**시험**: 8개 모두 통과(단위 1 + 통합 7).
- RegistryProbe: 오류·경고 0, immutable_deny `["sid=2A","sid=86"]`.
- HookHarness: SID 256 × 서브기능 6 전수. 호출마다 ≤ 1 ms, 차단은 0x2A/0x86만.
- 표 사례 18개, 타입 인자 분류와 presend.
- TargetSession을 SimEcu 응답으로 구동: S3 stale을 sim 실제 세션 복귀와 대조, 재연결 시 generation 2.
- dry-run JSON 모양, 무작위 바이트·텍스트 10만 회 panic 없음.

관문: fmt --check(세 crate) OK · clippy `-D warnings` 경고 0 · test 8 passed.

**임시 정의·통합 시 교체**:
- `loom-uds-doip`·`loom-uds-runner`가 아직 없어 의존하지 않았다. P1에서 핸들러를 연결할 때 붙인다.
- `presend_check`는 아직 상태가 없다. quarantine·lease·현재값 검사는 TargetSession 레지스트리를 연결하면서 넣는다.

**결정 (질문 대신 기록)**:
1. CLI `raw`와 ui-uds의 `uds.request`를 RPC 하나(`uds.request`)로 합쳤다. 별도 `uds.raw`는 없다.
2. help 명령은 RPC `uds.help`로 실제 데이터를 돌려준다.
3. 0x83 AccessTimingParameter는 plan 표에 없어서 read(01/03)는 R0, set은 R2로 정했다.
4. 0x2E "DB 없는 쓰기 R3" 규칙은 hook에 DB가 아직 없어 적용하지 않았다. 지금은 ISO 식별 DID(F180–F19E)만 R3다.
5. 0x11 긍정 응답 뒤 세션 확신도는 `assumed`로 했다(응답 직후 리셋되므로).

**CORE 추가 요청**: 없음(codec·sim은 path 의존).
**미해결**:
- 패널 manifest는 UI 형식이 정해지지 않아 미등록이다.
- policy_section 스키마는 P1에서 등록한다.
- 승인을 켰을 때의 경로(R2 token_or_viewer)는 F1에 따라 구현하지 않았다.
