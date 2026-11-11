# M2a 보고 — `can.isotp.cancel/recv/barrier` gateway 등록 (can-opus)

작성: can-opus, 2026-11-11. 근거: `m2-isotp-contract.md` "리드 연결 요청 — gateway 등록 내용". 바뀐 파일: `src/gateway.rs`, `src/cli.rs`, `tests/gateway.rs`, `tests/cli.rs`. `worker.rs`·`isotp.rs`는 손대지 않았다. 커밋 안 함.

## 결과
`cargo fmt --all --check` ✓ · `cargo clippy -p loom-can --all-targets -- -D warnings` ✓ · `cargo test -p loom-can` ✓ **114개**(lib 7, cli 14(+1), gateway 22(+4), isotp 15, periodic 16, signals 23, worker 17).

## 등록 (계약 표 그대로)
| 메서드 | 경로 | 등급 |
|---|---|---|
| `can.isotp.cancel{endpoint, transfer ≤ 64 B}` | **worker write** `{method, endpoint, transfer}`(요청 id 그대로, `.call` 아님), 결과에 `request_id` | `WriteClass::Request`, roles WRITE, risk row **R0** "stops an ISO-TP transfer". `classify`가 메서드를 먼저 보고 env와 상관없이 R0. `presend_check`는 채널 대상이 없어 그대로 통과(프레임 파싱·tx_id 검사 없음). 소유권은 worker가 검사 |
| `can.isotp.recv{endpoint, after_pdu_seq?, timeout_ms 0..=60000, max_pdus 1..=16}` | worker **call**, gateway 폴링 | R0 read |
| `can.isotp.barrier{endpoint, epoch, deadline_mono_ns, timeout_ms?}` | worker **call**, gateway 폴링 | R0 read |

- **폴링**: worker는 기다리지 않으므로 gateway가 **같은 인자로**(recv는 같은 cursor) 10 ms마다 다시 묻는다. 끝 = recv는 PDU가 하나라도 있을 때, barrier는 `complete:true`일 때, 또는 `min(timeout_ms, 호출 deadline)`. 시간이 다 되면 **마지막 결과를 그대로**(barrier는 `complete:false` 그대로 — UDS가 "판정 불확실") 돌려준다. worker 오류(`E_NOT_OPEN`, epoch 변경의 `E_CURSOR_GAP` 등)는 즉시 그대로. 결과에 `polls`, `waited_ms`, `timed_out`을 더한다. CommandSpec max_wait = 60 s + IPC 여유 5 s.
- 인자 검사(worker와 같은 규칙, 보내기 전에): unsigned는 정수 또는 ≤ 20자리 decimal string, 음수·형식 오류·u64 넘침·모르는 키·범위 밖 → `E_USAGE`.
- **이벤트 포인터**: `can.isotp_rx_complete`에 `/payload/pdu_seq`, `/payload/ts_last_frame`(둘 다 `U64String`) 추가. `can.isotp_tx_error`·`rx_error`의 `/payload/code`를 문자열에서 **Enum**으로 — worker 코드 13개 + `CANCELLED`.
- **CLI**: `loom can isotp cancel EP TRANSFER`, `isotp recv EP [--after N] [--timeout MS] [--max N]`, `isotp barrier EP --epoch N --deadline-ns NS [--timeout MS]`(큰 수는 문자열로 보냄, CLI wait = timeout + 2 s). 출력 예: `cancelled tp1 transfer 0.3 (residual_tx_possible 2)`, `#5 tp1 62f190 (3 B, rx 0x7E8)` + `-- 1 PDUs · next 5 · dropped 2 · CURSOR GAP …`, `barrier tp1 epoch 3: NOT complete (uncertain)  covered_ingest_seq=41 transport_event_seq=17`.

## 시험 (가짜 worker)
cancel이 write로 가고 call은 0, op 모양, 인자 오류 4종, sim·vehicle 모두 R0, presend 통과, write class · recv: 빈 응답 2번 뒤 PDU에서 즉시 반환(3회, 매번 같은 인자), timeout 0이면 1회, 시간 초과 시 마지막 스냅샷 + `timed_out`, worker 오류 즉시 종료(1회), 인자 오류 9종 · barrier: 미완료 2번 뒤 완료, 끝까지 미완료면 `complete:false` 그대로, epoch 오류 그대로, 인자 오류 5종, 두 RPC가 R0이고 wait ≥ 60 s · 포인터(`pdu_seq`, `ts_last_frame`, code Enum에 `CANCELLED`) · CLI 변환·출력.

## 메모
- 계약의 STOP 예외(cancel은 STOP 중에도 허용)는 worker가 집행한다. gateway는 R0로 표시만 하고 막지 않는다 — CORE가 Request 클래스 write를 latch로 막는다면 cancel만 예외로 두도록 CORE 확인이 필요하다(요청).
- 폴링은 CORE `ctx.wait`가 생기면 구독 기반으로 바꿀 수 있다(기존 요청 1과 같음).
