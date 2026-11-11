DONE

# P0 보고: `loom-uart-worker` 전송·reader·bridge·TX actor·가짜 장치 (T03·T04)

- 작업 위치: worktree `/Users/ctmctm/workspace/Loom/.worktrees/uart` (브랜치 `uart/main`), `crates/loom-uart-worker/**`만 수정. 커밋·stash 하지 않음.
- `CARGO_TARGET_DIR=$HOME/.cache/loom-target/uart-worker`.
- 품질 관문: `fmt --check` 통과, `clippy --all-targets -D warnings` 통과(macOS·windows-gnu 둘 다), `cargo test` 59개 통과(전체 약 2.2 s, 6회 반복 모두 녹색).
- `unsafe` 0, `unwrap()` 0(테스트도 `expect("이유")`).

## 1. 파일

| 파일 | 내용 |
|---|---|
| `Cargo.toml` | `loom-console` 의존 **제거**(P0에서 쓰지 않고 동시 작업 중인 crate라 빌드를 막을 수 있음). 새 외부 crate 없음. nix 기능은 기존 `term, fs, poll, ioctl` 그대로 |
| `src/lib.rs` | 모듈 목록: `bridge, clock, reader, sim, transport, tx` (`clock` 추가) |
| `src/clock.rs` | `Clock` trait, `SystemClock`, `FixedClock`(시험용), `SeqSource`(RX·TX 공용 source_seq) |
| `src/transport/{mod,serial,sim}.rs` | `Transport`/`RxHandle` trait, `PortConfig`, `open`, `list_ports`, `SerialTransport`(Unix·Windows), `SimTransport` |
| `src/reader.rs`, `src/bridge.rs` | reader 스레드, bounded bridge(손실 범위 합산) |
| `src/tx.rs` | TX actor (Ctrl-C > user > agent, deadline, 이벤트) |
| `src/sim/{mod,bootshell,flood,ansi,pty}.rs` | `Device` trait, `SimSpec`, `Runner`, 시나리오 4개, pty 하네스 |
| `tests/{common/mod.rs,pty.rs,sim_transport.rs,tx.rs}` | 통합 시험 |

## 2. 공개 API 요약

### transport
```rust
pub trait Transport: Send + Debug {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize>; // 0 = wait 안에 데이터 없음, EOF/분리는 Err
    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult;        // 부분 쓰기, deadline 넘겨 막지 않음
    fn set_dtr(&mut self, on: bool) -> io::Result<()>; fn set_rts(&mut self, on: bool) -> io::Result<()>;
    fn set_break(&mut self, on: bool) -> io::Result<()>;
    fn config(&self) -> &PortConfig; fn kind(&self) -> TransportKind;          // Serial | Pty | Sim
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>>;                      // 추가(§5 편차 1)
}
pub trait RxHandle: Send + Debug { fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize>; }
pub struct WriteResult { pub written: usize, pub outcome: WriteOutcome }   // Complete | TimedOut | Failed(io::ErrorKind)
pub struct PortConfig { pub baud: u32, pub data_bits: u8, pub parity: Parity, pub stop: StopBits, pub flow: FlowControl }
  // Parity::{None,Odd,Even,Mark,Space}, StopBits::{One,OnePointFive,Two}, FlowControl::{None,Software,Hardware}
  // PortConfig::validate(): Mark/Space/1.5 → ErrorKind::Unsupported, data_bits∉5..=8·baud 0 → InvalidInput
pub struct OpenOpts { pub exclusive: bool /*기본 true*/, pub dtr: Option<bool>, pub rts: Option<bool>, pub sim_pty: bool }
pub fn open(target: &str, &PortConfig, &OpenOpts) -> io::Result<Box<dyn Transport>>  // sim://… → SimTransport, 그 외 → SerialTransport (worker 바이너리 전용 문서 주석, contract 08 §8.3)
pub struct SerialTransport; impl SerialTransport { pub fn open(path, &PortConfig, &OpenOpts) -> io::Result<Self>; pub fn path(&self) -> &str }
pub struct SimTransport; impl SimTransport { pub fn open(&SimSpec, &PortConfig); pub fn with_device(Box<dyn Device>, Splitter, &PortConfig) }
pub fn list_ports() -> io::Result<PortList>   // PortList { ports: Vec<PortInfo> (≤256), truncated: bool }
pub struct PortInfo { path, attach: PortAttach(Usb|Pci|Bluetooth|Unknown), vid, pid, serial, manufacturer, product, interface: Option<u8>, location: Option<String /* "bus-p.p" */> }
```

