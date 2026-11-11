DONE

# P0 보고: `loom-console` ring · cursor · text · matcher (T05·T06·T09 매처)

작업자: uart-core · worktree `/Users/ctmctm/workspace/Loom/.worktrees/uart`(uart/main) · `CARGO_TARGET_DIR=$HOME/.cache/loom-target/uart-core` · 커밋 안 함.

수정한 파일(배정 범위만):
- `crates/loom-console/src/{ring,cursor,text,matcher}.rs`
- `crates/loom-console/tests/{ring_limits,cursor_read,cursor_props,text_pipeline,text_props,matcher_expect}.rs`

`Cargo.toml`·`lib.rs` 변경 없음. 필요한 의존성은 이미 있었다(`loom-proto`, `memchr`, `regex`(feature `matcher`), `serde`, `proptest` dev). `cargo check -p loom-console --no-default-features`도 통과한다(`text`·`cursor`·`ring`은 regex를 쓰지 않음).

중간 기록: 07:42쯤 다른 작업자가 `cargo fmt -p loom-console`를 돌려 내 파일 공백이 바뀌었다(내용은 그대로). 최종 포맷은 내 파일에만 `rustfmt`를 직접 돌려 맞췄고, sanitize/arbiter 파일은 건드리지 않았다.

---

## 1. 공개 API 요약

### `ring` (plan §3.3, §3.6, §3.13 backlog 행)
```rust
pub struct Chunk { source_seq, dir: Dir, origin: Origin, client_id: Option<String>, request_id: Option<String>,
                   generation: u32, mono_ns, wall_ns, rx_offset: Option<u64> /* RX만 */, bytes: Vec<u8> }
pub enum GapLayer { Display, Record, Source }
pub struct GapMark { layer, cause: String, rx_offset_from: Option<u64>, bytes: Option<u64>,
                     source_seq_range: Option<(u64,u64)>, generation: u32, mono_ns: u64 }
pub enum Entry { Data(Chunk), Gap(GapMark) }
pub struct RingLimits { max_payload, max_entries, max_meta }        // Default = 16 MiB / 32 768 / 4 MiB
pub struct Ring;  // new(limits) / new_at(limits, rx_offset)
  push(Entry) -> Result<PushOutcome{evicted_entries, evicted_bytes}, RingError>
  iter_from_rx_offset(off) -> impl Iterator<ChunkSlice>   // RX만, 경계 청크는 부분 바이트
  rx_items_from(off) -> impl Iterator<RxItem{Data(ChunkSlice)|Gap(&GapMark)}>  // cursor용, RX 순서
  iter_from_source_seq(seq) -> impl Iterator<&Entry>
  rx_offset_for_source_seq(seq) -> u64
  last_n_bytes(n, include_tx) -> Vec<ChunkSlice>
  stats() -> RingStats { entries, payload_bytes, meta_bytes, slot_capacity, slot_size, evicted_entries,
                         evicted_bytes, retained_from_rx_offset, rx_end, retained_from_source_seq,
                         last_source_seq, oldest_mono_ns, limits }
  rx_end(), retained_from_rx_offset(), retained_from_source_seq(), generation(), len(), entries()
pub struct ChunkSlice<'a> { chunk, skip }  // bytes(), rx_offset()
pub enum RingError { EmptyChunk, ChunkTooLarge, MetaFieldTooLong{field,len}, MetaTooLarge,
                     NonMonotonicSeq, RxOffsetMismatch, NonMonotonicRxOffset }
```
- 세 상한을 동시에 적용하고, 넘치면 가장 오래된 것부터 제거한다. 제거는 gap이 아니다(Gap 항목을 만들지 않음).
- `source_seq`는 엄격 증가해야 한다. RX `rx_offset`은 감소할 수 없다(점프 = 링 앞에서 생긴 hole). 세대가 바뀌어도 offset은 이어진다.
- 메타 문자열이 128 B를 넘으면 `Err`로 거절한다(자르지 않음). TX에 `rx_offset`이 있거나 RX에 없으면 `Err`.

