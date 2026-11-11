# UART v2.0 삽입 문안 — Sol: Rust·플랫폼·C2

## 0. 적용 전제

`docs/plan/02-core-principle.md` 최우선, 마스터 C > B > A > D. **[제안] UART Python 0**: CLI·장치 경로·공유 콘솔·시험 하네스 모두 Rust로 전환한다. 에이전트의 직접 open/read/send와 사용자의 동일 스트림 표시·개입은 유지한다. 입력권·`input_dirty`·단일 writer·사용자 Ctrl-C 우선·STOP 사용자 예외·엄격 신원·세대/커서·위험 매핑·공통 쓰기 상태·손실 3층은 언어 교체로 바꾸지 않는다.

**[제안] M1부터 웹 Loom UI가 1차 화면**이다(C6). UART는 패널 내용·질의·입력/승인 연결 명세를, UI 팀은 공통 셸·패널 구현을 소유한다. 같은 WS 스트림의 RX/TX 출처·backlog→live·gap·env·개입을 웹에서 검증한다. TUI는 보조이며 M1 웹 UI를 대체하지 않는다. Rust serde 정본→JSON Schema→TS 생성, Registry/worker 프로토콜·crate 경계는 CORE 결정에 따른다(C3). 아래 수치·구조는 구현 전 **[제안]**, 실제 조회·소스·실험은 **[확인]**으로 구분한다.

## 3.13 C2: 큐·버퍼와 손실의 경계

**[제안] 포트당 worker 프로세스 1개**, 최대 8개. v1.2의 모든 UART 포트를 한 worker에 넣는 구조를 바꾼다. 이유: 그 구조에서는 worker 하나의 크래시로 다른 UART 채널도 끊겨 C2의 다른 채널 격리를 만족하지 못한다. CORE supervisor의 worker 인스턴스 모델과 P0에 합의할 가정이다. gateway에 Transport를 넣지 않는다. 원본의 정본은 CORE 디스크 저널이고 `chunks.jsonl`은 sidecar다.

상한은 payload뿐 아니라 대기 중인 인코딩 결과·메타데이터·공유 참조가 붙잡는 메모리를 포함해 계수한다. byte permit과 항목 수를 **동시에** 제한한다. 항목만 제한한 `mpsc`는 불충분하다. 청크 ≤16 KiB, 배치 ≤10 ms. 다음 값은 UART 몫이며 CAN/UDS 큐와 별도 계정이다.

| 경로/소유자 | 초기 상한 [제안] | 포화 동작 | 표시·판정 |
|---|---|---|---|
| reader→채널 reducer / worker | 1 MiB 또는 256청크 | reader는 기다리지 않고 오래된 미전달 RX를 drop. 바이트 offset·손실량은 고정 슬롯에 합산 | **`record.gap` + `display.gap`**, cause=`reader_bridge_overflow`. 이미 호스트가 읽었으므로 `source.gap`으로 바꾸지 않음 |
| worker→gateway 데이터 IPC / worker | 2 MiB 또는 512이벤트/채널 | 오래된 미전달 데이터 drop, producer seq/offset 손실 범위 합산. 링에서의 무제한 재전송 없음 | gateway 저널에도 못 간 범위는 `record.gap`, 라이브는 `display.gap`. worker 링에 남았더라도 기록 성공으로 표시하지 않음 |
| 제어·gap·상태 IPC / 양쪽 | 방향별 256 KiB·256항목/worker | gap은 범위/바이트 수 합산, 상태는 키별 최신값으로 coalesce. 감사/쓰기 종단은 조용히 drop하지 않음 | STOP·취소·사용자 입력에 별도 우선 경로. 감사 수용 불능이면 새 agent/script R1+를 차단(A12), 사용자 입력 예외·데몬 로그 유지 |
| CORE 저널 데이터 큐 / gateway | UART 채널당 8 MiB·2,048항목, 합계 64 MiB | 디스크 writer로 spill; writer가 못 따라오면 새 데이터 drop, 손실 범위 보존. 디스크 작업으로 reader/팬아웃을 block하지 않음 | `record.health=degraded` + `record.gap`; 디스크 full/I/O 실패는 `failed` + A12. 감사/제어 예약분은 CORE 별도 계약 |
| live 구독 / gateway | 구독당 **인코딩 후** 2 MiB·512항목, UART 데이터 구독 전체 8개 | 해당 구독의 오래된 미전송 데이터 drop; OS/WS send도 deadline·유한 버퍼 | 그 구독만 `display.gap{cause:slow_client}`. 실제 저널 범위가 온전할 때만 `record:ok` |
| backlog 링 / worker | payload 16 MiB 또는 32,768이벤트, 메타데이터 pool 4 MiB; 8채널 payload 합계 128 MiB | 가장 오래된 것부터 제거. 과거는 유한 디스크 질의 | 정상 보존기간 만료 자체는 기록 손실 아님. 오래된 커서는 만료/재개 위치 안내, 라이브 skip과 구별 |
| expect / worker | 공유 창 64 KiB, 대기 8개·컴파일 패턴 **총 8개/채널**, 작업 큐 16개 | admission 거절, wakeup coalesce. 대기마다 창 복제 금지. regex 예약 8 MiB/채널 | 상한·스캔 예산 초과는 `E_SCAN_LIMIT` 계열(exit 8), timeout(exit 3)과 구별; `resume_from` 제공 |
| TX/InputArbiter / worker | agent 64 KiB·64요청, user 4 KiB·64항목, Ctrl-C 별도 64 B·64항목 | agent 초과 거절; 사용자 입력은 credit으로 보류 표시, 수용한 키의 조용한 drop/자동 재송신 금지. Ctrl-C 예약분도 초과하면 명시 backpressure/미수용 응답 | `E_RATE_LIMIT`/입력 보류, partial은 `outcome_unknown + written_bytes`. 이미 수용한 입력의 결과 추적 |
| 파일 업로드·과거 스캔 / CORE+worker | 파일 기본 1 MiB·명시 hard 16 MiB, 동시 1개/채널; RAM 청크 16 KiB | 파일은 quota 있는 임시 디스크로 streaming, 크기/해시 검증 후 승인. 스캔도 byte/deadline/cancel 상한 | 파일 전체를 큐에 복사하지 않음. quota/상한 초과 거절, scan 초과는 이어 읽을 위치 반환 |
| 텍스트·UI | 부분 줄 64 KiB, escape 4 KiB/채널; 웹 수신 대기 2 MiB/패널, 스크롤백 payload 16 MiB 또는 32,768항목 | 줄/escape는 절단·차단, UI는 오래된 표시분 drop/가상화 | text truncation은 메타 필드, UI skip은 `display.gap`; 원시 기록과 구별. TS 문자열·DOM 포함 실제 브라우저 메모리 예산은 UI 팀과 확정 |