### reader / bridge / clock
```rust
pub fn bridge() -> (BridgeTx, BridgeRx)
BridgeTx::push(RxChunk) -> usize /*이번 push로 버린 청크 수*/;  BridgeTx::disconnect(io::ErrorKind);  Drop = 닫힘
BridgeRx::pop(timeout) -> Result<BridgeItem, PopError /*Timeout|Closed*/>;  pop_batch(&mut Vec, max, timeout);  queued_cost();  stats() -> BridgeStats
pub enum BridgeItem { Data(RxChunk), Lost(LostRange), Disconnected(io::ErrorKind) }
pub struct RxChunk { source_seq, rx_offset, mono_ns, wall_ns, bytes: Vec<u8> }
pub struct LostRange { rx_from, rx_to /*배타*/, bytes, seq_from, seq_to /*포함*/, chunks, cause: "reader_bridge_overflow" }
Reader::spawn(Box<dyn RxHandle>, BridgeTx, Arc<SeqSource>, Arc<dyn Clock>, start_offset) -> io::Result<Reader>
Reader::{request_stop, stop() -> ReaderExit /*Stopped|Disconnected(kind)*/, is_finished};  Drop = stop+join
pub trait Clock: Send+Sync+Debug { fn mono_ns(&self) -> u64; fn wall_ns(&self) -> u64 }   // SystemClock, FixedClock
SeqSource::{starting_at, next, reserve}
```

### tx
```rust
TxActor::spawn(Box<dyn Transport>, Arc<SeqSource>, Arc<dyn Clock>, SyncSender<TxEvent>) -> io::Result<TxActor>
TxActor::{handle() -> TxHandle, shutdown(self) -> Option<Box<dyn Transport>>}    // Drop = shutdown
TxHandle::submit(TxRequest) -> Result<(), SubmitError /*QueueFull{lane,queued_bytes,queued_items}|TooLarge|Empty|Stopped*/>
TxHandle::control(ControlOp /*Dtr|Rts|Break(bool)*/, timeout) -> io::Result<()>   // actor 스레드에서 청크 경계에 실행
TxHandle::queued(lane) -> (bytes, items)
pub fn event_channel() -> (SyncSender<TxEvent>, Receiver<TxEvent>)   // 용량 TX_EVENT_CHANNEL_CAP
pub struct TxRequest { request_id: u64, lane: TxLane /*CtrlC|User|Agent*/, client_id: String, bytes: Vec<u8>, deadline: Instant }
pub enum TxEvent { Chunk(TxChunk), Done(TxDone) }
pub struct TxChunk { source_seq, lane, origin: TxOrigin /*User|Agent*/, client_id, request_id, mono_ns, bytes }
pub struct TxDone  { request_id, lane, origin, client_id, state: TxState /*Sent|Failed|OutcomeUnknown*/, written_bytes, remaining_bytes,
                     first_mono_ns: Option<u64>, last_mono_ns: Option<u64>, error: Option<TxErrorCode> }
pub enum TxErrorCode { Timeout /*E_TIMEOUT, cause "deadline"*/, InterruptedByUser /*E_INTERRUPTED, "interrupted_by_user"*/, Io(kind) /*E_IO*/, Shutdown /*E_SHUTDOWN*/ }  // .code() .cause()
```

