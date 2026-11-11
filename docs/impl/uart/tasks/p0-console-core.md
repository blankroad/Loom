docs/plan/02-core-principle.md 준수.

# P0: `loom-console` 링·커서·텍스트·매처 (T05·T06·T09 매처)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. CARGO_TARGET_DIR = `$HOME/.cache/loom-target/uart-core`.
보고서: `docs/impl/uart/reports/p0-console-core.md`.

## 수정 가능한 파일 (이것만)
- `crates/loom-console/src/{ring,cursor,text,matcher}.rs` (하위 디렉터리 허용)
- `crates/loom-console/tests/{ring,cursor,text,matcher}_*.rs`

`sanitize.rs`·`arbiter.rs`는 다른 작업자(uart-opus)가 동시에 만든다. 건드리지 말 것. 의존성: `loom-proto`(`event::{Dir, Origin}` 재사용), `memchr =2.8.3`, `regex =1.13.1`(feature `matcher`), `serde`, `serde_json`, `proptest`(dev).

## A. `ring` — 채널당 고정 링 (plan §3.3, §3.6, §3.13 backlog 행)
```rust
pub struct Chunk { pub source_seq: u64, pub dir: Dir, pub origin: Origin, pub client_id: Option<String>,
                   pub request_id: Option<String>, pub generation: u32, pub mono_ns: u64, pub wall_ns: u64,
                   pub rx_offset: Option<u64> /* RX만: 이 청크 첫 바이트의 누적 RX offset */, pub bytes: Vec<u8> }
pub enum Entry { Data(Chunk), Gap(GapMark) }   // GapMark{layer: Display|Record|Source, cause, rx_offset_from, bytes: Option<u64>, source_seq_range}
pub struct RingLimits { pub max_payload: usize /*16 MiB*/, pub max_entries: usize /*32_768*/, pub max_meta: usize /*4 MiB*/ }
```
- `push`는 상한을 넘기면 가장 오래된 것부터 제거하고 제거 통계(`evicted_bytes`, `retained_from_rx_offset`, `retained_from_source_seq`, 가장 오래된 `mono_ns`)를 갱신한다. 제거는 **기록 손실이 아니다**(정상 보존기간 만료) — gap을 만들지 않는다.
- 메타데이터(`client_id` 등 문자열)도 `max_meta`에 계수한다. 문자열이 길면(> 128 B) 자르지 말고 push를 `Err`로 거절(호출자 버그).
- 조회: `iter_from_rx_offset(off)`(RX 청크만, 경계 청크는 부분 바이트로), `iter_from_source_seq(seq)`, `last_n_bytes`, `stats()`. RX offset은 단조 증가, 세대가 바뀌어도 이어진다(세대 경계는 `generation` 필드로 구분).
- 시험: 바이트·건수·메타 상한 각각, wrap 후 offset 연속성, 부분 청크 조회, 1M push에서 메모리 상한 유지(링 내부 용량 합이 상한 이하임을 `stats()`로).

## B. `text` — 표시용 텍스트 파이프라인 (plan §3.6, §7.5)
원시 바이트·offset은 바꾸지 않고 **에이전트 `read`용 텍스트**를 만든다. 모두 증분(청크 경계에서 잘린 UTF-8·ESC·CRLF를 상태로 보유)이며 상태 크기는 고정.
- `Utf8Decoder`: 유효 prefix를 내보내고 끝 미완성 ≤ 3바이트 보유, 확정 invalid는 `\xNN`. `Encoding::{Utf8, Latin1}`.
- `AnsiMode::{Strip, Keep, Escape}`: Strip = CSI/OSC/DCS/APC 등 제거(4 KiB 넘는 escape는 버리고 카운트), Escape = `\x1b` → `\e` 표기.
- `LineAssembler`: 줄 종결 `\r\n`, `\n`, 단독 `\r`. 연속 `\r` 덮어쓰기(진행률)는 마지막 상태만(`keep_cr=true`면 원형). 부분 줄 상한 64 KiB(넘으면 줄을 끊고 `truncated` 표시). 출력 `Line { text, rx_offset_start, rx_offset_end, source_seq_first, generation, complete: bool, truncated: bool }`.
- 표시 한 줄 512자 상한(`…[+N chars]`), 비출력 문자 `\xNN`.
- `BinaryDetector`: 최근 창(4 KiB)의 비출력 비율 > 30% → `likely_binary_or_wrong_baud`.
- `reset()`: gap·세대 경계에서 상태 재동기화.
- 시험: proptest — 같은 입력을 임의 분할해 넣어도 줄 결과 동일; 잘린 UTF-8·ESC·CRLF 경계; `\r` 덮어쓰기; 64 KiB 긴 줄; latin-1.

