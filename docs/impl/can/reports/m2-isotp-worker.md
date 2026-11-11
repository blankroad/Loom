# M2a 보고 — ISO-TP를 CAN worker에 올리기 (K10 worker 쪽, `can.isotp.*`)

작성: can-opus, 2026-11-11. 과제 `docs/impl/can/tasks/m2-isotp-worker.md`. 커밋 안 함.

## 결과 (워크스페이스 루트)
- `cargo fmt --all --check` ✓ · `cargo clippy -p loom-can --all-targets -- -D warnings` ✓ · `cargo test -p loom-can` ✓ **66개**(lib 5, cli 11(+2), gateway 14(+1), isotp 8(새), signals 11, worker 17).
- `lib.rs`·`schema.rs`·`Cargo.toml` 안 건드림. 새 의존성 없음. `unsafe`·Python 없음.

## 바뀐 파일
- `src/isotp.rs`(새): `IsoTpServerModel` trait(UDS `SimEcu::handle`과 같은 모양), 내장 모델 `vin_demo`(`22 F1 90`→`62 F1 90`+VIN 17B, 그 밖 `7F <sid> 11`, 2 ms 지연)·`echo`, `OpenParams`/검증, 엔드포인트(엔진 구동·이벤트·STOP·abort·list 정보).
- `src/worker.rs`: 엔드포인트 표, drain에서 rx_id 프레임 → 엔진 공급, pump에서 drain↔drive를 같은 시각에 수렴할 때까지(≤ 64 라운드), `can.isotp.open/close/abort/list` 호출, `can.isotp.send` write(`op.method`로 구분), 예약 tx_id 거절, STOP, 채널 close 시 엔드포인트 close, 서버용 device sender, status에 isotp 수, 모델 등록 `CanSimWorker::register_isotp_model`(≤ 16).
- `src/gateway.rs`: RPC 5개 등록(open/abort/close = 제어, roles WRITE / send = write `Request` + risk row R1 / list = R0), 이벤트 kind 6개(포인터 `/payload/endpoint` 공통, `tx_state` Enum, `/payload/pdu/data` Hex 등), open은 채널 자동 결정·파라미터 선검사, help 예시·note.
- `src/cli.rs`: `loom can isotp open|send|abort|close|list` + 출력. `tests/isotp.rs`(새), `tests/gateway.rs`, `tests/cli.rs`, `skills/loom-can/SKILL.md`("UDS는 `loom uds`" 한 줄).

## 구현 요점
- 엔진 README 절차 그대로: 입력 후 **이벤트 먼저, 그다음 프레임** 꺼냄 → `rx_start`가 FC 프레임보다 먼저 발행(시험으로 확인: FF 프레임 < rx_start < FC). 제출 직전 `is_frame_pending` 재확인, `not_before_ns` 지킴, `on_frame_submitted_at`, `poll_at`에 `on_timer`, `OutputFull`이면 비우고 **같은 입력 재시도**(두 번째 실패는 카운트). sim은 TX 확인이 없어 `DriverAccepted` 뒤 `release_transfer`(`unconfirmed_frames` 기록).
- 모든 FF/CF/FC는 일반 `can.frame`으로 발행되고 `payload.isotp{endpoint, request_id}` 주석. 보낸 쪽 프레임은 최상위 `request_id`도. 주석 우선순위: client 엔드포인트(송신·수신 모두) → 송신한 server → 수신한 server. 그래서 요청/응답 한 번의 프레임이 모두 client 엔드포인트·요청 id로 묶인다.
- `rx_complete{pdu(hex), len, first_seq, last_seq, frames, ts_first_frame}`, `rx_start{expected_length, first_seq, ts_first_frame}`, `tx_done{transfer, tx_state, frames, ts_first/last_frame, ts_source:"synthetic"}`, `tx_error{code, residual_tx_possible}`, `rx_error{code}`, `state{state, reason}` — 전부 priority lane. 코드: `N_BS_TIMEOUT`, `N_CR_TIMEOUT`, `WRONG_SN`, `UNEXPECTED_PDU`, `FC_OVERFLOW`, `OVERFLOW`, `WFT_OVRN`, `ABORTED` 등.
- 예약 tx_id: raw `can.send`(agent·user 모두, takeover 없음)와 진행 중 burst(엔드포인트가 나중에 열려도 다음 프레임에서 중단)를 `E_DENIED`(exit 4) `detail{reason:"tx_reserved", endpoint, owner, opened_by, client_id, id}`로 거절, `can.tx` failed 행도 냄. close 후 허용.
- STOP: agent·script 요청이 진행 중인 엔드포인트만 abort(`tx_error ABORTED` + `state stopped`), 새 agent·script 송신 `E_STOPPED`(exit 6). user 전송·FC와 server(가짜 ECU, device sender, latch 불가)는 계속. 해제 시 `state open`.