### `text` (plan §3.6, §7.5)
```rust
pub enum Encoding { Utf8, Latin1 }      pub enum AnsiMode { Strip, Keep, Escape }
pub struct TextOptions { encoding, ansi, keep_cr: bool, line_chars: u32 /*≤512*/, max_line_bytes: usize /*≤64 KiB*/ }
pub struct Line { text, rx_offset_start, rx_offset_end, source_seq_first, generation, complete, truncated, elided_chars }
pub struct AnsiFilter;     // classify(b) -> ByteClass{Text|Sequence}, reset(), overlong_count()
pub struct Utf8Decoder;    // push(b, out), decode(bytes, out), flush(out), pending() — Decoded{Char|Invalid}
pub struct LineAssembler;  // note_byte / cr / lf / unit / check_limit / partial / cut
pub struct BinaryDetector; // record(bad), record_n, likely_binary(), counts()
pub struct TextPipeline;   // new(opts), feed(rx_offset, source_seq, generation, bytes, &mut Vec<Line>),
                           // partial(), reset(&mut Vec<Line>), binary_hint(), counters() -> TextCounters
pub fn render_lines(opts, rx_offset, bytes) -> Vec<Line>   // 일회성 렌더(시험·도구용)
```
- 모든 단계가 바이트 단위 상태기계라서 결과가 청크 분할과 무관하다(proptest로 증명).
- `feed`는 offset 점프나 세대 변경을 만나면 스스로 `reset`한다.

### `cursor` (plan §3.3, §3.6)
```rust
pub enum ReadMode { Summary, Page, Peek }
pub enum ReadFrom { Last, RxOffset(u64), SourceSeq(u64), Token(String) }
pub struct ReadLimits { max_lines, max_bytes, hard_lines, hard_bytes, line_chars }  // effective()로 clamp
pub struct ReadResult { lines, truncated, omitted: Option<Omitted>, more_available, next_cursor: String,
                        cursor_gap: Option<CursorGap>, binary_hint, from_rx_offset, next_rx_offset,
                        revision, evicted_client: Option<String> }
pub struct Omitted { from_seq, to_seq, lines, bytes, from_rx_offset, to_rx_offset, at_line }
pub struct CursorGap { lost_from, lost_to, exact }
pub struct CursorStore;  // new(), with_token_key(k0,k1), get, commit(client, expected_rev, rx_offset, gen),
                         // remove, iter, len, encode_token, decode_token, check_name
pub enum CursorError { EmptyClientName, ClientNameTooLong(usize), BadToken, RevisionConflict{expected,current} }
pub fn read(ring, &mut store, client, ReadFrom, ReadMode, &ReadLimits, &TextOptions) -> Result<ReadResult, CursorError>
pub fn line_cost(&Line) -> u32   // text.len() + 1
```
- `Line`에는 `source_seq`만 있다. **stream `seq`로 바꾸는 일은 P1 gateway 몫이다**(이 crate는 stream seq를 모름).
- 토큰 형식: `c1-` + hex(rx_offset 16 + generation 8 + revision 16 + tag 16). tag는 store별 키로 계산한 SipHash다. 형식 오류, 위조, 손상, 다른 store의 토큰은 모두 `BadToken`.
- revision 규칙: commit 때 `expected ≠ 저장 revision`이면 `RevisionConflict`이고 커서는 움직이지 않는다. 단, 커서가 이미 목표 위치에 있으면 멱등 성공이다(`--page` 응답 유실 후 같은 토큰을 다시 보내는 경우).

