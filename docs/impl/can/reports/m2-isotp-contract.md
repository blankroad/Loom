# can-sol M2a ISO-TP 계약 마무리 (2026-11-11)

worker 측 `can.isotp.cancel/recv/barrier` 구현 완료. 변경은 `crates/loom-can/src/{worker,isotp}.rs`, `tests/{isotp,periodic}.rs`와 이 보고서뿐이다. `gateway.rs/signals.rs/cli.rs`, 공통 타입, ISO-TP 엔진 crate, Cargo 파일은 수정하지 않았다. 병렬 작업 파일은 그대로 두었고 커밋하지 않았다.

## 동작과 결정

- **cancel**: `Write.op={method:"can.isotp.cancel",endpoint,transfer}`. 진행 중인 전송의 transfer ID와 원 요청의 `(client_id, origin)`을 확인한다. 이미 terminal/없는 transfer는 `E_NOT_FOUND`, 다른 client/origin은 `E_LOCKED`, 닫힌 endpoint는 `E_NOT_OPEN`. 성공 시 `WriteOutcome.state=accepted`, detail `{endpoint,epoch,transfer,state:"cancelled",residual_tx_possible}`. 잔여 값은 이미 제출·미확인인 데이터 프레임의 보수적 **개수**이며 실제 wire 취소/완료 보장이 아니다. 원 송신 request_id/client_id/origin을 유지한 `can.isotp_tx_error{code:"CANCELLED"}`를 발행한다. cancel은 worker call로는 거부하며 STOP 상태에서도 정지 제어를 허용한다.
- 엔진에 전송별 취소 API가 없고 `abort`는 RX도 지우므로, adapter에서 **송신용/수신·FC용 Engine을 분리**했다. FC ingress는 송신 엔진, SF/FF/CF는 수신 엔진으로 보낸다. frame handle은 `transfer_id` 유무로 해당 엔진에서 확인한다. cancel은 송신 엔진만 abort하므로 같은 endpoint의 진행 중 RX·FC, 다른 endpoint, 완료 PDU 링이 보존된다. 기존 전체 abort/STOP/close는 두 엔진을 정리한다. 엔진 자체 API/코드는 바꾸지 않았다.
- **recv**: endpoint별 완료 링 **32개**. `pdu_seq`는 1부터 증가하며 event와 링 행에 함께 붙는다. FIFO overflow는 oldest 제거 + 누적 `dropped` 증가. 조회는 비소비·재조회 가능하며 `after_pdu_seq`보다 큰 항목을 순서대로 반환한다. `next_pdu_seq`는 **마지막 반환 항목의 cursor**이며 아직 반환하지 않은 head로 건너뛰지 않는다. 빈 결과는 입력 cursor(기본 0)를 유지한다. `latest_pdu_seq/oldest_pdu_seq/dropped/cursor_gap`으로 보존 창 유실을 표시한다. endpoint close/reopen 때 새 링/seq가 시작하고 cancel/abort는 기존 완료 링을 보존한다.
- 완료 행은 `pdu_seq,pdu:{encoding:"hex",data},len,endpoint,epoch,request_id,rx_id,first_seq,last_seq,frames,ts_first_frame,ts_last_frame,ts_source`를 담는다. 새 PDU cursor·누적 dropped·watermark/ns는 decimal string이고 epoch·기존 frame source_seq 필드의 기존 표현은 유지했다. 완료 이벤트 발행이 실패해도 링을 조회할 수 있다.
- **barrier**: SDK host monotonic clock domain의 `deadline_mono_ns`를 받는다. epoch를 먼저 검사하고, bounded sim poll → observer drain → matching transport feed → `rx_start` 우선 발행을 동기 수행한다. 큐가 비었고 bus head까지 처리했으며 due sim poll 잔여가 없고 현재 host 시각이 deadline에 도달했으며 처리/발행 손실이 없을 때만 `complete=true`다. PDU 전체 수신 완료나 FC 실제 송신을 기다리는 barrier가 아니다.
- `covered_ingest_seq`는 **해당 sim bus의 ingest/source_seq**이고, `transport_event_seq`는 **해당 endpoint에서 마지막으로 성공 발행한 transport 이벤트의 producer source_seq**다. 두 seq와 gateway stream seq는 서로 다른 영역이다. `covered_mono_ns`는 배출 완료 시 host now, 아직 배출 중이면 마지막 처리 ingress 시각이다. 입력 유실·처리 실패·이벤트 발행 실패 시 proof를 sticky invalid로 만들고 `covered_mono_ns="0",complete=false`를 유지한다. 재개에는 endpoint reopen이 필요하다. epoch 불일치는 현 공통 코드 중 `E_CURSOR_GAP` + `detail.code="E_EPOCH_CHANGED"`, expected/current epoch를 반환한다.
- **periodic list**: 현재 커밋에 이미 `data`(대기 patch를 포함한 최신 hex)·`revision`·`applied_revision`·`effective_frame`이 있었다. production 코드는 중복 수정하지 않고 연속 masked patch 후 tick 전/후 각각 최신 data/revision과 적용 상태를 검사하도록 기존 시험을 확장했다.

## C2와 대기

recv는 최대 **16 PDU**, 직렬화된 PDU 행 합계 **48 KiB**까지만 반환해 64 KiB worker envelope에 여유를 둔다. 최대 PDU 4095 B, 보관 행 12 KiB 상한, 링 32개, request/client ID 각각 128 B 상한이다. 최대 크기 PDU 8개 조회에서 byte cap으로 페이지가 나뉘고 cursor 누락 없이 나머지를 받는 시험을 통과했다. 엔진 큐와 pending frame 64개 등 기존 상한은 유지하며 송신/수신 엔진 각각에 엔진의 유한 queue/ledger 상한이 적용된다. 전체 worker 최악 RSS/장기 soak를 새로 측정했다는 뜻은 아니다.

