# `loom-console` 공개 API 초안 (T07b, P0)

- 작성: uart-opus, 2026-11-11. docs/plan/02-core-principle.md 준수.
- 대상 독자: UART worker(P1), UI 팀(웹 패널 WASM 정제기), EXPLORE 팀(SSH/ADB/RTT 콘솔 재사용, A4).
- 근거: UART plan §3.2·§3.3·§3.6·§3.7·§3.13·§5.3–5.5·§7.4–7.5.
- 상태:
  - `sanitize`·`arbiter`는 구현과 시험이 끝났다(이 문서의 시그니처가 실제 코드다).
  - `ring`·`cursor`·`text`·`matcher`는 다른 작업자가 만들고 있다. 아래는 작업 지시(`tasks/p0-console-core.md`)의 시그니처를 인용한 것이고, 확정 시그니처는 그 보고서(`reports/p0-console-core.md`)를 본다.

## 1. 모듈과 책임

| 모듈 | 책임 | 장치 I/O | 시계 | WASM |
|---|---|---|---|---|
| `ring` | 채널당 고정 링(바이트·건수·메타 상한), 세대·gap 표시, `rx_offset` 연속 | 없음 | 없음(시각은 청크 필드) | 가능 |
| `text` | 에이전트 `read`용 텍스트: 증분 UTF-8, ANSI strip/keep/escape, 줄 조립, 바이너리 감지 | 없음 | 없음 | 가능 |
| `cursor` | 클라이언트 커서·읽기 상한(Summary/Page/Peek)·opaque 토큰 | 없음 | 없음 | 가능 |
| `matcher` | 제한된 expect(literal `memchr` / Rust `regex`, 공유 창) | 없음 | 없음 | feature `matcher`(기본 켬). WASM 빌드에서는 끔 |
| `sanitize` | 렌더 정제기: 장치 바이트 → 단말에 넘겨도 안전한 바이트 | 없음 | 없음 | **필수**. 다른 모듈에 의존하지 않는다 |
| `arbiter` | 채널 하나의 입력 중재: 입력권·dirty·hold·락·takeover·지연 송신 | 없음 | 주입(`now_ms`) | 가능(웹은 쓰지 않음, worker 전용) |

공통 규칙:
- 원시 바이트와 offset은 바꾸지 않는다. 변환은 표시용으로만 한다.
- 모든 큐와 버퍼에는 상한 상수가 있고, 넘치면 보고한다.
- async 런타임과 스레드는 쓰지 않는다. 호출자가 스레드를 정한다.

## 2. `sanitize` (실제 시그니처)

```rust
pub const MAX_SEQ_BYTES: usize = 4096;  // escape/control string cap (§7.5)
pub const RECENT_CAP: usize = 64;       // recent-blocked ring
pub const GAP_RESET: &[u8] = b"\x1b[0m";
pub enum Profile { Web, Tui }
pub struct SanitizeConfig { pub profile: Profile, pub allow_mouse: bool, pub rows: u16 }
impl SanitizeConfig { pub const fn web() -> Self; pub const fn tui(rows: u16) -> Self }
pub enum BlockKind { Query, WindowOp, Title, OscColor, Osc8, Osc52, OscOther, Mouse, Focus,
                     Mode, Dcs, ApcPmSos, Ris, Esc, Csi, C0, C1, Overlong }
pub struct Counters { pub query: u64, /* … one u64 per BlockKind … */ pub utf8_replaced: u64 }
pub struct Blocked { pub kind: BlockKind, pub offset: u64 }   // offset = cumulative input byte
pub struct Sanitizer { .. }
impl Sanitizer {
    pub fn new(cfg: SanitizeConfig) -> Self;
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>);  // appends
    pub fn on_gap(&mut self, out: &mut Vec<u8>);               // reset parser + ESC[0m
    pub fn counters(&self) -> &Counters;
    pub fn take_recent(&mut self) -> Vec<Blocked>;             // ≤ 64, oldest first, drains
    pub fn alt_screen(&self) -> bool;                          // ?47/?1047/?1049
    pub fn bracketed_paste(&self) -> bool;                     // ?2004
    pub fn set_rows(&mut self, rows: u16);                     // TUI resize
    pub fn buffered_bytes(&self) -> usize;                     // ≤ 4096 + 3
}
```
- 허용 목록 방식이다. 통과하는 것:
  - 인쇄 문자, UTF-8, BEL·BS·HT·LF·CR.
  - SGR, 커서 이동·지우기.
  - DECSC/DECRC, IND/NEL/RI, 키패드 모드, G0/G1 문자 집합(B·0), 커서 모양.
  - DECSTBM. Web은 그대로 두고, TUI는 하단을 `rows−1`로 줄인다.
  - DECSET 중 1·6·7·12·25·47·1047·1048·1049·2004, SM/RM 4.