### `matcher` (plan §7.4, §4.5, §3.13 expect 행)
```rust
pub enum Pattern { Literal(Vec<u8>), Regex(String), Prompt(String) }
pub struct MatcherLimits { pattern_bytes, size_limit, dfa_size_limit, nest_limit, window, max_patterns, max_waiters }
pub type WaiterId = u64;
pub struct MatchHit { id, pattern_idx, start_off, end_off, text }
pub enum ResetReason { Gap, Generation }
pub enum MatchError { PatternTooLong{idx,len}, CompileTooBig{idx}, Syntax{idx,msg}, EmptyPattern,
                      TooManyPatterns, TooManyWaiters, DuplicateWaiter }
pub struct Matcher;  // new(limits, strip_ansi), add(id, patterns, from_rx_offset), feed(rx_offset, bytes) -> Vec<MatchHit>,
                     // poll() -> Vec<MatchHit>, reset(reason), remove(id), window_len(), segment_count(), ...
```
- 공유 창 하나를 모든 대기가 함께 쓴다(대기마다 복제하지 않음).
- literal은 `memmem::Finder`로 찾고, 이어서 찾을 때 이미 본 끝부분 `needle−1` 바이트만 겹쳐 다시 본다.
- regex는 `regex::bytes::RegexBuilder`에 위 상한을 걸고, 대기 시작점부터 창 끝까지 단일 `find`를 한다.
- `strip_ansi`(기본 true)일 때 `text::AnsiFilter`로 escape를 걸러 창에 넣는다. 원시 offset으로 되돌릴 때는 run-length 매핑을 쓴다.
- 대기는 한 번 맞으면 제거된다. 여러 패턴이 맞으면 시작 위치가 가장 이른 것, 같으면 패턴 index가 작은 것이 이긴다.

**MatchError → P1 오류 코드 매핑(제안):**

| MatchError | P1 코드 | exit |
|---|---|---|
| `PatternTooLong`, `Syntax`(backreference·look-around·nest 초과 포함), `EmptyPattern`, `DuplicateWaiter` | `E_USAGE` | 2 |
| `CompileTooBig` | `E_SCAN_LIMIT`(또는 §4.5의 `E_PATTERN_TOO_EXPENSIVE`) | 8 |
| `TooManyPatterns`, `TooManyWaiters` | `E_SCAN_LIMIT` | 8 |

참고: `regex`의 `nest_limit` 초과는 `regex::Error::Syntax`로 나오므로 `Syntax`로 분류된다(시험 `limits_each`).

---

## 2. 상수 표 (상한·넘칠 때 정책)

