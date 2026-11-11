# P0 보고 — uds-sol · `crates/loom-uds-runner` (D2 관문)
> uds-sol(Codex) 대신 **uds-lead의 하위 에이전트(Claude)**가 작업했다. 커밋하지 않았다.
**만든 것**: Transport trait, DoIP·가짜 transport, 대상별 직렬 러너(P2·0x78·P2*·완료 상한·총 deadline·generation), 격리 집합 U, `write_state` 매핑. 의존은 tokio, thiserror, tracing, loom-proto(`WriteState`·`ErrorCode`), loom-uds-codec(`classify_response`·`service`·`SPRMIB`·`MAX_PDU_LEN`만), loom-uds-doip이다. codec·doip은 `loom-uds`처럼 path 의존으로 걸었다.
**공개 API**
- `Transport: Send`: `generation()`, `submit(pdu, until)`(소켓에 넘긴 시각), `confirm(until)`(DoIP ACK = P2 시작), `recv(until)`, `try_recv()`. 비동기 메서드는 `BoxFuture`를 돌려준다(async-trait 없음). `Incoming`은 `RxStart` 또는 `Pdu`.
- 구현 2개: `DoipTransport::new(DoipTarget)`, `FakeTransport::new(script) -> (_, FakeControl)`. 가짜 쪽 스크립트는 지연 응답·RxStart·BumpGeneration·Close를 내고, 제어 핸들은 `inject`, `fail_next_submit/confirm`, `sent()`를 제공한다.
- `Runner::new(transport, RunnerConfig, MonoClock, observer)`: `execute(TxnRequest) -> TxnResult`, `probe(timeout)`, `set_timing`, `replace_transport`, `note_ecu_reset`, `unresolved()`.
- `RunnerHandle::spawn`: `try_submit`(큐가 차면 `E_BUSY`), `request`.
- `TxnResult`: status, write_state, timeout, nrc, sent, may_have_executed, response, pending_count, late_count, generation, `error: Option<ErrorCode>`, `times{received, sent, p2_start, first_rx, final_rx}_mono_ns`(`MonoClock`으로 gateway 시계에 맞춘다).
- `RunnerObserver::on_event(Sent|Pending|Late{released}|QuarantineReleased)`: 동기 호출이고 블로킹 금지. publish-before-reply용이다.
**write_state 매핑**

| 결과 | write_state |
|---|---|
| 소켓에 넘김 / 긍정 응답 | `sent` / `confirmed` |
| NRC | `failed` + `nrc` (`E_NRC`) |
| 읽기 timeout | `failed` + `timeout` (`E_NO_RESPONSE`) |
| 쓰기 timeout·ACK 없음·0x78 상한·송신 후 링크 끊김/generation 변경 | `outcome_unknown` (재전송 없음) |
| SPRMIB 무응답 | `ok_suppressed` → `sent` |
| DoIP NACK·송신 전 거부 | `failed` |

**C2 상한**

| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_PENDING_PER_TARGET` | 32 (+ 진행 중 1) | `E_BUSY`, 미송신 |
| `MAX_RESPONSE_PENDING_HARD` | 1024 (기본 50) | timeout(PendingLimit) + U 등록 |
| `MAX_UNRESOLVED_PER_TARGET` | 256 | 대상의 새 요청 전부 `E_QUARANTINE`, 내쫓지 않음 |
| `MAX_PROBE_CANDIDATES` / `MAX_FAKE_SCHEDULED` / `MAX_FAKE_LOG` | 8 / 256 / 256 | 잘라냄 / 버리고 셈 / 가장 오래된 것 제거 |

**시험**: 33개 통과(가짜 transport + 정지 시계 30, DoIP 엔티티 루프백 종단 3). 5회 반복해도 결과가 같았다.
- 타이밍·연결: P2 경계(49/51 ms), 0x78 → P2*, P2* timeout, 0x78 상한, 총 deadline, 큐 대기의 deadline 포함, 완료 상한, 요청 중 generation 변경(쓰기 unknown·읽기 failed), 링크 끊김, NACK·ACK timeout·TxFull.
- 응답 판정: SPRMIB(무응답·NRC·0x78 뒤 최종 응답), 에코 불일치는 결과로 쓰지 않음, 무작위 응답 2000건. 격리: late 표시와 해제, 다음 요청 도중 도착한 늦은 응답도 결과로 쓰지 않음, 정숙 구간 거부, 용량 상한.
- **반례**: `22 F190` timeout 뒤 `22 F191` → `E_QUARANTINE`. 늦게 온 `7F 22 31`이 F191의 NRC로 분류됨을 assert로 보였다.
- 해제 조건: 늦은 0x78로는 해제하지 않음, **프로브 성공으로도 해제하지 않음**, timeout된 프로브는 다시 쓰지 않음, 재연결·0x51·검증 경과 시간으로 해제.
- 관문: `cargo fmt -p loom-uds-runner --check` OK · `cargo clippy -p loom-uds-runner --all-targets -- -D warnings` 경고 0 · `cargo test -p loom-uds-runner` 33 passed.
**결정**: P2는 DoIP ACK부터 잰다. 겹침은 SID로 판정한다(D19). 겹치지 않는 요청은 max(P2*, 1 s)를 기다리고 U가 비면 일찍 끝낸다. 프로브 기본은 `3E 00`·`22 F186`이다. 큐에서 deadline이 지나면 `failed` + `E_BUSY`.
**임시·통합 시 교체**: 로컬 `BoxFuture`(domain-api와 같은 정의). `MonoClock` 기준점을 worker-sdk `Clock`과 맞춰야 한다. 재연결 시 `replace_transport`는 loom-uds가 부른다. **CORE 요청**: 없음.
**질문**: proto `WriteState::Confirmed` 문서("Backend confirmed transmission")는 리드 결정(ECU 긍정 = confirmed)과 다르다. 문서를 고칠지 묻는다. ok_suppressed = `sent`, 읽기 중 링크 끊김 = `failed`로 정했는데 확인을 바란다.
**미해결**: 읽기 1회 재시도, 우선순위(abort > user > agent), CAN-TP barrier(U-13).