## 결정 (리드 확인 바람)
1. **송신 origin = 요청 origin**(엔드포인트를 연 쪽이 아님). FC는 그 엔드포인트의 마지막 요청 origin. 이래야 "STOP은 agent·script 요청만" 규칙이 엔드포인트를 공유해도 맞다.
2. **`E_LOCKED` 범위**: 같은 채널에서 tx_id **또는** rx_id가 이미 쓰이면 거절(같은 rx를 두 엔진이 받으면 FC가 둘 나감). client(7E0→7E8)와 거울 server(7E8→7E0)는 허용.
3. **`first_seq`/`last_seq` = 해당 `can.frame` 이벤트의 producer `source_seq`**(버스 내부 번호 아님). gateway가 `source_ref`로 stream seq에 대응시킬 수 있다. 발행 실패 프레임이면 `null`.
4. **SF 수신에도 `rx_start`(`single_frame:true`)를 합성**: 엔진은 FF에서만 내지만 계약 v0.2 §9-3은 "FF(또는 SF)".
5. **`can.isotp.send`는 `can.tx` 행을 내지 않는다**(요청 `write_state`는 UDS 행 소유, 메인 확정). 응답 `WriteOutcome.state = accepted`, detail `{accepted:true, endpoint, epoch, transfer, len}`.
6. **server 엔드포인트로는 수동 송신 불가**(`E_USAGE`) — 모델만 답한다. 모델 없는 server도 열 수 있다(FC는 하고 PDU는 이벤트로만).
7. rx 이벤트 origin은 `device`, tx 이벤트는 요청 origin, server의 tx는 `device`.
8. `can.open{nodes}`로 ISO-TP 서버를 켜는 길은 만들지 않았다(`loom-can-sim` TOML 변경 필요). `can.isotp.open{role:"server", model}`만.
9. 기본값: padding 0xCC(`null`/`"none"` = 없음), BS 8, STmin 0, N_Bs/N_Cr 1000 ms(≤ 60000), FD면 TX_DL 64, 29bit는 `ext` 또는 id > 0x7FF.
10. 결함 주입 `faults{drop_cf, wrong_sn_cf, fc_delay_ms, fc_wait_count, fc_overflow}`는 sim worker에서만(실 HW worker가 생기면 거절해야 함).

## 상한 (C2)
엔드포인트 채널당 32·전체 128(`E_BUSY`), 대기 프레임 64/엔드포인트, 전송 태그 8(오래된 것부터 밀어내고 카운트), 모델 응답 큐 16(넘치면 버리고 `replies_dropped`), 요청당 응답 8개, 응답 지연 ≤ 60 s, drive당 제출 64, pump당 라운드 64(넘으면 `more_pending`), 모델 팩토리 16. 엔진 자체 상한(PDU 4095 등)은 그대로.

## 시험 (FakeWorker + ManualClock, `tests/isotp.rs`)
VIN 왕복(SF→FF/FC/CF 2개, 프레임 바이트·주석·origin, 이벤트 순서, STmin 5 ms 간격, 모델 지연, NRC 응답, list 카운터) · FD 29bit 100바이트 echo(64바이트 프레임, user origin) · 예약 tx_id(agent·user 거절, rx_id는 허용, 진행 중 burst 중단, close 후 허용) · 중복 open `E_LOCKED`(tx·rx), 파라미터 오류 9종, 채널당 32 상한, 채널 close가 엔드포인트 close · send 오류(없는 엔드포인트, 잘못된 hex, 4096B, server로 송신, 전송 중 `E_BUSY`) · abort 잔여 보고 + 상대 N_Cr · CF 누락 결함 → client N_Cr · STOP(agent abort·거절, user 전송·server 응답 계속, 해제 후 재개).

## 요청 / 남은 일
- `lib.rs`: 필요하면 상수 `CALL_*`처럼 `ISOTP_MODEL_*` 이름 공개는 불필요 — 현재 요청 없음.
- 계약 미구현(이후 단계): `can.isotp.barrier`(drain barrier), `cancel{transfer}`, `recv`, `lease_ms`/heartbeat, functional·group, `set_params`, 외부 송신 감지, FC 지연 p99 측정(R-TP-15 M2 측정 항목).
- UDS crate가 main에 오면 `register_isotp_model`로 `SimEcu` 어댑터를 붙이면 된다.
- `docs/impl/requests/can-ui.md`에 isotp 이벤트 6종과 `payload.isotp` 주석 반영 필요(리드/UI 소유라 여기선 안 고침).