- 나머지는 지우고 종류별로 센다. 섞인 DECSET은 허용된 모드만 남겨 다시 쓴다.
- 잘못된 UTF-8은 U+FFFD로 바꾼다(baud가 틀린 것이 화면에 보이게). C1 코드 포인트는 지운다.
- 통과하는 시퀀스는 **입력 바이트 그대로** 내보낸다. 예외는 DECSET 재작성과 TUI DECSTBM 둘뿐이다.
- 청크를 어디서 나눠 넣어도 결과가 같다. 이는 proptest로 확인했다.
- 골든 코퍼스: `crates/loom-console/tests/golden/sanitize/`. 형식은 그 폴더의 README에 있다.

## 3. `arbiter` (실제 시그니처)

```rust
pub struct ArbiterConfig { pub has_prompt: bool, pub line_kill_ctrl_u: bool }
pub enum Owner { None, User(String), Agent { client: String, request_id: String } }
pub enum AckState { Sent, Held, Discarded }
pub struct InputDecision { pub send: Vec<u8>, pub interrupt: bool, pub ack: AckState,
                           pub reason: Option<&'static str>, pub preempt_agent: Option<String> }
pub enum AgentAdmit { Now, Deferred { until_ms: u64 }, Reject { code: ErrorCode, detail: Value } }
pub enum TickAction { AgentReady { request_id: String, deferred_ms: u64 },
                      AgentRejected { request_id: String, code: ErrorCode, detail: Value } }
pub enum YieldResult { Released, ConfirmClear { line_len: u32 }, Cleared { send: Vec<u8> },
                       ManualClearNeeded { line_len: u32 } }
pub enum ArbiterEvent { InputOwner{..}, Hold{..}, Lock{..}, AgentActivity{..}, InputAck{..} }
impl ArbiterEvent { pub fn kind(&self) -> &'static str; pub fn payload(&self) -> serde_json::Value }

impl InputArbiter {
    pub fn new(cfg: ArbiterConfig) -> Self;
    // user (viewer connection only)
    pub fn user_input(&mut self, viewer: &str, input_seq: u64, bytes: &[u8], now_ms: u64) -> InputDecision;
    pub fn takeover(&mut self, viewer: &str, now_ms: u64) -> InputDecision;
    pub fn yield_input(&mut self, viewer: &str, now_ms: u64) -> (YieldResult, Vec<TickAction>);
    pub fn clear_input(&mut self, viewer: &str, now_ms: u64) -> (Vec<u8>, Vec<TickAction>);
    pub fn viewer_left(&mut self, viewer: &str, now_ms: u64) -> Vec<TickAction>;
    pub fn set_hold(&mut self, on: bool, now_ms: u64) -> Vec<TickAction>;
    pub fn interrupt_written(&mut self, n: usize);
    // RX hints
    pub fn prompt_seen(&mut self, now_ms: u64) -> Vec<TickAction>;
    pub fn set_alt_screen(&mut self, on: bool);
    // agent
    pub fn agent_send(&mut self, client: &str, request_id: &str, label: &str,
                      defer_timeout_ms: u64, now_ms: u64) -> AgentAdmit;
    pub fn agent_done(&mut self, request_id: &str, now_ms: u64) -> Vec<TickAction>;
    pub fn lock(&mut self, client: &str, ttl_ms: u64, reason: &str, now_ms: u64) -> Result<(), ErrorCode>;
    pub fn unlock(&mut self, client: &str, now_ms: u64) -> Result<Vec<TickAction>, ErrorCode>;
    pub fn tick(&mut self, now_ms: u64) -> Vec<TickAction>;   // every ≤ 100 ms and after RX
    pub fn take_events(&mut self) -> Vec<ArbiterEvent>;       // drain after every call
    // read-only: owner(), input_dirty(), user_line_len(), hold_on(), lock_info(),
    //            pending_agent_sends(), held_bytes(), events_dropped()
}
```
Worker 연결 방법(P1, 리드):
- `InputDecision.send`는 TX actor의 사용자 우선 큐로 보낸다.
- `interrupt`가 참이면 Ctrl-C 차선으로 보내고, 쓴 뒤 `interrupt_written(1)`을 부른다.
- `preempt_agent`가 있으면 그 요청의 남은 청크를 다음 청크 경계에서 취소한다.
- `AgentAdmit::Now` 또는 `TickAction::AgentReady`를 받은 뒤에만 에이전트 바이트를 쓰고, 끝나면 `agent_done`을 부른다.
- 프롬프트 감지는 matcher가 하고 결과를 `prompt_seen`으로 넘긴다. alternate screen 상태는 RX 쪽 `Sanitizer::alt_screen()`을 `set_alt_screen`으로 넘긴다.
- `take_events()`의 `kind()`·`payload()`를 그대로 `uart.*` 이벤트로 발행한다(UI `uartState.ts`와 같은 필드).

