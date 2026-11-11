# can-sol M2a — ISO-TP 계약 마저 (barrier·cancel·recv) + periodic list 바이트

공통 규칙 `docs/impl/can/tasks/p0-common.md`(질문 금지, 검사 worktree 루트 `cargo fmt --all --check` · `cargo clippy -p loom-can --all-targets -- -D warnings` · `cargo test -p loom-can`, 커밋 안 함). **범위:** `crates/loom-can/src/worker.rs`, `src/isotp.rs`, `src/periodic.rs`, 시험 `tests/isotp.rs`·`tests/periodic.rs`, 보고서 `docs/impl/can/reports/m2-isotp-contract.md`. **`gateway.rs`·`signals.rs`·`cli.rs`는 can-opus가 작업 중 — 건드리지 말고**, gateway 등록이 필요한 부분은 보고서에 정확한 등록 내용(메서드·인자·결과·등급)을 적어라(리드가 연결). 리드가 준비: `lib.rs` 상수 `RPC_ISOTP_CANCEL`, `RPC_ISOTP_RECV`, `RPC_ISOTP_BARRIER`.

정본: `docs/plan/_xteam/can-uds-isotp.md` §8-5(recv)·취소 행(163)·**§213 drain barrier 정의**, 맨 아래 v1.1 정본 절. 엔진 `crates/loom-can-isotp` README.
1. **`can.isotp.cancel{endpoint, transfer}`**(write, 요청 origin): 그 전송만 취소 → `{state, residual_tx_possible}`, 이벤트 `can.isotp_tx_error{code:"CANCELLED"}`. 이미 끝난 전송·다른 요청 → 명시 오류.
2. **`can.isotp.recv{endpoint, after_pdu_seq?, timeout_ms ≤ 60000, max_pdus ≤ 16}`**(call): 이벤트를 못 받는 클라이언트용 폴백 — 엔드포인트별 **상한 있는 완료 PDU 링**(예: 32개, 넘치면 오래된 것 버리고 `dropped` 카운트)에서 `after_pdu_seq` 이후를 돌려준다. worker call은 길게 막지 않는다: 있으면 즉시, 없으면 `{pdus:[], next_pdu_seq}`(대기는 gateway가 폴링 — 보고서에 메모).
3. **`can.isotp.barrier{endpoint, epoch, deadline_mono_ns, timeout_ms}`**(call): §213 그대로 — host-ingest 시각 `deadline_mono_ns`까지 들어온 프레임의 ISO-TP 처리와 `rx_start` 발행이 **끝났는지** 확인해 `{covered_ingest_seq, transport_event_seq, covered_mono_ns, complete}`. raw 큐 drain만으로 complete라 하지 않는다. epoch가 다르면 `E_EPOCH_CHANGED`(또는 가장 가까운 코드, 기록).
4. **`can.periodic.list` 항목에 최신 바이트**: `data`(hex, 대기 중 patch 반영값)와 `revision` — can-opus의 `sig.set`이 패치 대기 중에도 바로 인코드할 수 있게.
5. 시험(FakeWorker+ManualClock): cancel(진행 중 CF 남음 → residual, 상대 N_Cr), recv(순서·after·상한·dropped), barrier(FF가 deadline 전 → rx_start 발행 후 complete, 프레임이 아직 drain 전이면 drain 후 complete, epoch 변경), list data가 patch 반영.
끝나면 보고서를 쓰고 멈춘다.