| 상수 | 값 | plan 근거 | 넘칠 때 |
|---|---|---|---|
| `ring::DEFAULT_MAX_PAYLOAD` | 16 MiB | §3.6, §3.13 backlog | 가장 오래된 항목 제거(gap 아님) |
| `ring::DEFAULT_MAX_ENTRIES` | 32 768 | §3.6, §3.13 backlog | 〃 |
| `ring::DEFAULT_MAX_META` | 4 MiB | §3.13 backlog "메타데이터 pool" | 〃 |
| `ring::MAX_META_FIELD_BYTES` | 128 B | task §A | push `Err`(자르지 않음) |
| `text::MAX_ESCAPE_BYTES` | 4 096 | §7.5, §3.13 텍스트 | `overlong` 카운트 후 terminator(ST/BEL/CAN/SUB/ESC) 또는 LF까지 버림 |
| `text::MAX_PARTIAL_LINE_BYTES` | 64 KiB | §3.13 텍스트 "부분 줄" | 줄을 끊고 `truncated=true, complete=true` |
| `text::MAX_LINE_CHARS` | 512 | §3.6 | `…[+N chars]`, `elided_chars=N` |
| `text::BINARY_WINDOW_BYTES` / `BINARY_THRESHOLD_PERCENT` / `BINARY_MIN_SAMPLE_BYTES` | 4 096 / 30 % / 64 | §3.6 | `binary_hint=true`(경고만) |
| `cursor::MAX_CLIENTS` | 256 | §3.3, task §C | LRU 제거 → `ReadResult.evicted_client`로 보고 |
| `cursor::MAX_CLIENT_NAME_BYTES` | 128 B | ring 메타 규칙과 같음 | `ClientNameTooLong` |
| `cursor::DEFAULT_MAX_LINES` / `DEFAULT_MAX_BYTES` | 100 / 16 KiB | §3.6, A7 | 요약 또는 page |
| `cursor::HARD_MAX_LINES` / `HARD_MAX_BYTES` | 1 000 / 64 KiB | §3.6 | 요청값을 clamp |
| `cursor::MIN_MAX_BYTES` | 4 KiB | (신규) 512자 한 줄은 늘 들어가야 함 | 요청값을 clamp |
| `cursor::SUMMARY_HEAD_LINES` | 20 | §3.6 | 머리 = min(20, max_lines/2), 바이트는 max_bytes/2까지 |
| `cursor::FEED_STEP_BYTES` | 16 KiB | §3.13 "청크 ≤16 KiB" | 한 번에 파이프라인에 넣는 양(줄 batch 상한) |
| `matcher::MAX_PATTERN_BYTES` | 2 048 | §7.4 | `PatternTooLong` |
| `matcher::REGEX_SIZE_LIMIT` / `REGEX_DFA_SIZE_LIMIT` | 256 KiB / 256 KiB | §7.4 | `CompileTooBig` |
| `matcher::REGEX_NEST_LIMIT` | 64 | §7.4 | `Syntax` |
| `matcher::WINDOW_BYTES` | 64 KiB | §7.4, §3.13 expect | 오래된 바이트를 창에서 밀어냄(창 밖 무손실은 약속하지 않음) |
| `matcher::MAX_PATTERNS` / `MAX_WAITERS` | 채널당 8 / 8 | §3.13 expect | admission 거절 |
| `matcher::FEED_STEP_BYTES` | 16 KiB(=창/4) | (신규) | 큰 feed를 나눠 넣어, 모든 대기가 보기 전에 바이트가 창을 떠나지 않게 함 |
| `matcher::MAX_HIT_TEXT_BYTES` | 4 KiB | (신규) | 응답 `text`를 `…`로 자름 |

메모리 메모:
- 링 payload와 메타는 `shrink_to_fit` 뒤 **capacity**로 센다.
- 항목마다 고정 slot 오버헤드가 있다: `slot_size` = 152 B × 슬롯 수(기본 32 768이면 약 4.75 MiB). 이 값은 `max_meta`와 따로 `stats()`에 나온다(`slot_capacity` ≤ 2×max_entries, 시험으로 확인). **worker 메모리 예산(§3.13 약 32 MiB)에 이 약 5 MiB를 더해야 한다.**
- matcher가 원시 offset으로 되돌릴 때 쓰는 run 표는 최악 창 바이트 수 × 16 B(약 1 MiB)이고, 보통은 몇 개뿐이다.
- 텍스트 파이프라인 상태는 고정 크기다(줄 텍스트 ≤ 512자 × 2 세그먼트, 이진 감지 bitmap 512 B).

---

## 3. 시험