v1.2의 “reader 브리지가 가득 차면 기록 큐로 우선 전달해 표시만 손실”은 **대체한다**. reader 뒤 단일 경로가 막히면 디스크로의 전달도 보장할 수 없다. 이를 숨기지 않고 record/display 손실로 계수한다. `source.gap`은 USB 단절·확인 가능한 드라이버 오버런 등 **Loom 수신 전** 공백에만 쓴다. drop 범위가 아직 stream seq를 받지 않았다면 producer seq/바이트 offset으로 보고하고, 존재하지 않는 stream seq 범위를 만들지 않는다. gap 메타데이터도 고정 pool이며, 비연속 범위 상세가 포화되면 알려진 손실량·전체 경계와 `detail_incomplete`를 보고한다(전체 경계 내부가 전부 손실됐다고 주장하지 않음). gap 뒤 매처·UTF-8·escape 상태를 재동기화한다.

**[제안] 메모리 admission/복구**: worker당 데이터 예약 약 32 MiB(링 payload 16 + metadata 4 + bridge 1 + IPC 2 + regex/scratch 8 + 나머지 약 1), 런타임 여유를 합쳐 RSS 목표 **48 MiB**, hard **64 MiB**로 시작한다. regex 두 builder limit은 전체 heap의 엄격한 상한이 아니므로 이 계산은 **[추정]**이며 P0에서 실제 최대치로 조정한다. 8 workers hard 합계 512 MiB, gateway의 UART 큐·인코더·구독 계정은 추가 128 MiB 이내를 제안한다(전체 loomd/다른 도메인 예산은 CORE 소유). payload 링을 gateway에 또 복제하지 않는다.

soft 초과 시 새 채널/expect/scan admission 중지→종료한 작업·cache 반환→경고한다. RX/사용자 입력을 임의로 정지시키지 않는다. 지속 hard 초과/응답 불능은 supervisor가 **그 worker만** 종료하고 오류·기록 경계를 알린다. Linux cgroup/Windows Job 제한 및 플랫폼별 RSS 계측은 CORE 담당; 주기적 RSS 감시만으로 순간 최대 할당까지 제한했다고 주장하지 않는다. 호스트 전체 예산이 부족하면 신규 open을 거절하고 기존 큐 크기를 몰래 늘리지 않는다.

**[제안] 크래시 격리**: gateway·CORE 저널 writer·CAN/UDS·다른 UART worker는 계속 동작한다. worker→gateway 사이의 미전달 원본은 손실 가능, gateway 수용분도 `durable_through` 이전만 내구성이 확정된다. 마지막 watermark 이후 범위는 모르면 unknown으로 공개한다. worker 재기동은 가능하되 **포트 자동 재open/미완료 TX 재송신 금지**(DTR reset 위험). 채널·뷰어·커서·마크는 gateway에 유지하고 실패 상태를 즉시 방송한다. 새 worker는 새 `producer_epoch`; 사용자가 명시적으로 resume/open할 때 신원·정책을 재검사하고 새 generation으로 연다. 살아 있는 worker의 기존 USB 재연결 정책과 크래시 복구는 구별한다.

## 7. Rust 라이브러리 선택

### 7.1 실제 배포·유지 상태

**[확인: 2026-11-10]** crates.io API에서 non-yanked 정식 버전의 `created_at`·license를 조회했다. serialport/mio-serial/tokio-serial/regex/vte/udev는 배포 소스도 대조하고 나머지 API 판단은 아래 정본 문서에 근거한다. 날짜는 UTC. 아래 저장소는 조회 당시 모두 non-archived; “push”는 저장소 API `pushed_at`이며 해당 하위 crate의 유지보수 보증은 아니다. 최종 workspace lock·MSRV·전이 라이선스 검사는 CORE 빌드 단계에 남긴다. `serialport`의 MPL-2.0 고지는 MIT로 바꿔 적지 않는다.