### sim
```rust
pub trait Device: Send { fn on_open(&mut self, out); fn on_bytes(&mut self, b, now: Duration, out); fn tick(&mut self, now, out) -> Option<Duration>; fn on_signal(&mut self, Signal, out) }
pub enum Signal { Dtr(bool), Rts(bool), Break(bool) }
SimSpec::parse("sim://<bootshell|flood|binary|ansi>?seed=N&split=0|1&rate=B/s&bytes=N&frames=N") -> io::Result<SimSpec>   // 모르는 키·시나리오는 InvalidInput
SimSpec::{new(Scenario, seed), build() -> (Box<dyn Device>, Splitter)}
Runner::{start, feed, signal, tick_if_due, piece, consumed, next_wakeup, pending_len, dropped}   // 장치 구동기(SimTransport·pty 공용)
Splitter::{interactive(seed), bulk(seed), whole()};  Rng(SplitMix64)
Bootshell::new(seed), Flood::new(rate, limit){emitted,digest}, Binary::new(frames), Ansi::new()
pub const QUERY_SEQUENCES: &[QuerySeq{name, bytes}]   // DA1 DA2 DA3 XTVERSION DSR CPR DECRQM DECRQSS XTWINOPS_18T KITTY_KBD_QUERY OSC10_QUERY OSC11_QUERY
ansi_corpus(), ANSI_DONE_MARKER, LOGIN_PROMPT/PASSWORD_PROMPT/SHELL_PROMPT, BOOT_LOG_LINES
flood_bytes(rate, len) -> Vec<u8>, flood_digest(rate, len) -> u64, binary_frames(n), Fnv64{new,update,finish}
#[cfg(unix)] PtyDevice::{spawn(Box<dyn Device>, Splitter), spawn_spec(&SimSpec)} -> io::Result<PtyHandle>
#[cfg(unix)] PtyHandle { pub slave_path: PathBuf, slave_path_str(), stop() }   // Drop = 스레드 종료
```

## 3. 상수 표 (상한·정책)

| 상수 | 값 | 넘칠 때 정책 | 근거 |
|---|---|---|---|
| `BRIDGE_MAX_BYTES` | 1 MiB (payload + 청크당 `BRIDGE_CHUNK_OVERHEAD` 64 B) | reader는 기다리지 않음, 가장 오래된 미전달 청크 drop → `Lost` 범위 1개 슬롯에 합산 | §3.13 1행 |
| `BRIDGE_MAX_CHUNKS` | 256 | 위와 같음 | §3.13 1행 |
| `READ_WAIT` | 20 ms | 첫 바이트 대기, 중지 지연 상한 | §3.5 |
| `RX_CHUNK_MAX` | 16 KiB | drain 상한 | §3.5·§3.13 |
| `TX_CHUNK_MAX` | 1 KiB | 청크 경계마다 Ctrl-C > user > agent | §3.5 |
| `CTRLC_QUEUE_BYTES/ITEMS` | 64 B / 64 | `Err(QueueFull)`·`TooLarge` (조용한 drop 없음) | §3.13 TX행 |
| `USER_QUEUE_BYTES/ITEMS` | 4 KiB / 64 | 같음 | §3.13 TX행 |
| `AGENT_QUEUE_BYTES/ITEMS` | 64 KiB / 64 요청 | 같음 | §3.13 TX행 |
| (lane 계수) | 대기 + 전송 중 포함, `TxDone` 때 반환 | | §3.13 |
| `TX_CONTROL_QUEUE` | 8 | `WouldBlock` 오류 | §3.13 제어 경로 |
| `TX_EVENT_CHANNEL_CAP` | 1024 | 가득 차면 actor가 **대기**(결과를 버리지 않음) | §3.13 "쓰기 종단 drop 금지" |
| `SIM_BUFFER_BYTES` | 방향별 1 MiB | dev→host 가득: 장치 출력은 runner에 보류 / host→dev 가득: write가 deadline까지 대기 후 부분 쓰기 | 작업 지시 |
| `SIM_SIGNAL_QUEUE` | 64 | `WouldBlock` 오류 | §3.13 |
| `RUNNER_PENDING_HIGH_WATER` | 64 KiB | 넘으면 `tick` 중지(장치 쪽 속도 조절) | §9.1 |
| `RUNNER_PENDING_MAX` | 1 MiB | 입력이 일으킨 출력이 넘치면 잘라내고 `dropped()`에 계수 | §3.13 |
| `MAX_LISTED_PORTS` | 256 | 잘라내고 `truncated=true` | §7.5 |
| `SIM_TARGET_MAX` | 256 B | 거절 | |
| bootshell `LINE_MAX` / `PING_SAMPLES_MAX` | 1024 B / 4096 | BEL(0x07) / 통계 표본만 제한(횟수는 정확) | |
| flood `FLOOD_TICK_BYTES` | 16 KiB/tick | | |
| pty `PTY_READ_BUDGET` | 64 KiB/loop | | |

