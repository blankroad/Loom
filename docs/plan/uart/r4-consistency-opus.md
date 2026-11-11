# R4 내부 일관성 점검 — uart-opus → plan.md (통합본 v1)

- 작성: uart-opus, 2026-11-09. docs/plan/02-core-principle.md 준수. plan.md는 수정하지 않았다.
- 표기: 위치는 `§절 (Lnnn)` = plan.md 줄 번호. 심각도 **H** = 구현자가 틀린 동작을 만들거나 M1 판정이 불가능해짐, **M** = 혼란·재작업, **L** = 표기 정리.
- 가장 중요한 3가지:
  1. **expect 패턴 기본값이 literal인데 예시 다수가 regex를 전제한다**(`'# $'`, `'TEST DONE|FAIL'`, `seq=5000\b`). SKILL.md도 같다. 이대로면 에이전트의 대표 호출이 **조용히 timeout**난다.
  2. **M1 수용 시나리오 E1이 M2 기능(`--secret-env`, `run`, 프로필)에 의존**한다.
  3. **승인 팝업·락 탈취 프롬프트가 사용자 타이핑과 키를 공유**해서, 사용자가 장치에 치던 `y`가 R3 승인으로 해석될 수 있다. 팝업은 패스스루 화면을 덮은 뒤 "복원"한다고 했지만, 화면 모델이 없으면 복원할 수 없다.

---

## 1. 내부 모순

