DONE

# P1 보고: `loom-uart-worker::channel` — ChannelCore (T08 일부 + T09 read/expect 엔진)

작업자: uart-core · worktree `/Users/ctmctm/workspace/Loom/.worktrees/uart`(uart/main) · `CARGO_TARGET_DIR=$HOME/.cache/loom-target/uart-core` · 커밋·stash 안 함.

## 0. 먼저 볼 것 — 계약과 다른 점 (worker 작성자 uart-opus 필독)

**과제 공개 API의 이름·시그니처·의미는 그대로다.** 바뀐 것은 아래뿐이다.

1. **`MAX_READ_SCAN`을 4 MiB가 아니라 128 KiB로 정했다.**
   - 디코드 속도가 debug 약 10 MB/s, release 약 80 MB/s다. 그래서 4 MiB면 채널 mutex를 debug 약 430 ms, release 약 55 ms 동안 잡는다. "debug 1회 < 50 ms" 목표와 동시에 지킬 수 없다.
   - 128 KiB는 보통 줄 약 1 500개에 해당한다.
   - `LINES_SCAN_BYTES`와 `RUN_SCAN_BYTES`도 같은 이유로 128 KiB다.
2. **`read`는 예산을 넘어도 `Err(ScanLimit)`를 내지 않는다.** 결과 안에 `scan_limit{code:"E_SCAN_LIMIT", budget_bytes, resume_from}`을 넣는다(과제의 "E_SCAN_LIMIT 성격의 결과 + resume_from").
   - Page/Peek: 예산 끝에서 멈춘다. `more_available:true`, `resume_from` = 커서가 간 위치. 예산 끝에 걸린 가짜 미완 줄은 버린다.
   - Summary: **최신 128 KiB만** 줄 경계부터 디코드한다. 그 앞은 `skipped{from_rx,to_rx,bytes,reason:"scan_limit"}`로 알리고, `resume_from`은 skipped의 시작이다. 커서는 끝까지 간다(plan §3.6 "커서는 끝까지 전진 — 폭주 반복 수신 방지"). 건너뛴 부분은 `--from off:N --page`로 계속 읽을 수 있다.
   - `ChannelError::ScanLimit`은 expect backfill에서만 나온다.
3. **추가 API**(기존 이름은 건드리지 않음):
   - `set_producer(id, epoch)`: gap payload의 `producer_range`에 `producer_id`/`producer_epoch`를 채운다. **부르지 않으면 `{from,to}`만 들어가서 `GapData` 역직렬화에 실패한다. 현재 worker는 payload를 그대로 `record.gap`/`display.gap`에 쓰므로, open 때 꼭 불러 달라.**
   - `commit_cursor(client, rx_offset)`: 앞으로만 움직인다. `send --expect`(ExpectFrom::Now)가 성공했을 때 worker가 그 client의 커서를 매치 끝으로 옮기는 데 쓴다(plan §4.5). `ExpectFrom::Cursor`는 코어가 직접 옮긴다.
   - 파서·헬퍼: `ReadQuery::from_params(&Value)`, `LinesQuery::from_params(&Value)`, `ExpectFrom::from_spec(Option<&str>, client)`, `PosSpec::parse`, `PatternSpec::{literal, regex, prompt}`, `ChannelError::detail()`, `Eol::as_str()`, `impl Default for Profile`(= generic).
   - 다시 내보내는 것: `MatchHit`, `MatchError`, `ReadFrom`, `ReadMode`, `AnsiMode`, `Encoding`.
4. **`ReadQuery`·`LinesQuery` 모양은 과제에 없어 내가 정했다**(아래 §1).

## 1. 공개 API