## 4. 시험 (59개)

- 단위(39): bridge(개수·바이트 상한 drop 범위, disconnect 순서, timeout, wakeup 합침, **proptest: 어떤 push/pop 순서든 Data+Lost가 스트림을 정확히 한 번 순서대로 덮음**), clock, reader(16 KiB drain·stamp·offset·seq, 오류 시 데이터 후 Disconnected, 중지 지연 < 60 ms), transport(설정 검증, list_ports 상한·USB 필드 매핑, 실제 열거 호출), SimTransport(DTR/RTS/break가 장치에 순서대로 도착, bootshell break 표시, Mark 거절), sim(spec 파싱, 같은 seed = 같은 바이트·같은 분할, 다른 seed = 다른 분할, **ESC 시퀀스 중간 분할 발생**, runner 상한), bootshell(부팅 로그 모양, 비밀번호 무에코, 명령, BS/^U/^C·UTF-8 BS, ping+^C 통계, reboot), flood/binary(참조 스트림·해시 일치, 속도 조절, tick 상한, loopback), ansi(질의 전부 포함, rx 보고).
- `tests/pty.rs`(6, unix): `pty_bootshell_login_uname_ping_ctrl_c`(open → root → 비밀번호 → `uname -a` → ping → Ctrl-C lane 0x03 → 통계 → 프롬프트, **transport+reader+bridge+TX actor 전체 경로**), `pty_binary_all_bytes_both_directions`(0x00–0xFF 4프레임 수신 + 256 B loopback 일치), `pty_second_open_is_refused_while_exclusive`(2차 open → `ResourceBusy`, 닫은 뒤 재open 성공), `pty_open_rejects_unsupported_settings`(Space → Unsupported), `pty_flood_4mib_hash_matches`(4 MiB FNV 일치, 오프셋 연속), `pty_flood_bridge_overflow_reports_exact_lost_ranges`(소비자를 멈춘 채 reader가 4 MiB 전부 읽게 한 뒤: Lost 1개, `rx_from`=직전 데이터 끝, Lost 바이트 = stats.dropped_bytes, 전달된 청크는 참조 스트림의 같은 위치와 바이트 일치, Data+Lost가 [0, 4 MiB) 정확히 덮음, 대기 중 queued_cost ≤ 1 MiB).
- `tests/sim_transport.rs`(5): `sim://`로 같은 bootshell 스크립트, user lane 줄 편집·모르는 명령, binary 양방향, ansi 질의 전부 + 유출된 응답이 `rx:`로 보임, 같은 seed 같은 바이트.
- `tests/tx.rs`(9): lane 상한(agent 바이트·요청 수, user 바이트·항목, Ctrl-C 항목·바이트, TooLarge, Empty), **Ctrl-C가 agent 4 KiB 송신을 청크 경계에서 끊음**(written 3072 / remaining 1024 정확, `OutcomeUnknown` + `interrupted_by_user`, agent Done이 0x03 청크보다 먼저, 이후 추가 write 없음), user 키 끼어들기(agent 취소 없이 완료), 막힌 transport(1500/3000 B → `OutcomeUnknown`+`E_TIMEOUT`, 150 ms deadline 준수 < 1 s; 0 B → `Failed`+`E_TIMEOUT`; 이미 지난 deadline은 write 없이 Failed), 같은 lane FIFO, control op 실행·오류 반환, shutdown 시 모든 수용 요청에 TxDone.
- pty flood 측정(디버그 빌드, macOS arm64): **4 MiB / 0.39 s ≈ 10.3 MiB/s**, bridge wakeup 2860회(청크마다 깨우지 않음).

마지막 실행 출력 끝:
```
     Running tests/pty.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.18s
     Running tests/sim_transport.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
     Running tests/tx.rs
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.15s
   Doc-tests loom_uart_worker
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
real 2.22
```
(단위 `test result: ok. 39 passed`.)

Windows: `cargo check -p loom-uart-worker --target x86_64-pc-windows-gnu` → **성공**(`Finished`), `cargo clippy --all-targets --target x86_64-pc-windows-gnu -- -D warnings`도 통과. 실행 시험은 M2.

## 5. 계획·지시와 다르게 한 점과 이유