| # | 위치 A | 위치 B | 모순 | 추천 | 심각도 |
|---|---|---|---|---|---|
| C1 | §4.3 (L527) "`--expect` 패턴 기본 **literal**, `--regex`로 정규식", §4.5 (L565) `[--regex]` | §2.0 ① (L85) `--expect '# $'`, §2.1 A1 (L174) 동일, A5 (L208) `expect dut 'TEST DONE\|FAIL'`, A2 (L184) `grep dut 'E \(\|panic\|Guru'`, §3.11 폭주 테스트 (L434) `expect 'seq=5000\b'`, §4.2 status (L512) `/TEST DONE\|FAIL/`, SKILL (L647) `--expect '# $'` | literal로 해석하면 `# $`는 문자 그대로 "# $"를 찾는다(프롬프트 `root@dut:~# `에는 `$`가 없음) → 항상 timeout. `\|`는 OR가 아니다 | **literal 기본은 유지**(안전·성능). 모든 정규식 예시에 `--regex`를 붙이고, SKILL에 "패턴은 literal, 정규식은 `--regex`" 한 줄 추가. 대안: `run --prompt`처럼 **프롬프트 대기용 `send --expect-prompt`**(프로필 프롬프트 regex 사용)를 두어 `'# $'`를 에이전트가 직접 쓰지 않게 | **H** |
| C2 | §2.0 ① (L85) `send --secret-env` — E1은 **M1 수용 기준 (a)** (L1047) | §11 M2 (L1048) "`--secret-env`(redaction 정책 전체)", §12 T14 (M2) | M1 판정 시나리오가 M2 기능을 씀 | `--secret-env` + redaction을 M1 ③으로 올린다(E1이 로그인을 하므로 비밀값이 M1 기록에 원문으로 남는 문제도 함께 해결). 또는 E1의 로그인 단계를 M1 판정에서 빼고 `bootshell`에 비밀번호 없는 로그인 모드를 둔다. **전자 추천** | **H** |
| C3 | §2.0 ④ (L157) `loom uart run dut 'ip -br addr'` — M1 (a) | §4.6 (L577) "명령 실행 **(M2)**", T14 (M2) | 위와 같음 | E1 ④를 `send dut 'ip -br addr' --expect-prompt`(또는 `--expect '# ' `) 로 바꾸거나 `run`을 M1로 올린다. `run`은 `send --expect` 위의 얇은 정제 계층이라 M1 비용이 작음 → **M1로 올리기 추천** | **H** |
| C4 | §2.0 ① (L79) `--profile linux-shell`, ④ (L162) "프롬프트(`# $`, **프로필 정의**)가 다시 보이면 입력권이 넘어감", §3.7-3 (L320) 프롬프트 재출현 조건 | §11 M1 (L1047)에 프로필 없음, T17 "프로필 TOML" = **M2** | M1에서 입력권 반환 규칙의 프롬프트 분기·`eol`·`run --prompt` 기본값이 정의되지 않는다 | M1에 **내장 프로필(읽기 전용) `generic`, `linux-shell`**(eol·prompt만)을 넣는다. TOML 사용자 정의·리셋 시퀀스는 M2 유지 | **H** |
| C5 | §2.0 ⓑ (L141) `input: you (line done, returns in 1.5s)` — 프로필 linux-shell 사용 중, 프롬프트가 이미 보임 | §3.7-3 (L320) "프롬프트 정규식이 있으면 **프롬프트 재출현**, 없으면 1.5초" | 프로필이 있는데 1.5 s 타이머로 표시 | 화면 ⓑ를 `input: you (line done — returns on prompt)` 또는 `input: free`(프롬프트가 이미 보였으므로 반환 완료)로 수정 | M |
| C6 | §2.0 ④ ⓒ (L160) `⏳ agent waiting: "ip -br addr" (4.2s/10s)`, (L163) "10초 넘게 미완결이면 E_USER_ACTIVE" | §4.6 (L579) `run --timeout 30`(기본), §4.1 (L451) `run` 30 s, §3.7-4 (L324) "`--timeout`(기본 10 s)까지 대기" | 사용자 대기 한도가 명령의 timeout인지 별도 10 s인지 불명. run은 30 s인데 화면은 10 s | **대기 한도를 분리 정의**: `--defer-timeout`(기본 10 s, 사용자 입력권 대기) + 명령 `--timeout`(응답 대기). 사용자 대기 시간은 응답 timeout에서 빼지 않는다. §3.7-4, §4.3, §4.6 문구 통일 | M |
| C7 | §3.3 (L267) "커서 위치 = `rx_offset`" | §4.2 status (L511) `cursors: agent @ seq 1903221`, §4.4 (L558) `(cursor agent → seq 41022)` | 커서 단위가 seq인지 rx_offset인지 | 내부 저장은 `rx_offset`, 표시는 `seq`(사용자·에이전트가 쓰는 좌표)로 하고 status·read 출력 문구를 "`seq N (rx_offset M)`"로 명시. 또는 표시를 rx_offset으로 통일 | M |
| C8 | §4.1 envelope (L455) `"channel":"dut"`, 전역 용어 "채널" | §2.0 ③ JSON (L148) `"port":"dut"`, §4.1 (L450) env `LOOM_UART_PORT`, 오류 `E_AMBIGUOUS_PORT` | 용어 혼재 | JSON은 `channel`로 통일. env는 `LOOM_UART_CHANNEL`(구 이름은 별칭 허용), 오류 `E_AMBIGUOUS_CHANNEL` | L |
| C9 | §3.9 (L365) "`user` role은 **attach 전용 토큰 파일**로만" | §3.10 (L402) `~/.loom/run/uart.json`에 `"attach_token"`을 `token`과 **같은 파일**에 | 에이전트가 읽는 descriptor에 user 토큰이 함께 있으면 role 분리가 의미 없다 | attach 토큰은 별도 파일(`uart.attach.json`, 0600). 같은 OS 계정이라 보안 경계가 아니라는 한계 문구(§3.9)는 유지 | M |
| C10 | §3.11-11 (L432) "never miss를 약속하지 않음", §4.5 (L571) window 64 KiB, "타임아웃 시 커서 불변" (L572) | §2.1 A5 (L210) "`--timeout 300` 반복(**커서가 이어 주므로 놓치는 구간 없음**)" | 반복 expect는 커서가 안 움직이므로 스캔 범위가 매번 커지고, 결국 window·링 밖 → `E_SCAN_LIMIT`. "놓치지 않음"과 모순 | expect timeout 시 응답에 **`resume_from`(= 끝 − 최대 패턴 길이)** 를 주고 `--from` 으로 이어 걸게 한다(또는 `--advance-on-timeout`). A5 문구를 "`--from <resume_from>`로 이어 걸면 구간 손실 없이 이어진다(스캔 예산 내)"로 수정 | M |
| C11 | §10.1 표 (L981) interactive R2 = "세션당 종류별 **첫 1회 승인**"(첫 승인이 곧 세션 승인) | §5.6 (L759) R2 팝업 `[y] approve once` + `[s] allow this kind for this session` | 첫 승인이 자동으로 세션 범위라면 `[y] once`가 무의미. 아니면 §10.1 문구가 틀림 | §10.1을 "R2: 승인 필요, 팝업에서 `[s]`를 고르면 그 종류는 세션 동안 재질문 없음"으로 수정(사용자가 범위를 고르는 쪽이 명확) | M |
| C12 | §5.5 (L725) 뷰어 Enter = **CR(기본)** | §4.3 (L523) 에이전트 텍스트 줄끝 = 프로필, §4.7 esp32 프로필 `eol = "crlf"` (L607) | 같은 채널에서 사용자 Enter와 에이전트 줄끝이 다를 수 있음 | 뷰어 Enter 기본 = **채널 프로필 eol**(없으면 CR). `Ctrl-] l`은 뷰어 로컬 덮어쓰기 | L |
| C13 | §4.6 (L582) 프로필 이름 `linux-shell`, `uboot`, `esp-idf-console`, `zephyr-shell` | §4.7 (L606) `esp32`, §4.8 profiles.md (L670) `generic, linux-shell, uboot, esp32, stm32-vcp, zephyr-shell`, §10.1 규칙 묶음 (L984) `generic, linux, uboot, esp-idf, zephyr`, M1 ⑥ "generic+linux+uboot" | 프로필 이름과 안전 규칙 묶음 이름이 섞임 | **프로필 목록 하나**를 확정(`generic, linux-shell, uboot, esp32, stm32-vcp, zephyr-shell`)하고, 각 프로필이 쓰는 규칙 묶음을 매핑 표로(`linux-shell → generic+linux` …). `esp-idf-console` 표기 제거 | L |
| C14 | §0 표 (L19) M1 키 `Ctrl-] q/h/v` | §11 M1 ④ (L1047) `q/h/v/y`, §5.5 (L725–731) M1 = q, ?, v, h, y, `^]^]` | 목록 차이 | §0을 "`Ctrl-] q/h/v/y/?`"로 | L |
| C15 | §5.1 (L685) 트레이스 모드 용도 "타임스탬프·hex·검색" — M1부터 | §5.5 `Ctrl-] x` hex = M2, `Ctrl-] t` = M3, 검색 = M3(로그 모드) | M1 트레이스 모드가 무엇을 하는지 불명 | §5.1에 "M1 트레이스 = 고정 abs 타임스탬프 + 출처 행 + 기호화. hex는 M2, 검색·필터는 M3 로그 모드" 명시 | L |
| C16 | §3.4 (L283) STOPPED에서 `reconnect --extend` / `open --resume` | §4 CLI 전체 | 두 명령/옵션이 §4에 없다 | §4.2에 `loom uart reconnect CH --extend 5m`, `open CH --resume` 추가(M2) | M |
| C17 | §3.8 (L345) `loom uart open dut --pick 2` | §4.2 open 문법 (L485) | `--pick` 없음 | §4.2에 `--pick N`(AMBIGUOUS 후보 선택, R1) 추가 | L |
| C18 | §3.4 (L288) `send --wait-connected 30s`, §10.2 (L1005) `--wait-approval 60s` | §4.3 send 문법 (L519) | 옵션 누락 | §4.3 문법에 두 옵션 추가(마일스톤 표기 포함: wait-connected M2, wait-approval M2) | L |
| C19 | §3.3 (L269) `--cursor-token`, §4.4 (L551) `--skip-gap`, §4.5 (L569–570) `--raw`, `--settle`, §6.4 (L824) export `--lines`, §6.1 (L787) `--pin` | 각 절의 문법 줄 | 본문에만 있고 문법 줄에 없음 | 문법 줄과 cli.md 기준표 하나로 통일(아래 §3 A4) | L |
| C20 | §4.7 (L597) `attach --mode term\|trace` | §5.1·M3 "로그 모드" | M3 로그 모드의 진입 값이 없음 | M3에서 `--mode log` 추가 또는 "로그 모드 = 트레이스 모드의 확장, 같은 값"으로 명시 | L |
| C21 | §4.1 종료 코드 표 (L459–469) | `E_CONFIG_CONFLICT`(L497, L603), `E_UNSUPPORTED_PARAM`(L287, L498), 커서 `revision` 충돌(L268), `E_NOT_FOUND`, `E_PERMISSION`(L287) | 표에 매핑이 없는 코드 | `E_CONFIG_CONFLICT`·`E_UNSUPPORTED_PARAM` → 2(사용법), `E_REVISION_CONFLICT` → 5(동시 사용 충돌), `E_NOT_FOUND`·`E_PERMISSION` → 4. 표에 추가 | M |
| C22 | 승인 용어 `approval_id`, `--approval`, `uart.approval.respond` (§3.9, §4.3, §10.2) | 오류 코드 `E_CONFIRM_REQUIRED` (L195, L467) | 용어 혼재(R1 잔재) | `E_APPROVAL_REQUIRED`로 통일 | L |
| C23 | §2.1 A1 (L171), A3 (L191) `signal dut reset --seq rts-pulse` | §4.7 프로필 예시는 `esp32-run`, `esp32-download`만 | `rts-pulse` 시퀀스 정의 없음 | `generic` 프로필에 `rts-pulse`(RTS on 100 ms off, `unverified`) 정의 추가 | L |
| C24 | §3.3 (L267) client_name = `LOOM_CLIENT` 없으면 **`agent`** | 예시 전반의 client 표시 `claude-7f3a` (L109, L362, L512, L752) | 기본 설정에서 `claude-7f3a`가 어디서 오는지 불명 | "커서 키 = `LOOM_CLIENT`(기본 `agent`), 표시용 client = `LOOM_CLIENT`(기본 `agent`)+`#pid`" 식으로 정의, 예시를 맞춘다 | L |
| C25 | §3.1 (L251) `raw.bin`, §9.3 (L953) "raw 해시" | §6.1 (L779) `raw.0000.bin`(회전) | 파일명 | `raw.NNNN.bin`으로 통일 | L |
| C26 | §10.1 R2 예 (L974) `reboot`/`reset` **문자열** = R2 | §11 M1 ⑥ (L1047) "R3/R4 문자열 규칙"만 | M1에서 `reboot`가 R1로 통과(§4.3 예시 L531은 `[R2: session approval ok]`) | M1 규칙에 R2 문자열 분류도 포함(승인 객체가 이미 M1에 있음) 또는 "M1은 R2 문자열을 R1로 취급"을 명시. **포함 추천** | M |

