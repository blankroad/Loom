# CAN → UI 답변: CAN 이벤트·RPC 스키마 v1 (M1) — `ui-can.md` 1–6 (can-lead, 2026-11-11)

docs/plan/02-core-principle.md 준수. 기준: 계약 v1(`docs/contract/02-events.md` E1/E8), CORE fixture 게이트웨이(`crates/loom-gateway/src/replay/{conn,scenario}.rs`의 `can.send → can.tx` 행), CAN `plan.md` v2.0 §5. 바뀌면 이 파일을 갱신하고 `CHANGELOG` 줄을 남긴다.

## 1. 송신 한 번 = Trace 한 줄 (request_id)
- `can.send` 요청마다 **쓰기 상태 행 `can.tx`**(top `request_id`, `payload.write_state`)가 `requested → accepted → sent → confirmed`(또는 `failed`/`outcome_unknown`)로 발행된다(CORE fixture와 같은 모양).
- 버스 에코 `can.frame`(`dir:"tx"`)에도 **같은 top-level `request_id`**가 붙는다(Loom이 보낸 프레임만). 외부·장치 프레임에는 없다.
- UI 규칙: `request_id`가 같은 `can.tx`·`can.frame` 행은 **한 줄**로 합치고 그 줄의 상태 칸만 갱신. 프레임 시각·바이트는 에코 `can.frame` 값을 쓴다(없으면 `can.tx`의 `payload.frame` 요약).
- 한 요청이 여러 프레임(`count > 1`)이면 에코 프레임마다 `payload.frame_index`(0부터)가 붙는다 — 줄은 프레임 수만큼, 상태는 요청 단위.

## 2. `can.frame` payload (kind별 구조, 계약 E1)
| 필드 | 형 | 뜻 |
|---|---|---|
| `id` | JSON 숫자 (u32, ≤ 0x1FFFFFFF) | CAN id. u64가 아니라 숫자로 둔다 |
| `ext` | bool | 29비트 여부 |
| `fd` | bool | CAN FD 여부 |
| `brs`, `esi` | bool | FD 플래그(Classic이면 false) |
| `rtr` | bool | Classic 원격 프레임 |
| `dlc` | 0..15 | **DLC 코드**(바이트 수 아님. FD 12바이트 = 9) |
| `len` | 0..64 | 바이트 수 |
| `bytes` | `{encoding:"hex", data}` | 데이터. 원격 프레임은 `""` |
| `device_ts` | `{value_ns: "u64 문자열", source: "hardware"\|"kernel_sw"\|"host", clock_id}` 또는 없음 | 드라이버 시각(sim은 없음). 화면 기준 시각은 계약대로 top `ts.mono_ns` |
| `isotp` | `{endpoint, request_id?}` 또는 없음 | ISO-TP가 보낸/받은 FF/CF/FC 주석(R-TP-07, M2) |
| `frame_index` | 숫자 | 다중 프레임 송신의 순번(§1) |
- top-level: `origin`(4종), `external_actor`(`canoe`/`host`/`unknown`, EXT 배지), `env`, `generation`(채널 재open), `request_id`(§1).
- `dlc` 뜻은 CORE에도 명시 요청한다(fixture `dlc:8`은 코드·길이가 같아 모호).

## 3. 송신 RPC = `can.send`
CORE echo 이름 그대로 `can.send`(plan의 `can.tx.send`는 쓰지 않음).
```json
{"channel":"ch0",
 "frame":"300#01",                      // cansend 문법(loom-can-frame::parse_frame) — 또는 아래 구조
 "id":768, "ext":false, "fd":false, "brs":true, "esi":false, "data":"01",
 "count":1, "gap_ms":0,                 // 유한 연속(≤ 10이 R2, 넘으면 R3 — 표시용)
 "expect":{"id":769, "within_ms":100},  // 선택: 응답 확인 → effective_verified
 "dry_run":false}
```
- `frame`과 구조 필드 중 하나만. `brs`는 FD일 때만 의미(기본 true), Classic에 주면 `E_USAGE`.
- 결과 `data`: `{request_id, write_state, source_seq, risk, reason}` (+ `expect` 결과·증거 ref).

## 4. `can.trace.query`
```json
{"channel":"ch0", "since":"@last|seq:N|mark:X|now|-5s|<ISO8601>", "until":"…", "ids":["0x300","0x301"],
 "origins":["agent","device"], "dir":"tx|rx", "max":100, "peek":false}
```
- `max` 기본 100, 상한 1,000 / 64 KiB(D5). 결과 = D5 봉투: `data.rows[]`(발행 레코드), `meta{truncated, next_cursor, range{seq_a, seq_b}, display_gap}`, `others_since_last`.
- **agent role 질의는 `core.activity{client_id, action:"read", domain:"can", channel, range{seq_a, seq_b}}`를 남긴다**(UI의 "에이전트가 읽은 범위" 막대 근거). viewer 질의는 남기지 않는다. `expect`·`sig wait-for` 대기 중에는 대기 술어를 넣은 `core.activity{action:"wait", predicate}`.

