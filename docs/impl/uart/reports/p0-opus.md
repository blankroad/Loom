DONE

# P0 uart-opus 보고 — `loom-console` 정제기(T07) + InputArbiter(T07b·T10 핵심)

- 작성: uart-opus, 2026-11-11. docs/plan/02-core-principle.md 준수.
- worktree: `.worktrees/uart`. 커밋하지 않았다. CARGO_TARGET_DIR는 `~/.cache/loom-target/uart-opus`.

## 0. 먼저 알릴 것 — 다른 작업자 파일을 건드렸다
`cargo fmt -p loom-console`를 한 번 실행했다(07:42:44). 이 명령이 crate 전체를 포맷해서, 다른 작업자가 작성 중이던 **`src/ring.rs`·`src/text.rs`의 공백 배치가 바뀌었다**.
- 바뀐 것은 rustfmt 서식뿐이고 내용은 그대로다.
- 이전 상태를 저장해 두지 않아 되돌리지 못했다.
- 이후에는 내 파일에만 `rustfmt`를 실행했다.
- 그 작업자의 편집 도구가 "디스크에서 바뀜" 경고를 낼 수 있다.

## 1. 바꾼 파일
| 파일 | 내용 |
|---|---|
| `crates/loom-console/src/sanitize.rs` | 공개 API, 바이트 상태기계 (+ 하위 모듈 선언) |
| `crates/loom-console/src/sanitize/csi.rs` | CSI 분류: 허용 목록, DECSET 재작성, TUI DECSTBM 줄이기 |
| `crates/loom-console/src/sanitize/utf8.rs` | 증분 UTF-8 검증(최대 3바이트 보유) |
| `crates/loom-console/src/arbiter.rs` | InputArbiter 상태기계 |
| `crates/loom-console/tests/sanitize_golden.rs`, `sanitize_props.rs` | 골든 비교, 단위 시험, proptest, adversarial |
| `crates/loom-console/tests/golden/sanitize/*.in/.out` + `README.md` | 골든 34쌍. 기대 출력은 손으로 썼고 코드로 만들지 않았다 |
| `crates/loom-console/tests/arbiter_rules.rs`, `arbiter_props.rs` (+ `arbiter_props.proptest-regressions`) | 규칙 시험 25개, 불변식 proptest |
| `docs/impl/uart/console-api.md` (**worktree 안**) | T07b 공개 API 초안. 커밋 대상이라 worktree에 두었다 |

## 2. 공개 API 요약
**`sanitize`**:
- `Sanitizer::new(SanitizeConfig)`, `feed(&[u8], &mut Vec<u8>)`, `on_gap(&mut Vec<u8>)`.
- `counters() -> &Counters`, `take_recent() -> Vec<Blocked>`, `alt_screen()`, `bracketed_paste()`.
- 과제 외 추가: `set_rows(u16)`(TUI 크기 변경), `buffered_bytes()`(시험·관측용), `config()`.
- `SanitizeConfig { profile: Profile::{Web,Tui}, allow_mouse, rows }`, 생성자 `web()`/`tui(rows)`.
- `BlockKind` 18종: Query, WindowOp, Title, OscColor, Osc8, Osc52, OscOther, Mouse, Focus, Mode, Dcs, ApcPmSos, Ris, Esc, Csi, C0, C1, Overlong.
- `Counters`는 종류마다 u64 하나, 그리고 차단이 아닌 `utf8_replaced`.
- `Blocked { kind, offset }`. offset은 생성 이후 누적 입력 바이트 기준이다.

