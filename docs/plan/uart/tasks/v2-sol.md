docs/plan/02-core-principle.md 준수 (최우선)

# v2.0 — uart-sol: Rust crate · 실현성 · 성능/메모리 (40분 내외)
먼저 `docs/plan/uart/tasks/v2-common.md`를 읽는다.

출력: `docs/plan/uart/v2-sol.md` (plan.md에 넣을 문안, 절 번호 붙여서). 끝에 "V2 SOL DONE" 한 줄.

## 쓸 것
1. **§7 라이브러리 v2 (Rust)** — 실제 crates.io·docs.rs·소스로 확인(버전·날짜·라이선스·유지 상태):
   - 시리얼: `serialport`(serialport-rs), `tokio-serial`/`mio-serial`. 확인할 것: Windows/Linux/macOS, baud·data·parity·stop(1.5 지원 여부), DTR/RTS 설정, **break**(`set_break`/`clear_break`), 배타 open(Unix `TIOCEXCL` 기본 여부·끄는 법), timeout 의미, write timeout, **USB 정보(vid/pid/serial/manufacturer/product/interface 번호)**, Linux `libudev` 의존(기능 플래그·정적 빌드 영향), open 시 DTR 처리 순서(v1.2 §8.1 #6 — pyserial처럼 open 뒤 설정인지).
   - async vs 블로킹 reader 스레드 판단(Windows overlapped I/O 지원 상태 포함).
   - pty 가짜 장치: `nix::pty::openpty`, `portable-pty` 등 (macOS·Linux), Windows는 `sim://` 인프로세스 유지.
   - 정규식: Rust `regex`(선형 시간 보장 → v1.2의 Python `regex` timeout worker 설계를 대체 가능한지, `size_limit`/`dfa_size_limit`로 상한), literal은 `memchr::memmem`.
   - 텍스트·escape: 점진 UTF-8 디코딩, escape sanitizer용 `vte` 파서 등.
   - 핫플러그 감지: 폴링 vs Linux udev monitor / Windows 장치 알림 crate.
   - TUI 보조(선택): `crossterm`(Windows 콘솔 원시 입력), `ratatui` 필요 여부.
   - 테스트: `cargo test`/`nextest`, `proptest`, pty E2E 하네스.
2. **Python 의존 제거표**: v1.2의 pyserial·regex·Textual·prompt_toolkit·pytest 등 → Rust 대체. Python이 남는 곳이 있으면 "왜 Rust로 안 되는가" 근거, 없으면 "UART Python 0"을 명시.
3. **C2 성능·메모리**: UART 큐·버퍼 상한 표(reader→worker, worker→gateway, 저널 큐, 구독자별, expect 창, 입력 큐, 백로그 링) — 각 상한·넘칠 때 정책(drop·block·spill)·표시(gap 층). worker 메모리 예산(채널 수 × 링 등)과 초과 시 동작. **24 h soak에 넣을 UART 부하 패턴**(예: 3 Mbaud/921600 연속 flood, 바이너리+ANSI 혼합, 재연결 반복, 구독자 attach/detach 반복, expect 생성/취소 반복) + RSS 증가 관문 제안. worker 크래시 시 동작(데몬·저널·다른 채널 무영향, 재기동, 포트 재open 정책).
4. **짧은 Rust 스파이크**(선택, 20분 이내): `spikes/uart/rust/`에 cargo 프로젝트. 예: macOS pty slave를 `serialport`로 열기(실패하는 ioctl 확인), 0x00–0xFF 왕복, 처리량, `regex`의 병적 패턴 시간. `cargo build`/`cargo run`만 — **`cargo install`·전역 설정 변경 금지**, target 디렉터리는 스파이크 폴더 안.
5. **§12 작업 분해 v2 중 Rust crate 관련 행 제안**: crate 구조(예: `loom-console`(A4 공유), `loom-uart-worker`, gateway 측 UART 모듈) — CORE crate 경계는 core 팀 결정이므로 가정으로 표시.