1. **`Transport::rx_handle()` + `RxHandle` trait 추가.** reader 스레드와 TX actor가 동시에 I/O해야 하는데 지시의 trait은 `&mut self` 한 객체라 Mutex로 묶으면 20 ms read 대기 동안 TX가 막힌다. 장치 설정 owner는 TX actor가 쥔 `Transport` 하나, reader는 읽기 전용 핸들(Unix: 데이터 fd `dup`, Sim: 공유 버퍼, Windows: `try_clone`) — §7.3 "fd/handle은 clone해도 설정 owner는 하나". `Transport::read`도 그대로 둠.
2. **Unix 데이터 fd를 따로 연다(unsafe 금지 때문).** serialport `TTYPort`는 fd를 `RawFd`로만 내주고, `BorrowedFd`로 바꾸려면 `unsafe`가 필요하다. 그래서 순서를 `nix::fcntl::open(path, O_RDWR|O_NOCTTY|O_NONBLOCK|O_CLOEXEC)`(OwnedFd) → serialport `open_native`(TIOCEXCL + flock + raw termios + 회선 설정, DTR 건드리지 않음) → 명시 DTR/RTS → nonblocking 불변식 확인으로 했다. 우리 fd가 배타 잠금 **전에** 열렸으므로 이후의 다른 open만 막힌다. 데이터 I/O는 이 fd로 `poll` + nonblocking + 부분 처리 + 절대 deadline, DTR/RTS/break·termios는 `TTYPort`로. 같은 장치에 open이 2개 생기는 점(같은 프로세스)은 Linux 실장치 DTR 파형 gate에서 확인 필요.
3. **2차 open 오류를 `ResourceBusy`로 바꿈.** macOS pty에서는 TIOCEXCL이 아니라 serialport의 flock이 2차 open을 막았고, serialport는 이를 `NoDevice`(→ `NotFound`)로 돌려준다. 우리 open이 먼저 성공했으니 경로는 존재하므로 `ResourceBusy`로 바꿔 E_BUSY/E_NOT_FOUND가 섞이지 않게 했다.
4. **`sim_pty` baud 처리: "실패 무시" 대신 macOS에서 baud ioctl을 아예 건너뜀(serialport에 baud 0).** 실패 후 재시도하면 처음 시도가 이미 TIOCEXCL을 걸어 둔 상태(우리 데이터 fd가 열려 있어 해제되지 않음)라 재open이 실패한다. 효과는 같다: sim이 발급한 pty에서 macOS ENOTTY만 피함. Linux는 sim_pty여도 실제 baud를 설정(pty가 받아들임). 실 장치(`sim_pty=false`)에는 절대 적용하지 않는다.
5. **`list_ports()` 반환형** `Vec<PortInfo>` → `io::Result<PortList{ports, truncated}>`(잘림 표시·열거 오류 공개).
6. **Mark/Space·1.5 stop은 SimTransport에서도 거절**(어느 경로든 같은 의미). 하드웨어 흐름 제어 중 수동 RTS는 `InvalidInput`(§7.2 "충돌 검증/거절"). Windows `exclusive=false`는 무시하지 않고 `Unsupported`.
7. **flood 해시는 SHA-256 대신 FNV-1a 64**(`Fnv64`, `flood_digest`). 새 의존(sha2 + 전이 의존 6개)을 넣지 않기 위함. 무결성은 해시 외에 오버플로 시험에서 참조 스트림과 바이트 단위로도 비교한다. 암호학적 성질은 필요 없음.
8. **flood의 `t=<ns>`는 시계값이 아니라 그 줄의 명목 송출 시각**(`offset × 1e9 / rate`, unthrottled면 0). "같은 seed = 같은 바이트"를 지키면서 지연 측정 기준으로도 쓸 수 있게(실제 송출은 명목 시각 이후). 시험용 매개변수 `bytes=`(총량 제한)와 공통 `split=0`(분할 끔)을 추가.
9. **Ctrl-C는 진행 중인 agent 요청 하나만 취소**하고 아직 시작 안 한 agent 대기 요청은 그대로 둔다(§3.7 표 "다음 청크 경계에서 에이전트 잔여분 취소"를 진행 중 요청으로 해석). 대기 요청 보류는 P1 InputArbiter(입력권/hold)의 몫으로 봄 → 질문 1.
10. **user 일반 키는 agent를 취소하지 않고 청크 경계에 끼어든다**(지시대로). 계획 §3.5 마지막 줄("사용자 바이트가 오면 에이전트 잔여분 취소")과 다르지만, 취소는 Ctrl-C만 하는 것이 맞다고 판단: 일반 키가 agent 대용량 송신을 매번 깨면 부분 송신(`outcome_unknown`)이 쉽게 생기고, 사용자 줄 보호는 입력권(§3.7-2·4)이 이미 맡는다. 또 user 요청 진행 중 Ctrl-C가 오면 청크 경계에서 Ctrl-C를 먼저 보내되 user 요청은 취소하지 않는다(수용한 사용자 키를 버리지 않음 §3.13).
11. **TX actor에 control 큐 추가**(`TxHandle::control`): actor가 transport를 독점하므로 DTR/RTS/break도 actor 스레드에서 청크 경계에 실행하고 오류를 돌려준다. break 길이 타이머(set→timer→clear)는 P1 몫.
12. **TX 이벤트는 bounded `sync_channel`, 가득 차면 actor가 대기**(버리지 않음). 소비자가 멈추면 TX도 멈춘다 — 결과 유실보다 낫다고 판단(§3.13 "감사/쓰기 종단 조용히 drop 금지"). 오류 코드 문자열 `E_TIMEOUT/E_INTERRUPTED/E_IO/E_SHUTDOWN`은 임시 → 질문 2.
13. **bridge 바이트 계수에 청크당 64 B 메타데이터 포함**(§3.13 "payload뿐 아니라 메타데이터도 계수"). `LostRange`의 `seq_from..=seq_to` 사이에는 TX 청크 seq도 섞일 수 있음(RX·TX가 seq 공간 공유) — 문서 주석에 명시.
14. **`mono_ns`는 `SystemClock` 생성 시점 기준 상대값**. P1에서 core.time `clock_id`와 맞춰야 함.
15. **pty 하네스는 slave fd 하나를 수명 내내 연 채 유지**(Linux에서 slave가 모두 닫히면 master가 EIO/HUP로 바쁜 루프가 됨). bootshell: 비밀번호는 아무 값이나 통과, root 외 사용자는 `Login incorrect`, `echo` 명령 추가, break 신호 → `\r\n<BREAK>\r\n`(§9.2 YAML 예). 분할: interactive(bootshell·ansi) 1–48 B 조각 + 약 1/12 확률로 1 ms 쉼, bulk(flood·binary) 1–4096 B.
16. `loom-proto`·`serde`·`serde_json` 의존은 P1용으로 남겨 둠(현재 미사용).