---

## 2. 마일스톤 의존성 오류

### 2.1 M_n 수용 기준이 이후 단계 기능에 의존
| # | 수용 기준 | 의존하는 기능(배정) | 수정 |
|---|---|---|---|
| D1 | M1 (a) "§2.0 E1 ①–④" | `--secret-env`(M2, T14), `run`(M2, T14), `--profile linux-shell`·프롬프트 기반 입력권 반환(프로필 = M2, T17) | C2·C3·C4 수정안(필수) |
| D2 | M1 (a) E1 ③ 에이전트 응답 `warnings: user_input_since_last_call` | M1 목록 ①–⑩에 명시 없음, 담당 작업 없음 | M1 ⑤ InputArbiter에 "개입 warnings" 추가, T10 범위에 명시 |
| D3 | M1 (g) "Claude가 SKILL.md만 보고 …" | T13 스킬 v0 — 그런데 판정 작업 T12의 의존에 T13이 없음 | T12 의존에 T13 추가 |
| D4 | M2 "레시피 R-1~R-8 자동 통과" | R-8 = herdr `pane split`/`pane run` → CI에서 자동화 불가(herdr 필요). R-2 "부트로그 캡처"가 `log export`를 쓰면(A2 L185, SKILL L649) export = **M3**(T22) | R-8은 "수동 체크리스트"로 분리. `log export`(최소 `txt`)를 **M2**로 올리거나 R-2를 grep만으로 정의. A2·SKILL은 export를 M2부터로 표기 |
| D5 | M2 "Claude·Codex 각각 A1–A3" | A3 마지막 단계는 뷰어 팝업 승인(M2 T18) — OK. A1 `signal reset --seq rts-pulse` — 시퀀스 미정의(C23) | C23 |
| D6 | §8 (L894) Windows "M1 CI(`sim://`)" | M1 ④ attach는 "(POSIX)" — Windows CI에서 attach 하네스 불가 | 문제 없음. 다만 M1 (a)에 "Windows는 attach 제외"를 명시해 판정 혼선 방지 |

