# 02. 이벤트 레코드

라이브 스트림 = 기록. 같은 스키마(D3). 타입 정본: `loom_proto::event::{SourceRecord, PublishedRecord}`.

## 2.1 두 층 순번 (D3, E1)
- **원본**: worker가 `(producer_id, producer_epoch, source_seq)`를 붙인다. `producer_epoch`는 worker 기동마다 증가, `source_seq`는 producer 안 단조 증가. 저널 원본(`SourceRecord`)에는 이것만 있다.
- **스트림**: 허브가 `(epoch, seq)`를 붙인다. `epoch` = **stream_epoch**(gateway 기동마다 증가), `seq` = stream 안 단조 증가. 구독·커서·snapshot은 이 값을 쓴다(`PublishedRecord`).
- 허브는 배치마다 stream 범위 ↔ producer 범위 대응을 **stream mapping 로그**(정본, `06-journal.md`)에 남긴다.
- 장치·링크 세대는 최상위 `epoch`가 아니라 **`generation`**(UART 포트 재open, UDS 링크 재연결, CAN 채널 재open). 도메인 프로토콜 고유 번호는 `payload` 안에 도메인 이름으로(예: `payload.isotp_epoch`).

## 2.2 필드 (D3, E1, E8)
| 필드 | 필수 | 의미 |
|---|---|---|
| `schema_version` | ✔ | 1 |
| `session_id` | ✔ | 기록 세션(데몬 기동 단위) |
| `domain`, `kind` | ✔ | `kind` = `<domain>.<event>` |
| `channel` | | 도메인 채널 별칭(`dut`, `ch0`, `ecu1`). 코어 이벤트는 없음 |
| `producer_id`, `producer_epoch`, `source_seq` | ✔ | 원본 ID |
| `epoch`, `seq` | 발행 시 ✔ | stream_epoch, stream_seq |
| `ts` | ✔ | `{mono_ns, wall_ns, clock_id, source, quality?}` |
| `dir` | | `rx` / `tx` / 없음(상태·제어) |
| `origin` | ✔ | **`agent` / `user` / `device` / `script`** 고정 4종 |
| `client_id`, `actor_id` | | 요청자. `actor_id` 예: `run:r-12`, `test:t-4` |
| `external_actor` | | origin=device이고 Loom 밖 주체가 보낸 것으로 관측된 경우(`canoe`, `host`, `unknown`) |
| `original_origin` | | 재생 이벤트만 |
| `request_id`, `job_id` | | 상관관계. 쓰기 행은 `request_id` 필수 |
| `generation` | | 장치·링크 세대 |
| `purpose` | | `agent_send`, `keepalive`, `run`, `replay`, `sim`, `test_step` … |
| `risk` | | R0–R4(쓰기·쓰기 상태 이벤트) |
| `env` | 쓰기 시 ✔ | `sim` / `bench` / `vehicle` |
| `test_session`, `test_step` | | 세션에 묶인 client의 이벤트에 허브가 자동 부여 |
| `payload` | ✔ | **kind별 스키마를 가진 구조 객체**(없으면 `{}`) |
| `metrics` | | `[{name, value, unit?}]` — viz가 도메인 무관하게 자동 플롯 |
| `quality` | | `stale`, `decimated:N`, `simulated` … |

- uint64(`mono_ns`, `seq`, `source_seq` 등)는 **JSON에서 10진 문자열**(정규형: 선행 0 금지, `"0"`만 예외 — 같은 수는 같은 문자열이어야 digest가 안정적), MessagePack에서 정수. JSON 숫자로 주면 거절한다. 바이트는 JSON hex(RPC 인자)/base64(이벤트 대용량), MessagePack bin. 디코더는 실패를 오류로 돌려준다(panic 금지).
- **payload 규칙**: 원시 바이트는 `payload.bytes{encoding: "hex"|"base64"|"raw_ref", data | ref}`, 쓰기 상태는 `payload.write_state`. 도메인 필드는 `payload.<field>`. 최상위 `data`·`write_state` 필드는 없다.
- 시간: `ts.mono_ns`는 같은 호스트 monotonic(QPC / CLOCK_MONOTONIC / mach_absolute_time), `wall_ns`는 UTC. `ts.source` = `host_read_complete | driver | hardware | device | gateway | synthetic | pcap_host | pcap_adapter | replay | sim`(마지막 4개는 eth). 청크마다 wall을 기록한다(sleep 대비).
- 증거 JSON Pointer의 기준은 **발행 레코드의 JSON 표현**(예: `/payload/value`, `/payload/bytes/data`, `/metrics/0/value`).

