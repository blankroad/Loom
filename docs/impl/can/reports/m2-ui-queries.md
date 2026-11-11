# M2a 보고 — UI 데이터 질의 (Graphics·Data·IG, G2)

작성: can-opus, 2026-11-11. 과제 `docs/impl/can/tasks/m2-ui-queries.md`. 커밋 안 함.
범위: `src/signals.rs`(질의 4개), `src/gateway.rs`(`classify_send`를 `pub(crate)`로 — 한 줄), `tests/signals.rs`, `tests/gateway.rs`, `tests/common/mod.rs`(`Journal::push_at` 추가), `skills/loom-can/SKILL.md`(Graphics 링크 한 줄). `worker.rs`·`periodic.rs`·`isotp.rs`는 손대지 않았다.

## 결과
- can-sol이 지금 `isotp.rs`를 고치는 중이라(컴파일 안 되는 중간 상태) worktree 그대로는 빌드가 안 된다. **스크래치 복사본**(내 파일 + 커밋된 `isotp.rs`)에서 검사: `cargo fmt --all --check` ✓, `cargo clippy -p loom-can --all-targets -- -D warnings` ✓, `cargo test -p loom-can` ✓ **102개**(lib 7, cli 13, gateway 18(+1), isotp 8, periodic 16, signals 23(+7), worker 17).
- 포맷은 내 파일만 `rustfmt`로.

## RPC (모두 R0 — DBC와 저널만 읽고 버스에 쓰지 않음, 시험으로 확인)
### `can.db.catalog{channel?, cursor?, max? (기본 200, ≤ 1000), filter?}`
- 메시지: `index, name, id("0x100"), id_raw, ext, size, fd, cycle_ms, transmitter, comment, multiplexed, ambiguous_id, encode_blocked, decode_blocked, e2e_suspect([이유] | null)`.
- 신호: `name, start_bit, size, byte_order(intel|motorola), kind(unsigned|signed|float32|float64), factor, offset, min, max, range([lo,hi]|null), unit, values([[raw, label]] ≤ 256) + values_total, initial(물리값|null), multiplexor, mux({multiplexor, ranges}|null), receivers, comment`(주석 ≤ 512 B).
- 페이지: 메시지 index 기준 `next_cursor`. 메시지 수 `max` **또는 64 KiB**에서 끊는다(한 메시지가 혼자 64 KiB를 넘어도 한 페이지에 하나는 넣고 `oversize_message:true`). `filter`는 메시지·신호 이름 부분 일치(대소문자 무시, 자동완성용).
### `can.encode.preview{channel?, msg, signals:{Sig: 물리값 | "라벨" | {"raw": n}}, base?: "initial"|"last"|hex, force_range?}`
- 결과 `{ok, bytes, frame, padded, sources:[{signal, source: given|initial|base|auto_multiplexor, decoded:{value,label}}], mux:[{multiplexor, value}], range_errors:[{signal, value, min, max}], sendable, base_source, base_job, base_note, risk, reason, risk_periodic, elapsed_us, db_sha256}`.
- **범위 오류는 전부 한꺼번에**(라이브러리 encode는 첫 오류에서 멈추므로 미리 검사, raw 입력도 물리값으로 환산해 검사). 범위 오류가 있으면 바이트는 보여 주되 `sendable:false`. 구조 오류(모르는 신호·라벨, 시작값 없는 신호, mux 충돌, raw 비트 초과 …)는 RPC 오류가 아니라 **`ok:false, error:{kind, message}`**로 — IG가 입력 옆에 바로 표시하도록. 메시지 이름이 없거나 base hex가 잘못되면 `E_USAGE`.
- `base:"last"`: 그 메시지의 실행 중 잡 바이트(`can.periodic.list`의 `data`/`frame`, 패치 대기 중이면 건너뜀) → 버스의 마지막 프레임(최근 250 ms부터 넓혀 최대 60 s) → 없으면 DBC 시작값 + `base_note`. `elapsed_us`로 20 ms 목표를 볼 수 있다(가짜 저널 시험에서는 수백 µs).
### `can.signal.query{channel?, signals:[1..=16], t0: cursor, t1?: cursor, t0_ns?, t1_ns?, px: 1..=4000, mode?: "minmax"}`
- **격자는 모든 신호 공통**. `t0_ns/t1_ns`(UI 시간축 = 이벤트 `ts.mono_ns`)를 주면 고정 격자(폭 = 범위/px, 범위 밖 표본 제외), 안 주면 **적응 격자**: 첫 프레임에서 1 µs 폭으로 시작해, 표본이 끝을 넘으면 폭을 두 배로 하고 이웃 버킷을 합친다(first/last/min/max/n 합치기는 정확 — 극값 보존). 한 번 스캔, 메모리는 신호당 O(px).
- 출력은 신호별 **열 블록** `columns:{i, first, last, min, max, n, brk}`(빈 버킷 생략). `t = grid.start_ns + i × grid.width_ns`. **ns 값은 모두 문자열**(mono ns ≈ 10^18은 JS 수의 정밀도 2^53을 넘는다).
- gap: 범위 안의 `source.gap`/`record.gap` 이벤트와 저널 스캔 gap이 오면 그 뒤 첫 버킷에 `brk:true`(선 끊기, 보간 금지), `gaps:[{t_ns, kind, cause, seq}]`(≤ 256, `gaps_total`), 저널 gap이면 `complete:false`.
- `decimated:{raw, points}` (points = Σ min(n, 4) ≤ 4×px ≤ 16,000 < 20,000).
- 상한: 응답 64 KiB를 넘으면 **같은 시각에서 모든 신호를 함께 자르고** `truncated:true, resume_t0_ns`(같은 cursor, `t0_ns`만 바꿔 다시 부르면 된다). 원본 스캔 **2,000,000 프레임** 상한 → `scan_limited:true, complete:false, resume`(이어갈 cursor).
- 비 viewer가 부르면 `core.activity{action:"read"}` 발행(다른 읽기와 같음).
### `can.signal.at{channel?, signal, t (mono ns, 수 또는 문자열), from?: cursor (기본 -60s)}`
- `from`부터 앞으로 읽어 `t` 이하의 마지막 표본(`before`)과 `t` 초과 첫 표본(`after`)을 찾으면 멈춘다. 각각 `{value, unit, raw, label, t_ns, dt_ns, seq, origin, ref("can:ch0@e1:seq:N#/signals/Sig")}` + `evidence[]`(`can.signal`). 두 표본 사이에 gap이 있으면 `gap_between:true`. 스캔 상한 2M 프레임(넘으면 `complete:false`).