### 2.2 §12 작업 표의 의존성 누락
| 작업 | 현재 의존 | 누락 | 근거 |
|---|---|---|---|
| T11 attach | T02, T07, T10 | **T06**(트레이스 모드 줄 분리·증분 디코더), **T09**(상태줄 `agent_activity`의 expect 대기 표시), T08(채널·백로그) | M1 ④ 상태줄 항목 |
| T12 M1 판정 | T04, T05, T11 | **T09**(동시 expect·E1), **T10**(Ctrl-C 우선·미완결 줄 보호), **T13**(수용 (g)) | M1 (a)(c)(e)(g) |
| T10 SafetyGate 최소 | T08 | T06(줄 재조립·hex 텍스트 디코딩 판정, §10.1 L985) | 우회 방지 규칙 |
| T19 뷰어 M2 키 | T11 | **T14**(`Ctrl-] m` 마크), **T17**(`b/r/d/s/R` 시그널·release), **T18**(`k` 전송 취소) | §5.5 M2 키 |
| T20 스킬 v1 | T14, T18 | **T16**(재연결 레시피 R-4/A2), **T17**(R-2·R-3 리셋, R-7 release) | references/recipes.md |
| T18 | T10, T11 | T17(R2 신뢰 모드 판정 대상인 signal/configure/release) | §10.1 R2 |
| (신규) | — | **M1 내장 프로필(generic, linux-shell)** 작업이 없음 | C4 — T08 또는 신규 T08b(O, M1) |
| (신규) | — | `attach --open`(U6)의 마일스톤·담당 없음 | M2 T19 또는 T11에 배정 |