| 파일 | 내용 |
|---|---|
| `tests/ring_limits.rs` (10) | 기본값=plan; 바이트·건수·메타 상한 각각; 128 B 초과 거절; 호출자 버그 거절; wrap 후·세대 경계 offset 연속; 부분 청크 조회와 gap 위치; source_seq 조회·`last_n_bytes`; **1M push**(기본 상한, 그리고 16 KiB 청크로 바이트가 먼저 묶이는 구성)에서 `stats()`로 entries·payload·meta·slot 상한 유지 |
| `tests/text_pipeline.rs` (16) | LF/CRLF/offset; CRLF가 청크에 걸침; `\r` 덮어쓰기(진행률, `\r\r\n`, `\r ESC[K`); keep_cr; UTF-8을 모든 경계에서 자름·1바이트씩; invalid → `\xNN`; reset 때 잘린 UTF-8; ANSI strip/keep/escape(ESC가 청크에 걸침, OSC·DCS); 4 KiB 초과 escape와 LF 재동기화; 제어 문자; latin-1; 512자 상한; **64 KiB 긴 줄**; 세대 변경·offset 점프 reset; `source_seq_first`; 이진 감지 |
| `tests/text_props.rs` (2 proptest, 각 512회) | 임의 분할 불변(줄·partial·카운터)이 모든 ansi × encoding × keep_cr × 작은 상한 조합에서 성립; 줄이 입력을 빈틈·겹침 없이 덮고 상한을 지킴 |
| `tests/cursor_read.rs` (12) | Summary 상한 안 + partial 미소비; Summary 머리 20 + 꼬리 + omitted + 끝까지 전진; Page 3회로 250줄을 빠짐없이; Peek 불변 + 토큰 이어 읽기; page 토큰 재전송 시 같은 범위; **같은 이름 동시 갱신 → `RevisionConflict{1,2}`**, 커서 불변; 토큰 위조·손상·다른 키; 링 밖 커서 → `cursor_gap` 후 남은 범위; **하드 상한**(1000줄·64 KiB·512자, 세 모드 모두); LRU 256; gap 항목이 줄을 끊음; 이름 검증; binary_hint |
| `tests/cursor_props.rs` (1 proptest, 256회) | 임의 청크 링 + 임의 page 크기 + 모든 ansi/keep_cr에서, page로 끝까지 읽은 줄 = 한 번에 렌더한 줄(순서·offset 동일, 중복·누락 없음); Summary 커서 = 마지막 완결 줄 끝 |
| `tests/matcher_expect.rs` (11 + proptest 256회) | 1바이트씩 넣은 literal 경계; regex 경계; 가장 이른 시작 우선; strip_ansi이면서 원시 offset 유지(끄면 매치 없음); Prompt 끝 앵커; 상한 각각(길이·CompileTooBig·backreference·look-around·nest·빈 패턴·패턴 8·대기 8·중복 id·effective clamp); **8 대기 동시 매치**; **gap/세대 reset 뒤 이전 바이트와 이어 붙이지 않음**(offset 점프 포함); 창 앞 from_offset → 창 시작부터, 미래 offset → 대기; 1.2 MB 입력에서 창·매핑 상한; **병적 패턴 시간 측정**; proptest: literal·regex 매치가 분할과 무관 |

추가 확인: `PROPTEST_CASES=5000` 릴리스 빌드로 text_props / cursor_props / matcher proptest를 돌려 모두 통과했다.

**병적 패턴 `(a|aa)*b`, 'a' 64 KiB 측정**(Apple Silicon 개발 머신):

| 빌드 | 4 KiB × 16회 feed(매번 창 전체 `find`) | 64 KiB 창 전체 단일 `find` |
|---|---|---|
| debug | 15.1 ms | 1.84 ms |
| release | 1.18 ms | 0.119 ms |

regex crate는 선형 시간(O(m×n))이라 폭발하지 않는다. 다만 이 수치는 이 패턴 하나의 값이고, 허용되는 모든 패턴의 최대 지연으로 일반화하지 않는다(§7.4).

**참고 측정:** 링 15 MiB를 가득 채운 상태(약 23.6만 줄)에서 Summary read 한 번이 debug 1.67 s, release 203 ms 걸린다. 끝까지 디코드해야 커서를 끝으로 옮길 수 있기 때문이다. P1에서는 read를 reader나 TX 스레드가 아닌 곳에서 실행해야 한다.

