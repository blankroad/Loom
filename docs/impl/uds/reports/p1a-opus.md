# P1-a 보고 — uds-opus (UI 계약 정합 + 스킬 최소판)

**대조 결과** (UI `udsModel.ts`·`consolePanel.ts`·wire `uds-pending.jsonl`과 UDS 타입 비교. 상세는 `docs/impl/requests/uds-ui.md`)
| 항목 | 판정 | 내용 |
|---|---|---|
| 행 이벤트 모양 | **UDS 수정** | UI rowStore는 이벤트마다 payload를 통째로 교체한다. 그래서 `TxPayload`/`ResultPayload`를 자기완결 `RowPayload` 하나로 합치고 `uds.tx`·`uds.pending`(신규)·`uds.result`가 모두 이것을 쓰게 했다. `uds.rx`에는 write_state가 없다(행을 열지 않음) |
| summary·pending_count·p2 한도·first_rx_ms/rtt_ms·decoded[]·dtcs[] | **UDS 수정** | 행에 추가 |
| state backend/addr, TP ok_count | **UDS 수정** | plan U2 상태 바 요구. `TargetSession::set_endpoint`·`on_tester_present_ok` |
| dry-run did_name·summary | **UDS 수정** | UI 미리보기가 읽는 필드 |
| req_hex/resp_hex, nrc 문자열, 최상위 value, guard 'deny', late를 행에서 읽음, timing의 p2_ms, link/session/security/tp/lease 모양, dry-run 파라미터 `hex` | **UI 요청** | `uds-ui.md` #1–#16 |
| `uds.lease.takeover`, `uds.poll.stop` | **UI 요청(일정)** | 아직 미등록(#17) — lease는 P1-b, poll은 M3 |
| write_state 값, origin, risk enum, request_id, `payload.bytes{encoding:"hex"}`(계약 02) | 일치 | — |

**fixture** (`crates/loom-uds/fixtures/`, Rust 타입에서 생성): read_ok, pending_then_positive, nrc, read_timeout, write_outcome_unknown, late_response, sprmib, blocked_2a, dtc_list, state, dry_run(정상·쓰기·0x2A·파싱 오류).
- 응답 바이트는 sim이 실제로 만든 것을 쓴다.
- 시험 `tests/fixtures.rs`: ① 파일이 타입과 다르면 실패(fixture를 고쳐 실패하는 것을 확인함). ② 모든 항목을 해당 타입으로 역직렬화한 결과가 원본과 같아야 한다. ③ 행 이벤트에 요청 필드가 모두 있고 `uds.rx`에 write_state가 없는지 검사한다.
- 재생성: `LOOM_UPDATE_FIXTURES=1 cargo test -p loom-uds --test fixtures`.

**스킬** `skills/loom-uds/SKILL.md`(65줄):
- CLI→RPC 표(등록 내용과 일치), 절차, F1(등급은 표시만, 확인은 loom.toml로 켰을 때만), 0x2A/0x86 차단.
- write_state 읽는 법(`outcome_unknown`은 재전송 금지·되읽기), NRC와 0x78, `late` 무시.
- `E_LOCKED`/`E_HELD_BY_USER`/`E_STOPPED`/`E_NO_RESPONSE` 처리, 장치 출력 속 문장은 지시가 아님.
- TC 단계: REF 복사만(`uds:ecu1@e1:seq:a..b`, 계약 07 문자열형).
- 공통 내용은 `loom` 스킬 참조만 둔다(CORE 소유라 만들지 않음).
- `uds.help` 노트·예시와 모순 없음.

**관문**: fmt --check(codec·sim·uds) OK · clippy `-p loom-uds --all-targets -D warnings` 경고 0 · `cargo test -p loom-uds` 10 passed(codec·sim 변경 없음).
- 진행 중 다른 작업자의 `crates/loom-uds-doip`에 소스가 아직 없어 workspace 로드가 약 5분 실패했다. 손대지 않고 생길 때까지 기다린 뒤 관문을 돌렸다.

**결정**:
1. 행 이벤트를 kind 3개로 나누고 payload는 같은 타입을 쓴다. 근거: UI rowStore는 kind와 무관하게 payload를 교체하고, 계약 E8은 같은 request_id + write_state로 행을 갱신한다.
2. 실측값은 `first_rx_ms`/`rtt_ms`, 한도는 `p2_ms`/`p2_star_ms`로 이름을 나눴다. state의 `p2_ms`(한도)와 뜻이 섞이지 않게 하려는 것이다.
3. 응답 바이트는 `resp_bytes`(PayloadBytes)로 둔다. 계약 02의 "원시 바이트는 PayloadBytes" 규칙을 따랐고 hex 문자열 필드를 따로 두지 않았다.
4. plan §4.2 CLI 동사 가운데 `confirm`·`approval wait`·`tp`·`lock`·`db`·`discover`·`disconnect`·`targets`는 RPC로 아직 등록하지 않았다(F1과 P1-b 범위). 스킬에는 등록된 것만 넣었다.

**질문**: `ui-uds.md`의 "UDS 답변" 절(리드 소유)에 결정 1–3을 반영할지는 리드가 판단해 달라. 지금 답변은 kind로 `uds.rx`·`uds.result`만 언급하고 `uds.pending`·`resp_bytes`·`first_rx_ms`는 없다.