## C. `cursor` — 클라이언트 커서와 읽기 결과 상한 (plan §3.3, §3.6)
```rust
pub struct CursorStore { .. }  // 키 = client_name(String), 값 = {rx_offset, revision}; 클라이언트 수 상한 256(LRU 제거 + 보고)
pub enum ReadMode { Summary, Page, Peek }
pub struct ReadLimits { pub max_lines: u32 /*100*/, pub max_bytes: u32 /*16 KiB*/, pub hard_lines: u32 /*1000*/, pub hard_bytes: u32 /*64 KiB*/, pub line_chars: u32 /*512*/ }
pub struct ReadResult { pub lines: Vec<Line>, pub truncated: bool, pub omitted: Option<Omitted{from_seq,to_seq,lines,bytes}>,
                        pub more_available: bool, pub next_cursor: String /*opaque token*/, pub cursor_gap: Option<CursorGap>, pub binary_hint: bool }
```
- `read(ring, client, from: From{Last|RxOffset|SourceSeq|Token}, mode, limits, text_opts)`:
  - Summary: 상한 초과 시 머리 20줄 + 꼬리 나머지, 가운데 생략 정보, **커서는 끝까지 전진**.
  - Page: 상한까지만, 반환분만큼 전진, `more_available`.
  - Peek: 커서 안 움직임.
  - 커서가 링 밖(이미 제거됨)이면 `cursor_gap{lost_from, lost_to}`를 붙이고 남은 범위부터 읽는다(디스크 조회는 P1 — 여기서는 표시만).
  - stream `seq`는 이 crate가 모른다: `Line`에는 `source_seq`만 두고 seq 변환은 P1 gateway 몫(보고서에 명시).
- `next_cursor` 토큰 = rx_offset + generation + revision을 담은 opaque 문자열(형식 자유, 위조·손상 시 `Err`).
- 시험: Summary/Page/Peek, 커서 전진 규칙, 같은 이름 동시 갱신 revision 충돌, 링 밖 커서, 출력 상한 하드값.

## D. `matcher` — 제한된 expect 매처 (plan §7.4, §3.13 expect 행)
```rust
pub enum Pattern { Literal(Vec<u8>), Regex(String), Prompt(String) /*프로필 프롬프트 정규식, 줄 끝 앵커*/ }
pub struct MatcherLimits { pattern_bytes: 2048, size_limit: 256<<10, dfa_size_limit: 256<<10, nest_limit: 64,
                           window: 64<<10, max_patterns: 8, max_waiters: 8 }
pub struct Matcher { .. }   // 채널당 1개, 공유 창(대기마다 창 복제 금지)
impl Matcher {
    pub fn add(&mut self, id: WaiterId, patterns: Vec<Pattern>, from_rx_offset: u64) -> Result<(), MatchError>; // 컴파일 실패·상한 → Err(코드)
    pub fn feed(&mut self, rx_offset: u64, bytes: &[u8]) -> Vec<MatchHit>;   // MatchHit{id, pattern_idx, start_off, end_off, text}
    pub fn reset(&mut self, reason: ResetReason /*Gap|Generation*/);       // 창 비움, 대기는 유지
    pub fn remove(&mut self, id: WaiterId);
}
```
- literal은 `memchr::memmem::Finder`, 청크 이음매는 needle 길이−1만 겹침(무손실). regex는 Rust `regex::bytes`, `RegexBuilder` 상한 위 값, 창(64 KiB) 안에서 단일 `find`. 창 밖 무손실은 약속하지 않음(문서화). 매처는 ANSI strip된 텍스트가 아닌 **원시 바이트**에 대해 동작하되 옵션 `strip_ansi`(text 모듈 Strip 재사용)를 둔다 — 기본 strip(에이전트가 보는 텍스트와 같은 기준).
- 오류는 `MatchError::{PatternTooLong, CompileTooBig, Syntax(msg), TooManyPatterns, TooManyWaiters}` → P1에서 `E_USAGE`/`E_SCAN_LIMIT`로 매핑(보고서에 표).
- `from_rx_offset`이 현재 창보다 앞이면 창 시작부터(보고서에 명시), 링 재스캔은 P1.
- 시험: 경계에 걸친 literal(1바이트씩 feed), regex 경계, 상한 각각, gap reset 뒤 이전 바이트와 이어 붙여 매치하지 않음, 8 대기 동시, 병적 패턴(`(a|aa)*b`류) 64 KiB에서 시간 측정(보고서에 수치).

## 끝나면
보고서 작성 후 대기. 리드가 diff 검토·커밋한다.
