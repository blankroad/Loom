# P1 독립 검토 — uds-sol

대상: worktree `uds`, `68f9277`의 DoIP/runner(P1 우선순위·abort 포함). 기존 crate는 수정하지 않았다. F1 기본 자동 정책은 유지한다.
기준: p0-common·p1-sol, plan §3.4·§3.8·D19, v2-sol·Rust spike, 계약 02/04/08.
기존 시험: DoIP **29개**, runner **44개** 통과. 아래 경계 반례는 기존 suite가 잡지 못한다.
재현: worktree에서 `CARGO_HOME=.cargo-home CARGO_TARGET_DIR=target cargo run -p loom-uds-load --example review_counterexamples --offline`.
실행 증거: [p1-sol-review-repro.json](p1-sol-review-repro.json). 예제는 관측 도구이며 거짓 성공 동작을 acceptance assertion으로 고정하지 않는다.

## P1 — 고쳐야 함

1. **송신 후 future drop 시 U 누락 / 이전 응답이 새 쓰기를 confirmed** — `crates/loom-uds-runner/src/runner.rs:618` (`execute`), `:693`(submit), `:611`(명시적 abort만 U 등록).
   재현(실행): 첫 `2E F1A0 01` 응답은 60 ms, execute를 10 ms timeout으로 drop → U가 비어 있음. 두 번째 `2E F1A0 02`의 진짜 응답은 300 ms인데 첫 응답으로 약 52 ms 만에 Positive. 제안: 송신 가능 시점부터 in-flight 상태를 보존하는 취소 guard, 또는 외부 실행을 취소 안전한 RunnerHandle로 제한하고 직접 API에도 drop 회복 계약을 둔다.
2. **수신 timestamp를 deadline과 대조하지 않아 늦은 최종 응답을 성공 처리** — `crates/loom-uds-runner/src/runner.rs:738`, `:744`, `:751`.
   재현(실행): P2 50 ms·총 70 ms, 응답 stamp 80 ms; 30 ms 뒤 runtime을 80 ms 막으면 Positive/confirmed. FakeTransport도 ready 응답부터 꺼내며 실제 DoIP `timeout_at(rx.recv())` 역시 ready future 우선이다(tokio 1.54.1 `time/timeout.rs:215`). 제안: RxStart/Pdu의 `at`를 해당 P2/P2*/완료/총 deadline과 비교하고, 초과는 timeout+late/U로 처리한다. 제시간 stamp는 executor 지연만으로 실패시키지 않는다.
3. **만료·취소된 TX가 이후에도 송신 / writer 무한 대기** — `crates/loom-uds-doip/src/client.rs:633`, `:658`, `:670`, `:424`; entity도 `crates/loom-uds-doip/src/entity.rs:392`의 `write_all`에 같은 대기 문제가 있다.
   재현(실행): 이미 지난 until로 submit → TxTimeout인데 entity `diag_received` 증가. 정적 반례: peer가 읽기를 멈추면 writer가 write_all 안에서 close/control을 관측하지 못한다. 제안: TxItem에 deadline·취소/hand-off 상태를 보존하고 쓰기 전 재검사, write 중 시간/close 상한, 부분 frame 전송 실패 시 연결 폐기(다음 frame 이어쓰기 금지).
4. **reactivate 취소 후 영구 ActivationBusy** — `crates/loom-uds-doip/src/client.rs:338`, `:345`, `:348`, `:360`.
   재현(실행): RA 응답을 보류하고 reactivate future를 5 ms에 drop → 다음 호출이 즉시 ActivationBusy(원 timeout cleanup도 실행되지 않음). 제안: 취소/encode 실패까지 activation waiter를 정리하는 guard 또는 activation 전용 actor; 늦은 RA가 새 waiter를 해제하지 않도록 연결/시도 구분도 필요하다.
5. **다른 tester 주소의 RA 응답으로 Active 전환** — `crates/loom-uds-doip/src/client.rs:270`, `:372`.
   재현(실행): tester=0x0E80 요청에 peer가 tester=0x0E81/code=0x10 응답 → connect 성공. 제안: 응답 tester를 요청 SA와 검증한 뒤 waiter를 완료하고, 불일치 시 프로토콜 오류/연결 종료로 처리한다.
6. **요청 큐는 건수만 제한하고 PDU/메타데이터 bytes를 admission에서 제한하지 않음** — `crates/loom-uds-runner/src/handle.rs:194`, `:213`, `:221`; 길이 검사는 `runner.rs:660`에서 뒤늦게 한다.
   재현 조건(정적): 실행 중인 요청 뒤에 대형 pdu/request_id를 try_submit하면 각 lane 건수 이내로 그대로 보관되고 request()는 추가 clone도 한다. 제안: enqueue/clone 전 PDU ≤4095 및 request_id 상한 검사, lane/대상/전역 bytes quota를 예약·해제한다. config의 probe_candidates도 개수뿐 아니라 각 PDU 크기를 제한한다.

## P2 — 개선·계약 확인

1. **ACK 상관관계는 SID가 아니라 LA 단위** — `crates/loom-uds-doip/src/client.rs:326`, `:575`, `:652`. ACK timeout/취소 후 다른 SID 요청을 보내면 늦은 이전 ACK가 새 waiter를 깨울 수 있다. SID 기반 U만으로는 방지되지 않는다. 제안: ACK 불확실 LA를 재연결까지 잠그거나 해당 연결을 폐기하고, 늦은 ACK/NACK 교차 시험을 추가한다.
2. **generation 변화가 모든 Transport에서 U 전체 해제** — `crates/loom-uds-runner/src/runner.rs:526`. 현재 DoIP의 구 소켓 폐기+RA에는 맞지만 CAN 채널 재개방에는 D19 회복 증거가 아니다. 제안: CAN 어댑터 도입 전 generation 알림과 검증된 응답 원천 폐기 증거를 분리한다.
3. **count cap과 plan byte 예산 차이** — `crates/loom-uds-doip/src/client.rs:28`, `:31`: RX 32×4095 B/target, 64 targets면 약 8 MiB/connection으로 §3.8 연결당 64 KiB와 다르다. 제안: 통합 단계에서 연결/global bytes quota를 합의하고 RxOverflow를 손실/health 이벤트로 연결한다(링크 이벤트 자체도 drop 가능하므로 counter를 함께 확인).

## 확인·결정·질문

- 0xE0, RA 0x10/0x11 구분, 분할/연속 frame, 길이 검사 후 할당, 0x78 횟수·SPRMIB의 78 뒤 최종 응답, SID 중첩 U·프로브 불해제, 명시적 abort 쓰기 outcome_unknown은 기존 시험/코드에서 확인했다.
- 수정 배정은 리드 소유다. load crate에만 재현 예제를 두었고 공유 DoIP/runner/loom-uds는 읽기만 했다. CORE 외부 의존 추가 요청 없음.
- 질문(보고만): 직접 Runner::execute를 외부 호출 가능 API로 유지할지, RunnerHandle만 노출할지 리드 결정 필요. P2의 ACK/generation 회복 계약도 명시 요청.
