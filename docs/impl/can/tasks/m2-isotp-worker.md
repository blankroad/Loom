# can-opus M2a(앞당김) — ISO-TP를 CAN worker에 올리기 (K10 worker 쪽, `can.isotp.*`)

먼저 `docs/impl/can/tasks/p0-common.md`(공통 규칙, **대화형 질문 창 금지**). 검사는 worktree 루트에서 `cargo fmt --all --check`, `cargo clippy -p loom-can --all-targets -- -D warnings`, `cargo test -p loom-can`. 경로는 `/Users/ctmctm/workspace/Loom/...`만. 커밋하지 않는다. 보고서: `docs/impl/can/reports/m2-isotp-worker.md`.

**범위:** 새 파일 `crates/loom-can/src/isotp.rs`(+ `tests/isotp.rs`)를 채우고, `worker.rs`(이제 네 소유 — 하위 에이전트가 만든 파일)와 `gateway.rs`·`cli.rs`에 연결. `lib.rs`·`schema.rs`·`Cargo.toml`은 리드 소유이며 **이미 준비됨**: `loom-can-isotp` 의존성, `pub mod isotp;`, 상수 `RPC_ISOTP_{OPEN,SEND,ABORT,CLOSE,LIST}`, `KIND_ISOTP_{TX_DONE,TX_ERROR,RX_START,RX_COMPLETE,RX_ERROR,STATE}`. 더 필요하면 보고서에 요청.

## 정본
- 계약 `docs/plan/_xteam/can-uds-isotp.md` — **맨 아래 "v1.1 이름·상태 정본" 절 + uds 확인 + 메인 확정**이 우선(이름, `tx_state: driver_accepted|bus_confirmed`는 전송 계층 세부, 요청 `write_state`는 UDS 소유, STOP은 agent·script 전송만 abort, 예약 tx_id 위반 exit 4 `E_DENIED{reason:"tx_reserved"}`, 같은 주소 중복 open exit 6 `E_LOCKED`). 위쪽 v0.2/v0.3 절의 필드(epoch·req_id·deadline·rx_start/rx_complete·ts_source·소유권 키·drain barrier·max_pdu 4095)는 의미 그대로.
- 엔진: `crates/loom-can-isotp`(README의 구동 절차 1–7 그대로: pop_frame/pop_event 모두 비우기, `not_before_ns`, `on_frame_submitted_at`, `poll_at`/`on_timer`, 확인을 못 주는 sim 백엔드는 `DriverAccepted` 뒤 `release_transfer`).
- worker: `crates/loom-can/src/worker.rs`, sim 버스 `crates/loom-can-sim`.

## 할 일 (M2a 최소판 = UDS M1b 전제)
1. **엔드포인트**(채널당 ≤ 32, 전체 상한 명시): `can.isotp.open{channel, tx_id, rx_id, ext?, fd?, tx_dl?, padding?, block_size?, st_min?, n_bs_ms?, n_cr_ms?, role:"client"|"server", owner?}` → `{endpoint, epoch}`; `close`, `abort{endpoint}`(→ `residual_tx_possible`), `list`. 같은 (채널, tx/rx 주소) 중복 → `E_LOCKED`. normal 11/29bit, CC/FD.
2. **송신**: `can.isotp.send{endpoint, pdu: hex, request_id}`(worker write) → `{accepted:true, transfer}`. 엔진이 내는 프레임은 연 쪽 origin의 sender endpoint로 sim 버스에 보내고, 그 tx 에코 `can.frame`에 `payload.isotp{endpoint, request_id}` 주석(수신 FF/CF/FC에도 엔드포인트 주석). 사용자 origin 요청은 STOP 예외.
3. **수신·이벤트**(priority lane): observer drain에서 rx_id 일치 프레임을 엔진에 넣는다(관측 필터 ≠ 장치 필터 — 모든 프레임은 여전히 `can.frame`으로 발행). `can.isotp_rx_start`는 FF 처리 직후 **FC보다 먼저**, `rx_complete{endpoint, pdu(hex), first_seq, last_seq}`, `rx_error`, `tx_done{endpoint, request_id, tx_state}`, `tx_error`, `state`. 엔진 타이머는 pump에서 `poll_at`에 맞춰.
4. **예약 tx_id**: 열린 엔드포인트의 tx_id로 raw `can.send`(그리고 이후 주기·IG·replay)는 worker가 거절 → `E_DENIED` + `detail.reason="tx_reserved"`, `owner`. 사용자 raw 1회도 거절(takeover 없음, plan D13).
5. **STOP(A3)**: latch on → agent·script가 연 엔드포인트의 진행 전송 abort(+ `tx_error`/`state` 이벤트), 새 송신 거절. user 엔드포인트·서버 역할의 sim ECU 응답은 계속(가짜 ECU는 멈추지 않음).
6. **서버 역할 + 가상 ECU 모델(sim 전용)**: `role:"server"` 엔드포인트에 붙는 trait를 `isotp.rs`에 정의 — `pub trait IsoTpServerModel: Send { fn handle(&mut self, pdu: &[u8], now_ns: u64) -> Vec<(Duration, Vec<u8>)>; }`(UDS `loom_uds_sim::SimEcu::handle(pdu, now) -> Replies{delay, bytes}`와 같은 모양 — UDS crate가 main에 들어오면 리드가 어댑터를 붙인다). 응답은 `request 도착 + delay`에 송신(상한 있는 deadline 큐). 시험용 내장 모델: `0x22 F1 90` → `62 F1 90` + 17바이트 VIN, 그 밖 `0x7F <sid> 0x11`. `can.open`의 `nodes` 또는 `can.isotp.open{role:"server", model:"vin_demo"}`로 켠다.
7. **gateway·cli**: 위 RPC 등록(읽기 R0 / send는 write `Request`, classify는 sim R1), 이벤트 kind 등록(고정 pointer: `/payload/endpoint`, `/payload/tx_state`, `/payload/pdu/data`), CLI `loom can isotp open|send|abort|close|list`(디버그용 — 스킬에는 "UDS는 `loom uds`를 쓴다"만).
8. **C2**: 엔드포인트·deadline 큐·요청 태그 모두 상한과 넘칠 때 동작.

## 시험 (FakeWorker + ManualClock)
- client(0x7E0→0x7E8) ↔ server(내장 VIN 모델) 왕복: `22 F1 90` SF → 응답 20바이트 멀티프레임(FF/FC/CF)이 sim 버스를 거쳐 client `rx_complete`로. 이벤트 순서(rx_start가 FC 프레임보다 먼저), 모든 FF/CF/FC가 `can.frame`(isotp 주석 포함)으로 보임, STmin 간격.
- FD 엔드포인트 왕복(64바이트 이상 PDU).
- 예약 tx_id raw 송신 거절(agent·user 모두), close 후 허용.
- 중복 open `E_LOCKED`, 상한, abort 잔여 보고, N_Cr 타임아웃(서버 모델 지연·결함).
- STOP: agent 전송 abort·새 송신 거절, user 전송 계속, 서버 응답 계속.

끝나면 보고서를 쓰고 멈춘다.
