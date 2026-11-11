docs/plan/02-core-principle.md 준수.
질문 창 금지: 대화형 질문을 띄우지 말고, 스스로 정해 보고서에 적거나 보고서에 질문을 쓰고 끝낸다(playbook §6, p0-common.md).

# P1: `loom-uart-worker::channel` — ChannelCore (T08 일부 + T09 read/expect 엔진)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. CARGO_TARGET_DIR = `$HOME/.cache/loom-target/uart-core`.
보고서: `/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-channel-core.md`.

## 수정 가능한 파일 (이것만)
- `crates/loom-uart-worker/src/channel.rs` (하위 `src/channel/` 허용)
- `crates/loom-uart-worker/tests/channel_*.rs`

`src/worker.rs`는 uart-opus가 **동시에** 이 API를 써서 만든다 — 아래 공개 API 이름·의미를 지킬 것(바꿔야 하면 보고서 첫 절에 크게 쓴다). 다른 모듈(transport·reader·bridge·tx·sim)과 `loom-console`은 읽기만. Cargo.toml·lib.rs는 리드 소유(필요한 의존성은 이미 있음: `loom-console`, `loom-proto`, `base64`, `hex`, `serde_json`).

## 목적
채널 하나(포트 하나)의 **동기·순수 상태**: RX/TX 청크를 받아 링·텍스트·매처를 갱신하고, 에이전트 `read`/`expect`/패널 `lines`에 답하며, 발행할 이벤트 payload를 만든다. 스레드·I/O·시계·async 없음(시각은 청크에 들어 있음). worker가 `Mutex`로 감싸 reader 펌프와 RPC 핸들러가 함께 쓴다 — **모든 메서드는 짧게 끝나야 한다**(상한 있는 작업만; 링 전체 Summary 읽기가 debug 1.6 s였던 점 — `p0-console-core.md` 측정 — 을 고려해 read는 스캔 바이트 예산 `MAX_READ_SCAN` 4 MiB를 두고 넘으면 `E_SCAN_LIMIT` 성격의 결과 + `resume_from`).