### 최종 명령 출력(끝부분)
```
$ cargo fmt -p loom-console -- --check          → 출력 없음(통과)
$ cargo clippy -p loom-console --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.14s
$ cargo test -p loom-console
     Running tests/cursor_props.rs   test result: ok. 1 passed; 0 failed; ... finished in 1.55s
     Running tests/cursor_read.rs    test result: ok. 12 passed; 0 failed; ... finished in 0.19s
     Running tests/matcher_expect.rs test result: ok. 12 passed; 0 failed; ... finished in 0.07s
     Running tests/ring_limits.rs    test result: ok. 10 passed; 0 failed; ... finished in 0.25s
     Running tests/text_pipeline.rs  test result: ok. 16 passed; 0 failed; ... finished in 0.01s
     Running tests/text_props.rs     test result: ok. 2 passed; 0 failed; ... finished in 2.10s
     (sanitize_*/arbiter_* 도 모두 ok — 다른 작업자 소유)
cargo test -p loom-console  → 컴파일 포함 24.7 s wall (60 s 이내)
$ cargo check -p loom-console --no-default-features → Finished
```

---

## 4. 계획·과제와 다르게 한 점과 이유

1. **`From` → `ReadFrom`.** `From`이라는 이름은 `std::convert::From`을 가린다. 변형(variant)은 그대로다.
2. **필드·변형 추가.**
   - `Line.elided_chars`, `GapMark.{generation, mono_ns}`(가장 오래된 `mono_ns` 통계와 세대 표시용)
   - `Omitted.{from_rx_offset, to_rx_offset, at_line}`, `CursorGap.exact`
   - `ReadResult.{from_rx_offset, next_rx_offset, revision, evicted_client}`(LRU 제거 보고)
   - `MatchError::{EmptyPattern, DuplicateWaiter}`, 오류마다 패턴 `idx`
   - `Matcher::poll()`: `add` 전에 이미 창에 있던 바이트를 검사한다(`add` 시그니처가 `Result<(), _>`로 고정이라서).
