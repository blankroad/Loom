# v2.0 검증 — uart-opus (plan.md v2.0, 1791줄)

- 작성: uart-opus, 2026-11-10. docs/plan/02-core-principle.md 준수. 점검만, plan.md는 고치지 않았다.
- 기준: `00-master-plan.md` §2.3 C1–C8, `04-v2-tasks.md`, 02 체크리스트. 대조한 파일: `_xteam/uart-ui.md`, `_xteam/uart-core.md`, `_xteam/core-test.md`, `_xteam/test-domains.md`, `_xteam/core-ui.md`.
- 줄 번호는 현재 plan.md 기준이다.

## 1. 02 체크리스트 (v2.0)

| # | 항목 | 판정 | 근거·남은 점 |
|---|---|---|---|
| 1 | 핵심 루프 표 | ✅ | §0 4요소 + TUI 보조 행 + 하나의 스트림 원칙 v2 |
| 2 | 종단 간 시나리오 | ✅ | §2.0 ①–④: 명령, 패널 화면, 구간 예산(≈66 ms), 시계 맞춤 측정, Ctrl-C, `others_since_last` |
| 3 | 실시간 목표·고부하 정직성 | ✅ | §3.11, §3.13 손실 계층, §5.7 배지. 웹 메모리 수치가 세 곳에서 다르다(#6) |
| 4 | M1 = 걷는 뼈대 | ⚠️ 조건부 | §11 M1은 웹 패널 기준이다. **UI 셸 M1이 늦으면 UART M1에는 사용자 화면이 하나도 없다**. TUI가 M2라서 대체 화면이 없다. §13에 이 리스크가 없다(권장 R1) |
| 5 | 비분리·숨은 연결 금지 | ✅ (보완 권장) | §3.12, M1 (d)(h). `ui_url`은 viewer 쿠키가 없는 브라우저에서는 열리지 않는다(`core-ui.md` L13 bootstrap). 안내 문안 보완 필요(R6) |
| 6 | 후순위·비범위 | ✅ | §1, §11 M4 |

**사용자 개입 경로가 웹으로 옮겨지면서 빠진 곳**
- 키 입력, Ctrl-C, HOLD, 입력 반환, 줄 지우기, takeover: 모두 M1 패널에 있다(§5.3, §5.8, §5.9, §11 ④).
- STOP: 셸 STOP과 `loom stop-all` 둘 다 있다.
- 기록 장애 사용자 예외: §3.7-8, §5.7 REC, M1 (f)에 있다.
- 승인: DR-V10에 따라 조건부다.
- 문서 안에서 서로 어긋나는 곳이 있다.
  - §3.7-7은 takeover를 M2로 둔다(#1).
  - §10.1–10.2는 R2·R3 확인을 "뷰어 `y`/`Y`"로 둔다(#3).
  - §5.15.5는 TUI `Ctrl-] A` 승인 화면을 남겨 두었다(#4).
- 이 셋은 v1.2 표현이 남은 것이다. 개입 경로 자체가 빠진 것은 아니다.

## 2. 내부 일관성

| # | 위치(줄) | 현재 | 문제 |
|---|---|---|---|
| 1 | §3.7-7 L361 | "M1은 보류 키 폐기 … `Ctrl-] o` 탈취는 M2", "관전 전용은 `--readonly`" | §5.8·§5.9·DR-V5·§11 ⑤·T10은 **takeover M1**이다 |
| 2 | §3.7-3 L355 | "사용자가 다시 attach해 줄을 끝내거나 지움" | TUI 표현. 웹은 탭을 다시 열거나 [줄 지우기]를 쓴다(§5.8) |
| 3 | §2.1 L185, §10.1 L1462, §10.2 L1485–1486 | "또는 뷰어 `y`(R3/R4는 대문자 `Y`)", "키 비가로채기 — §5.6", "M1에는 패널이 없어 `E_NO_APPROVER`" | 승인은 셸 대기열이 받는다(§5.12). §5.6은 지금 출처 강조 절이다. M1 동작은 DR-V10의 조건문을 따라야 한다 |
| 4 | §5.15.1 L980·L987, §5.15.5 L1025–1055 | "(M1부터)", "M1 트레이스 모드", Textual/prompt_toolkit, 키 표의 M1/M2 표기, `Ctrl-] A` 검토 화면 M2, `Ctrl-] o` M2, "§5.6과 같은 메커니즘" | TUI는 M2(터미널)·M3(트레이스)이다(§5.14·T19·T21). TUI는 승인하지 않는다(DR-V11). 입력 계층은 crossterm이다(§7.6·T19). 부록이라도 일정·승인 표기가 본문과 충돌한다 |
| 5 | §4.7 L687 | "문법의 **단일 원본은 `references/cli.md`**" | §4.8은 cli.md를 없애고 `help --json`을 정본으로 정했다(DR-V8) |
| 6 | §5.2 L813, §3.13 L503, DR-V22, `uart-ui.md` L81 | 그리기 대기 4 MiB / 웹 수신 대기 2 MiB·스크롤백 16 MiB 또는 32,768항목 / 2 MiB·10 000줄·20 000행 / 4 MiB | DR-V22(2 MiB, 10 000줄, 20 000행)가 정본인데 세 곳이 다르다 |
| 7 | §3.13 표 L498 | "UART 데이터 구독 전체 8개" | 바로 아래 L512(DR-V21)가 채널당 4개·전체 16개로 대체했다. 표 칸을 고치지 않았다 |
| 8 | §9.2 표 | `ansi` = M2, `sim://ecu-console` 행 없음 | §11 M1 ②·(f2)·T04는 M1에 `ansi` 질의 부분집합을 쓴다. §9.6.4·T27은 `ecu-console`(M2)을 쓴다 |
| 9 | §9.3 L1323–1324 | 계약 fixture 도구 "CORE fixture + pytest", 지연·폭주 "뷰어 stdout write 완료 `mono_ns`" | Python 0(DR-V19). 게이트 끝점은 write 콜백 뒤 첫 rAF다(DR-V6) |
| 10 | §3.8 L383 | "`list_ports.comports()` 폴링(백오프 0.25→0.5→1→2 s)" | DR-V23은 1 s 폴링, 끊긴 채널은 250 ms다. 함수 이름도 pyserial 것이다 |
| 11 | §3.4 L317 | `[확인: serialposix.py]` | 근거를 Rust로 바꿔야 한다(§7.2·DR-V17) |
| 12 | §3.9 표 L407 | `uart.input` params `channel, data(hex)` → `ack` | §5.3과 `uart-ui.md` 2.3은 `data_hex, input_seq`, 결과는 `uart.input_ack` 이벤트다. `uart-ui.md` 2.2의 `uart.help{}`가 §3.9에 없다 |
| 13 | §4.1 종료 코드 표 L539, §4.5 문법 L646 | 3 = "조건 대기형 timeout만", expect 문법에 M2 옵션 없음 | §9.6.1은 exit 3 확장(`E_UNEXPECTED_MATCH`·`E_COUNT_OUT_OF_RANGE`)과 `--within/--absent/--count/--sequence`를 쓴다. 참조 한 줄이 필요하다 |
| 14 | §9.6.3 L1410 | "Rust `regex`는 선형 시간 [uart-sol 확인 예정]" | §7.4·DR-V15에서 이미 확인했다 |
| 15 | §13 L1587 | "JSON/렌더/GIL" | Rust 전환 뒤에는 GIL이 없다 |
| 16 | 참조 번호 L1055, L1510, L4·L1642 | `§5.6`(승인 화면), `§5.2`(escape 차단), `§5.3`(상태줄 vehicle) | 지금 번호는 §5.12, §5.4·§5.5·§5.15.2, §5.7이다(L4·L1642는 이력 표라 낮은 우선순위) |
| 17 | §3.3 L292, §3.12 L471 | "`attach COM3`과 `attach dut`", "UART 질의 RPC(§_xteam 2.2)" | TUI 표현이 남았고, 존재하지 않는 절을 가리킨다 |
| 18 | §1 L45–46, L240, L513 | 범위 번호 순서가 "11"이 "10"보다 앞, `---` 앞 빈 줄 없음 | 형식 문제(렌더러에 따라 `---`가 제목 밑줄로 해석됨) |

**정합 확인(문제 없음)**
- §2.0 E1 ↔ §5.3·§5.8 ↔ M1 (a)(d)(e) ↔ T10·T11w·T12가 맞는다.
- §3.9 이벤트 kind와 `uart-ui.md` 2.1이 일치한다(`uart.expect_result`, `uart.input_ack`, `uart.input_suspect` 포함).
- §5.13 요약 ↔ §5.1–5.12의 M 배치가 맞는다.
- §11 M2 ↔ T14–T20·T27, M3 ↔ T21·T23·T28이 맞는다.
- 지연 목표 수치(100/20/500/150 ms)가 §2.0·§3.11·§5.13·`uart-ui.md` §4에서 같다.

## 3. `_xteam`과 plan.md의 차이 (+ 다른 팀 계약)

| # | 파일·위치 | 차이 | 영향 |
|---|---|---|---|
| X1 | `uart-core.md` #1 | "uart worker는 별도 프로세스"만 있다. **포트당 1개·최대 8**(DR-V13)과 supervisor 인스턴스 모델 합의 요청이 없다 | CORE가 단일 worker로 설계할 수 있다 |
| X2 | `uart-core.md` #5 | viewer 자격 = 브라우저 세션만 있다. M2 TUI(SSH 접속 Linux TTY)의 viewer 자격이 빠졌다(v1.2 §15 #1에는 있었음) | M2 TUI가 viewer를 받지 못할 수 있다 |
| X3 | `uart-core.md` | §3.13의 CORE 담당 항목이 요청에 없다: worker RSS 목표 48 MiB·hard 64 MiB와 cgroup/Job 집행, gateway UART 계정 128 MiB, 구독 상한 채널당 4·전체 16(DR-V21) | C2 관문을 집행할 주체가 비어 있다 |
| X4 | `uart-core.md` #8 | `loom-console`을 **WASM으로도 빌드**한다(DR-V2). 그러면 그것이 의존하는 `loom-schema` 등도 `wasm32` 대상에서 빌드되어야 하는데, 이 조건이 없다 | 웹 정제기 계획이 막힐 수 있다 |
| X5 | `uart-core.md` | 다른 도메인 기준점용 조회가 없다. `expect --from seq:N`이 **임의 도메인 이벤트의 `ts.mono_ns`를 seq로 조회**해야 한다(§9.6.1) | `--within` 구현 불가 |
| X6 | `uart-core.md` L5·L31–32 | 근거 "plan.md v1.2", 훅 "3종", Python 0 "목표" | plan v2.0, 훅 4종(`verify_evidence`), "UART Python 0"이 확정(DR-V19) |
| X7 | `uart-ui.md` L14 | `uart.approval_card` "(§3.4)" | 그런 절이 없다. → plan §5.12 |
| X8 | `uart-ui.md` L81 | 그리기 백로그 4 MiB | DR-V22 2 MiB(#6) |
| X9 | `uart-ui.md` | §3.12의 UI 요청 두 개가 없다: **새 채널 패널 자동 추가·토스트**, `ui_url` 딥링크와 `loom ui open` 패널 표기. 구독 상한(채널당 4 = 탭 수 한계)도 알리지 않았다 | UI가 숨은 채널 방지 요구를 모를 수 있다. 탭 5개째에서 구독이 거절될 수 있다 |
| X10 | `core-ui.md` L66·L108(CORE 제안, 합의 전) | 이름 차이: `core.time`·`core.metrics.report`·`core.viewer.presence{visible,rendered_through_seq,oldest_pending_age_ms}`·`no_visible_viewer`. plan은 `core.clock.ping`·`core.ui.latency_report`·`viewer_not_visible`을 쓴다 | P0 스키마에서 한쪽으로 맞춰야 한다 |
| X11 | `core-ui.md` L13·L42 | 브라우저 viewer는 `loom ui` bootstrap 일회성 토큰 → 쿠키로만 얻는다. `loom ui open <panel> [--at seq:N]`은 `ui.focus_request`를 보낸다 | plan의 `ui_url`(`…/#/uart/dut`)은 쿠키가 없으면 viewer가 되지 않는다. `loom ui open uart:dut`의 패널 표기도 미정이다 |
| X12 | `core-test.md` O3–O5, `test-domains.md` §1·§3·§4 (**TEST 정본**) | 증거 모델이 다르다(아래 목록) | **M2 수용 기준(TC-MIX-01)이 정본 CLI와 다르다** |

X12의 구체적 차이:
- ref 문법: TEST는 `<domain>:<channel>@seq:a..b`, plan은 `uart:dut:ev:88412`.
- 결과 필드: TEST는 `data.evidence[]` 배열, plan은 단일 `evidence` 객체.
- 검증 절차: TEST는 `loom test evidence add --range --pred`로 등록하고 `verdict --evidence e1,e2`로 인용한다. plan은 `step … --evidence <ref>`다.
- 도메인 술어: TEST는 `EvidenceVerifier::verify(pred, events, coverage)`에 `uart.text_contains`/`uart.text_absent`를 쓴다. plan은 `verify_evidence(evidence, claim)`다.
- 검증 상태: TEST는 `verified|mismatch|not_found|uncertain|error`, plan은 `valid`/`unverifiable`이다.
- 쓰기 기준점 kind: TEST는 `uart.tx`를 요구하고, plan에는 그런 kind가 없다(`uart.write`/`uart.data dir=tx`).
- timeout 증거: TEST는 timeout 증거를 `evidence[]`에 넣으라고 요구하는데, plan에는 없다.
- `test-domains.md`에 `## uart 의견`이 없다. UDS는 이미 답했다.

---

## 리드 수정 목록

### 필수
| # | plan.md 위치 | 대체 문안 |
|---|---|---|
| M1 | §3.7-7 L361 전체 | "7. **여러 뷰어**: 입력(writer) viewer는 **한 번에 하나**(먼저 키를 친 viewer — 브라우저 탭 1개 또는 TUI 1개, §5.8). 다른 viewer에서 키를 치면 그 키는 보류되고 `탭 2가 입력 중 — [입력 가져오기]`(TUI는 `Ctrl-] o`) 안내. 가져오기 = `uart.input.takeover`(보류 키 송신) — **M1**(DR-V5). 5 s 안에 응답이 없으면 보류 키 폐기 + 알림. Ctrl-C는 어느 viewer에서든 즉시 통과. 관전 전용은 패널 읽기 전용 토글(TUI `--readonly`)." |
| M2 | §3.7-3 L355 | "해제: 사용자가 다시 attach해 줄을 끝내거나 지움" → "해제: 사용자가 패널(새 탭 포함)에서 줄을 끝내거나 [⌫ 줄 지우기]/`Ctrl-] c`(input clear — 검증 프로필이면 ^U 송신 후 해제)" |
| M3 | §2.1 L185 주석, §10.1 L1462 R2 칸, §10.2 L1485–1486 | L185: "`# R2: bench는 확인 토큰(loom uart confirm) 또는 UI 셸 대기열 확인, sim은 auto`". L1462: "… → `loom uart confirm <tok>` 또는 **UI 셸 승인 대기열의 R2 확인**(R3/R4는 같은 대기열의 승인 카드, §5.12)". L1485: "→ 공통 승인 서비스가 UI 셸 대기열에 카드 + 패널 상태줄 `⚠ 승인 대기`(키 비가로채기 — §5.12). viewer가 없으면 exit 5 `E_NO_APPROVER`". L1486 끝: "`r2_channel = viewer`면 토큰 없이 셸 대기열만(셸 M1에 대기열이 없으면 M1에서는 `E_NO_APPROVER`, DR-V10)" |
| M4 | §5.15 제목 바로 아래(L978)에 한 단락 추가 | "> 이 부록은 v1.2 원문 보존이다. **일정·도구·승인은 본문이 우선**한다: TUI 터미널 모드 M2·트레이스 M3(§5.14, T19·T21), 입력 계층은 crossterm(Textual/prompt_toolkit 언급은 이력), 표 안의 M1/M2 표기는 'TUI 도입 시점 기준 기본/확장'으로 읽는다. `Ctrl-] A` 승인 검토 화면은 **없다** — TUI는 `⚠ 승인 대기 — UI에서 검토: <url>` 안내만(DR-V11). `Ctrl-] o`는 M2 TUI 도입과 함께. L1055 '§5.6과 같은 메커니즘'은 '§5.3 붙여넣기 확인 규칙과 같음'." |
| M5 | §4.7 L687 | "- 문법의 **단일 원본은 `loom uart help --json`**(§4.8, DR-V8)이며, 이 절·SKILL.md·`recipes.md`는 그 출력 기준으로 동기화 시험한다(M2)." |
| M6 | §5.2 L813, §3.13 L503 텍스트·UI 칸, `uart-ui.md` L81 | L813: "백로그가 4 MiB를 넘으면" → "**2 MiB**를 넘으면(DR-V22)". L503 UI 칸: "웹 수신·그리기 대기 2 MiB/패널, xterm scrollback 10 000줄(상한 50 000)·트레이스 링 20 000행(§5.2, DR-V22)". `uart-ui.md` L81: "그리기 백로그 4 MiB" → "수신·그리기 대기 2 MiB" |
| M7 | §3.13 표 L498 포화 동작 칸 | "구독당 **인코딩 후** 2 MiB·512항목, UART 데이터 구독 **채널당 4개·전체 16개**(DR-V21)" — L512의 "(위 표 '전체 8개'를 대체)"는 삭제 |
| M8 | §9.2 표 | `ansi` 행 M 칸 "M2" → "**M1**(질의 부분집합: DA1·DA2·DA3·XTVERSION·DSR·CPR·DECRQM·DECRQSS·`CSI 18 t`·`CSI ? u`·OSC 10/11 `?`·OSC 52/8/제목·마우스·포커스 — M1 (f2)) / M2(나머지)". 새 행: "`ecu-console` \| SimWorld `on_external{ecu_reset}`을 받으면 재부팅 로그 → `App start` → 쉘. `panic` 주입 옵션 \| M2(§9.6.4, T27)" |
| M9 | §9.3 L1323·L1324 | 계약 fixture 도구 "CORE fixture + pytest" → "CORE fixture + cargo test". 지연·폭주 내용 앞부분 → "측정 하네스: 데몬 청크 `mono_ns` vs **패널 write 콜백 뒤 첫 rAF**(시계 맞춤 §2.0②, TUI는 stdout write 완료 — M2)" |
| M10 | §3.8 L383 | "감지는 `serialport::available_ports()` 폴링 **1 s**(끊긴 채널을 찾는 동안 250 ms, 결과 최대 256포트 — DR-V23), 2분 안에 못 찾으면 STOPPED. OS 핫플러그 알림은 깨우는 힌트로만(M4)" |
| M11 | §9.6.2–9.6.4 + §4.8 SKILL "Evidence for tests" (TEST 정본에 맞춤, X12) | (a) 9.6.2 첫 문단 뒤: "**형식은 TEST 정본**(`core-test.md` O3–O5, `test-domains.md` §1)을 따른다: 결과 `data.evidence[] = [{ref:"uart:dut@seq:88120..88409", kind, …요약}]`. ref 문법은 `<domain>:<channel>@seq:a..b`. 에이전트는 ref를 복사만 한다. kind: `uart.match`·`uart.absence`·`uart.count`·`uart.write`(쓰기 시간 기준점 = `uart.write` 행 seq — TEST가 요청한 `uart.tx`에 대응하며 `## uart 의견`으로 회신). timeout·미충족 때도 `uart.expect_result{outcome:"timeout"\|"unexpected_match"…}` 이벤트를 발행하고, 그 seq·검사 범위·gap을 `evidence[]`에 넣는다." 예시 JSON의 `"ref": "uart:dut:ev:88412"` → `"ref": "uart:dut@seq:88120..88412"`. (b) 9.6.3 첫 줄: "CORE `EvidenceVerifier::verify(pred, events, coverage)`의 UART 구현. 도메인 술어 `uart.text_contains{literal\|regex, ignore_case?}`, `uart.text_absent{…}`, `uart.text_count{…, min, max}`. 결과 상태는 코어 공통 `verified\|mismatch\|not_found\|uncertain\|error`" — 항목 1의 `unverifiable`·항목 6의 `{valid, reason}`도 이 상태로 바꾼다. 부재·개수 술어에서 coverage가 겹치면 `uncertain`(코어 O4-3과 같음). (c) 9.6.4 명령열을 정본 CLI로: `loom test step --begin 1` → `loom uds reset hard --json` → `loom uart expect dut 'App start' --from seq:88117 --within 5s --json` → `loom test evidence add --step 1 --range seq:88117..seq:88412 --domain uart --channel dut --pred '{"op":"uart.text_contains","literal":"App start"}'`(→ e2, CAN·UDS·absence도 같은 방식 e1·e3·e4) → `loom test verdict 1 pass --evidence e1,e2,e3,e4 --note "…"` → `loom test end` → `loom test report --html --junit`. (d) SKILL 절 제목 "## As a test step (with `loom-test`)", 내용: "Judge with `expect` (`--within/--absent/--count/--sequence`); copy `data.evidence[].ref` verbatim, never type seqs; writes as time anchors: `uart.write` ref from `send --json`; `inconclusive`/`uncertain` → report inconclusive." |
| M12 | `_xteam/uart-core.md` | #1 끝에 "**포트(채널)당 worker 1개, 최대 8**(plan DR-V13) — supervisor 인스턴스 모델을 P0에 합의" 추가. #5 끝에 "M2 보조 TUI(SSH로 접속한 Linux TTY 포함)의 viewer 자격도 함께" 추가. 새 요청 3개: **#17** "worker 메모리 집행: RSS 목표 48 MiB·hard 64 MiB, Linux cgroup/Windows Job 제한·RSS 계측, gateway UART 계정 128 MiB(plan §3.13)", **#18** "UART 데이터 구독 상한 채널당 4·전체 16, 구독당 인코딩 후 2 MiB·512항목(DR-V21)", **#19** "`loom-console`이 의존하는 core 타입 crate는 `wasm32-unknown-unknown`에서도 빌드 가능해야 함(DR-V2 WASM 정제기), 그리고 임의 도메인 이벤트의 `ts.mono_ns`를 `seq`로 조회하는 내부 API(`expect --from seq:N --within`, plan §9.6.1)". L5: "plan.md v2.0 §3·§9.6·§15". L31: "훅 4종(`classify`/`approval_binding`/`presend_check`/`verify_evidence`)". L32: "**UART Python 0 확정**(plan §7.6, DR-V19)" |
| M13 | `_xteam/uart-ui.md` | L14 "(§3.4)" → "(plan §5.12)". §5 질문에 13–14 추가: "13. **새 채널 패널 자동 추가 + 모든 탭 토스트**(끄더라도 장치 목록에는 남김, plan §3.12) — 숨은 채널 방지 요구다", "14. `no_viewer_attached`의 `ui_url` 딥링크(`/#/uart/<ch>`)와 `loom ui open` 패널 표기(`uart.terminal --channel dut --at seq:N`?)를 정해 달라. 쿠키가 없는 브라우저에서 딥링크를 열면 bootstrap 안내로 보내는가?" 2.1 아래 한 줄: "구독 상한: UART 데이터 구독 채널당 4개(= 같은 채널을 동시에 여는 탭·패널 수), 전체 16개(DR-V21). 넘으면 거절 대신 '읽기 전용 미러'로 표시할지 UI가 제안해 달라" |

### 권장
| # | 위치 | 대체 문안 |
|---|---|---|
| R1 | §13 표에 행 추가 | "UI 셸 M1 지연 \| UART M1에 사용자 화면이 없음(02 항목 4 불합격) \| 중 \| P1 착수 때 UI 셸 일정을 확인한다. 늦으면 T19 TUI 터미널 모드를 P1 임시 화면으로 앞당긴다. 관문 판정은 웹 패널 기준을 유지하고, 임시 TUI로는 시연만 한다" |
| R2 | §3.9 표 L407 | `uart.input` 행: params "`channel`, `data_hex`, `input_seq` — viewer 전용, 쓰기 클래스 ③", 결과 "즉시 ack + `uart.input_ack{input_seq, sent\|held\|discarded, reason}` 이벤트(§5.3)". 새 행: "`uart.help` \| `verb?` \| `help --json`과 같은 원본(패널 도움말·버튼 가용 여부)". `uart.expect` params 끝에 "(M2) `within_ms`, `absent`, `for_ms`, `count{min,max}`, `sequence`(§9.6.1)" |
| R3 | §4.1 종료 코드 표 3행, §4.5 문법 아래 | 3행 의미 칸: "**조건 대기형 timeout만**(확장 요청 중: 부정·횟수 미충족 포함 — `_xteam/uart-core.md` #15, UDS `E_NO_MATCH`와 이름을 함께 정리)". §4.5 끝: "- 통합 테스트용 `--within/--absent --for/--count/--sequence`(M2)는 §9.6.1." |
| R4 | §3.4 L317 | "`[확인: serialposix.py]`" → "`[확인: §7.2 — serialport 정지 비트 enum에 1.5 없음, Windows만 native 확장(DR-V17), POSIX는 사전 거절]`" |
| R5 | §9.6.3 L1410 | "[uart-sol 확인 예정]" → "[확인: §7.4, DR-V15]" |
| R6 | §3.12 `no_viewer_attached` 표 아래 | "- `ui_url`은 이미 viewer 쿠키가 있는 브라우저에서만 바로 열리는 딥링크다. 쿠키가 없으면 사용자가 `loom ui`(일회성 bootstrap, `core-ui.md`)로 먼저 연다. 에이전트는 bootstrap 토큰 URL을 받지도 전달하지도 않는다(자기 승인 방지)." |
| R7 | §2.0② 측정 3, §3.12, §11 (b) | RPC·경고 이름 뒤에 "(이름은 CORE P0 스키마 확정명을 따름 — `core-ui.md` 제안 `core.time`·`core.metrics.report`·`no_visible_viewer`)"를 한 번 붙인다(X10) |
| R8 | §13 L1587 | "고속(921600–3M)·다중 포트 팬아웃 시 JSON 인코딩·브라우저 렌더 부하" (GIL 삭제). 같은 표 "정제 패스스루…" 행의 "adversarial 시험(M1)" → "웹 정제기 adversarial 시험(M1), TUI(M2)" |
| R9 | §3.3 L292, §3.12 L471 | L292: "(경로 이름(`COM3`)과 별칭(`dut`)이 같은 채널이면 패널 머리줄·TUI 헤더에 함께 표시.)". L471: "UART 질의 RPC(§3.9, `_xteam/uart-ui.md` 2.2)" |
| R10 | 참조·형식 | L1510 "뷰어 escape 차단(§5.2)" → "(§5.4·§5.5, TUI §5.15.2)". L4·L1642 "§5.3" → "§5.7"(이력이라도 링크용). §1 범위 항목 순서를 10 → 11로. L240·L513의 `---` 앞에 빈 줄 |
| R11 | `test-domains.md`에 리드가 `## uart 의견` 추가 | "전부 수용. ① `data.evidence[]`, ref `uart:<ch>@seq:a..b`. ② kind: 판정 `uart.match`/`uart.absence`/`uart.count`, 쓰기 기준점 **`uart.write`**(TEST 요청 `uart.tx`에 대응 — UART 정본 이벤트 이름 유지), timeout은 `uart.expect_result{outcome}` seq. ③ JSON Pointer 고정: `/payload/text`, `/payload/match/text`, `/payload/match/rx_offset`, `/payload/outcome`, `/payload/generation`. ④ 정규식 = Rust `regex` 문법(술어 `regex`와 공유, TC-C7). ⑤ `loom-uart` 스킬에 'As a test step' 5줄 절" |