| 용도·crate(정본 링크) | 버전 / 배포일 | SPDX | 저장소 마지막 push / 판단 |
|---|---|---|---|
| [serialport](https://crates.io/crates/serialport/4.10.1) | 4.10.1 / 2026-09-08 | MPL-2.0 | [2026-09-08](https://github.com/serialport/serialport-rs), **채택**. README에 특히 Windows maintainer 모집 명시; Windows gate 필수 |
| [tokio-serial](https://crates.io/crates/tokio-serial/5.5.0) | 5.5.0 / 2026-06-15 | MIT | [2026-06-15](https://github.com/berkowski/tokio-serial), async 후보, M1 기본 아님 |
| [mio-serial](https://crates.io/crates/mio-serial/5.0.7) | 5.0.7 / 2026-06-15 | MIT | [2026-06-15](https://github.com/berkowski/mio-serial), tokio-serial 기반·nonblocking 후보 |
| [nix](https://crates.io/crates/nix/0.31.3) | 0.31.3 / 2026-05-11 | MIT | [2026-11-09](https://github.com/nix-rust/nix), Unix PTY·fd 어댑터에 채택, `term`/`fs` 기능 |
| [portable-pty](https://crates.io/crates/portable-pty/0.9.0) | 0.9.0 / 2025-02-11 | MIT | [2026-11-08](https://github.com/wezterm/wezterm)(모노레포), 이번 UART에는 불필요 |
| [regex](https://crates.io/crates/regex/1.13.1) | 1.13.1 / 2026-07-15 | MIT OR Apache-2.0 | [2026-11-07](https://github.com/rust-lang/regex), 채택; Python regex timeout 설계 교체 |
| [memchr](https://crates.io/crates/memchr/2.8.3) | 2.8.3 / 2026-07-08 | Unlicense OR MIT | [2026-08-10](https://github.com/BurntSushi/memchr), literal `memmem::Finder` 채택 |
| [vte](https://crates.io/crates/vte/0.15.0) | 0.15.0 / 2025-02-02 | Apache-2.0 OR MIT | [2026-10-25](https://github.com/alacritty/vte), 제한된 escape 파서에 채택 제안; 터미널 에뮬레이터 아님 |
| [crossterm](https://crates.io/crates/crossterm/0.29.0) | 0.29.0 / 2025-04-05 | MIT | [2026-11-01](https://github.com/crossterm-rs/crossterm), 보조 TUI raw input/terminal 복구 |
| [ratatui](https://crates.io/crates/ratatui/0.30.2) | 0.30.2 / 2026-06-19 | MIT | [2026-11-09](https://github.com/ratatui/ratatui), 선택; 웹 M1의 의존 아님 |
| [proptest](https://crates.io/crates/proptest/1.12.0) | 1.12.0 / 2026-10-10 | MIT OR Apache-2.0 | [2026-10-10](https://github.com/proptest-rs/proptest), 생성형 불변식 시험 |
| [cargo-nextest](https://crates.io/crates/cargo-nextest/0.9.151) | 0.9.151 / 2026-11-03 | Apache-2.0 OR MIT | [2026-11-08](https://github.com/nextest-rs/nextest), CI 선택; `cargo test`가 기본 |
| [udev](https://crates.io/crates/udev/0.9.3) | 0.9.3 / 2025-01-23 | MIT | [2025-01-23](https://github.com/Smithay/udev-rs), Linux 이벤트 최적화 선택; 필수 경로로 두지 않음 |
| [windows-sys](https://crates.io/crates/windows-sys/0.61.2) | 0.61.2 / 2025-10-06 | MIT OR Apache-2.0 | [2026-11-09](https://github.com/microsoft/windows-rs)(모노레포), native DCB/장치 알림 확장용 |

### 7.2 시리얼 API에서 확인한 지원과 빈틈

**[확인]** `serialport`는 Windows COM·Linux tty·macOS TTY용 blocking I/O와 열거를 제공한다. 실 HW 지원은 Windows x64/Linux x64만, macOS는 개발/시뮬레이션(B1). `tokio-serial`/`mio-serial`도 설정 타입을 공유하므로 enum에 없는 기능을 자동 추가하지 않는다. 근거: [SerialPort](https://docs.rs/serialport/4.10.1/serialport/trait.SerialPort.html), [builder/enum/USB 배포 소스](https://docs.rs/crate/serialport/4.10.1/source/src/lib.rs).

| 항목 | 확인한 동작 / v2 적용 |
|---|---|
| baud/data/flow | baud `u32`, data 5/6/7/8, flow none/software/hardware. 임의 baud의 실제 적용은 OS·칩·드라이버에 따름; 3 Mbaud를 API 존재만으로 지원 선언하지 않음 |
| parity/stop | enum은 **None/Odd/Even, One/Two만**. Windows에서도 1.5 stop 및 Mark/Space는 이 crate API로 불가. v1.2 범위를 조용히 축소하지 않고 아래 native 확장 작업으로 분리 |
| DTR/RTS/break | `write_data_terminal_ready`, `write_request_to_send`, `set_break`/`clear_break` 있음. flow hardware 중 수동 RTS는 충돌을 검증/거절. break는 set→host timer→clear, finally에도 clear 시도; 실제 선로 길이·clear 실패는 별도 기록/오류 |
| Unix 배타 | **기본 `exclusive=true`: `TIOCEXCL` + exclusive `flock`**. pyserial의 flock-only 설명을 폐기. open 전 `.exclusive(false)` 또는 native `TTYPort::set_exclusive(false)`로 해제 가능하나 production은 기본 배타 유지. 커널·권한·이미 열린 fd까지 막는다는 보장 없음 |
| Windows 배타 | `CreateFileW` share mode 0. `serialport` 자체는 `FILE_FLAG_OVERLAPPED` 없이 synchronous `ReadFile`/`WriteFile` |
| timeout | builder 기본 **0 ms**, 따라서 항상 명시. 단일 `timeout`이 read와 write에 적용되고 별도 `write_timeout` API는 없음. 부분 read/write 가능; `read_exact`/`write_all`의 여러 호출 전체에 대한 deadline은 앱이 관리 |
| USB | `UsbPortInfo`: vid/pid, optional serial_number/manufacturer/product. **`usbportinfo-interface`**, **`usbportinfo-location`** 기능을 명시해야 interface/location 필드가 존재. interface도 optional이며 Windows/Linux의 communication 번호와 macOS의 data 번호가 다를 수 있음 |
| libudev | 기본 기능 `libudev`는 glibc Linux에서 외부 libudev 사용. `default-features=false`면 sysfs 열거 경로, musl도 해당 경로. 배포 소스에는 sysfs VID/PID/serial/manufacturer/product/interface/location 읽기가 있으나 누락·필터 차이가 있으므로 실제 장치 정합성 gate 필요 |

근거 소스: [Unix open·I/O·break](https://docs.rs/crate/serialport/4.10.1/source/src/posix/tty.rs), [Windows COM](https://docs.rs/crate/serialport/4.10.1/source/src/windows/com.rs), [Linux/macOS 열거](https://docs.rs/crate/serialport/4.10.1/source/src/posix/enumerate.rs), [Windows 열거](https://docs.rs/crate/serialport/4.10.1/source/src/windows/enumerate.rs), [기능/target 의존성](https://docs.rs/crate/serialport/4.10.1/source/Cargo.toml).

**[제안] 의존 설정**: `serialport = { version = "4.10.1", default-features = false, features = ["usbportinfo-interface", "usbportinfo-location"] }`. 폴링+sysfs로 시작해 Linux 단일 바이너리에 libudev 동적 의존을 강제하지 않는다. `udev` monitor를 켠 배포는 별도 동적 의존을 문서화한다. “Rust 정적 빌드”만으로 libudev가 자동 정적 링크되는 것은 아니다. musl 빌드/실장치 열거는 아직 **미검증**이다. 위치는 기존처럼 사전 bind에만 쓰며 신원 누락·복수 후보는 자동 TX 금지다.

**[제안] v1.2 기능 보존용 작은 native 확장**: Windows는 `windows-sys` DCB의 `MARKPARITY`/`SPACEPARITY`/`ONE5STOPBITS`, Linux는 해당 target의 `CMSPAR`를 별도 어댑터에서 설정·readback한다. API의 존재와 칩 지원을 구별하고 실패는 `E_UNSUPPORTED_PARAM`, 조용한 대체 금지. Windows의 8250 조합 제약(5 data+2 stop 불가, 6–8 data+1.5 stop 불가)을 검사한다. macOS Mark/Space 및 POSIX 1.5는 검증 전 거절. serialport getter가 native 확장값을 해석 못 하므로 `status/configure`도 어댑터의 readback을 사용해야 한다. Windows 상수/제약은 [Microsoft DCB 문서](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-dcb)에서 **[확인]**; 확장 구현·Linux CMSPAR/물리 동작은 **미검증 작업**이다.

### 7.3 open 순서·I/O 방식 결정

**[확인] DTR을 open 이전에 보장하는 builder가 아니다.** POSIX는 `open`→배타/raw/termios/baud→선택적 DTR이고, Windows는 `CreateFileW`→DCB 초기화/적용→선택적 DTR→timeout이다. DTR builder 기본은 `None`이며 **DTR 설정 오류를 양쪽 모두 best-effort로 무시**한다. Windows `dcb::init`은 DTR Disable, flow none은 RTS Disable을 적용하므로 `preserve_dtr_on_open()`도 물리 선 상태 보존 보증으로 쓰지 않는다. `rts_on_open` builder는 없다. 근거: 위 Unix/Windows 소스 및 [Windows DCB 구현](https://docs.rs/crate/serialport/4.10.1/source/src/windows/dcb.rs).

**[제안]** 프로필 `open_dtr/open_rts`는 open 뒤 명시적으로 적용하고 실패를 사용자/에이전트 모두에게 공개한다. builder 성공만으로 선 설정 성공이라고 답하지 않는다. Linux open의 순간 DTR assertion은 upstream 문서에도 경고가 있다. reset_sensitive 경고, close의 HUPCL, 재열거, Saleae 파형 gate는 유지한다. PTY의 baud-zero 예외를 실제 tty에 fallback하지 않는다.

**[확인] async는 Windows 폴링으로만 구현된 것이 아니다.** `mio-serial 5.0.7`은 Windows에서 synchronous 포트를 먼저 열고 설정을 읽은 다음 **닫고 `FILE_FLAG_OVERLAPPED`로 다시 열어** 설정을 복원한다. `tokio-serial`은 이를 감싸 Unix `AsyncFd`, Windows `NamedPipeClient` 경로를 사용한다. 두 crate의 `set_timeout`은 성공 반환하지만 **무시**되므로 async는 `tokio::time::timeout` 등 앱 deadline이 필요하다. timeout으로 future를 버렸다고 이미 쓴 바이트가 취소/미송신으로 확정되는 것도 아니다. 근거: [mio 소스](https://docs.rs/crate/mio-serial/5.0.7/source/src/lib.rs), [tokio 소스](https://docs.rs/crate/tokio-serial/5.5.0/source/src/lib.rs).

**[제안] M1 기본은 reader 스레드 + 단일 TX actor**로 하며 GIL 회피라는 이전 이유는 삭제한다. 이유는 한 번의 open·명확한 바이트 수/청크 중재·작은 이행 범위다. read/write용 fd/handle은 clone해도 장치 설정 owner는 하나, timeout 변경과 configure는 직렬화한다. Windows는 명시적 유한 `COMMTIMEOUTS`를 쓰고 0바이트/부분 write와 wall deadline을 처리한다.

**[확인→제안] POSIX timeout 보강은 필수**: `serialport` write는 poll 후 **blocking fd**에 write하므로 readiness timeout을 write syscall 전체의 hard deadline으로 간주할 수 없다. Unix native 어댑터가 open/configure 후 `O_NONBLOCK`을 유지하고 readiness + 부분 write + `WouldBlock` + 절대 deadline을 처리하도록 한다(읽기도 `WouldBlock` 허용). read 대기 20 ms, TX 청크 ≤1 KiB; 설정 변경 뒤에도 nonblocking 불변식을 확인한다. 이는 소스에서 도출한 설계이며 CTS stall 실측 전 완료 표시하지 않는다. `flush()`도 POSIX `tcdrain`/Windows `FlushFileBuffers`를 호출하므로 send 성공 조건에 넣지 않는다. `sent`=driver 수용, wire 완료 아님을 유지한다. async 전환은 8포트 CPU/지연 및 Windows double-open 파형 gate 이후 결정한다.

### 7.4 expect: timeout 엔진 대신 제한된 Rust 검색

**[확인]** Rust `regex`의 단일 `find`/`is_match`는 **O(m×n)**(m=확장된 패턴 크기, n=검색 바이트 수)이며 n에 대해서 선형이다. backreference/look-around를 지원하지 않는다. `find_iter`/`captures_iter` 전체 열거는 최악 O(m×n²)이므로 “모든 regex 작업이 선형”이라고 쓰지 않는다. `size_limit`은 컴파일된 regex의 **근사** 크기, `dfa_size_limit`은 lazy DFA cache의 **근사** 용량이며 프로세스 전체 RAM/wall timeout이 아니다. [정본 설명](https://docs.rs/regex/1.13.1/regex/#untrusted-input), [builder 제한](https://docs.rs/regex/1.13.1/regex/struct.RegexBuilder.html#method.size_limit).

**[제안]** Python `regex(timeout, concurrent=True)`와 kill 가능한 Python matcher 프로세스는 제거한다. Rust 전용 bounded matcher 스레드에서 단일 `find`를 수행하며 장치 reader/TX/gateway 루프를 막지 않는다. 패턴 UTF-8 2 KiB, `size_limit(256 KiB)`, `dfa_size_limit(256 KiB)`, `nest_limit(64)`, 창 64 KiB, 채널 총 8패턴, compile 동시 1개, 작업 큐 16개로 시작한다. 지원하지 않는 문법은 요청 시 명시 오류로 반환하고 Python/PCRE로 우회하지 않는다. 기존 프로필 regex를 전부 재컴파일하는 이행 gate를 둔다.

단일 검색을 중간에 preempt하는 API는 없으므로 outer timeout이 hard cancellation을 보장한다고 쓰지 않는다. compile/search 사이 deadline·취소 확인, 10 ms coalescing, byte/작업 예산으로 제한하고 초과는 `E_SCAN_LIMIT + resume_from`. 소수 병적 패턴의 실험값을 모든 허용 패턴의 최대 지연으로 일반화하지 않는다. `grep`도 match 수·스캔 바이트·deadline을 제한한다. literal 기본은 [memchr::memmem::Finder](https://docs.rs/memchr/2.8.3/memchr/memmem/index.html); 청크 이음매는 needle 길이−1만 겹친다. regex는 창 처음부터 재검색하며 창 밖 무손실 약속 없음, gap/세대 경계 리셋 등 DR-33을 유지한다.

### 7.5 텍스트·escape·핫플러그·보조 화면·시험

- **[제안] 점진 UTF-8**: `std::str::from_utf8`로 유효 prefix를 내보내고 청크 끝 미완성 최대 3바이트만 보유한다. 확정 invalid byte는 기존 `\\xNN` 표시, 원시 바이트·offset은 보존한다. 청크마다 `from_utf8_lossy`를 독립 호출해 분할 문자를 깨뜨리지 않는다. latin-1은 직접 단일 바이트 매핑; 그 밖의 인코딩은 별도 요구·crate 검증 전 capability로 약속하지 않는다.
- **[확인→제안] vte의 기본 `std` 기능은 OSC를 `Vec`에 모아 무한 미종결 OSC에서 커질 수 있다.** `default-features=false`, `Parser::<4096>::new_with_size()`의 고정 OSC 버퍼를 사용한다. 추가로 파서 **앞**에서 모든 escape/string 길이 4 KiB를 제한한다(의존 feature 통합으로 std가 켜져도 상한 유지). 초과 시 terminator까지 버리는 유한 상태, 파서 리셋·표시 경고; OSC52/클립보드/제목/링크와 미허용 제어는 실행하지 않는다. 파싱 자체는 sanitizer 정책이 아니다. UTF-8 invalid 표시와 escape filtering의 순서·원시 offset 매핑을 shared console 시험으로 고정한다. [vte 배포 소스](https://docs.rs/crate/vte/0.15.0/source/src/lib.rs).
- **[제안] 핫플러그**: M1–M2는 단일 watcher가 1 s 폴링(끊긴 채널 탐색 중 250 ms), 스캔 중복 실행 없음·enumeration 결과 최대 256포트. 초과 시 경고하고 불완전 목록으로 자동 재bind하지 않는다. native 알림은 재열거를 깨우는 힌트만 된다. Linux [udev MonitorBuilder](https://docs.rs/udev/0.9.3/udev/struct.MonitorBuilder.html)는 선택, Windows는 `windows-sys`의 [CM_Register_Notification](https://docs.rs/windows-sys/0.61.2/windows_sys/Win32/Devices/DeviceAndDriverInstallation/fn.CM_Register_Notification.html) 후보. [Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_register_notification)는 Windows 8+·기존 장치 별도 열거·callback 비차단을 명시한다. 콜백→고정 dirty flag만, 장치별 무한 큐 금지. 신원 판정은 기존 strict matcher가 수행한다.
- **[확인→제안] 보조 TUI**: [crossterm raw mode](https://docs.rs/crossterm/0.29.0/crossterm/terminal/index.html)가 Ctrl-C의 OS 처리를 끄고 raw key를 읽게 한다. 실제 UART 바이트 매핑·paste·Ctrl-]·Windows Terminal 동작은 별도 gate다. ratatui는 복잡한 보조 TUI를 만들 때만 선택; 웹 UI 프레임워크는 UI 팀 결정이다.
- **[제안] 시험 기반**: `cargo test` + proptest를 기본, nextest는 CORE CI에 이미 준비됐을 때만 선택한다. proptest 대상은 임의 청크 분할의 UTF-8/escape 동등성, seq·cursor 이음매, gap 경계 매처 reset, partial TX/사용자 중재 불변식이다. 패널 검증은 같은 증거 seq의 agent 결과와 실제 웹 표시·개입을 확인한다. C7의 자연어/표 TC→agent `loom` 실행→Loom 증거 인용 검증→HTML/JUnit은 EXPLORE/CORE와 연결하고 TC마다 Rust 코드를 새로 쓰게 하지 않는다.

### 7.6 Python 의존 제거표

| v1.2 | v2 대체 [제안] |
|---|---|
| pyserial·list_ports·pyserial-asyncio | serialport + 제한된 Unix/Windows native 어댑터, polling watcher; async는 선택 |
| Python regex·timeout worker | Rust regex + 크기/창/개수/작업 예산, memchr literal |
| asyncio·thread bridge | Rust reader/TX/matcher + byte/count bounded channel; CORE executor/IPC는 CORE 결정 |
| Textual·prompt_toolkit·termios/Win32 Python 입력 | 웹 Loom UI 1차(UI 팀), 보조 CLI에 crossterm, 선택 ratatui |
| pytest·pytest-asyncio·hypothesis·pexpect·os.openpty | cargo test/proptest, nix openpty + Rust sim/CLI 프로세스 하네스; 선택 nextest |
| Python json·TOML·entry point | CORE serde 정본/스키마·Rust TOML 처리·Rust Registry/worker 등록(C3), Python entry point 폐기 |
| pyte 등 전체 VT 화면 | 현재 불필요. vte + bounded sanitizer, 완전 화면 모델은 요구 확인 뒤 별도 검증 |

**UART Python 0.** 장비 접근·뷰어·시뮬레이션·자동시험 어느 곳에도 Python adapter가 필요하다는 근거가 없다. RFC2217/pyserial URL handler와 XMODEM/YMODEM은 위 crate에 포함되지 않으므로 자동 기능 승계로 표시하지 않는다. UART M1–M3의 로컬 tty/COM·PTY·`sim://`를 먼저 완료하고, 기존 후순위 전송/원격 기능은 별도 Rust 실현성 검증 전 비지원 capability로 둔다.

## 8.2 Rust 전환 후 OS 관문

**[제안]** Windows/Linux 실 HW 표·권한·udev 별칭·ModemManager/brltty 간섭·재열거·strict identity·Saleae 파형 관문은 유지한다. 달라지는 검증은 (1) Linux 기본 libudev 없는 sysfs 신원 누락/복합 USB interface (2) TIOCEXCL 실제 두 번째 open 차단 (3) native parity/1.5 stop readback (4) open/close DTR/RTS (5) CTS low에서 finite write·Ctrl-C 반응·부분 바이트 수다. PTY의 ioctl 성공은 물리 신호 검증으로 승격하지 않는다. Windows 보조 콘솔 gate(B6)도 폐기하지 않으며, 웹 UI의 사용자 입력 경로를 M1에 별도로 검증한다.

## 9.4 짧은 Rust 스파이크 결과

**[확인: 2026-11-10 로컬]** [소스](../../../spikes/uart/rust/src/main.rs), [lock](../../../spikes/uart/rust/Cargo.lock), [실행 결과](../../../spikes/uart/rust/results.txt), [재현법](../../../spikes/uart/rust/README.md). macOS 27.0.1(26A434) arm64, UID 501, rustc/cargo 1.93.1, release 빌드. `cargo build`/`cargo run`만 사용했고 CARGO_HOME/target은 spike 내부다. UART 제품 구현이나 Windows/Linux 실측은 아니다.

| 확인 | 결과 |
|---|---|
| `nix::pty::openpty`→serialport native open | 115200 open은 **ENOTTY**. 소스상 macOS `IOSSIOSPEED`가 PTY에 불가. 시험 전용 baud=0 open은 성공. 이번 spike는 read/write만 검증했으며 중재·기록은 미구현. 향후 M1 하네스는 PTY 설정 예외를 공개하고 같은 중재·기록 경로에 연결 |
| 00–FF 왕복 | 양방향 각각 256바이트 정확히 일치 |
| RX 4 MiB | 모든 바이트를 반복 00–FF와 비교해 일치, FNV-1a64 `529aaa6348e22325`; **1,008.116 ms / 3.968 MiB/s**. PTY 처리량이며 실제 UART/전체 Loom 처리량 아님 |
| read timeout | 200 ms 설정→빈 read `TimedOut`, 로컬 관측 **202.015 ms** |
| 시그널 | 명시 DTR/RTS는 ENOTTY, set/clear break는 API 성공만 확인. 물리 파형·길이 증거 아님 |
| 배타 | 객체 `exclusive=true`지만 이 macOS PTY에서 비협조적 `std::fs::OpenOptions` 두 번째 open은 성공. `set_exclusive(false)` 뒤도 성공. 이 결과로 실 Linux tty의 TIOCEXCL 효과를 추정하지 않음 |
| Rust regex | `^(a+)+$`, `^(a\|aa)+$`, `a…a!` 실패 입력. 각 100회 평균 1/16/64 KiB에서 각각 **1.955/28.110/117.754 µs**, **1.843/28.261/113.843 µs**. compile **377.541/73.125 µs**. 두 패턴 실험일 뿐 worst-case wall 보증 아님 |
| 제한/기능 | backreference `(a)\1` 거절, 256 KiB size limit에서 Unicode `\w{1000}` 팽창 거절, NUL 포함 memmem 위치 일치 |

**[제안] POSIX 하네스**는 [nix openpty](https://docs.rs/nix/0.31.3/nix/pty/fn.openpty.html)+raw/echo-off를 채택한다. macOS는 sim가 발급한 PTY임이 확인된 내부 경로에만 위 예외를 둔다. master는 reader가 drain할 때까지 살아 있어야 하며 송신은 nonblocking+deadline으로 한다. Linux/macOS PTY는 byte/중재/기록 E2E용, Windows 필수 CI는 Rust `sim://` 유지. [portable-pty](https://docs.rs/portable-pty/0.9.0/portable_pty/)의 자식 쉘/플랫폼 PTY 추상화는 UART COM 장치 검증을 대신하지 않으므로 추가하지 않는다.

## 9.5 C2: 24 h 혼합 soak 수용안

모두 **[제안]**, 아직 실행하지 않았다. Windows x64·Linux x64 각각 release 빌드로 **CAN 고부하 + UDS polling + UART flood를 같은 loomd에서 24 h 동시** 실행한다. macOS는 개발용 축소 시험이다. 시뮬레이션 CPU/메모리 gate와 실제 칩/드라이버의 무손실·파형 gate를 나눠 보고한다.

| 부하 | 내용·관측 |
|---|---|
| 지속 수신 | 8채널 admission 상한, 921600 8N1 상당 92,160 B/s/채널 연속. 검증된 실장치 채널은 3 Mbaud 8N1 상당 300,000 B/s로 올리고 나머지는 sim로 상한까지 부하. 모든 8채널 3M은 지원 선언 전 별도 최대부하 gate |
| 바이트/파서 | seq+길이+체크섬 프레임, 00–FF 바이너리·정상 UTF-8 분할·invalid UTF-8·CR 덮어쓰기·ANSI/OSC/DCS·4 KiB 초과 미종결 escape·64 KiB 초과 줄을 교대. 정상 구간은 source 송신량/해시↔호스트 수신↔저널을 따로 비교 |
| churn | 1분마다 1개 채널 USB/sim 재연결, 10초마다 웹 attach/detach·늦은 attach, 구독 총 8개 내에서 느린 client 1개, 초당 expect 10회 생성/취소(동시 8개 내), 시나리오 재현용 seed·주기 고정 |
| 사용자/격리 | 매분 agent send 중 사용자 입력/Ctrl-C, dirty line·hold·STOP·기록 실패 사용자 예외. 시간당 UART worker 1개 강제 종료→다른 7채널/CAN/UDS/웹/저널 지속, 해당 포트 자동 재open·재송신 0 |
| 과부하 별도 구간 | 저널 writer throttle·disk full·구독 credit 정지·각 큐 포화·CTS low를 fault 창으로 주입. 예상 loss/failed를 명시하며 정상 구간 무손실 판정을 희석하지 않음. 모든 손실 계층/범위·TX partial·복구 경계 검증 |

**[제안] RSS 관문**: 1 s마다 worker별·loomd·브라우저 프로세스 메모리, byte/count high-water·drop·FD/handle·task 수를 수집한다. 30분 warm-up 및 모든 링/regex cache를 한 번 채운 뒤 시간대별 10분 RSS 중앙값을 비교한다. 각 worker는 hard 64 MiB 이내, **마지막 창−첫 창 ≤8 MiB 및 회귀 기울기 ≤0.25 MiB/h**, loomd는 CORE가 정한 전체 예산 안에서 **≤32 MiB·≤1 MiB/h**를 초기 제안한다. 절대 예산과 성장률을 모두 통과해야 한다. forced restart 직전 고수위도 따로 보존해 restart가 누수를 숨기지 못하게 하고, worker 하나는 24 h 무재시작 표본으로 남긴다. CAN/UDS 변화까지 포함한 loomd 기준은 CORE와 사전 고정한다. 브라우저 JS heap/DOM·렌더 지연 gate는 UI 팀 소유이나 churn 시험에서 제외하지 않는다.

**[제안] 서비스 관문**: 정상 지원 부하에서 host RX↔저널 손실 0·view skip 0, 웹 실제 표시 p95 <100 ms(WS 도착만 측정하면 불합격), 사용자 키→driver 수용 p95 <20 ms 비부하/<100 ms flood, 상태 heartbeat 1 Hz. fault 구간은 명시 gap·정책 유지·회복을 판정한다. 웹/서버 clock domain 보정과 paint 계측은 UI/CORE 공통 하네스에 맡긴다. expect 취소 후 waiter/task/handle 누적 0, 다른 채널 seq/producer epoch 및 저널 정상성을 확인한다.

**[확인: 산술]** 8채널×3M 8N1×24 h는 UART payload만 **약 193.1 GiB**다(921600이면 약 59.3 GiB). base64·인덱스·sidecar·CAN 기록은 추가다. **[제안]** 디스크 사전 용량 검증 또는 CORE의 고정 디스크 quota·rotation·만료 안내를 사용하고, 회전 삭제 전에 segment 해시/seq를 streaming 검증한다. 24 h 전체 원본을 보존했다고 주장하려면 그 용량을 실제 확보한다. 임시 스풀/인덱스/RAM에 무한히 누적하지 않는다. 정상 retention 만료와 `record.gap`은 구별한다.

## 12. Rust crate 관련 작업 행 교체안

**[제안·CORE 경계 미확정]** 아래 이름은 논리 경계다. workspace/binary·등록 매커니즘·IPC·공통 타입 이름은 CORE 결정에 맞춘다. A4의 UART 소유 공유 엔진을 `loom-console`로 옮겨 SSH/ADB/RTT가 재사용하고, raw 장치 open은 worker 내부에만 둔다. 공통 gateway/CLI/정책/저널을 UART가 중복 구현하지 않는다.

| 기존 ID / 단계 | Rust 작업 행 | 의존·완료 근거 |
|---|---|---|
| T03 / P0 | `loom-uart-worker` Transport: serialport+native Unix nonblocking/Windows timeout, DTR 오류 표면화, parity/stop 확장·readback, byte bounded bridge/TX actor | CORE 없이 fake 가능. macOS PTY 예외, Windows/Linux CTS stall·partial write gate |
| T04 / P0 | Rust sim 엔진 + nix PTY `bootshell/flood/binary`; Windows `sim://`; enumerator 주입 | T03; 00–FF 왕복·경계/재연결 재현 |
| T05–T07b / P0 | `loom-console`: 고정 링·offset/세대/커서·gap·출력 제한·증분 UTF-8·vte sanitizer·InputArbiter/expect 공개 trait | A4 소비자와 trait/테스트 fixture 합의. UI 프레임워크/장치 open 의존 없음 |
| T08 / P1 M1 | worker 조립 + **채널별 worker** lifecycle, gateway 측 UART 모듈과 단일 `loom` CLI 명령 등록 | CORE Registry/worker/serde schema/정책·저널 stub; Python entry point 없음 |
| T09–T10 / P1 M1 | Rust literal/regex bounded matcher, cursor/read/run/send-expect, 사용자 중재·risk hook·STOP/A12 예외 | T05–T08; 프로필 regex 문법 이행·범위/취소·dirty-line 회귀 |
| T11·T24 / P1 M1로 이동 | UART **웹 패널 내용/질의/입력 명세** + UI 팀의 패널 구현 연결. 기존 T24 “M4 웹”을 폐기 | UI/CORE WS·viewer 자격·셸; agent RX/TX와 실제 웹 표시·개입 E1. 보조 TUI는 crossterm 별도 작업 |
| T12·T15 / P1 M1~M3 | cargo test/proptest/PTY/CLI·웹 E2E·스키마 golden + 24 h 혼합 soak·worker crash·RSS gate | CORE 공통 지연/증거 하네스·UI paint 계측·CAN/UDS workload. nextest는 CI 선택 |
| T16–T17 / P2 M2 | polling watcher·strict identity·generation + native signals/configure/release/reclaim | 실제 USB interface/location 결측·모호성, DTR/break Saleae 검증 |
| T19·T21 / 보조 | crossterm raw input·Windows Terminal 키 gate, 필요할 때 ratatui | 웹 M1 대체 불가. prompt_toolkit/Textual 제거 |
| T23 / P3 M3 | Windows/Linux 실장치 matrix·3M/8포트·24 h gate·Saleae 파형, static/musl 배포 시험 | 미검증 조합은 지원 표에 올리지 않음 |
| T13·T20 / M1/M2 | 짧은 `loom-uart` + 공통 `loom`/`loom-test`, 상세 정본 `loom uart help --json` | C7 증거 seq 검증·보고서 연결; 테스트 세션/승인은 CORE/EXPLORE, UI 구현은 UI 팀 |

V2 SOL DONE
