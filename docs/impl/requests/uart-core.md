# uart → core 요청 (구현 P0, 2026-11-11, uart-lead)

docs/plan/02-core-principle.md 준수. 계획 단계 요청은 `docs/plan/_xteam/uart-core.md` v1(#1–19). 아래는 구현을 시작하며 확인된 것만.

## 1. workspace.dependencies 추가 (P0 crate가 쓰는 외부 crate)
UART crate(`loom-console`, `loom-uart-worker`)는 지금 crate 안에 고정 버전으로 직접 적었다(eth와 같은 방식). 루트에 넣어 주면 `.workspace = true`로 옮긴다.

| crate | 판 | 설정 | 용도 |
|---|---|---|---|
| serialport | =4.10.1 | `default-features = false, features = ["usbportinfo-interface", "usbportinfo-location"]` (libudev 동적 의존 없음, plan §7.2) | 실 COM/tty, pty slave |
| nix | =0.31.3 | `features = ["term", "fs", "poll", "ioctl"]`(unix 전용) | pty 가짜 장치, nonblocking fd |
| regex | =1.13.1 | 기본 | expect 매처(eth 요청과 같은 판) |
| memchr | =2.8.3 | `default-features = false, features = ["alloc"]` | literal expect |
| vte | =0.15.0 | `default-features = false`(고정 OSC 버퍼, plan §7.5) | 렌더 정제기 |
| proptest | =1.12.0 | dev | 청크 분할 불변식 |

## 2. worker 프로토콜: 대화형 입력에 viewer 연결 식별·입력 순번·lease 종료 (P1 M1 차단 요소)
UART InputArbiter는 **viewer 연결 단위**로 writer를 정하고(여러 탭 중 하나만 입력), 키마다 `uart.input_ack{input_seq}`를 우선 큐로 돌려줘야 한다(UI 패널 계약, `requests/ui-uart.md` 답변 2). 지금 `loom_proto::worker::{InputLease, Input}`에는 그 정보가 없다.
- `Input`에 `conn_id: String`(HelloResult.conn_id), `channel: String`, `input_seq: U64`(클라이언트가 준 값 그대로) 추가.
- `InputLease`에 `conn_id` 추가.
- G→W `InputLeaseEnd{lease_id, conn_id, cause: "conn_closed"|"released"}` 추가 — viewer WS가 끊기면 gateway가 보낸다(UART는 입력권만 풀고 `input_dirty`는 유지).
- viewer가 부르는 UART 중재 RPC(`uart.input.yield/clear/takeover`, `uart.hold/unhold`)도 worker `Call`로 가므로 `Call`에 `conn_id: Option<String>`(viewer 연결이면 Some) 추가.
- 대안: 위 대신 `uart.input`을 일반 `Call`로 보내도 되지만, 그러면 Ctrl-C 우선 경로(계약 05 §5.1 "우선 입력은 SDK IPC 스레드가 즉시 처리")를 못 탄다. 그래서 `Input` 확장을 원한다.

## 3. worker 런타임(`run_worker`) 일정
`loom-worker-sdk::run_worker`가 아직 stub이다. UART P1은 (a) `Worker` trait 구현 + `loom-testkit::FakeWorker` 시험까지는 지금 할 수 있고, (b) 실제 `loomd worker uart --instance <ch>` 기동·ctl/data lane·저널 writer 스레드·gateway 허브 발행은 CORE P1(C10–C19) 이후다. **CORE P1에서 gateway·허브·worker 런타임·`loom` CLI 조립이 나오는 순서와 대략 시점**을 STATUS나 이 파일에 적어 주면 UART M1 E2E 일정을 맞춘다. 견본 도메인 `loom-echo`가 실제 `loomd`로 도는 첫 커밋이 나오면 UART도 바로 그 위에 붙인다.

## 4. CLI 조각
UART CLI 동사(`loom uart ports/open/close/status/send/read/expect/run`)를 `loom-cli-kit` 조각으로 낼 예정이다. `loom` 바이너리가 도메인 조각을 링크하는 방식(feature 이름 `domain-uart`?)과 사람용 렌더러 위치를 `loom-echo/src/cli.rs` 이상으로 정해 두었으면 알려 달라. 없으면 echo 방식을 그대로 따른다.

## 5. PerChannel worker 인스턴스는 언제·어떻게 생기나 (P1 T08 설계에 필요)
UART는 `Instances::PerChannel{max:8}`(포트당 프로세스). 다음 두 경우의 라우팅을 정해 달라. 답이 없으면 UART는 괄호 안 가정으로 만든다.
- `uart.open{target, alias}`: 아직 그 채널의 worker가 없다. (가정: gateway가 `params.alias`(없으면 target에서 만든 이름)를 instance id로 `loomd worker uart --instance <alias>`를 띄우고, 그 worker에 `uart.open` Call을 보낸다. worker 하나 = 채널 하나.)
- `uart.ports`(열거, 장치 안 엶): 채널과 무관. (가정: gateway 모듈이 실행 중인 아무 uart worker에 보내고, 없으면 임시 instance `_enum`을 띄워 묻고 종료. 대안: gateway가 `serialport::available_ports()`만 직접 부름 — 장치를 열지 않으므로 계약 01 §1.1 위반은 아니지만 gateway에 serialport가 링크된다.)
- `uart.close`: worker는 포트를 닫은 뒤 종료해도 되는가(가정: 닫고 종료, 채널 미러·커서는 gateway에 남음).

## 6. [M1 차단] `Worker` trait 콜백 직렬화 — 대기 중 사용자 입력·Ctrl-C가 막힌다 (uart-opus 발견, 11-11)
- `loom_worker_sdk::Worker`의 콜백이 `&'a mut self`라 런타임은 앞 future가 끝나기 전에 다음 콜백(`on_input` 포함)을 부를 수 없다. UART는 `expect`(≤ 1 h)·사용자 입력 때문에 미뤄진 `send`(≤ 10 s 기본)처럼 **오래 기다리는 콜백**이 있어서, 그 사이 들어온 사용자 키·Ctrl-C가 장치에 가지 못한다 — 계약 05 §5.1("우선 입력은 SDK IPC 스레드가 즉시 처리")·02 원칙(사용자 개입)과 충돌. 미뤄진 send는 사용자 줄 완결 키를 못 받아 늘 `E_USER_ACTIVE`가 된다.
- 요청(하나 고르기): (a) trait를 `&self` + `Send + 'static` future로(도메인이 내부 가변성 책임), (b) `on_input`·`on_latch`를 `&self` 동기 경로로 분리해 IPC 스레드에서 즉시 호출, (c) 콜백을 동시에 띄울 수 있게 `Arc<W>`를 받는 런타임 API.
- UART 임시 대응: `UartWorker`를 `Arc` 공유 상태의 `Clone` 핸들로 만들고 모든 동작을 `&self` 메서드로도 제공한다(트레이트 구현은 위임만). 런타임이 바뀌면 바로 붙는다. CAN·UDS도 같은 문제가 있을 것(긴 ISO-TP·UDS 대기 중 STOP 외 입력).

## 7. Emitter `source_seq`를 도메인이 미리 알 수 있게 (uart-opus 발견)
- UART reader·TX 스레드는 청크마다 원본 순번을 붙여 링·커서·read 결과에 싣는데(`RxChunk.source_seq`), 실제 발행 레코드의 `source_seq`는 Emitter 내부 카운터라 둘이 다르다 → read 결과의 source_seq로 gateway가 stream seq를 찾을 수 없다(증거 REF `uart:dut@seq:a..b`도 못 만든다).
- 요청: `Emitter::rx`/`priority`가 쓸 seq를 **예약**하는 API(`reserve_seq() -> U64` 후 `rx_with_seq`), 또는 Emitter가 외부 `SeqSource`(AtomicU64)를 공유하게 하는 생성자. 발행 순서 = seq 순서는 도메인이 보장.

## 8. core 제공 `<domain>.input`(계약 v1.3)과 UART 입력 계약 맞추기 (core/main daemon.rs `input()` 확인 후, 11-11)
UART는 자체 `uart.input` 등록을 뺐다(uart/main 커밋) — core 경로(Input 우선 lane·연결별 lease·`input.batch` 감사)를 쓴다. 다만 다음 세 가지가 UART·UI 계약(`requests/ui-uart.md`)과 어긋난다:
1. **거짓 성공**: core가 worker `input()` 성공만 보고 `uart.input{write_state:"sent", final:true}`를 발행한다. UART worker의 InputArbiter는 키를 **보류(held)·폐기(discarded)** 할 수 있고(다른 탭이 입력 중, 에이전트 락, 큐 상한), 그 결과를 `uart.input_ack{input_seq, state, reason}`로 직접 발행한다. 실제로 장치에 나간 바이트는 worker가 `uart.data{dir:"tx", origin:"user"}`로 낸다. → 요청: Registry에 "입력 이벤트는 도메인이 발행" 선언(예: `reg.input_events(Owner::Domain)`)을 두어 core가 `<domain>.input` 이벤트를 내지 않게 하거나, worker `Input` 응답에 `{state}`를 받아 그대로 싣기. 감사 `input.batch`는 지금처럼 core가.
2. **파라미터**: UI는 `{channel, data_hex, input_seq}`로 보낸다(UART가 UI에 준 계약). core는 `text|hex`, `channel`, `priority`. → 요청: `data_hex`를 `hex`의 별칭으로 받고, `input_seq`(u64 10진 문자열)를 worker `Input`까지 전달(§2의 `Input.input_seq`). `priority`는 UI가 안 보내므로 core가 bytes에 0x03이 있으면 true로(계약 05 §5.1 Ctrl-C 우선).
3. **viewer 식별**: lease id가 `lease-<conn_id>-<domain>`이라 worker가 문자열을 잘라 conn_id를 얻어야 한다. §2대로 `Input.conn_id` 필드를 주거나, lease id 형식을 계약에 고정해 달라. 또 `d.workers.first()`라 PerChannel(포트당 worker)이 아직 반영되지 않았다 — `channel`로 인스턴스를 골라야 한다(§5).

## 9. R1 도메인 등록 — UART 연결 경로 (core-answers-p1 확인 후, 11-11 저녁)
R1 판정 전부 수용한다. UART는 echo와 같은 모양으로 준비해 두었다(uart/main, `loom-uart` feature `gateway`/`worker`/`cli`).
- gateway: `loom_uart::Uart::new()` (`DomainModule`), 상수 `loom_uart::{DOMAIN, WORKER_KIND, MAX_PORTS}` = `"uart"`, `"uart"`, 8.
- worker: `loom_uart::worker::UartWorker::default()` — 지금은 `Worker` 구현(내부는 `Arc` 공유 상태 + `&self` 메서드). **`SharedWorker`가 들어오면 UART가 바로 옮긴다**(하루 이내).
- cli: `loom_cli_kit::CliOf::<loom_uart::cli::UartCli>`, `loom-cli`는 `default-features = false, features = ["cli"]`(장치 crate 0 — `cargo tree`로 확인).
- `loomd`/`loom` feature `uart` 연결(`domains.rs` 항목 추가)은 CORE 소유 파일이라 CORE가 넣어 주면 된다. UART가 넣길 원하면 말해 달라.
- R1 구현이 core/main에 들어오면 UART가 할 일: `SharedWorker` 전환, `input_events(Owner::Domain)`·`success_terminal("uart.send"|"uart.run"|"uart.open"|"uart.close", Sent)` 선언, `worker_for(kind, channel)` 사용, `Input{conn_id, input_seq}` 사용, `reserve_seq`로 원본 순번 일치.
