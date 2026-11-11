# P0 보고 — uds-sol · `crates/loom-uds-doip`
> uds-sol(Codex)을 쓸 수 없어서 **uds-lead의 하위 에이전트(Claude)**가 만들었다. 커밋하지 않았다.
**만든 것**: OS TCP/IP 위의 ISO 13400-2 DoIP(C4.1). 의존 crate는 tokio·thiserror·tracing뿐이고 codec에는 의존하지 않는다(UDS 바이트는 불투명 payload). 모듈: `frame`(순수 코덱), `client`, `discovery`, `entity`, 비공개 `io`.
**공개 API**
- 헤더: `parse_header(&[u8;8])`는 패턴(0x02/0x03 + 반전, 0xFF는 차량 식별 요청에만) → 타입 → **길이 상한** → 타입별 길이 순서로 검사하고 할당하지 않는다. `HeaderError::{nack_code, closes_connection}`. `Message::{decode, encode}`(16종)와 스트리밍 `decode_frame`도 있다.
- 활성화 값: `ActivationType`은 0x00·0x01·**0xE0**. `ActivationCode`는 0x00–0x07·0x10·0x11 + `name()`이고, 0x02 = "SA가 이 소켓의 등록 SA와 다름".
- `DoipClient::connect(cfg) -> (client, Receiver<LinkEvent>)`: 연결 timeout → 활성화 timeout. `reactivate`, `open_target(la)`(소켓 공유), `link_state()`(watch), `stats`, `close`를 제공한다.
  - task 2개: 수신 task는 alive check에 control 큐로 즉시 답하고 ACK·응답을 source LA로 분배한다. 송신 task는 control 큐를 먼저 처리한다.
- `DoipTarget`: `submit(pdu, until) -> AckWait`(`handed_at` = 소켓에 넘긴 시각), `AckWait::wait(until)`(ACK·`Nack(code)`·`AckTimeout`·`Closed`), `recv_until`, `try_recv`, `generation`.
- `LinkEvent`: Activated, Reactivated, ActivationDenied, GenericNack, RxOverflow, Unrouted, Error, Closed.
- 발견: `discover(&DiscoveryRequest)`(브로드캐스트·유니캐스트, EID·VIN 필터), `collect_announcements(sock, until, max)`.
- `DoipEntity::start(cfg, handler)`: 핸들러는 sim과 같은 `handle(target_la, now_mono, req) -> Vec<(Duration, Vec<u8>)>`이고 클로저도 받는다.
  - 결함 주입: `inject(NackNextDiag|DropNextAck|CloseOnNextDiag)`, `set_activation_code`, `close_all_connections`, `send_alive_check`.
  - 그 밖: UDP 식별·status·power mode 응답, `send_announcement`, `stats`.
**C2 상한**

| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_DOIP_PAYLOAD` | 4099 (주소 4 + UDS 4095) | 헤더만 보고 NACK 0x02 후 연결 종료(할당 전) |
| `CAP_RX_PER_TARGET` | 32 | 버리고 센 뒤 `RxOverflow`. 수신 task는 대기하지 않는다 |
| `CAP_TX_FRAMES` / `CAP_CONTROL_FRAMES` / `CAP_LINK_EVENTS` | 32 / 8 / 32 | `TxFull`(송신 전 거부) / 경고 후 버림 / 버리고 셈(상태는 watch) |
| `MAX_TARGETS_PER_CONNECTION` | 64 | `TooManyTargets` |
| `MAX_DISCOVERY_RESULTS`·`_TIMEOUT`·`_DATAGRAMS` | 64 · 10 s · 1024 | clamp, 상한에서 수집 종료 |
| 엔티티 `MAX_ENTITY_CONNECTIONS`·`MAX_SCHEDULED_REPLIES_PER_CONN`·`MAX_QUEUED_FAULTS`·`CAP_ENTITY_RX_FRAMES` | 32 · 256 · 16 · 16 | accept 후 즉시 닫음 · 버리고 셈 · 오류 · reader 대기(TCP 역압) |

**시험**: 27개 통과(단위 2, doip 22, fuzz 3). 12회 반복해도 결과가 같았다.
- 연결·활성화: 루프백 왕복, 2019 버전, 활성화 거부 코드 10종, 0x06, **0xE0 + OEM 수용**, 같은 SA 다른 소켓 → 0x03, 재활성화.
- 이번 관문 항목: **응답 대기 중 alive check 응답**, **u32::MAX 헤더**(엔티티는 NACK 0x02 후 종료, 클라이언트는 링크 Error·Closed), **소켓 끊김 → `LinkEvent::Closed`**.
- 그 밖: 알 수 없는 타입은 버리고 연결 유지, 활성화 전 진단은 NACK 0x02 후 종료, 발견·announce, 결함 주입 3종, 미등록 TA → 0x03, LA별 분배, 수신 큐 상한.
- fuzz(결정적 SplitMix64): 무작위 프레임 10만 회(재인코드 일치), 헤더 10만 회, 무작위 TCP 스트림 200개를 보낸 뒤에도 엔티티 정상.
- 관문: `cargo fmt -p loom-uds-doip --check` OK · `cargo clippy -p loom-uds-doip --all-targets -- -D warnings` 경고 0 · `cargo test -p loom-uds-doip` 27 passed.
**루프백 지연** (`cargo run --release -p loom-uds-doip --example loopback_latency`, macOS arm64, rustc 1.93.1, current_thread, 워밍업 20 + 1000회, ACK 포함, 3회 실행): **p50 0.027–0.063 ms, p99 0.106–0.150 ms**, max 0.20–0.85 ms.
**결정**
- 길이 초과는 ISO의 "버림" 대신 NACK 후 종료한다. 최대 4 GiB를 읽어 넘기는 일은 상한이 없다(plan §3.8).
- 알 수 없는 타입이라도 길이가 상한 이하면 512 B 스크래치로 건너뛰고 연결을 유지한다.
- 0x11은 활성화되지 않은 것으로 본다(`ActivationDenied(0x11)`). 엔티티는 거부(0x11 제외) 후 소켓을 닫는다. 같은 SA에는 alive check 없이 0x03을 준다(sim 단순화).
**임시 정의·통합 시 교체**: 없음. `ClientConfig.generation`은 loom-uds가 재연결할 때마다 올린다. **CORE 추가 요청**: 없음(TLS는 P0 밖).
**미해결**: 실HW 검증, TLS, ISO alive check를 거친 0x03 판정, T_TCP inactivity 타이머, ACK timeout 뒤 늦은 ACK가 다음 대기자를 깨울 가능성(러너가 해당 요청을 U에 넣어 막음).
