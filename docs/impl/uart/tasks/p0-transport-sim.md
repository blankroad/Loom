docs/plan/02-core-principle.md 준수.

# P0: `loom-uart-worker` 전송·reader·TX actor·가짜 장치 (T03·T04)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. CARGO_TARGET_DIR = `$HOME/.cache/loom-target/uart-worker`.
보고서: `docs/impl/uart/reports/p0-transport-sim.md`.

## 수정 가능한 파일 (이것만)
- `crates/loom-uart-worker/**` 전체(이 crate의 `Cargo.toml` 포함 — **고정 버전 `=x.y.z`만**, 새 외부 crate를 넣으면 보고서에 이유). `lib.rs` 모듈 목록도 이 작업 소유.
- `crates/loom-console/**`는 읽기만(다른 작업자가 동시에 만든다).

P0 범위는 **장치 쪽 부품**이다. CORE worker 프로토콜·RPC·저널 연결은 P1에서 리드가 붙인다. 여기서는 순수 Rust 부품 + 시험만.

## A. `transport` (plan §7.2, §7.3, §9.1)
```rust
pub trait Transport: Send {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize>;       // 0 = 대기 시간 안에 데이터 없음
    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult;               // 부분 쓰기 허용
    fn set_dtr(&mut self, on: bool) -> io::Result<()>; fn set_rts(&mut self, on: bool) -> io::Result<()>;
    fn set_break(&mut self, on: bool) -> io::Result<()>;
    fn config(&self) -> &PortConfig; fn kind(&self) -> TransportKind; // Serial | Pty | Sim
}
pub struct WriteResult { pub written: usize, pub outcome: WriteOutcome /* Complete | TimedOut | Failed(io::ErrorKind) */ }
pub struct PortConfig { baud: u32, data_bits: 5..=8, parity: None|Odd|Even|Mark|Space, stop: One|OnePointFive|Two, flow: None|Software|Hardware }
```
- `SerialTransport`(serialport 4.10.1): `open(path, &PortConfig, OpenOpts{exclusive: true, dtr: Option<bool>, rts: Option<bool>, sim_pty: bool})`.
  - Unix: open 뒤 fd를 **`O_NONBLOCK`** 으로 바꾸고, read/write는 `poll` + 부분 처리 + `WouldBlock` + **절대 deadline**으로 직접 처리(serialport의 blocking write를 쓰지 않음, plan §7.3). TX는 호출자가 ≤ 1 KiB로 잘라 부른다고 가정하되 더 커도 동작. nix의 안전 API만(`unsafe` 금지).
  - Windows: `cfg(windows)` 경로는 serialport의 `set_timeout`(유한 COMMTIMEOUTS)으로 컴파일만 되게(실행 시험은 M2). 컴파일 확인용 target이 이 머신에 있으면(`x86_64-pc-windows-gnu`) `cargo check --target x86_64-pc-windows-gnu -p loom-uart-worker` 결과를 보고서에(링커 없이 check만).
  - Mark/Space parity, stop 1.5는 **지금은 `Err(Unsupported)`**(조용한 대체 금지, plan §7.2 — native 확장은 M2 T17). Unix stop 1.5는 항상 Unsupported.
  - DTR/RTS 적용은 open 뒤 명시적으로 하고 **실패를 돌려준다**(serialport는 open 중 DTR 오류를 무시하므로 builder에 맡기지 않음).
  - `sim_pty=true`(sim이 발급한 pty만)는 baud ioctl 실패(macOS ENOTTY)를 무시한다. 실 장치에는 절대 적용하지 않는다.
  - `exclusive=true` 기본(serialport Unix 기본 TIOCEXCL+flock). 같은 경로 2차 open 실패를 시험.
- `SimTransport`: 인프로세스로 `sim::Device`와 고정 크기 양방향 버퍼(각 1 MiB)로 직결. 3-OS 동일(Windows CI용 `sim://<scenario>`). DTR/RTS/break를 장치에 신호로 전달.
- `open(target: &str, ...)`: `sim://<scenario>[?seed=N]` → SimTransport, 그 외 경로 → SerialTransport. 이 함수와 `SerialTransport::open`은 crate 밖 공개 API이되 **worker 바이너리만 쓴다**는 문서 주석(contract 08 §8.3 — CLI·UI 경로 없음).
- `list_ports() -> Vec<PortInfo>`: serialport 열거 → `{path, vid, pid, serial, manufacturer, product, interface, location}`, 결과 최대 256개(넘으면 잘림 표시).

## B. `reader` + `bridge` (plan §3.5, §3.13 첫 행)
- reader 스레드: `read(wait=20 ms)`로 첫 바이트를 기다리고, 이어서 가능한 만큼 최대 16 KiB까지 drain → `RxChunk{source_seq, rx_offset, mono_ns, wall_ns, bytes}`(시각은 `loom-worker-sdk`가 아니라 `std::time::Instant` 기준 mono + `SystemTime` wall — 시계 주입 trait `Clock`을 두고 시험에서 고정값).
- **bridge**: byte·count 동시 상한(1 MiB 또는 256청크). 가득 차면 reader는 **기다리지 않고** 가장 오래된 미전달 청크를 버리고, 버린 범위(`rx_offset` 구간, 바이트 수, source_seq 범위)를 고정 슬롯에 합산 → 소비자가 다음 pop에서 `BridgeItem::Lost{rx_from, rx_to, bytes, seq_from, seq_to, cause:"reader_bridge_overflow"}`를 받는다(plan: record.gap + display.gap 대상). 소비자 wakeup은 합쳐서(청크마다 깨우지 않음 — Condvar + 대기 중 플래그 등).
- reader 종료: stop 플래그 + 20 ms 안에 빠져나옴. I/O 오류 → `BridgeItem::Disconnected(kind)` 후 종료.