**`arbiter`**:
- `InputArbiter::new(ArbiterConfig { has_prompt, line_kill_ctrl_u })`.
- 사용자 쪽: `user_input(viewer, input_seq, bytes, now_ms) -> InputDecision { send, interrupt, ack, reason, preempt_agent }`, `takeover`, `yield_input -> (YieldResult, Vec<TickAction>)`, `clear_input -> (Vec<u8>, Vec<TickAction>)`, `viewer_left`, `set_hold`, `interrupt_written(n)`.
- RX 쪽 힌트: `prompt_seen`, `set_alt_screen`.
- 에이전트 쪽: `agent_send(client, request_id, label, defer_timeout_ms, now_ms) -> AgentAdmit::{Now, Deferred{until_ms}, Reject{code, detail}}`, `agent_done`, `lock`, `unlock`.
- 주기 호출: `tick(now_ms) -> Vec<TickAction::{AgentReady{request_id, deferred_ms}, AgentRejected{request_id, code, detail}}>`.
- 이벤트: `take_events() -> Vec<ArbiterEvent>`. `kind()`·`payload()`는 `uart.input_owner`/`uart.hold`/`uart.lock`/`uart.agent_activity`/`uart.input_ack`이다.
- 필드는 리드가 UI에 답한 계약(`requests/ui-uart.md` 답변 1–2)에 맞췄다:
  - owner가 agent면 `client`를 싣는다.
  - activity의 `text`는 80자 이하이고 `request_id`·`client`를 함께 싣는다.
  - `lock`은 TTL이 연장될 때마다 다시 발행한다.
  - ack reason은 `other_writer`/`agent_lock`/`queue_full`/`takeover_timeout`이다.
- 오류 코드는 `ErrorCode`의 기존 값만 쓴다: `HeldByUser`, `Locked`, `LockBroken`, `UserActive`, `RateLimit`.

상세 시그니처와 worker 연결 방법은 `docs/impl/uart/console-api.md`에 있다.

## 3. 상수 (상한·정책)
| 상수 | 값 | 넘칠 때 | 근거 |
|---|---|---|---|
| `sanitize::MAX_SEQ_BYTES` | 4096 | 그 시퀀스를 종결자(final, ST, BEL, CAN, SUB, ESC)까지 버리고 `overlong`을 1 센다 | §7.5, §5.15.2 |
| `sanitize::RECENT_CAP` | 64 | 가장 오래된 항목을 덮어쓴다 | §5.4 차단 목록 |
| `sanitize` 내부 보유량 | CSI 버퍼 ≤ 4096 B, UTF-8 ≤ 3 B. OSC·DCS·APC 본문은 버퍼에 담지 않는다 | — | C2 |
| `csi::MAX_MODES` | DECSET 한 번에 남기는 모드 16개 | 넘는 모드는 `mode`로 센다 | C2 |
| `arbiter::USER_HELD_MAX_BYTES` / `_ITEMS` | 4096 B / 64개(모든 viewer 합계) | 그 입력을 `discarded`, `queue_full` | §3.13 TX |
| `arbiter::INTERRUPT_MAX_IN_FLIGHT` | 64(Ctrl-C 차선) | `discarded`, `queue_full`. `interrupt_written`이 자리를 되돌린다 | §3.13 TX |
| `arbiter::AGENT_PENDING_MAX` | 64 | `E_RATE_LIMIT` | §3.13 TX |
| `arbiter::LOCK_TTL_MAX_MS` / `_DEFAULT_MS` | 600 000 / 120 000 | 상한으로 줄인다 | §3.7-6, §4.7 |
| `arbiter::TAKEOVER_WINDOW_MS` | 5 000 | 보류 키를 `discarded`, `takeover_timeout` | §3.7-6/7 |
| `arbiter::QUIET_RETURN_MS` | 1 500 | 프롬프트가 없는 프로필의 자동 반환 | §3.7-3 |
| `arbiter::MAX_ID_BYTES` / `ACTIVITY_TEXT_MAX_CHARS` | 128 B / 80자 | 자르고, 미리보기는 `…`를 붙인다 | UI 계약 |
| `arbiter::EVENT_BACKLOG_MAX` | 512 | 가장 오래된 이벤트를 버리고 `events_dropped`를 센다. 매 호출 뒤 `take_events`를 부르면 넘치지 않는다 | C2 |

## 4. 시험 결과
```
cargo fmt -p loom-console -- --check   → 내 6개 파일은 깨끗함(rustfmt --check).
                                          crate 전체는 다른 작업자가 작성 중인 cursor/matcher/tests 파일에서만 diff가 있다
cargo clippy -p loom-console --all-targets -- -D warnings → Finished (경고 0)
cargo test -p loom-console
  arbiter_props   2 passed (각 1024 case)            2.3 s
  arbiter_rules  25 passed
  sanitize_golden 1 passed (34쌍 × 분할 6가지)
  sanitize_props 14 passed (proptest 512 case × 2)   9.7 s (debug)
  (다른 작업자 cursor_read 12, ring_limits 10, text_pipeline 16, text_props 2 도 함께 통과)
```