recv/barrier의 `timeout_ms`는 **0..60000**, 기본 0이다. worker는 기다리거나 virtual clock을 전진시키지 않고 한 번의 bounded 작업 뒤 즉시 snapshot을 돌려준다. **gateway가 deadline까지 폴링해야 한다.** 미래 deadline·미배출·손실은 성공으로 포장하지 않는다. `max_pdus`는 1..16(기본 16), `after_pdu_seq`는 기본 0이다. unsigned 인자는 정수 또는 최대 20자리 decimal string을 받으며 음수·잘못된 형식·u64 overflow·head보다 앞선 recv cursor는 `E_USAGE`다.

## 리드 연결 요청 — gateway 등록 내용

리드 준비 상수 `RPC_ISOTP_CANCEL/RECV/BARRIER`를 `gateway.rs`에서 import하고 `register_isotp`에 다음을 추가해야 한다. 현재 외부 RPC 등록은 **아직 연결하지 않았다**.

| 메서드 | 인자 | 결과 | 등급·경로 |
|---|---|---|---|
| `can.isotp.cancel` | required `endpoint:string`, `transfer:string`(최대 64 B) | write 봉투, detail `{endpoint,epoch,transfer,state:"cancelled",residual_tx_possible:integer}` 또는 명시 오류 | **R0 정지 제어**, `roles=WRITE`, `write_class=Request`; `ctx.worker(...).write(request_id,{method,endpoint,transfer},deadline)` 사용. `.call`로 전달하지 않는다. |
| `can.isotp.recv` | required `endpoint`; optional `after_pdu_seq:u64=0`, `timeout_ms:0..60000=0`, `max_pdus:1..16=16` | `{endpoint,epoch,pdus:[],next_pdu_seq,latest_pdu_seq,oldest_pdu_seq:null|string,dropped,cursor_gap:boolean}` | **R0 read**, `write_class=None`; worker `.call(RPC_ISOTP_RECV,args,deadline)` |
| `can.isotp.barrier` | required `endpoint`, `epoch:u64`, `deadline_mono_ns:u64`; optional `timeout_ms:0..60000=0` | `{endpoint,epoch,covered_ingest_seq,transport_event_seq,covered_mono_ns,complete:boolean}` | **R0 read**, `write_class=None`; worker `.call(RPC_ISOTP_BARRIER,args,deadline)` |

- cancel handler는 send handler처럼 CORE가 정한 요청 origin/client를 worker write로 전달한다. `CanHooks.classify`에 cancel의 R0 분기를 두고 `presend_check`에서는 raw frame 파싱/tx_id 송신 검사로 보내지 않는다. endpoint/transfer 소유권은 worker에서 다시 검사한다. cancel을 새 CAN 송신처럼 승인·latch로 차단하지 않는다.
- recv/barrier는 read 등록 + bounded 폴링 wrapper를 권장한다. outer wait 상한은 요청 timeout과 `ctx.deadline` 중 이른 시각(최대 60 s, CommandSpec wait에 IPC 여유 포함). recv는 PDU가 있으면 즉시, 없으면 동일 cursor로 재조회한다. barrier는 complete 또는 유한 timeout까지 재조회하고 timeout이면 마지막 `complete=false`를 그대로 돌려 UDS가 판정 불확실로 처리하게 한다. worker 호출 자체는 장기 wait가 아니다.
- UDS 소비자는 `transport_event_seq`까지 transport 이벤트 수신을 확인한 뒤 `rx_start` 부재를 판정해야 한다. source watermark를 gateway stream cursor로 직접 비교하지 않는다. `complete=false`, publication loss, `E_EPOCH_CHANGED`는 P2 timeout 확정 근거가 아니다.
- 기존 `can.isotp_rx_complete` event schema/pointers에 필요하면 `pdu_seq`·`ts_last_frame`을 추가하고, tx_error code에서 `CANCELLED`를 허용한다. periodic list의 `data/revision`은 그대로 사용하면 된다.

## 검증

`CARGO_HOME=/private/tmp/loom-can-export-cargo`(앞 작업에서 준비한 쓰기 가능한 임시 Cargo cache)를 사용했다. 새 의존성·전역 설정 변경은 없다.

- `cargo fmt --all --check`: 통과.
- `cargo clippy -p loom-can --all-targets -- -D warnings`: 통과.
- `cargo test -p loom-can`: **109개 통과, 실패 0**. unit 7 + CLI 13 + gateway 18 + ISO-TP 15 + periodic 16 + signals 23 + worker 17. doc-test 0.
- ISO-TP는 기존 8개 + 새 계약 7개. FakeWorker/ManualClock으로 cancel 진행 중 residual·잘못된 transfer/다른 origin/client·중복/완료 후 오류·동시 RX 보존·상대 N_Cr, recv 순서/after/비소비/32개 상한/dropped·최대 PDU byte pagination, barrier 미배출 FF→rx_start→complete·미래 deadline·epoch 변경/재open·observer overflow를 확인했다. MemorySink 발행 실패 후 barrier가 거짓 완료하지 않고 recv fallback은 완성 PDU를 반환하는 것도 확인했다.
- 기존 periodic 시험에서 연속 patch의 tick 전 `data=1bc2,revision=2,applied_revision=0`과 tick 후 applied revision 2를 확인했다.
- `git diff --check`: 통과. 실HW·IPC/gateway 신규 RPC end-to-end 검증은 이 작업에 포함되지 않았다. 외부 RPC 연결 후 추가 검증은 리드 작업이다.