## 2.3 쓰기 상태 (D4, A14, E8)
`requested → accepted → sent → confirmed → effective_verified | effect_unverified`, 어느 단계에서든 `failed` / `outcome_unknown`.
- `sent` = 드라이버·소켓에 넘김(wire 완료 아님). UART 성공 종단은 `sent`.
- 같은 `request_id` 행을 순서대로 갱신한다(`payload.write_state`). 화면과 CLI 결과는 이 단계를 그대로 보여 준다 — 거짓 성공 금지.
- 거절된 쓰기도 `requested → failed` 행으로 사용자 화면에 보인다(STOP 중 에이전트 시도 포함).
- 대화형 입력 이벤트 `<domain>.input`의 기본 소유자는 코어다. `Registry::input_events(Owner::Domain)`을 선언한 도메인은 실제 TX·입력 처리 결과를 직접 발행하며, 코어는 중복 `sent` 행을 만들지 않는다. `<domain>.input` RPC와 입력 감사는 계속 코어가 제공한다.
- **confirmed 의미**: backend가 정의한 확인이다(CAN = 버스 송신 확인, UDS = ECU 긍정 응답). 장비 효과 검증은 `effective_verified`로 별도 표시한다.
- **종단 표시**(UI Q11): 도메인은 `Registry::success_terminal(method, WriteState)`로 선언한다(선언 없으면 `confirmed`). 그 요청의 마지막 쓰기 상태 행에는 `payload.final: true`를 코어가 붙인다. 도메인마다 성공 종단이 다르므로(UART = `sent`, CAN·UDS = `confirmed` 이상) 도메인이 `CommandSpec`에 성공 종단을 선언하고, `failed`·`outcome_unknown`·`effective_verified`·`effect_unverified`는 항상 종단이다.

## 2.4 손실 3층 (D3, A13)
| kind | 의미 | 예 |
|---|---|---|
| `display.gap` | 화면 전달만 버림(기록은 있음) | 느린 구독자, IPC 크레딧 부족, 허브 과부하 |
| `record.gap` | Loom 기록 손실 | 디스크 가득, 저널 큐 초과, 크래시 tail, 손상 |
| `source.gap` | Loom이 받기 전 손실 | 드라이버 오버런, DLT MCNT 불연속, gateway 재시작 공백 |
- 공통 payload `{seq_range?, producer_range?, count: "<n>"|"unknown", cause, bytes?, detail}`. 이벤트의 `domain`·`channel`은 **영향받은 스트림**의 것(예: `display.gap`의 domain = `can`, channel = `ch0`)이고, kind 접두어(`display.`·`record.`·`source.`)가 손실 층을 나타낸다(UI Q13).
- 도메인 cause 예: eth `kernel_drop`, `if_drop`, `capture_restart`, `stats_unavailable`. worker에서 드롭돼 허브에 들어오지 않은 이벤트는 stream seq가 없으므로 **`producer_range`로** 보고한다(stream seq를 지어내지 않는다).
- `record.health` = `ok | degraded | failed | disabled`.

## 2.5 코어 이벤트 kind
`core.latch`(payload `{on, by, origin, jobs_stopped[], approvals_cancelled[], driver_uncancellable, outputs_held[], missing[], persisted}`), `core.approval_requested`, `core.approval_resolved`, `core.confirm_issued`, `core.policy_denied`, `core.policy_changed`, `core.worker_state`, `core.job_state`, `core.job_progress`, `core.activity`(`range{seq_a, seq_b}`·대기 술어 포함), `core.agent_cursor{client_id, from_seq, to_seq, others_count}`, `core.mark`, `core.epoch_changed`, `core.write_recovered`, `core.session_closed`, `core.env`, `display.gap`, `record.gap`, `record.health`, `source.gap`, `ui.focus_request`, `run.*`, `test.*`.
- 기록하지 않는 것: `core.heartbeat`(1 s, `{stream_seq_hw, daemon_mono_ns, daemon_wall_ns, record_health, latch, sub_stats}`), hello 응답.
- 출처 색(UI, E8): agent 청록, user 노랑, device 회색, script 보라, external 보라 점선.