순환 의존은 없다. M1 흐름도(L1087)는 위 누락을 반영해 "T09 ∥ T10 → T11 → T12(T13 포함)"로 고치면 된다.

---

## 3. 에이전트 사용성 점검 (SKILL.md 초안 §4.8 ↔ §4 CLI)

| # | SKILL.md 내용 | 문제 | 수정 |
|---|---|---|---|
| A1 | `loom uart send dut 'cmd' --expect '# $'` (L647) | C1: literal 기본이라 매치 안 됨. 에이전트가 가장 많이 복사할 줄이다 | `--expect '# ' ` 같은 literal 예시로 바꾸거나 `--expect '[#$] $' --regex`. 프롬프트 대기는 `run`/`--expect-prompt` 권장 문구 |
| A2 | Core verbs에 `run`, `grep`, `--since 30s\|mark:X`, `log export`, `--secret-env`, description의 "resetting boards via DTR/RTS", Rules의 `configure --baud` | 모두 M2(일부 M3). **M1 스킬 v0**가 이 초안이면 수용 (g)에서 에이전트가 없는 기능을 호출한다 | §4.8에 **v0(M1)와 v1(M2) 두 판**을 명시. v0 = ports/open/status/read(new·tail·page)/expect/send(--expect, --hex, --ctrl, --eol)/attach 안내/exit 코드 규칙만. 또는 C2·C3 수정으로 secret·run을 M1로 올린 뒤 나머지만 v1로 |
| A3 | "Exit 6 = approval required … resend with `--approval <id>`" | exit 6에는 `E_DENIED`, `E_FORBIDDEN`(R4), `E_RATE_LIMIT`, `E_RECORDING_FAILED`도 포함(L467) → 에이전트가 승인 ID 없이 사용자에게 물을 수 있음 | "exit 6이면 `error.code`를 본다: `E_APPROVAL_REQUIRED`만 승인 요청, `E_FORBIDDEN`은 사용자가 정책을 바꾸지 않는 한 불가, `E_RATE_LIMIT`은 잠시 후, `E_RECORDING_FAILED`는 사용자에게 디스크 문제 보고" |
| A4 | Exit 5 규칙 "the user is typing/holding/took over" | exit 5에는 **다른 에이전트의 락**(`E_LOCKED`)과 커서 revision 충돌(C21 수정 시)도 포함 → "사용자에게 알리고 기다려라"가 맞지 않는 경우 | `E_LOCKED`는 "다른 에이전트가 사용 중: `status`로 락 사유·TTL 확인 후 대기" 분기 추가 |
| A5 | 종료 코드 3/4/7/8 안내 없음 | 에이전트가 가장 자주 만날 4(`E_DISCONNECTED`)·8(`E_CURSOR_GAP`)·7(데몬)에 대한 행동 규칙 없음 | 한 줄씩: 4 → `status` 확인, 재연결 대기 또는 사용자에게 케이블 확인 요청, **재송신은 delivery 확인 후**; 8 → `--skip-gap` 또는 `--since seq:<earliest>`; 7 → `loom uart daemon status` |
| A6 | `delivery: unknown` / `partial` 처리 규칙 없음 | 재연결·사용자 Ctrl-C 후 에이전트가 같은 명령을 자동 재송신할 위험(§3.5 L298은 런타임 쪽만) | Rules에 "`delivery: unknown`이면 다시 보내기 전에 `read`로 장치 상태 확인" |
| A7 | `expect` 반복(장시간) 레시피 R-4 | C10: 커서 불변 → 스캔 한도 | `resume_from`/`--from` 사용으로 recipes.md 갱신 |
| A8 | "If a response warns `no_viewer_attached`, ask the user to run …" | 매 send마다 경고가 반복되면 에이전트가 매번 사용자에게 같은 말을 할 수 있다 | 데몬은 채널당 **처음 1회 + 뷰어 0으로 바뀐 직후**만 경고(§0-5·§3.12 문구에 빈도 규칙 추가), 스킬은 "한 번만 안내" |
| A9 | references 목록에 `cli.md`가 "모든 동사·인자"의 기준 | §4 본문과 문법 줄이 다수 어긋남(C16–C20) | cli.md를 **문법의 단일 원본**으로 하고 §4는 거기서 생성/검증(레시피 동기화 테스트와 같은 방식) |
| A10 | `--secret-env VAR` 안내 | 에이전트 환경에 VAR가 없으면? | 오류 코드 `E_SECRET_MISSING`(exit 2)과 "사용자에게 변수를 설정해 달라고 요청" 규칙 |