## 6. 알려진 한계 (M2 이후)

- Windows: 복제한 COM 핸들의 COMMTIMEOUTS는 장치 단위라 reader의 read timeout과 TX write timeout이 서로 덮어쓴다(코드에 `NOTE(M2)`). Windows gate에서 별도 설계 필요(예: reader를 짧은 고정 timeout으로 두고 TX는 남은 예산을 슬라이스로 나눔).
- CTS stall 실측(§7.3)은 HW gate 전까지 미완료. 가짜 막힘 transport로 deadline 의미만 검증.
- 같은 tty를 두 번 연 상태의 Linux 실장치 DTR/HUPCL 동작은 미검증(편차 2).

## 7. 리드에게 묻는 것

1. Ctrl-C가 오면 **대기 중인(아직 시작 안 한) agent 요청**도 `interrupted_by_user`로 함께 끝낼까? 지금은 진행 중 요청만 취소한다. P1 InputArbiter가 Ctrl-C 직후 입력권을 user로 돌려 대기 요청을 보류하면 지금 방식으로 충분하다.
2. `TxErrorCode` 문자열(`E_TIMEOUT`, `E_INTERRUPTED`, `E_IO`, `E_SHUTDOWN`)과 cause(`deadline`, `interrupted_by_user`, `io_error`, `shutdown`)를 contract 오류 코드표에 맞출 이름이 있으면 알려 달라.
3. TX 이벤트 채널 가득 참 → actor 대기(편차 12)가 P1 worker 루프 구조와 맞는지.
4. `client_id`는 `String`으로 뒀다(contract hello의 문자열 client_id 가정). 정수 id로 바꿀 계획이면 알려 달라.