## C. `tx` — TX actor (plan §3.5, §3.7-1, §3.13 TX 행)
전용 스레드, 큐 3개: Ctrl-C(64 B·64항목), user(4 KiB·64항목), agent(64 KiB·64요청). 넘치면 enqueue가 `Err(QueueFull)`(조용한 drop 금지).
- 순서: 매 청크 경계마다 Ctrl-C > user > agent. agent 요청은 ≤ 1 KiB 청크로 나눠 쓰고, 사이에 Ctrl-C가 오면 **그 agent 요청의 남은 부분을 취소**하고 결과 `outcome_unknown`이 아니라 `{state: Failed|OutcomeUnknown, written_bytes, remaining_bytes, cause:"interrupted_by_user"}`(이미 드라이버가 받은 바이트는 written에 포함 — plan §3.7 표 6행). user 일반 키는 agent를 취소하지 않고 다음 청크 경계에 끼어든다(취소는 Ctrl-C만 — 계획과 다르다고 판단하면 보고서에).
- 각 요청에 절대 deadline. 막히면(CTS low 등) timeout → `written_bytes>0`이면 `OutcomeUnknown`, 0이면 `Failed{E_TIMEOUT}`. 자동 재전송 없음. `flush()`는 부르지 않는다.
- 결과는 요청별 완료 콜백 또는 채널(상한 있는 것)로 돌려준다: `TxDone{request_id, origin, state: Sent|Failed|OutcomeUnknown, written_bytes, remaining_bytes, first_mono_ns, last_mono_ns, error}`. 실제로 쓴 바이트 청크마다 `TxChunk{source_seq, origin, client_id, request_id, mono_ns, bytes}`도 낸다(P1에서 `uart.data dir=tx` 이벤트가 됨).
- 시험: 큐 상한, Ctrl-C가 agent 대용량 송신을 청크 경계에서 끊음(written_bytes 정확), deadline(막힌 가짜 transport), 순서 보존(같은 origin FIFO).

## D. `sim` — 가짜 장치 (plan §9.1, §9.2)
```rust
pub trait Device: Send { fn on_open(&mut self, out: &mut Vec<u8>); fn on_bytes(&mut self, b: &[u8], now: Duration, out: &mut Vec<u8>);
                         fn tick(&mut self, now: Duration, out: &mut Vec<u8>) -> Option<Duration> /*다음 tick까지*/; fn on_signal(&mut self, s: Signal, out: &mut Vec<u8>); }
```
M1 시나리오 4개(Rust 코드, YAML은 M2):
- `bootshell`: 부팅 로그(~200줄, ANSI 색·`\r` 진행률 섞음) → `login: ` → `root` → `Password: `(에코 없음) → `root@sim:~# `. 명령 `uname -a`, `cat /proc/meminfo`, `ping 10.0.0.1`(1 Hz, ^C로 통계 + 프롬프트), `status`(`temp=41.5 rpm=1200`), `reboot`(부팅 로그 다시), 모르는 명령 `sh: x: not found`. 줄 편집: 에코, BS(0x7F/0x08) 처리, ^U 줄 지우기, ^C(`^C` + 새 프롬프트).
- `flood`: 지정 속도(기본 92 KB/s, `?rate=`)로 `seq=N t=<ns>\r\n` 연속 + 끝에 해시 확인용 누적 SHA-256을 낼 수 있는 API(sha2 crate가 필요하면 고정 버전으로 추가, 아니면 단순 FNV — 보고서에).
- `binary`: 0x00–0xFF 프레임 반복(`?frames=`).
- `ansi`: 색·커서 이동·진행률·alternate screen 진입/해제, OSC 52·OSC 0 제목·OSC 8 링크, 마우스(?1000h)·포커스(?1004h), **질의 시퀀스 전부**(DA1·DA2·DA3·XTVERSION·DSR·CPR·DECRQM·DECRQSS·`CSI 18 t`·`CSI ? u`·OSC 10/11 `?`)를 차례로 출력. 질의 목록은 상수 배열로 공개(UI E2E와 sanitize 시험이 재사용).
- 모두 seed 기반 결정적(같은 seed = 같은 바이트), 출력 청크 분할도 seed로 흔듦(UTF-8·ESC 중간 분할 포함).

**pty 하네스** (`cfg(unix)`): `PtyDevice::spawn(device) -> PtyHandle{slave_path, ..}` — `nix::pty::openpty`, master를 raw·nonblocking으로 두고 장치 스레드가 poll 루프로 구동(첫 동기 write가 막히는 문제 회피 — plan §9.1). slave는 raw mode/echo off로 설정해 line discipline이 CR/LF·^C·NUL을 바꾸지 않게. `SerialTransport::open(slave_path, sim_pty=true)`로 **실 장치와 같은 경로**로 연다(M1 "직접 접근" 증명). Drop 시 스레드 종료.

## E. 시험 (`crates/loom-uart-worker/tests/`)
- pty `bootshell`: open → `root\r` 로그인 → `uname -a\r` → 응답 줄 확인 → `ping` 후 0x03 → 통계 줄 확인. 0x00–0xFF 양방향 일치(`binary`). 2차 open 실패(exclusive).
- pty `flood` 4 MiB 수신 해시 일치 + bridge 오버플로 시험(소비자를 일부러 멈춰 Lost 범위가 정확한지).
- SimTransport로 같은 bootshell 시험(Windows CI가 돌릴 경로).
- TX deadline: 쓰기가 막히는 가짜 transport.
- 전체 `cargo test -p loom-uart-worker` 60 s 이내.

## 끝나면
보고서 작성 후 대기. 리드가 diff 검토·커밋한다.
