docs/plan/02-core-principle.md 준수.

# P0 중간 공지 — 임시 타입을 CORE 계약 v1에 맞춤 (ui-lead, 2026-11-11 02:05)

CORE 계약 v1(`.worktrees/core/docs/contract/`, 읽기 전용)과 `loom-proto`가 나와서 `ui/src/proto/provisional.ts`·`ui/src/net/api.ts`를 고쳤다. 자기 코드를 새 이름에 맞춰 주세요(보고서에 맞춘 곳 요약).

| 바뀐 것 | 이전 | 지금(계약 v1) |
|---|---|---|
| 연결 첫 메시지 | `hello{proto:number, want_role, capabilities}` | **`core.hello`**`{proto:"loom/1.0", token?, client_id, want_role, client{kind,version,tty}}` |
| hello 결과 | `rights: Right[]`, `session_id`, `daemon_version` | `role, rights?: 'watch'|'operate'|'approve'`(**단일 등급**, `RIGHT_RANK`), `conn_id, stream_epoch, server{version,build,schema_hash?}, env, latch, viewers{subscribed,visible}, capabilities[]` |
| 이벤트 순번 | `stream_epoch`, `seq` | **`epoch`, `seq`** + `producer_id, producer_epoch, source_seq`. 선택 필드는 **null이 아니라 생략**(`channel?`, `request_id?`, `dir?`, `env?` …). `quality: string[]` |
| 원시 바이트 | 도메인마다 | `payload.bytes{encoding:'hex'|'base64'|'raw_ref', data?|ref?}` |
| gap payload | `{first_seq, last_seq, count:number|'unknown', cause}` | **`GapData{seq_range?{from,to}, producer_range?, count: "<10진 문자열>"|"unknown", cause, bytes?, detail?}`**. producer_range만 있는 손실은 stream seq를 지어내지 말 것 |
| 배치 알림 `core.events` | `EventBatch{sub_id, first_seq, last_seq, hub_publish_mono_ns, record_health…}` | **`EventsBatch{sub, first, last, events, display_gap?: GapSummary, lag_ms}`** |
| snapshot | `{sub_id, snapshot_id, part, count, state, events}` / end `{h}` | `core.snapshot_part{sub, n, N, state?, events?}` / **`core.snapshot_end{sub, stream_seq}`** ([contract text] — loom-proto 타입 아직 없음) |
| heartbeat | `{stream_seq_hw, mono_ns, wall_ns, …}` | `core.heartbeat{stream_seq_hw, daemon_mono_ns, daemon_wall_ns, record_health, latch, sub_stats?}` |
| 시계 | `core.time → {d1_mono_ns, d2_mono_ns}` | **`{recv_mono_ns, send_mono_ns, wall_ns, clock_id}`** |
| 구독 인자 | `after_cursor`, `stream_epoch`, `encoding:'columns'`, `control` | **`after_seq`**(같은 epoch만), `backlog?`, `encoding:'json'|'binary'`, `control` 필드 없음 |
| 제어 구독 | 클라이언트가 `control:true`로 구독 | **연결마다 서버가 자동으로 1개**. 와이어 표기 미정 → 임시로 `core.events`의 `sub === CONTROL_SUB('control')` |
| 연결 끊기 예고 | — | `core.closed{reason}`(예: `slow_consumer` = 우선 큐 초과) → 재연결 + snapshot |
| 가리키기 이벤트 | `core.ui_focus` | **`ui.focus_request`** |
| latch payload | `{on, by, origin, acked, jobs_stopped, approvals_cancelled, uncancellable, outputs_held}` | `{on, by, origin, jobs_stopped[], approvals_cancelled[], driver_uncancellable, outputs_held[], missing[], persisted}` |

계약 상한(구현에 반영): **viewer 연결당 구독 ≤ 8**(전체 32) — 패널마다 구독을 따로 열지 말고 같은 domain·channel은 합쳐서 공유할 것. 서버 쪽 구독 data 큐 넘침 = 그 구독만 `display.gap{cause:"slow_client"}`, lag > 2 s면 `display.gap{cause:"jump_to_live"}`.

rights 주의: 계약 v1은 STOP을 operate 권한으로 두었다. UI 계획(안전 방향은 watch도 허용)과 다르므로 CORE에 질의 중 — **UI는 일단 계약대로 STOP 버튼을 operate 이상에서만 활성**하고, watch에서는 비활성 + "STOP은 operate 권한 필요(질의 중)" 툴팁.
