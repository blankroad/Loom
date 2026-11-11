docs/plan/02-core-principle.md 준수.

# P0 uart-opus: `loom-console` 정제기(T07) + InputArbiter 상태기계(T07b·T10 핵심)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. CARGO_TARGET_DIR = `$HOME/.cache/loom-target/uart-opus`.
보고서: `docs/impl/uart/reports/p0-opus.md`.

## 수정 가능한 파일 (이것만)
- `crates/loom-console/src/sanitize.rs` (필요하면 `src/sanitize/` 하위 모듈로 나눠도 됨 — 그때 `sanitize.rs`가 `mod` 선언)
- `crates/loom-console/src/arbiter.rs` (같은 규칙으로 `src/arbiter/` 허용)
- `crates/loom-console/tests/sanitize_*.rs`, `crates/loom-console/tests/arbiter_*.rs`, `crates/loom-console/tests/golden/**`
- `docs/impl/uart/console-api.md` (T07b: 공개 API 초안 — UI·EXPLORE 검토용)

`ring.rs`·`cursor.rs`·`text.rs`·`matcher.rs`는 다른 작업자가 동시에 만든다. 건드리지 말 것. 의존성은 이미 `vte =0.15.0`(default-features=false), `loom-proto`, `serde`, `serde_json`, `proptest`(dev)가 있다.

## A. 렌더 정제기 `sanitize` (plan §5.4, §5.5 ①, §5.15.2, §7.5)
웹 패널(WASM)과 보조 TUI가 `term.write` 직전에 쓰는 **바이트 → 바이트** 필터. 서버 스트림은 원본 그대로이고 이 정제기는 그리는 쪽에만 있다.

공개 API(이름은 바꿔도 되나 보고서에 적을 것):
```rust
pub struct SanitizeConfig { pub profile: Profile /* Web | Tui */, pub allow_mouse: bool /* M1 false */ }
pub struct Sanitizer { .. }               // 고정 크기 상태, 청크를 몇 조각으로 나눠 넣어도 결과 동일
impl Sanitizer {
    pub fn new(cfg: SanitizeConfig) -> Self;
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>);   // out에 덧붙임
    pub fn on_gap(&mut self, out: &mut Vec<u8>);              // 파서 리셋 + SGR 리셋(ESC[0m) 출력
    pub fn counters(&self) -> &Counters;                      // 종류별 차단 수(질의, OSC52, OSC8, 제목, 마우스, 포커스, DCS, APC/PM/SOS, 과길이)
    pub fn take_recent(&mut self) -> Vec<Blocked>;            // 최근 차단 목록(고정 링 ≤ 64개): 종류 + 입력 내 바이트 offset
    pub fn alt_screen(&self) -> bool;                         // ?1049/?47/?1047 상태 (InputArbiter 자동 반환 조건)
    pub fn bracketed_paste(&self) -> bool;                    // ?2004 상태 (패널 붙여넣기 확인 조건)
}
```
규칙:
- **통과**: 인쇄 문자, UTF-8(잘린 다바이트는 다음 feed까지 보유 — 최대 3바이트), C0 중 BEL/BS/HT/LF/CR, SGR, 커서 이동·지우기(CUU/CUD/CUF/CUB/CUP/HVP/ED/EL/ICH/DCH/IL/DL/SU/SD/DECSC/DECRC/`ESC 7`/`ESC 8`), DECSTBM(Web은 그대로, Tui는 `rows−1` clamp — rows는 `SanitizeConfig`에 추가해도 됨), DECSET/DECRST 중 커서 표시(?25)·자동 줄바꿈(?7)·alternate screen(?47/?1047/?1049)·bracketed paste(?2004).
- **차단(제거 + 카운트)**: §5.5 ①의 질의 전부 — DA1 `CSI c`/`CSI 0 c`, DA2 `CSI > c`, DA3 `CSI = c`, XTVERSION `CSI > q`, DSR/CPR `CSI 5 n`·`CSI 6 n`·`CSI ? 6 n`, DECRQM `CSI ... $ p`·`CSI ? ... $ p`, `CSI ... t`(창 조작·보고 전부), kitty `CSI ? u`, DCS 전체(DECRQSS `$q`, XTGETTCAP `+q` 포함), APC/PM/SOS, OSC 0/1/2(제목), OSC 4/10/11/12/17/19(색 질의·설정), OSC 8(링크: 시퀀스만 지우고 링크 텍스트는 남김), OSC 52, 그 밖의 모든 OSC; 마우스 추적 DECSET ?9/?1000/?1001/?1002/?1003/?1005/?1006/?1015/?1016(`allow_mouse=false`), 포커스 보고 ?1004; RIS `ESC c`(화면을 통째로 초기화 — Web은 차단하고 카운트, plan §5.15.2 참고); 알 수 없는 CSI final·private 모드는 기본 **차단**(허용 목록 방식). 한 DECSET에 여러 모드가 섞이면(`CSI ? 25 ; 1000 h`) 허용 모드만 남겨 다시 쓴다.
- **상한**: 파서 **앞**에서 하나의 escape/string 길이 4 KiB 제한(plan §7.5). 넘으면 terminator(ST·BEL·CAN·SUB·ESC)까지 버리는 유한 상태 + `overlong` 카운트. vte는 `Parser::<4096>::new_with_size()` 같은 고정 OSC 버퍼를 쓴다(구조가 다르면 보고서에). 어떤 입력에서도 메모리 사용이 입력 길이에 비례해 늘지 않을 것.
- 원시 C0 중 위 목록 밖(ENQ 0x05 포함 — 일부 터미널이 answerback으로 응답)은 제거 + 카운트. DEL 0x7F는 제거.
- **청크 독립성**: 같은 바이트열을 임의 지점에서 나눠 넣어도 `out`과 카운터가 같아야 한다(proptest로 증명).