```rust
// channel/profile.rs
pub enum Eol { None, Cr, Lf, CrLf }  // bytes(), parse("none|cr|lf|crlf"), as_str()
pub struct Profile { pub name: String, pub prompt: Option<String>, pub line_kill_ctrl_u: bool, pub eol: Eol }
impl Profile { pub fn named(&str) -> Option<Profile> }   // generic `[#$>] ?$`·^U 없음 / linux-shell `[#$] $`·^U / uboot `=> $`·^U, eol 모두 Cr

// channel/query.rs
pub enum PosSpec { Last, Now, RxOffset(u64), SourceSeq(u64) }  // "@last" | "now"/"-0s" | "off:N" | "src:N"/"source_seq:N"; "seq:N"·mark·시각 → E_USAGE(gateway 몫)
pub struct ReadQuery { from: ReadFrom, until: Option<u64>, mode: ReadMode, max_lines: Option<u32>, max_bytes: Option<u32>,
                       ansi: AnsiMode, keep_cr: bool, encoding: Encoding }       // Default = Last/Summary/strip; from_params
pub struct LinesQuery { from: Option<u64>, until: Option<u64>, limit: u32 /*≤2000, 기본 500*/, dir: Option<Dir>, ansi, keep_cr } // from_params
pub enum ExpectFrom { Now, RxOffset(u64), Cursor(String) }                       // from_spec
pub enum PatternKind { Literal /*기본*/, Regex, Prompt }
pub struct PatternSpec { kind: PatternKind, pattern: String }   // serde {"kind","pattern"}; prompt + 빈 pattern = 프로필 prompt

// channel.rs
pub struct Ingest { data_payload: Value, hits: Vec<MatchHit>, prompt_seen: bool, alt_screen: Option<bool> }
pub enum ChannelError { Usage(String), ScanLimit{resume_from}, CursorGap{earliest}, Pattern(MatchError) }  // code(), detail(), thiserror Display
impl ChannelCore {
  new(channel, profile, generation) / set_producer / channel() / generation() / profile() / rx_end()
  on_rx(&RxChunk) -> Ingest / on_lost(&LostRange) -> Value / on_tx(&TxChunk) -> Value / new_generation(u32)
  read(client, &ReadQuery) -> Result<Value> / commit_cursor(client, off) / lines(&LinesQuery) -> Result<Value>
  expect_add(id, &[PatternSpec], ExpectFrom) -> Result<Vec<MatchHit>> / expect_remove(id)
  expect_result(&hit, started_rx) -> Value / expect_timeout(id, started_rx) -> Value / run_output(sent, &hit, started_rx) -> Value
  status() -> Value
}
```

JSON 규칙:
- u64 offset과 seq는 **10진 문자열**이다(`uart.data` `rx_offset`과 같음).
- stream `seq`는 쓰지 않는다. `source_seq`만 넣고, gateway가 seq로 바꾼다. 그래서 omitted의 키도 `from_source_seq`/`to_source_seq`다.

| 메서드 | 결과 모양 |
|---|---|
| `on_rx` | `data_payload = {bytes{encoding:"base64",data}, len, rx_offset}` |
| `on_tx` | 같은 모양에서 `rx_offset`만 없음 |
| `on_lost` | `{producer_range{from,to[,producer_id,producer_epoch]}, count:"<청크 수>", cause, bytes, detail{channel,generation,rx_from,rx_to,chunks}}` |
| `read` | `{lines:[{text, rx_offset_start, rx_offset_end, source_seq, generation, complete, truncated?, elided_chars?}], truncated, omitted?, more_available, next_cursor, revision, cursor_gap?{lost_from,lost_to,exact,earliest}, binary_hint, gaps?[{layer,cause,rx_offset,bytes,source_seq_from,source_seq_to,generation}], gaps_omitted?, range{from_rx,to_rx}, scan_limit?, skipped?, evicted_client?}` |
| `lines` | `{rows:[rx 줄(read와 같은 필드 + dir:"rx") \| {dir:"tx", text(escape), rx_offset, len, source_seq, generation, origin, client_id, request_id}], range, more_before, more_after, next_from?, cursor_gap?, gaps?, scan_limit?}` |
| `expect_result` | `{matched:true, pattern_index, matched_index, pattern, kind, text, match, rx_offset_start, rx_offset_end, channel, generation(매치 바이트의 세대), started_rx_offset, range, before(≤15줄, "\n"로 연결), before_lines, gaps?}` |
| `expect_timeout` | `{matched:false, cause:"timeout", channel, generation, resume_from, started_rx_offset, scanned_to, patterns, context(마지막 15줄), context_lines, gaps?}` |
| `run_output` | `{matched:true, pattern_index, text, prompt, rx_offset_start/end, output, lines, echo_stripped, prompt_stripped, truncated, omitted_lines?, skipped?, range, generation, binary_hint, gaps?}` |
| `status` | `{channel, generation, profile{…, prompt_error}, rx_end, retained_from, retained_from_source_seq, last_source_seq, oldest_mono_ns, rx{bytes,chunks}, tx{…}, lost{events,chunks,bytes}, ring{entries,payload_bytes,meta_bytes,slot_bytes,evicted_*,max_*,rejected}, expect{pending,patterns,max_waiters,max_patterns,window_bytes}, cursors, binary_hint, alt_screen}` |

- `lines`의 `from`이 있으면 거기서부터 앞으로 읽는다. `from`이 없으면 `until`(또는 최신) **앞의 마지막 `limit`행**을 준다(패널이 위로 스크롤할 때).

동작 요점:
- **on_rx**: 처리 순서는 링 push → 공유 matcher(64 KiB 창, 대기 8개/패턴 8개) → 프롬프트 감시 → alt-screen 감지다.
  - 프롬프트 감시는 별도의 1 KiB 창 matcher를 쓰므로 expect 슬롯을 차지하지 않는다. 프롬프트가 나온 뒤에만 다시 컴파일한다.
  - `prompt_seen`은 매치 뒤에 escape 시퀀스만 남았을 때 참이다. 예: 프롬프트 뒤에 `ESC[?2004h`가 붙어도 참이다.
- **alt-screen 감지**: `CSI ? … 47/1047/1049 h|l`만 보는 고정 크기 상태기계다(파라미터 32 B). 청크 경계에 걸쳐도 동작한다. 정제기 전체를 돌리지 않으려고 따로 만들었다.
- **on_lost / new_generation**: matcher, 프롬프트, escape 상태를 리셋한다. 링은 그대로 둔다.
  - on_lost는 링에 `GapMark{layer: Record}`를 넣는다. `rx_end()`에는 잃은 바이트도 포함된다.
  - 대기 중인 expect는 세대가 바뀌어도 살아남아 새 세대의 바이트에 매치된다(`send reboot --expect login:`이 재열거를 거쳐도 동작해야 하므로).
- **expect_add**: 순서는 다음과 같다.
  1. 프로필 prompt를 해석한다.
  2. 시작점을 정한다: Now = `rx_end()`, Cursor = 그 client 커서(없으면 링의 가장 오래된 바이트, read `@last`와 같음).
  3. 링보다 오래되면 `CursorGap{earliest}`.
  4. **공유 matcher에 먼저 등록한다**(입장 검사·컴파일 오류를 먼저 보기 위해).
  5. 별도 matcher로 링을 backfill한다. gap과 세대 경계에서 리셋하므로 이어 붙인 가짜 매치가 생기지 않는다.
  6. 결과: hit면 즉시 반환하고 대기를 제거한다(Cursor면 커서를 매치 끝으로 옮김). 예산을 넘으면 등록을 지우고 `ScanLimit{resume_from}`.
- **resume_from**(plan §4.5):
  - literal: 스캔 끝 − (가장 긴 패턴 길이 − 1).
  - regex·prompt: timeout은 matcher 창의 시작, backfill 초과는 스캔 끝 − min(64 KiB, 스캔량/2)(재시도가 늘 앞으로 가게 함).
  - 어느 경우든 대기 시작점보다 앞으로 가지 않는다.
- **expect_remove 뒤에도** 대기 정보는 최근 16개까지 남는다. worker는 timeout 때 `expect_remove` → `expect_timeout` 순서로 부르는데, 이 순서에서도 패턴과 resume 정보가 맞게 나온다(시험으로 확인).
- **run_output**:
  - `started_rx`부터 프롬프트 매치 시작까지를 렌더한다.
  - 끝의 미완 줄(프롬프트 머리, 예 `root@sim:~`)은 지우고 `prompt`에 붙인다.
  - 첫 줄이 명령과 같거나 명령으로 끝나면 에코로 보고 지운다.
  - 100줄 / 16 KiB를 넘으면 머리 20줄 + 꼬리를 남기고 `omitted_lines`로 센다.

## 2. 상수 (상한과 넘칠 때 정책)

| 상수 | 값 | 근거 | 넘칠 때 |
|---|---|---|---|
| `MAX_READ_SCAN` | 128 KiB | §3.6 "유한 작업 + E_SCAN_LIMIT·이어 읽을 위치" | Page/Peek은 멈추고 `scan_limit.resume_from`. Summary는 최신 128 KiB만 읽고 `skipped` |
| `EXPECT_BACKFILL_SCAN` / `EXPECT_BACKFILL_MIN` | 256 KiB ÷ regex 패턴 수, 최소 64 KiB | §4.5·§7.4 byte·작업 예산 | `ScanLimit{resume_from}`, 등록 안 됨 |
| `LINES_SCAN_BYTES` | 128 KiB | §3.13, A5(큰 범위는 HTTP/raw_ref) | `scan_limit`, `more_before`/`more_after` |
| `LINES_MAX_BYTES` | 64 KiB(행 텍스트) | §3.6 하드 64 KiB, RPC 64 KiB | 앞으로 읽기는 멈추고 `next_from`. 뒤로 읽기는 오래된 행부터 버림 |
| `LINES_MAX_LIMIT` / `LINES_DEFAULT_LIMIT` | 2000 / 500 | UI 계약 `limit≤2000` | 범위 밖이면 `E_USAGE` |
| `CONTEXT_LINES` / `CONTEXT_SCAN_BYTES` | 15 / 16 KiB | §4.5 `--context 15` | 더 오래된 줄은 생략 |
| `RUN_SCAN_BYTES` | 128 KiB | §4.6 + 위 성능 이유 | 머리를 자르고 `skipped`, `truncated` |
| `RUN_MAX_LINES` / `RUN_MAX_BYTES` / `RUN_HEAD_LINES` | 100 / 16 KiB / 20 | §4.6 `--max-lines 100`, §3.6 | 머리 20 + 꼬리, `omitted_lines` |
| `FINISHED_WAITERS_MAX` | 16 | (신규) | 가장 오래된 것부터 잊음 |
| `TX_ROW_CHARS` | 512 | §3.6 한 줄 512자 | `…[+N B]` |
| `render::MAX_GAPS_REPORTED` | 32 | §3.13 | `gaps_omitted` 개수 |
| `render::LINE_ALIGN_SCAN` | 64 KiB | text `MAX_PARTIAL_LINE_BYTES` | 줄 경계를 못 찾으면 그 자리에서 시작 |
| `prompt::PROMPT_WINDOW_BYTES` | 1 KiB | matcher 최소 창 | 창 밖은 버림 |
| `alt::ALT_PARAM_BYTES` | 32 | §3.13 | 그 시퀀스는 무시 |
| 링·matcher·커서 | loom-console 기본값(16 MiB/32 768/4 MiB, 창 64 KiB·대기 8·패턴 8, 커서 256) | P0 보고서 | P0 정책 그대로 |

## 3. 시험

| 파일 | 내용 |
|---|---|
| `tests/channel_core.rs` (20) | 모두 `sim::Bootshell`/`sim::Binary`의 Device API를 직접 구동했다(transport 없음). 청크 분할은 `[1]`, `[3,5,64]`, `[512,37,4096]` 등을 섞었다. |
| | login 프롬프트를 cursor expect로 backfill → hit, 커서가 매치 끝으로 감, `before` 15줄 |
| | `send --expect`(Now)가 이미 찍힌 `login:`에는 매치하지 않고 응답 `Password:`에 매치 |
| | 프로필 prompt expect와 `prompt_seen` |
| | 에코·부팅 로그·reboot 중에는 `prompt_seen`이 거짓 |
| | **run**: `uname -a` 출력에서 에코·프롬프트 제거, meminfo 10줄(1바이트 분할 포함) |
| | **read**: Summary 100줄 + omitted, `login: `은 소비하지 않음, Page 40줄씩 끝까지 = 한 번의 Peek와 동일(연속성 확인), Peek 불변, 토큰 재전송으로 같은 범위, 잘못된 토큰 → Usage |
| | `until`과 `src:N` |
| | **lines**: 마지막 N행에 TX 행 포함(`root\r`, origin·client), 앞으로 `next_from` 연결, `dir` 필터, 커서 수 변화 없음 |
| | **timeout**: 커서 불변, literal `resume_from` = 끝 − 5, context, regex resume가 시작점보다 앞으로 가지 않음 |
| | **경계**: base64 payload, alt-screen(청크 경계 포함), `sim::binary`의 binary_hint |
| | **세대 변경 후 가짜 매치 없음**(live와 backfill 모두), 새 세대에서 대기 3개 발화, `generation` 2 |
| | literal·regex를 1바이트씩 넣어도 찾음 |
| | **on_lost**: gap payload, 줄이 gap에서 끊김, read에 `gaps` 1회만, 매치가 gap을 넘지 않음, lines에도 gaps, status lost, `set_producer` |
| | **예산**: backfill 예산 초과 → `resume_from`으로 반복해 결국 찾음(literal·regex, 매번 진척) |
| | Page가 예산에서 멈추고 이어 읽어 100줄을 빠짐없이 읽음 |
| | Summary 예산 초과 → `skipped`, 줄 경계 정렬, 커서가 끝에 있음 |
| | 33 000 청크로 링 축출 → expect `CursorGap{earliest}`, read·lines의 `cursor_gap` |
| | **오류 코드**: 문법·backreference·빈 패턴·대기 9번째·중복·긴 이름·prompt 없는 프로필 |
| | 프로필·Eol·uboot `=> ` 감지, RPC params 파싱(`seq:N` 거절, limit 2001 거절, 패턴 serde) |
| `tests/channel_perf.rs` (1) | **16 MiB 꽉 찬 링**(17 MiB를 4 KiB 청크로 넣어 축출 발생)에서 모든 메서드를 최악 입력으로 3회씩 재고, **최솟값 < 50 ms를 단언**한다(병렬 빌드의 스케줄러 잡음 제거용, 3회 값 모두 출력). |
| 단위(`src/channel/*`) (5) | alt 감지 2, prompt 감시 2, escape 미리보기 1 |

**측정값**(debug, Apple Silicon, 다른 작업자 빌드로 load avg 8–11, 3회 중 최소):

| 호출 | ms |
|---|---|
| on_rx 16 KiB, regex 대기 8개(창 64 KiB 전체 재검색) | 4.0 |
| on_rx 16 KiB, 대기 없음 | 0.9 |
| on_tx / on_lost / expect_result | ≤ 0.01 |
| read Summary(16 MiB 밀림, 예산으로 잘림) | 21.8 |
| read Page 하드 상한(1000줄/64 KiB) | 13.3 |
| read Peek | 1.4 |
| lines 마지막 2000행 | **24.2(최악)** |
| lines 앞으로 2000행 | 15.3 |
| expect backfill regex 1개 / regex 8개 / literal | 11.3 / 9.4 / 6.2 |
| run_output(1 MB 출력, 예산으로 잘림) | 14.6 |
| expect_timeout / status | 1.9 / 0.4 |

참고로 같은 측정에서 4 MiB 예산 Summary는 428 ms였다(§0 1).

명령 출력(끝부분):
```
$ rustfmt --edition 2024 --check crates/loom-uart-worker/src/channel.rs tests/channel_core.rs tests/channel_perf.rs  → 통과
$ cargo clippy -p loom-uart-worker --all-targets -- -D warnings      → 경고 0
$ cargo test -p loom-uart-worker
  unittests src/lib.rs     test result: ok. 44 passed
  tests/channel_core.rs    test result: ok. 20 passed; finished in 0.40s
  tests/channel_perf.rs    test result: ok. 1 passed; finished in 1.78s
  tests/pty.rs / sim_transport.rs / tx.rs   ok (6 / 5 / 9)
  tests/worker_e2e.rs      9 passed; 1 FAILED (ctrl_c_cuts_a_large_agent_send — uart-opus 작업 중 파일, 아래 참고)
  전체 약 9 s
```

- `cargo fmt -p loom-uart-worker -- --check`의 diff는 uart-opus가 작업 중인 `worker.rs`, `worker/*.rs`, `tests/worker_e2e.rs`에서만 난다. 내 파일은 rustfmt로 깨끗하다.
- `worker_e2e::ctrl_c_cuts_a_large_agent_send`는 사용자 Ctrl-C의 TX `uart.data`가 `origin=user`로 발행됐는지 본다. origin은 worker `tx_loop`의 draft에서 정해지고, `on_tx`의 payload와는 관계없다.

## 4. 계획·과제와 다르게 한 점과 이유

1. **예산과 read 반환 방식**: §0 1·2.
2. **backfill 예산을 regex 패턴 수로 나눴다.** regex는 feed 단계마다 창(≤ 64 KiB)을 다시 검색하므로, 실제 일은 바이트 × regex 수다. 최소값은 64 KiB다. regex 초과 시 resume 겹침을 min(64 KiB, 스캔량/2)로 했다. 창 시작으로 하면 64 KiB 예산에서 재시도가 앞으로 가지 않는다.
3. **"같은 세대 안에서만"의 해석**:
   - 매치 텍스트가 gap이나 세대 경계를 **넘지 않는다**(live와 backfill 둘 다 그 지점에서 리셋).
   - 대기 중인 expect는 새 세대에서도 매치된다(reboot·재열거 시나리오).
   - backfill은 이전 세대 안에 완결된 매치를 찾을 수 있다. 결과의 `generation`으로 구분할 수 있다. 더 엄격하게 "현재 세대 바이트만" 보려면 한 줄만 바꾸면 된다(질문 2).
4. **커서가 없는 새 client의 `expect`(Cursor)**는 링의 가장 오래된 바이트부터 backfill한다(read `@last`와 같다). 그래서 `open → expect 'login:'`이 이미 지나간 프롬프트를 찾는다. 링이 크면 `ScanLimit`이 나오고, 이때는 `--from now`를 쓴다.
5. **커서 revision 충돌은 `E_USAGE`로 매핑했다**(전용 코드가 없음). 메시지에 "같은 client 이름을 쓰는 다른 프로세스"라고 쓴다.
6. **TX `wall_ns`**: `TxChunk`에 wall 시각이 없다. 그래서 마지막 RX 청크의 (wall − mono) 차이를 더해 추정하고, RX가 아직 없으면 0이다. 정확한 값이 필요하면 `TxChunk`에 `wall_ns`를 넣어 달라(tx.rs 소유자).
7. **lines의 TX 행 위치**: TX 행은 "그 시점 RX 끝" 위치(`rx_offset`)에 놓인다. 순서는 방출 순서다. RX 줄은 줄이 완결될 때 나오므로, TX 앞에서 시작해 뒤에서 끝난 줄은 TX 행 뒤에 온다.
8. **read `--include-tx`와 `--tail N`은 이번에도 없다**(P0와 같음).
9. **`read`의 `from:"now"`**는 `ReadFrom::RxOffset(u64::MAX)`로 처리한다(현재 끝으로 clamp). 아무것도 읽지 않고 커서를 끝으로 옮긴다.
10. **gap 보고는 반열린 구간 `[from, to)`다.** 읽은 범위 끝에 정확히 놓인 gap은 다음 read가 보고한다. 그래서 같은 gap을 두 번 보고하지 않는다(P0 질문 3의 답 "gaps 추가"를 이렇게 구현).

## 5. 리드에게 묻는 것

1. **read 예산**: 128 KiB를 받아들일지, 루트 `Cargo.toml`(CORE 소유)에 `[profile.dev.package.loom-console] opt-level = 2`를 넣고 예산을 올릴지 정해 달라. 텍스트 파이프라인이 바이트 단위라 debug에서 특히 느리다. release 기준으로도 4 MiB는 약 55 ms다.
2. **"매치는 같은 세대 안에서만"**: §4 3의 해석(경계를 넘지 않음, 이전 세대 안의 완결 매치는 backfill로 허용)이 의도와 맞는가?
3. **live regex 비용**: P0 matcher 설계상 regex 대기는 on_rx마다 창 전체(≤ 64 KiB)를 다시 검색한다. 청크가 작고 잦으면(115200 baud에서 수십 B짜리 청크가 초당 수백 개) regex 대기 8개 기준으로 debug에서 청크당 수 ms가 든다. 펌프가 `pop_batch`로 받은 연속 RX 청크를 10 ms 단위로 묶어 넣는 것(plan §7.4 coalescing)을 worker에 권한다. 필요하면 ChannelCore에 `on_rx_batch`를 추가하겠다.
4. **uart-opus에게 전달할 것**:
   - open 때 `set_producer(producer_id, producer_epoch)`를 부를 것.
   - `send --expect`·`run` 성공 시 `commit_cursor(client, hit.end_off)`를 부를 것(plan §4.5 "성공 시 커서 = 매치 끝").
   - read/lines/expect params 파싱에 `ReadQuery::from_params`, `LinesQuery::from_params`, `ExpectFrom::from_spec`을 쓸 수 있다.