3. **끝의 미완결 줄은 소비하지 않는다.** 결과에는 `complete=false`로 보여 주지만, 커서는 마지막 *완결* 줄 끝까지만 간다. 그래서 `login: ` 프롬프트가 다음 read에서 `login: root`로 완결된 채 다시 보인다. 과제의 Summary "커서는 끝까지 전진"은 "읽은 범위의 마지막 완결 줄 끝까지"로 해석했다. 64 KiB 초과로 끊긴 줄, gap이나 세대에 잘린 줄은 소비한다.
4. **`\r` 덮어쓰기는 세그먼트 단위다.** `keep_cr=false`에서는 단독 `\r`이 줄을 끝내지 않고 새 덮어쓰기 세그먼트를 연다. LF에서 **마지막 비어 있지 않은 세그먼트**를 낸다. 열 단위 터미널 합성이 아니므로 `abc\rX` → `X`. `\r\r\n`과 `\r ESC[K …`도 처리한다. `keep_cr=true`에서는 단독 `\r`이 줄 종결자다(세그먼트마다 한 줄). CR만 쓰는 장치는 기본 모드에서 64 KiB 상한까지 한 줄로 합쳐지므로 `--keep-cr`가 필요하다.
5. **에이전트 텍스트에서는 CR·LF가 진행 중인 ESC/CSI를 끝낸다**(터미널은 실행만 하고 시퀀스를 이어 간다). cursor 이음매 proptest가 찾은 실제 결함 때문이다. `ESC LF LF a LF`를 한 번에 디코드하면 `a`가 `ESC a`로 묶여 사라지지만, 줄 경계부터 다시 디코드하면 `a`가 보였다. 이 규칙으로 모든 줄 경계가 깨끗한 재동기화 지점이 된다. 4 KiB 초과 escape도 LF에서 재동기화한다(미종결 OSC가 이후 출력을 영영 숨기지 않게). 렌더 정제기(sanitize)와는 별개다.
6. **Keep/Escape 모드에서 시퀀스 안의 비ASCII 바이트는 `\xNN`으로 표시한다.** OSC 문자열 속 UTF-8도 그렇다(시퀀스를 버퍼링하지 않기 위해).
7. **`Pattern::Prompt`는 `(?:p)\z`, 즉 받은 데이터의 끝에 앵커한다.** "줄 끝 앵커"를 "프롬프트가 지금 마지막으로 출력된 것"으로 해석했다. 한 청크에 프롬프트와 그 뒤 에코가 함께 오면 매치되지 않는다(휴리스틱 — §4.5 `--settle`과 같은 성격).
8. **regex는 feed 단계(최대 16 KiB)마다 대기 시작점부터 창 끝까지 다시 `find`한다**(§7.4 "창 처음부터 재검색"). 비용은 대기 수 × 창 크기다. 10 ms 합치기(coalescing)는 P1 worker 몫이다.
9. **링 밖 `SourceSeq`.** 링에서 이미 제거된 `source_seq`는 RX offset을 알 수 없다. 그래서 `cursor_gap{lost_from:0, exact:false}`를 내고 보존 범위 시작부터 읽는다. 디스크 조회는 P1이다.
10. **read 범위 안의 gap 항목**은 파이프라인을 reset하고, 그 앞 줄을 `complete=false`로 끊는다. `ReadResult`에 gap 목록은 넣지 않았다. 커서 offset만으로는 경계에 있는 gap을 매번 다시 보고하게 되기 때문이다(아래 질문 3). 손실 사실은 gateway의 gap 이벤트가 전달한다.
11. **아직 없는 것:** `--include-tx` 텍스트 read, `--tail N`(줄 단위)은 구현하지 않았다(ring의 `last_n_bytes`만 있음). 처음 오는 클라이언트의 `ReadFrom::Last`는 보존된 가장 오래된 바이트부터 읽는다(요약 모드가 길이를 제한함).
12. **바이트 비용은 `text.len()+1`이다(JSON 직렬화 크기가 아님).** 하드 상한도 이 기준이다. 요약의 머리는 `max_bytes/2`까지만 쓴다.
13. **토큰 tag는 SipHash(store별 무작위 키)다.** 손상과 위조를 막지만 암호학적 MAC은 아니다. 키가 프로세스마다 바뀌므로 데몬을 재시작하면 기존 토큰은 `BadToken`이 된다. §3.3의 epoch 변경 → `E_CURSOR_GAP`과 같은 결과다. `with_token_key`로 키를 저장·복원할 수 있다.
14. **링의 엄격 검사.** 빈 청크, TX에 `rx_offset`이 있음, RX에 `rx_offset`이 없음, offset이나 seq 역행은 모두 `Err`다(조용히 고치지 않음).

---

## 5. 리드에게 묻는 것

1. **미완결 줄 정책(§4 3).** 커서가 끝의 미완결 줄 앞에서 멈추고 다음 read에서 다시 보여 준다. 이 동작이 §3.6 "끝까지 전진" 의도와 맞는가? 아니면 미완결 줄까지 소비하고, 다음 read 첫 줄에 `continued` 표시를 다는 쪽을 원하는가?
2. **`max_bytes` 계수 기준.** 지금은 텍스트 바이트 + 1이다. 실제 JSON 응답 64 KiB를 지키려면 줄마다 메타 필드를 직렬화한 크기를 더해야 한다. 이것을 P1 gateway에서 할지, 여기서 줄당 고정 오버헤드(예: 96 B)를 더할지 정해 주면 맞추겠다.
3. **read 결과에 gap 목록이 필요한가?** 필요하다면 커서에 `rx_offset`과 함께 마지막으로 본 `source_seq`를 저장해서, 경계에 있는 gap을 중복 보고하지 않게 하겠다.
4. **Prompt 앵커.** `\z`(데이터 끝)와 `(?mR:…$)`(아무 줄 끝, CRLF 인식) 중 무엇으로 할지는 T09 프로필 regex 이행 시험 결과에 맞춰 바꿀 수 있다. 지금은 `\z`다.
5. **예산.** 링 slot 고정 오버헤드 약 5 MiB(§2 메모리 메모)를 §3.13 worker 예산표에 더해 달라.