## 공개 API (계약)
```rust
pub struct Profile { pub name: String, pub prompt: Option<String> /*regex*/, pub line_kill_ctrl_u: bool, pub eol: Eol }
impl Profile { pub fn named(name: &str) -> Option<Profile> }   // generic: prompt `[#$>] ?$`, ^U 아님 / linux-shell: `[#$] $`, ^U / uboot: `=> $`, ^U
pub enum Eol { None, Cr, Lf, CrLf }  impl Eol { pub fn bytes(&self) -> &'static [u8]; pub fn parse(s: &str) -> Option<Eol> }

pub struct ChannelCore { .. }
impl ChannelCore {
    pub fn new(channel: &str, profile: Profile, generation: u32) -> Self;      // RingLimits/MatcherLimits 기본값
    pub fn channel(&self) -> &str;  pub fn generation(&self) -> u32;  pub fn profile(&self) -> &Profile;
    pub fn rx_end(&self) -> u64;                                               // 지금까지 받은 RX 바이트 수
    pub fn on_rx(&mut self, c: &crate::bridge::RxChunk) -> Ingest;
    pub fn on_lost(&mut self, l: &crate::bridge::LostRange) -> Value;                         // 링에 GapMark(Record) + 매처·텍스트 리셋, 반환 = gap payload(계약 02 §2.4 공통 payload, producer_range 사용)
    pub fn on_tx(&mut self, t: &crate::tx::TxChunk) -> Value;                         // 링에 TX 청크, 반환 = `uart.data` payload(dir tx)
    pub fn new_generation(&mut self, generation: u32);                         // 매처·텍스트 리셋, 링은 유지(세대 필드로 구분)
    pub fn read(&mut self, client: &str, q: &ReadQuery) -> Result<Value, ChannelError>;   // `uart.read` 결과 data(JSON)
    pub fn lines(&self, q: &LinesQuery) -> Result<Value, ChannelError>;                   // `uart.lines` (panel trace, 커서 불변, limit ≤ 2000)
    pub fn expect_add(&mut self, id: u64, patterns: &[PatternSpec], from: ExpectFrom) -> Result<Vec<MatchHit>, ChannelError>; // 등록 즉시 링 backfill로 이미 매치되면 hit 반환
    pub fn expect_remove(&mut self, id: u64);
    pub fn expect_result(&self, hit: &MatchHit, started_rx: u64) -> Value;      // `uart.expect` 성공 data + `uart.expect_result` payload 공용
    pub fn expect_timeout(&self, id: u64, started_rx: u64) -> Value;            // timeout data: context 15줄 + resume_from
    pub fn run_output(&self, sent: &[u8], hit: &MatchHit, started_rx: u64) -> Value;      // `uart.run`: 명령 에코 첫 줄·마지막 프롬프트 제거한 출력
    pub fn status(&self) -> Value;                                              // 링 통계·retained_from·rx/tx 바이트·대기 expect 수·binary_hint
}
pub struct Ingest { pub data_payload: Value /* uart.data rx payload */, pub hits: Vec<MatchHit>, pub prompt_seen: bool, pub alt_screen: Option<bool> }
pub enum ExpectFrom { Now, RxOffset(u64), Cursor(String /*client*/) }       // send --expect = Now(송신 직전 watermark), expect 기본 = Cursor(client)
pub struct PatternSpec { pub kind: PatternKind /*Literal|Regex|Prompt*/, pub pattern: String }  // JSON {"kind","pattern"}에서 serde로
pub enum ChannelError { Usage(String), ScanLimit{resume_from: u64}, CursorGap{earliest: u64}, Pattern(MatchError) }  // + fn code(&self) -> loom_proto::error::ErrorCode
```
- `uart.data` payload = `{"bytes":{"encoding":"base64","data":…}, "len":N, "rx_offset":"<u64 10진 문자열>"}`(RX만 rx_offset). 정본 `crates/loom-uart/src/kinds.rs`.
- `prompt_seen`: 이번 청크로 RX 끝이 프로필 prompt에 매치(InputArbiter 자동 반환 힌트). `alt_screen`: RX에 `?1049h/l`·`?47`·`?1047` 진입·해제가 보이면 Some(새 상태).
- read 결과 JSON: `{lines:[{text, rx_offset_start, rx_offset_end, source_seq, complete, truncated?}], truncated, omitted?, more_available, next_cursor, cursor_gap?, binary_hint, gaps?:[…], range:{from_rx, to_rx}}`. stream seq는 모른다(gateway가 source_seq→seq 변환). P0 질문 답: 미완 줄은 보여 주되 소비하지 않음(유지), `max_bytes`는 텍스트 길이 기준(유지 — gateway가 64 KiB JSON 상한을 따로 집행), 결과에 범위 안 gap 목록 `gaps` 추가, prompt 앵커 `\z` 유지.
- expect `from` 이 링 안이면 그 지점부터 링 바이트로 backfill(스캔 예산 256 KiB, 넘으면 `ScanLimit{resume_from}`), 링 밖이면 `CursorGap{earliest}`. 매치는 **같은 세대 안에서만**.
- 에이전트 커서: `read` Summary/Page는 커서 전진, `expect` 성공은 커서를 매치 끝으로(plan §4.5), timeout은 불변.

## 시험 (`tests/channel_*.rs`)
- `sim::bootshell` 장치를 직접 구동해(transport 없이 Device API로 바이트 생성) on_rx로 넣고: login 프롬프트 expect, `send --expect` 시나리오(ExpectFrom::Now), run 출력 정제, read Summary/Page/Peek, lines.
- 청크 경계·세대 변경 후 가짜 매치 없음, on_lost 후 read에 gap 표시, backfill 스캔 상한, binary_hint(`sim::binary`).
- 모든 메서드 1회 호출 시간이 16 MiB 꽉 찬 링에서도 debug 빌드 < 50 ms(read는 예산으로 잘림) — 측정값 보고.

## 끝나면
보고서 첫 줄 `DONE`. 대기.