---

## 4. 사용자 UX 점검 (§5 뷰어 ↔ §3.7 입력 규칙 ↔ §10 승인)

| # | 상황 | 현재 규정 | 문제 | 제안 |
|---|---|---|---|---|
| X1 | **승인 팝업이 뜬 순간 사용자가 장치에 타이핑 중** | §5.6 팝업 `[y]/[n]/[s]` 키, §3.7-2 사용자 키는 원시 전달 | 사용자가 치던 `y`(예: `yes`, `ymodem`)가 **R3 승인**으로 소비될 수 있음. 또 팝업이 떠 있는 동안 다른 키는 장치로 가는지 버려지는지 미정 | 팝업은 **키를 가로채지 않는다**. 상태줄에 `⚠ approval pending (R3) — ^] A to review` + 벨 1회만 띄운다. 검토 화면은 `Ctrl-] A`로만 열고, 그 안에서도 **0.7 s 무장 지연** 후 `y` 허용(R3는 `Y` 대문자 또는 Enter 재확인). 검토 화면 중 장치 키 전달은 중단, 닫으면 재개 |
| X2 | **팝업 오버레이 후 화면 복원** | §5.6 (L760) "상태줄 위 3–5줄 오버레이 후 화면 복원" | 터미널 모드는 **화면 모델이 없는 정제 패스스루**(§5.2) → 덮어쓴 줄을 복원할 데이터가 없다(특히 전체화면 앱) | 오버레이 대신 모드 전환을 재사용: `Ctrl-] A` → 전체 모달(대체 화면) → 닫으면 터미널 모드 복귀 시처럼 **백로그를 줄 경계부터 다시 그림**(§5.1 기존 메커니즘), 전체화면 앱이면 `Ctrl-L on device to redraw` 안내(§3.12와 동일 문구) |
| X3 | **전체화면 모드(alternate screen) 중 상태 표시** | §5.2 (L694) 상태줄 숨김 | HOLD·입력권·`A▶`·승인 대기·LAG가 전부 안 보인다. 사용자가 `top`을 보는 중 에이전트 송신이 화면에 섞이는 걸 모를 수 있음 | 뷰어 자신의 **창 제목(OSC 2, §5.7에서 이미 허용)** 에 압축 상태를 표시: `uart:dut ● HOLD`, `uart:dut A▶ 2s`, `uart:dut ⚠approval`. herdr 패인 제목에 보인다. 승인 대기는 벨도 |
| X4 | 전체화면 모드 + hold/락 탈취 프롬프트 | §3.7-6 (L326) 락 중 사용자 키 → `Type anyway? [y/N]` | 프롬프트를 어디에 그리는지 미정(상태줄 숨김). 첫 키를 버리는지 보류 후 송신인지도 미정. 사용자가 친 첫 글자가 `y`면 답과 구분 불가 | 첫 키는 **보류**(폐기 아님). 프롬프트는 창 제목 + 상태줄(비전체화면)에 표시하고, 응답은 `Ctrl-] t`(take over) 같은 **접두키 명령으로만** 받는다. 보류된 키는 탈취 후 송신, 5 s 내 응답 없으면 폐기하고 알림. Ctrl-C는 기존대로 즉시 통과 |
| X5 | 사용자가 **미완결 줄 상태로 `Ctrl-] y`**(입력권 반환) | §3.7-3 명시 반환 허용 | 에이전트 명령이 사용자의 반쯤 친 줄 뒤에 붙는다 — §3.7 원칙("미완결 줄에 아무것도 섞지 않는다")과 충돌 | 미완결 줄이면 `Ctrl-] y`가 `partial line "ifconfig eth" — send ^U and give? [^] y again]`을 띄우고, 두 번째 `Ctrl-] y`에 **Ctrl-U(0x15) 송신 후 반환** |
| X6 | 에이전트 send가 대기(deferred) 중 사용자가 `Ctrl-] h` | §3.7-5 hold = "모든 에이전트 쓰기 즉시 E_HELD_BY_USER" | 이미 대기 중인 send도 즉시 실패하는지 불명 | "hold 진입 시 대기 중 send는 즉시 `E_HELD_BY_USER`로 종료"를 명시, 상태줄 `agent send cancelled (HOLD)` |
| X7 | 트레이스 모드에서 키 입력 | §5.1 정의 없음 | 트레이스 모드에서도 원시 키가 장치로 가는가? 에코는 D 행으로만 보여 사용자가 무엇을 쳤는지 헷갈림 | "트레이스 모드에서도 키는 원시 전달, 사용자 입력은 `U▶` 행으로 즉시 표시(확정 출처)" 명시. `--readonly`면 전달 안 함 |
| X8 | `Ctrl-] p`(화면 일시정지) 중 키 입력 | §3.11-9 정의는 표시 쪽만 | 멈춘 화면을 보며 친 키가 장치로 가면 결과를 못 본 채 명령이 실행됨 | 일시정지 중 키 입력 시 `PAUSED — keys not sent; ^] p to resume` 경고 후 **송신하지 않음**(Ctrl-C만 예외 통과) |
| X9 | 여러 뷰어 + 승인 | §5.6 "모두에게 뜨고 먼저 응답한 쪽 유효" | X1 수정과 결합하면 OK. 응답 후 다른 뷰어의 검토 화면도 닫혀야 함 | `approval_result` 이벤트로 모든 뷰어의 모달 닫기 명시 |
| X10 | strict 모드 "R3는 **뷰어 승인만**" vs `loom uart approve ID`(사용자 터미널 CLI) | §10.1 (L982), §4.7 (L594) | 사용자 터미널 CLI와 에이전트 CLI는 같은 OS 계정·같은 토큰 파일 접근 → strict에서 CLI 승인을 허용하면 "뷰어만" 의미가 사라지고, 허용하지 않으면 §4.7과 충돌 | strict: **뷰어 검토 화면 승인만**(`approved_via=viewer`). `loom uart approve`는 interactive에서만 유효하고 TTY에서 대문자 확인을 요구, `approved_via=cli` 기록. 한계 문구(§10.2)는 유지 |
| X11 | 4 KiB 초과 붙여넣기 확인(§5.5 L746) | 확인 프롬프트 위치 미정 | X2와 같은 복원 문제 | 같은 모달 메커니즘 사용 |
| X12 | 상태줄 폭 우선순위(§5.3 L711) | "상태 > 입력권 > 에이전트 동작 > lag" | 승인 대기·HOLD가 우선순위에 없음 → 좁은 패인에서 승인 대기가 잘려 안 보일 수 있음 | 우선순위: **상태(HOLD 포함) > 승인 대기 > 입력권 > 에이전트 동작 > lag > 나머지** |