## 4. `ring`·`cursor`·`text`·`matcher` (작업 지시 인용)

```rust
// ring
pub struct Chunk { pub source_seq: u64, pub dir: Dir, pub origin: Origin, pub client_id: Option<String>,
                   pub request_id: Option<String>, pub generation: u32, pub mono_ns: u64, pub wall_ns: u64,
                   pub rx_offset: Option<u64>, pub bytes: Vec<u8> }
pub enum Entry { Data(Chunk), Gap(GapMark) }
pub struct RingLimits { pub max_payload: usize /*16 MiB*/, pub max_entries: usize /*32_768*/, pub max_meta: usize /*4 MiB*/ }
// push / iter_from_rx_offset / iter_from_source_seq / last_n_bytes / stats

// text
// Utf8Decoder, Encoding::{Utf8, Latin1}, AnsiMode::{Strip, Keep, Escape}, LineAssembler → Line {
//   text, rx_offset_start, rx_offset_end, source_seq_first, generation, complete, truncated },
// BinaryDetector, reset()

// cursor
pub enum ReadMode { Summary, Page, Peek }
pub struct ReadLimits { pub max_lines: u32, pub max_bytes: u32, pub hard_lines: u32, pub hard_bytes: u32, pub line_chars: u32 }
// CursorStore::read(ring, client, from: From{Last|RxOffset|SourceSeq|Token}, mode, limits, text_opts) -> ReadResult

// matcher (feature "matcher")
pub enum Pattern { Literal(Vec<u8>), Regex(String), Prompt(String) }
impl Matcher {
    pub fn add(&mut self, id: WaiterId, patterns: Vec<Pattern>, from_rx_offset: u64) -> Result<(), MatchError>;
    pub fn feed(&mut self, rx_offset: u64, bytes: &[u8]) -> Vec<MatchHit>;
    pub fn reset(&mut self, reason: ResetReason);
    pub fn remove(&mut self, id: WaiterId);
}
```
이 crate는 stream `seq`를 모른다. `Line`과 `MatchHit`에는 `source_seq`·`rx_offset`만 있고, stream `seq`로 바꾸는 일은 P1 gateway가 맡는다.

## 5. 검토 질문 (3개)

1. **UI 팀:** 정제기를 WASM으로 쓸 때의 계획은 다음과 같다.
   - 형태: `Sanitizer`를 감싼 `feed(Uint8Array) -> Uint8Array` 한 함수, 그리고 `counters()`/`take_recent()`/`alt_screen()`의 JSON 반환.
   - 빌드: feature `matcher`를 끄고 `--no-default-features`로 빌드한다.
   - 질문: 이 형태로 충분한가? xterm 파서 핸들러 삼킴(plan §5.5 ②)과 사용자 의도 게이트(③)는 TS 쪽에 남는데, 그 범위가 맞는가?
2. **EXPLORE 팀:** SSH 콘솔에서 `InputArbiter`를 쓰려면 `ArbiterConfig`에 무엇이 더 필요한가? 지금은 `has_prompt`·`line_kill_ctrl_u` 둘뿐이다.
   - 예상 후보: 원격 셸의 `stty` 줄 편집 상태, PTY 크기 변경 같은 SSH 고유 신호.
   - 이벤트 kind는 지금 `uart.*`로 고정이다. 도메인 접두어를 인자로 받는 방식이 필요한가?
3. **UI·EXPLORE 공통:** 정제기의 TUI 프로필은 지금 DECSTBM 하단만 `rows−1`로 줄인다. plan §5.15.2는 CUP·VPA로 커서를 상태줄 행에 보내는 이동도 막으라고 한다.
   - 이 처리는 보조 TUI(M2, T19)에서 추가할 예정이다.
   - 질문: SSH 콘솔도 같은 TUI 상태줄 규칙을 쓰는가?