## 5. 위험 등급 배지
- 마스터 **F1: 기본 정책 = 모두 자동**, 등급은 표시만. 미리보기는 `can.send{..., "dry_run":true}` → `{risk, reason, bytes, policy:"auto"}`(R0 질의, 부작용 없음). 입력이 바뀔 때 디바운스해서 부르면 된다.
- M1(sim 채널)은 항상 `R1 reason:"sim channel"`. 실HW·연속 송신·예약 ID 등 규칙은 M2(CAN 정책 매핑 K8).

## 6. 출처 색
마스터 E8 통일안을 따른다: **AGT 청록 · USR 노랑 · DEV 회색 · SCR 보라 · EXT 보라 점선**. `_xteam/can-ui.md` §5.1의 옛 색(주황/파랑)은 이 값으로 바꾼다(CAN이 갱신).

## 7. (추가, M2a 앞당김) 신호·DBC — 2026-11-11
- 이벤트 `can.db_changed{channel, db_sha256, previous_sha256, path}` — 채널 DB가 바뀌면(첫 로드 포함) 발행. Graphics·Data·IG 패널은 이 뒤 신호 목록을 다시 읽는다(`can.db.info`).
- `can.trace.query`·`can.last`에 `decode: true`면 **응답 행에만**(저장 이벤트는 그대로) `decoded{message, signals:[{name, value, unit, raw, label?}]}`. 짧은 프레임 신호 `{unavailable:"short_frame"}`, 모르는 id는 `decoded` 없음, 모호·차단은 `{error}`. 결과에 `db_sha256`. DB가 없는데 `decode:true`면 `E_DB`(exit 2).
- `can.sig.get{channel?, signal, window_ms?}` → `{value, unit, raw, label, age_ms, signal:"Msg.Sig", message, id, seq, db_sha256, evidence, refs}` — Data 패널 초기값·에이전트 "읽음" 표시에 쓸 수 있다.
- `can.sig.wait_for` 대기 중 `core.activity{action:"wait", waiting:{predicate:"can.signal_range", args}}` — Graphics의 "agent waiting > X" 임계선 근거.

## 8. (추가, M2a 앞당김) ISO-TP — 2026-11-11
- 모든 FF/CF/FC는 일반 `can.frame`으로 나오고 `payload.isotp{endpoint, request_id?}` 주석이 붙는다(요청/응답 한 번의 프레임이 client 엔드포인트·요청 id로 묶임). Trace의 ISO-TP 묶음 행은 이 주석으로 접는다.
- 이벤트(priority): `can.isotp_state{endpoint, state, reason?}`, `can.isotp_rx_start{endpoint, expected_length, single_frame?, first_seq, ts_first_frame}`(FC 프레임보다 먼저), `can.isotp_rx_complete{endpoint, pdu{encoding:"hex",data}, len, frames, first_seq, last_seq}`, `can.isotp_rx_error{endpoint, code}`, `can.isotp_tx_done{endpoint, transfer, tx_state:"driver_accepted"|"bus_confirmed", frames}`, `can.isotp_tx_error{endpoint, code, residual_tx_possible}`. `first_seq/last_seq`는 producer `source_seq`.
- UDS 요청의 `write_state`는 UDS 행이 소유한다(이 이벤트들은 전송 계층 세부). UDS 패널의 주석 열은 `request_id`로 UDS 트랜잭션과 연결.

## 9. (추가, M2a 앞당김) Graphics·Data·IG 질의 — 2026-11-11 (전부 R0, 버스에 쓰지 않음)
- **`can.db.catalog{channel?, cursor?, max?≤1000, filter?}`** — IG 메시지 목록·신호 위젯·자동완성. 메시지 `{index, name, id, id_raw, ext, size, fd, cycle_ms, transmitter, multiplexed, encode_blocked, decode_blocked, e2e_suspect}`, 신호 `{name, start_bit, size, byte_order, kind, factor, offset, min, max, range, unit, values:[[raw,label]]≤256, initial, multiplexor, mux, receivers, comment}`. 64 KiB 페이지, `next_cursor`.
- **`can.encode.preview{channel?, msg, signals:{Sig: 값 | "라벨" | {"raw":n}}, base?: "initial"|"last"|hex, force_range?}`** — IG 입력마다(목표 ≤ 20 ms, `elapsed_us`). `{ok, bytes, frame, sources[], mux[], range_errors[] (전부 한꺼번에), sendable, base_source, risk, reason}`. 구조 오류는 RPC 오류가 아니라 `ok:false, error:{kind, message}`(입력 옆 표시).
- **`can.signal.query{channel?, signals:[1..16], t0, t1?, t0_ns?, t1_ns?, px≤4000}`** — Graphics. 모든 신호 공통 `grid{start_ns, width_ns}`, 신호별 열 `columns:{i, first, last, min, max, n, brk}`(빈 버킷 생략, `t = start_ns + i×width_ns`), **ns 값은 문자열**, gap 뒤 버킷 `brk:true`(선 끊기·보간 금지) + `gaps[]`, `decimated{raw, points}`, 64 KiB 넘으면 `truncated + resume_t0_ns`. 바이너리 열 블록(U-CAN-2) 확정 시 같은 열 이름으로 옮김.
- **`can.signal.at{channel?, signal, t, from?}`** — 커서 값: `before`/`after` 원본 표본(`value, unit, raw, label, t_ns, dt_ns, seq, ref`) + `gap_between`.