---

## 5. 리드가 반영할 수정 목록

### 필수 (M1 판정·안전·에이전트 기본 호출에 영향)
1. **C1/A1**: expect·grep 예시 전체에 `--regex`를 붙이거나 literal 예시로 교체(§2.0 L85, §2.1 L174·L184·L208, §3.11 L434, §4.2 L512, SKILL L647). SKILL에 "패턴은 literal, 정규식은 `--regex`" 추가. (선택) `--expect-prompt` 도입.
2. **C2·C3·C4/D1**: M1 범위에 `--secret-env`(+redaction), `run`, 내장 프로필 `generic`·`linux-shell`(eol·prompt)을 추가하거나, E1을 M1 기능만 쓰도록 다시 쓴다. 추천은 범위 추가(작업량이 작고 E1이 그대로 유효).
3. **A2**: §4.8에 스킬 **v0(M1)/v1(M2)** 구분을 명시하고 v0 본문은 M1 동사만 쓴다.
4. **X1·X2·X11**: 승인 팝업이 사용자 키를 가로채지 않게 한다(상태줄 알림 + `Ctrl-] A` 모달 + 무장 지연). 오버레이 "복원" 대신 모드 전환 재그리기를 쓴다.
5. **X4·X5**: 락 탈취 응답은 접두키 명령으로만 받고 첫 키는 보류한다. 미완결 줄에서의 `Ctrl-] y`는 Ctrl-U 후 반환한다.
6. **D2·D3 및 §2.2 표**: T11·T12·T19·T20 의존성을 보완하고 M1 내장 프로필 작업을 신설한다. T12가 T09·T10·T13에 의존하도록 고친다.
7. **C9**: attach(user role) 토큰을 descriptor와 별도 파일로 분리한다.
8. **C6**: 사용자 입력권 대기 한도(`--defer-timeout`, 기본 10 s)와 명령 응답 timeout을 분리해 §3.7-4·§4.3·§4.6·E1 ④를 통일한다.

