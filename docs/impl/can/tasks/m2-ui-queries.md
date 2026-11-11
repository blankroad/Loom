# can-opus M2a — UI 데이터 질의 (Graphics·Data·IG 패널, G2)

공통 규칙 `docs/impl/can/tasks/p0-common.md`(질문 창 금지, 검사 worktree 루트, 경로 `/Users/ctmctm/workspace/Loom/...`, 커밋 안 함). **범위:** `src/gateway.rs`, `src/signals.rs`, `src/cli.rs`(필요하면), 시험 `tests/signals.rs`·`tests/gateway.rs`, 보고서 `docs/impl/can/reports/m2-ui-queries.md`. `worker.rs`·`periodic.rs`는 can-sol 영역이 아니지만 이번엔 아무도 안 고친다 — 너도 건드리지 마라. 리드가 준비: `lib.rs` 상수 `RPC_SIGNAL_QUERY`, `RPC_SIGNAL_AT`, `RPC_DB_CATALOG`, `RPC_ENCODE_PREVIEW`.

정본: `docs/plan/_xteam/can-ui.md` §5.4(Graphics)·§5.5(IG)·§5.6(Data)·§5.9(데이터 계약), `docs/impl/requests/can-ui.md`.

1. **`can.db.catalog{channel?}`**(R0): 메시지(id·ext·이름·길이·FD·주기·송신 노드)와 신호(이름·단위·min/max·factor/offset·값 테이블·초기값·mux 조건·E2E 추정 표시) — 상한(메시지·신호 수, 64 KiB 넘으면 페이지 `cursor`).
2. **`can.encode.preview{channel?, msg, signals:{…}, base?: "initial"|"last"|hex}`**(R0, 목표 ≤ 20 ms): `{bytes, range_errors[], mux, sources, risk, frame}` — 송신하지 않는다. `base:"last"`는 그 메시지의 마지막 관측 payload(창 스캔) 또는 실행 중 잡 payload(`can.periodic.list`).
3. **`can.signal.query{channel?, signals:[≤16], t0, t1 (cursor/시각), px (1..=4000), mode:"minmax"}`**(R0): 신호별 픽셀 버킷 `{t, first, last, min, max, n}` 열 배열. 서버측 min/max(극값 보존), series당 ≤ min(4×px, 20000)점, gap(`record.gap`·`source.gap` 구간)에서 끊김 표시, `decimated: N→M`. 64 KiB 넘으면 `truncated` + 이어갈 범위(대용량 HTTP GET `raw_ref`는 CORE 경로가 생기면). 원본 스캔 상한(예: 2M 프레임) 명시.
4. **`can.signal.at{channel?, signal, t}`**(R0): 시각 t 직전·직후 원본 표본(커서 값) + 증거 ref.
5. CLI는 선택(`loom can sig series` 정도), 스킬엔 "추이는 UI Graphics 링크(`loom ui open can/graphics?sig=…`)"만.
6. 시험: fixture DBC + 가짜 journal로 catalog 페이지·preview(범위 오류·mux·base last)·query 버킷 정확도(극값 보존, 손계산)·gap 끊김·상한·at 앞뒤 표본.
끝나면 보고서를 쓰고 멈춘다.