**정제기**
- 골든: 34쌍을 통째, 1·2·3·7·4096바이트 분할로 넣어 모두 같은 결과를 확인했다. 질의 9종, DCS 2종, APC/PM/SOS, OSC 제목·색·8·52, ESC로 끝나는 OSC, 10 KiB OSC, 마우스·포커스, 섞인 DECSET, SGR·커서 통과, alternate screen, bracketed paste, RIS, ENQ·NUL·DEL, C1 코드 포인트, 원시 0x9B, 잘린 UTF-8, 4095/4096/4097 경계, CAN 중단, 문자 집합, TUI DECSTBM을 포함한다.
- 단위 시험:
  - 질의 11종 카운트, 문자열 분류, DECSET 혼합 카운트, `allow_mouse`.
  - alt/bracketed paste 상태 추적, recent 목록의 offset과 상한 링.
  - `on_gap`, TUI 줄이기와 크기 변경, Web DECSTBM 통과.
  - **10 MiB 미종결 OSC에서 버퍼가 4099 B 이하로 유지됨.**
- proptest:
  - ① 임의 분할 독립성: 출력·카운터·recent·모드가 같다.
  - ② 출력은 고정점이다: 출력을 다시 정제해도 차단이 0이고 결과가 같다. 질의가 남지 않는다는 증명이다.
- adversarial: 1 MiB 난수와 1 MiB escape 편향 난수를 Web·TUI에 넣었다. panic이 없고, 버퍼 상한이 지켜지고, 출력 ≤ 3 × 입력 + 4다.

**InputArbiter**
- plan §3.7 표:
  - 행 1–6을 시험했다. 행 7(R3 승인)은 core 승인 파이프라인 몫이라 제외했다.
- M1 (e): 미완결 줄 + 4 s 유휴 상태에서 에이전트 send를 넣고 250 ms마다 10 s 동안 tick했다. AgentReady가 0이고, 마지막에 `E_USER_ACTIVE{dirty_line, user_line_len:12}`이 나온다.
- M1 (f2):
  - 탭을 닫아도 dirty가 남고, 새 탭의 clear(^U)로 대기 송신이 진행된다.
  - 두 탭이 동시에 타이핑해도 섞이지 않고, takeover 뒤 보류 키가 송신된다.
  - 보류 키는 5 s가 지나면 폐기된다.
- Ctrl-C:
  - 락을 깨고 자기 보류 키 뒤에 송신된다. 다음 에이전트 호출은 `E_LOCK_BROKEN`이다.
  - 다른 viewer의 Ctrl-C도 즉시 통과한다.
  - 에이전트 전송 중에는 `preempt_agent`가 나온다.
- hold를 켜면 대기 중 send가 즉시 `E_HELD_BY_USER`가 된다.
- 상한 4종(보류 항목·보류 바이트·에이전트 대기·Ctrl-C 차선)과 이벤트 payload 모양을 시험했다.
- **proptest 불변식**: 모델은 arbiter 내부가 아니라 실제로 보내라고 한 바이트만으로 장치 줄을 추적한다. 모델 기준으로 다음을 확인했다.
  - 미완결 줄 중 에이전트 승인 0.
  - hold 중 승인 0.
  - 동시 에이전트 송신 0.
  - 사용자 바이트가 나갈 때는 항상 에이전트를 선점한다.
  - 검증 프로필과 일반 프로필 각각 1024 case다.
  - 처음 실패 1건은 모델 쪽 버그였다: 보류 키 없는 takeover의 선점을 놓쳤다. regression 파일에 그 사례를 남겼다.

## 5. 계획·지시와 다르게 한 점
1. **`vte`를 쓰지 않고 상태기계를 직접 만들었다.** 지시에서 "구조가 다르면 보고서에"라고 허용한 부분이다. 이유:
   - 통과하는 시퀀스를 입력 바이트 그대로 내보낼 수 있다. vte는 파싱 결과를 다시 직렬화해야 한다.
   - 4 KiB 상한을 상태기계 한 곳에서 집행한다. vte 앞에 길이 필터를 두면 같은 토크나이저가 두 개 생긴다.
   - OSC·DCS 본문을 아예 버퍼에 담지 않는다.
   - TS 포팅 규칙이 단순해진다.
   - 그래서 `loom-console`의 `vte =0.15.0` 의존성을 쓰는 곳이 없다. Cargo.toml에서 지울지 리드가 판단해 달라.