**골든 코퍼스** `crates/loom-console/tests/golden/sanitize/*.in` + `*.out`(바이트 파일) + `README.md`(형식 설명). UI 팀이 TS 포팅을 할 경우 같은 코퍼스를 쓴다. 최소 사례: §5.5 ① 질의 각 1건, OSC 52·8·제목, 마우스·포커스, 섞인 DECSET, 미종결 긴 OSC(10 KiB), 잘린 UTF-8, 4 KiB 경계 ±1, SGR·커서 이동 통과, alternate screen, bracketed paste, RIS, ENQ. 골든 비교 시험 + 청크 분할 proptest + adversarial(임의 바이트 1 MiB에서 panic 없음·출력 ≤ 입력 + gap 리셋 길이).

**WASM**: 이 모듈은 `std::time`·스레드·파일을 쓰지 않는다(`loom-console`을 `--no-default-features`로 wasm32에 빌드할 수 있게). 지금 머신에 wasm32 target이 없으니 빌드 확인은 하지 않아도 된다 — 대신 의존 모듈(`text` 등)을 쓰지 말고 `sanitize`만으로 닫힌 모듈로 둘 것.

## B. InputArbiter 상태기계 `arbiter` (plan §3.7 1–7, §5.3, §5.8, §3.13 TX 행)
worker 안에서 채널 하나의 입력 중재를 맡는 **순수 상태기계**(I/O·스레드·시계 없음, 시각은 `now_ms: u64`로 주입). TX actor·RPC 연결은 P1에 리드가 붙인다.