## 시험 (가짜 저널 + fixture DBC)
catalog 페이지·메타(EngRPM factor/min/max/unit/시작값, 바이트 순서, 값 테이블, 29bit id, filter, mux 조건, 64 KiB 페이지 — 메시지 300개 DBC를 끝까지 넘겨 전부 받음) · preview(손계산 바이트, sources, 범위 오류 2개 동시 + sendable false, 구조 오류 4종 kind, raw 입력, mux 자동 multiplexor·충돌, base last = 없음/버스/잡, hex base, worker 쓰기 0) · query(손계산 버킷: first/last/min/max/n, gap 뒤 brk, 다른 id 무시, 두 신호, 적응 격자에서 극값·개수·처음·끝 보존, 입력 오류 8종, 4000 px × 2신호 64 KiB 자르기 + 같은 끝 + resume_t0_ns) · at(앞뒤 표본·dt·ref·seq, 정확히 t인 표본은 before, 앞/뒤 없음, gap_between, 문자열 t, 기본 from).

## 결정·요청
1. **시간 지정**: CallCtx에 시계가 없고(CORE 요청 9 대기) 저널 cursor에 mono ns 형이 없어, 스캔 범위는 cursor(`seq:`, `mark:`, `-30s`, `HH:MM:SS` …), 화면 시간축은 이벤트 `ts.mono_ns`로 나눴다. **CORE 요청: mono ns cursor(`mono:<ns>`) 또는 시각→seq 찾기** — 생기면 `signal.at`의 `from` 기본값과 query의 `t0`를 그것으로 바꾼다.
2. **대용량 `raw_ref`**(HTTP GET, A5)는 CORE 경로가 없어 아직 없다. 지금은 64 KiB 시간 자르기 + `resume_t0_ns`로 이어 받는다.
3. **바이너리 열 블록**(U-CAN-2, `loom.columns.v1`)이 확정되면 query의 `columns`를 그 형식으로 옮긴다. 지금 JSON 열 배열은 같은 모양(열 이름·순서)이다.
4. `can.signal_sample`(라이브 구독)은 이번 범위가 아니다(reducer/허브 쪽 일, M2 남은 일).
5. CLI는 만들지 않았다(선택 항목). 스킬은 "추이는 Graphics 링크를 주고 숫자를 늘어놓지 말 것" 한 줄.
6. `docs/impl/requests/can-ui.md`(리드/UI 소유)에 위 네 RPC의 결과 모양을 옮겨 적어야 UI가 쓴다 — 특히 `columns.i` + `grid`, ns 문자열, `ok:false` 오류 kind 목록.