### 권장
9. **C10/A7**: expect timeout 응답에 `resume_from`을 넣고 A5·R-4 문구를 고친다.
10. **C21·C22**: 종료 코드 표에 누락 코드를 매핑하고 `E_CONFIRM_REQUIRED` → `E_APPROVAL_REQUIRED`로 바꾼다.
11. **A3–A6·A10**: SKILL Rules에 종료 코드별 분기(6의 하위 코드, 5의 `E_LOCKED`, 4·7·8)와 `delivery: unknown` 재송신 금지 규칙을 추가한다.
12. **C11**: R2 세션 승인 의미를 팝업 `[s]` 선택으로 통일한다(§10.1 문구 수정).
13. **C26**: M1 SafetyGate에 R2 문자열(`reboot` 등) 분류를 포함한다.
14. **X3·X12**: 전체화면 중에는 뷰어 창 제목(OSC 2)으로 HOLD·승인 대기·`A▶`를 표시한다. 상태줄 우선순위에 승인 대기를 추가한다.
15. **X6–X10**: hold 시 대기 중 send 즉시 종료, 트레이스 모드 키 전달·`U▶` 행, 일시정지 중 키 미송신, `approval_result`로 모든 뷰어 모달 닫기, strict 모드에서 CLI 승인 제외를 명시한다.
16. **D4**: R-8을 수동 체크리스트로 분리한다. `log export txt`를 M2로 올리거나 A2·R-2·SKILL에서 export를 M3 표기로 바꾼다.
17. **A8**: `no_viewer_attached` 경고 빈도 규칙(채널당 첫 1회 + 뷰어 0 전환 시)을 넣는다.
18. **C5·C7·C12·C14·C15·C20·C24**: 화면 ⓑ 문구, 커서 단위 표기, 뷰어 Enter = 프로필 eol, §0 키 목록, M1 트레이스 모드 기능, `--mode log`, client 표시 이름 규칙을 정리한다.
19. **C8·C13·C25·C23**: 용어 통일(channel / `LOOM_UART_CHANNEL`), 프로필↔규칙 묶음 매핑 표, `raw.NNNN.bin` 통일, `rts-pulse` 시퀀스 정의.
20. **C16–C19·A9**: 본문에만 있는 옵션(`reconnect --extend`, `open --resume`, `--pick`, `--wait-connected`, `--wait-approval`, `--cursor-token`, `--skip-gap`, `--raw`, `--settle`, `--lines`, `--pin`)을 문법 줄에 넣고, `references/cli.md`를 문법의 단일 원본으로 지정한다.