2. **출력 상한은 "≤ 입력 + gap 리셋"이 아니라 "≤ 3 × 입력 + 4"다.** 잘못된 UTF-8 1바이트를 U+FFFD(3바이트)로 바꾸기 때문이다. 원시 바이트를 그대로 단말에 넘기면 0x9B 같은 바이트가 8비트 C1 CSI로 해석될 수 있고, 지우면 baud가 틀린 콘솔이 빈 화면이 된다.
3. **허용 목록을 지시보다 조금 넓혔다.** vi·top·메뉴형 부트로더가 제대로 보이게 하려는 것이다(§14 Q8). 모두 응답을 만들지 않는다.
   - ESC: `D`/`E`/`M`(IND/NEL/RI), `=`/`>`(키패드), G0/G1 `( 0`·`( B`·`) 0`·`) B`.
   - CSI: `E F G d e \` a X g I Z`, 파라미터 1개 이하의 `T`, 파라미터 없는 `s`/`u`, DECSCUSR(`SP q`), SM/RM 4(IRM).
   - DECSET: 1(커서 키), 6(origin), 12(깜빡임), 1048(커서 저장).
4. **질의 차단을 지시 목록보다 넓혔다.** DECREQTPARM `CSI x`, DECRQCRA `CSI * y`, DECRQPSR/DECRQTSR `CSI $ w/u`, XTQMODKEYS `CSI ? m`, XTSMGRAPHICS `CSI ? S`. 모두 단말이 응답을 장치로 보내는 시퀀스다.
5. **C1 코드 포인트(U+0080–U+009F)를 지운다.** xterm.js는 C1 코드 포인트를 제어 문자로 해석하므로(8비트 CSI·OSC), 그대로 두면 질의 차단을 우회할 수 있다. 골든 `web_c1_csi_codepoint`.
6. **TUI 프로필은 DECSTBM만 줄인다.** plan §5.15.2의 CUP·VPA 상태줄 행 clamp는 보조 TUI(M2, T19)에서 추가한다.
7. **InputArbiter 의미 결정**:
   - ^U를 줄 끝으로 보는 것은 `line_kill_ctrl_u` 프로필에서만이다. 일반 프로필에서는 dirty를 유지한다.
   - `clear_input`은 dirty를 지우고 **입력권도 돌려준다.** 검증 프로필에서만 ^U를 보낸다.
   - 다른 에이전트가 송신 중이어서 defer 시간이 지나면 `E_LOCKED{reason:"agent_busy"}`다.
   - 사용자가 줄을 친 채 시간이 지나면 dirty면 `dirty_line`, 아니면 `user_typing`이다.
   - 보류 키 없는 takeover도 에이전트 전송을 선점한다(사용자가 이긴다).
8. **ack reason**:
   - 리드 목록에 `viewer_left`를 더했다. 그 viewer의 보류 키를 폐기할 때 쓴다.
   - Ctrl-C 차선이 넘칠 때도 `queue_full`을 쓴다.
   - `not_open`은 gateway·worker 몫이라 arbiter에는 없다.
9. **gateway·worker가 채울 필드는 arbiter에 없다.**
   - `viewer_label`: 연결 이름은 gateway가 안다.
   - `lock.ttl_ms`: arbiter는 (재)발행 시점의 TTL을 싣고, 패널이 로컬로 깎는다(UI 계약과 같다).

## 6. 리드에게 묻는 것
1. `vte` 의존성을 지울까?(5-1) 다른 모듈도 쓰지 않는다.
2. `docs/impl/uart/console-api.md`를 worktree에 두었다. 저장소 루트가 맞으면 옮겨 달라.
3. P1 연결 때 `ArbiterConfig`의 `has_prompt`·`line_kill_ctrl_u`는 프로필에서 받는다고 가정했다. linux-shell·uboot = (true, true), generic = (true, false).
4. `ring.rs`·`text.rs` 서식 변경(0절)을 그 작업자에게 알려 달라.