다룰 것:
- 입력권 `input_owner ∈ {none, user(viewer_id), agent(client)}`, writer viewer 하나, `input_dirty`(채널 상태 — viewer가 떠나도 남음), `hold`, 에이전트 락(`ttl`, 사유, 상한 10분, 호출마다 연장), takeover(보류 키 5 s 안에 가져오기, 아니면 폐기 + 알림), 자동 반환 조건(마지막 사용자 바이트가 CR/LF/^C/^U **그리고** (프로필에 프롬프트가 있으면 프롬프트 재출현 / 없으면 1.5 s 무입력) **그리고** alternate screen 아님), `Ctrl-] y` 두 번 규칙과 `line_kill="^U"` 검증 프로필에서만 ^U 송신 후 dirty 해제.
- 사용자 입력 `user_input(viewer, bytes, now) -> InputDecision { send: Vec<u8> /*지금 보낼 것*/, held: bool, discarded: .., reason }`; Ctrl-C(0x03)는 어느 viewer든·락 중이든 즉시 통과하고 락을 깬다(사용자 자신의 앞선 키 뒤 순서 유지).
- 에이전트 송신 `agent_send(client, request_id, len, defer_timeout_ms, now) -> AgentAdmit::{Now, Deferred{until}, Reject(code)}`: 사용자 입력권 중이면 Deferred, `hold`면 `E_HELD_BY_USER`, 다른 클라이언트 락이면 `E_LOCKED`, dirty면 Deferred 후 timeout에 `E_USER_ACTIVE{reason:"dirty_line", user_line_len}`(사용자 입력 내용은 넘기지 않음). 대기 중 상태는 `tick(now)`가 판단해 `AgentAdmit` 결과를 이벤트로 낸다.
- 오류 코드는 `loom_proto::error::ErrorCode`의 기존 값만 쓴다(`UserActive`, `HeldByUser`, `Locked`, `LockBroken` 등 — 이름은 crate에서 확인).
- 큐 상한(§3.13 TX 행): 사용자 보류 4 KiB·64항목, Ctrl-C 예약 64 B·64항목 별도, 에이전트 대기 64요청. 넘치면 사용자 입력은 `held/discarded` + 이유(조용한 drop 금지), 에이전트는 `E_RATE_LIMIT`.
- **출력 이벤트**: 상태가 바뀔 때마다 `ArbiterEvent`를 낸다. UI 팀이 이미 mock으로 쓰는 payload(`ui/src/panels/uart/uartState.ts` 머리 주석, worktree 경로)와 맞출 것:
  - `uart.input_owner {owner:"none"|"user"|"agent", viewer?, dirty, hold}`
  - `uart.hold {on}`
  - `uart.lock {on, reason?, ttl_ms?, actor?}`
  - `uart.agent_activity {kind:"deferred_send"|"expect"|"idle", text?, elapsed_ms?, timeout_ms?}` (expect는 여기서 안 냄 — 종류만 정의)
  - `uart.input_ack {input_seq, state:"sent"|"held"|"discarded", reason?}` — `input_seq`는 호출자가 넘김
  이벤트는 `serde_json::Value` payload를 만드는 함수 또는 `Serialize` 구조체로(필드 이름 = 위 표기, snake_case).
- 시험(`tests/arbiter_*.rs`): plan §3.7 표의 7행 전부 + M1 수용 (e)(미완결 줄 + 4 s 유휴 + agent send → 미송신), (f2) 첫 두 줄(탭 닫기 → dirty 남음·새 viewer clear로 해제 / 두 viewer 동시 타이핑 → 섞이지 않음, takeover 후 송신, 5 s 지나면 폐기), Ctrl-C 락 파괴, hold 중 대기 send 즉시 거절, 상한 초과. proptest: 임의 이벤트열에서 "사용자 미완결 줄 중 에이전트 바이트 송신 0" 불변식.

## C. `docs/impl/uart/console-api.md` (T07b)
`loom-console` 전체 공개 API 초안 1–2쪽: 모듈별 책임, 위 두 모듈의 실제 시그니처, 나머지 모듈(ring·cursor·text·matcher)은 `docs/impl/uart/tasks/p0-console-core.md`의 시그니처를 인용. EXPLORE(SSH 콘솔 재사용)·UI(WASM)가 볼 질문 3개 이내.

## 끝나면
보고서 작성 후 대기. 리드가 diff 검토·커밋한다.
