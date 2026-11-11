# UART R2 교차 리뷰 — uart-opus → r1-sol.md

- 작성: uart-opus (Opus 5.5), 2026-11-09. docs/plan/02-core-principle.md 준수.
- 리뷰 대상: `docs/plan/uart/r1-sol.md` (+ `r1-sol-pypi-evidence.json`, `spikes/uart/README.md`, `pty_probe.py`)
- 비교 기준: 내 초안 `r1-opus.md`, 00-brief §3/§4, uart/brief.md, 02-core-principle.md
- 중점: Sol 안의 아키텍처·UX·스킬·안전 설계가 **에이전트와 사용자에게 실제로 쓰기 좋은지**, **상태 모델의 빈틈**.
- 이번 라운드 확인 방법: pyserial 3.5 소스(Sol의 `spikes/uart/.venv` 안 설치본)를 직접 읽음, 로컬 Python으로 짧은 인라인 실험(파일 미생성), `uv run --no-project --with …` 임시 환경(전역 설치 없음), PyPI JSON API. 재현 명령은 각 항목에 적었다.

### 한 줄 총평
Sol 초안은 **사실 근거·실패 모드·정직한 한계 표기**가 내 초안보다 강하다(송신 결과 상태, 장치 출력 불신, ANSI 위험 escape 차단, 비밀값 기록 방식, 구독 이음매 알고리즘). 반면 **에이전트가 실제로 쓰는 표면**(기본값, 동사, 커서 정체성, 데몬 자동 기동, 스킬 본문)과 **사용자가 "터미널 수준"으로 보는 화면**은 얇거나 사용성을 해치는 기본값이 있다. 상태 모델은 입력권(lease)·세션 수명·재연결 중 expect에서 빈틈이 크다. 통합 시 "Sol의 안전·정합성 장치 + Opus의 동사·기본값·터미널 패스스루"가 가장 좋은 조합이라고 본다.

---

## 1. 틀린 사실

| # | Sol 초안의 주장 | 실제 | 근거 / 확인 방법 | 확신도 |
|---|---|---|---|---|
| F1 | (§8, §9) `send_break(20 ms)`가 403 ms 걸린 것은 "POSIX termios/driver별 길이 해석 차이" 때문이며, ms 단위 부트 타이밍에 "API 이름만 믿지 않는다" | 원인은 **pyserial 자체의 양자화**다. POSIX `send_break`는 `termios.tcsendbreak(self.fd, int(duration / 0.25))`를 호출한다. 0.02/0.25 → `int(0.08)` = **0**이고, POSIX에서 `tcsendbreak(fd, 0)`은 **0.25–0.5초 break**다. 따라서 403 ms는 예측 가능한 결과다. 결론("짧은 break는 send_break로 못 한다")은 맞지만 이유가 다르다. 대응도 달라진다: 250 ms 미만 break가 필요하면 `break_condition=True` → sleep → `False`(Sol 실험에서도 3.75 ms)를 쓰고, `send_break`는 POSIX에서 250 ms 배수로만 의미가 있다는 제약을 CLI(`--ms`)에 문서화해야 한다. Windows는 `serialutil.SerialBase.send_break`의 공통 구현(`break_condition` + `time.sleep`)을 쓴다 | `spikes/uart/.venv/lib/python3.15/site-packages/serial/serialposix.py:694-701`, `serialutil.py:573-582` 직접 읽음 | 높음 |
| F2 | (§6 표) `clock_id`: "monotonic은 **데몬 재시작마다 다른 epoch**", (§9) "임의 프로세스 monotonic 값의 차를 그대로 신뢰하지 않는다" | Python 문서상 기준점은 "정의되지 않음"이 맞다. 하지만 3-OS 구현은 모두 **호스트 전역 시계**다(macOS `mach_absolute_time()`, Linux `CLOCK_MONOTONIC`, Windows QPC). 같은 호스트의 두 프로세스 값은 직접 비교할 수 있다. 실험: 두 프로세스를 연달아 실행해 차이가 190 ms(프로세스 기동 시간)였고, 값 ≈ 803,983 s ≈ 부팅(2026-10-30 20:22) 이후 경과 시간과 일치했다. 그러므로 `clock_id`는 "데몬 부팅"이 아니라 **호스트 부팅 ID** 단위여야 한다. 측정 하네스도 같은 호스트의 뷰어 수신 시각과 데몬 청크 시각을 그대로 빼면 된다(지연 측정이 단순해진다). **추가 주의(양쪽 초안 공통)**: macOS `mach_absolute_time`은 **시스템 sleep 중 멈춘다** → 노트북 sleep 후 mono↔wall 차이가 변한다. 청크마다 wall도 같이 기록하는 Sol 방식이 맞고, sleep/wake를 기록 이벤트로 남겨야 한다 | `python3 -c "import time;print(time.get_clock_info('monotonic'))"` → `implementation='mach_absolute_time()'`. 프로세스 2개의 `monotonic_ns()` 비교, `sysctl kern.boottime` | 중-높음 (Linux/Windows는 문서 기반 [추정]) |
| F3 | (§7 표) pyte: "PyPI license 문자열 공란. 정확 면허 미확인" | PyPI `license` 필드는 비어 있지만 classifier가 **`License :: OSI Approved :: GNU Lesser General Public License v3 (LGPLv3)`** 이다. "미확인"이 아니라 LGPL-3.0이다(채택 시 LGPL 의무 검토 대상) | `curl https://pypi.org/pypi/pyte/json` → `info.classifiers` | 높음 |
| F4 | (§0 E2E, "M1 수용 시나리오") `loom uart expect dut --literal 'temp=' --after-request q1` 를 M1 시나리오로 제시 | 같은 문서 §11은 **expect·cursor·lease를 M2**로 둔다(M1 = "agent open/send/read"). 즉 M1 수용 시나리오가 M1 범위 밖 기능을 쓴다 — **내부 불일치**. 어느 쪽으로 고칠지는 §5-4 참조(내 추천: expect를 M1에 넣는다) | r1-sol.md §0 vs §11 표, §12 S4(cursor/read/expect는 O2·S3 이후) | 높음 |
| F5 | (§3) "POSIX `exclusive=True`를 보조로 사용", "비협조적 외부 프로그램의 open까지 막는 보장은 없다" | **맞다**(pyserial은 `fcntl.flock(LOCK_EX|LOCK_NB)`, `serialposix.py:385`). 내 초안 §3.12·§8의 "`exclusive=True`(TIOCEXCL)"는 **틀렸다**(내 오류 인정). 더 나아가, 이를 보강하려고 `TIOCEXCL` ioctl을 직접 시험했지만 **macOS pty에서는 다른 프로세스의 open을 막지 못했다**. 결론: macOS에서는 flock도 TIOCEXCL도 pty 기준 확실한 장벽이 아니다. "숨은 연결 금지"는 OS 잠금이 아니라 **설계(공개 API에 포트 open 경로 없음) + 스킬 규칙 + 실HW gate 검증**으로 보장해야 한다. 실제 USB 시리얼 드라이버(IOSerialFamily, Linux tty)에서 TIOCEXCL 효과는 미확인이다 → M3 HW gate 항목으로 추가 | 인라인 실험: `os.openpty()` slave를 열고 `fcntl.ioctl(fd, termios.TIOCEXCL)` 후 자식 프로세스에서 `os.open(path)` → "child open OK (not blocked)" | 높음(pty), 실HW는 [추정] |
| F6 | (§4) regex 폭주 대책으로 "제한된 worker subprocess, 평가당 50 ms 초과 시 worker 종료", 대안 `regex` 패키지는 "새 의존성 조사·라이선스 검증 후" | 사실 오류는 아니다. 미결이던 대안을 확인했다: `regex` **2026.11.9 (2026-11-09), Apache-2.0 AND CNRI-Python**이고 `timeout=` 인자를 받는다. stdlib `re`가 24자 입력에서 **3.4초** 걸린 고전적 폭주 패턴 `^(\w+\s?)*$`를 `regex`는 즉시 처리했다. 청크마다 subprocess IPC를 오가는 방식보다 **인프로세스 `regex` + timeout**이 지연·구현 비용 모두 낫다(§3 표 B9) | `uv run --no-project --with regex python -c "regex.search(..., timeout=0.05)"` (임시 환경) | 중간(timeout이 실제로 발동하는 패턴은 아직 못 만들었음) |
| F7 | (§1, §3) Python 버전 근거를 "pyserial3.5의 Python3.11+ 조합은 … CI한다"로 두고, 스파이크는 **Python 3.15.0**에서 수행 | 사실 오류는 아니지만 근거 범위에 주의해야 한다. 스파이크는 3.15에서 돌았고 공통 계약 하한(3.11)은 미검증이다. Sol도 README에서 이를 밝혔다. 수용 기준에 "3.11 CI 통과"를 명시해야 한다 | spikes/uart/README.md | — |

> 내 초안의 사실 오류(이 리뷰 중 발견, 기록용): ① 위 F5의 TIOCEXCL 주장. ② wall 시각을 `wall_at_start + (t − mono_start)`로 파생한 설계는 macOS sleep 후 틀린다(F2) → Sol의 청크별 wall 기록이 맞다. ③ websockets `unix_serve`·Textual `App.suspend`는 존재를 확인했다(websockets 17.4, Textual 8.2.8 임시 환경에서 `hasattr` True).

---

## 2. 빠진 것

우선순위 순서(★★★ = 통합 전 반드시 보완).

### G1 ★★★ "터미널 수준" 화면이 M1–M2에 없음
- Sol §5: "장치 ANSI를 host terminal에 그대로 통과시키지 않는다. **M1은 escaped text + 기본 CR/backspace**, M2는 안전한 SGR/cursor subset". 화면 목업도 줄 단위 이벤트 로그다.
- 이렇게 되면 M1–M2 동안 **readline/linenoise 줄 편집**(ESP-IDF 콘솔, Zephyr shell, bash의 history 재그리기), **U-Boot/메뉴형 부트로더**, 컬러 로그, `top`/`vi`가 모두 깨진다. 브리프의 "TeraTerm/minicom 수준"과 핵심 원칙의 "사용자가 같은 데이터를 본다"를 둘 다 약화한다(바이트는 같지만 **보이는 결과가 장치가 의도한 화면과 다르다**).
- 보완안: **정제된 패스스루**를 M1 터미널 모드로 둔다(B1). 장치 바이트를 호스트 터미널(herdr 패인)로 보내되 sanitizer를 거친다:
  - 통과: C0 제어(BEL 제외 레이트 제한), CSI 전체(커서 이동·지우기·SGR·스크롤 영역; 단 DECSTBM은 상태바 보호를 위해 범위 재매핑), 단순 ESC 시퀀스(ESC 7/8, ESC c 제외).
  - 차단·가시화: OSC 전체(52 클립보드, 0/2 제목, 8 링크, 7 cwd …), DCS/APC/PM/SOS, ESC c(RIS — 상태바·모드 파괴), BEL 폭주(초당 2회 초과). 차단 시 상태바 `⚠ blocked OSC52 ×3`.
  - Sol이 정확히 지적한 **보안 위험(OSC52 등)** 을 받아들이되 화면 충실도는 유지한다. Sol의 줄 단위 로그 화면은 **로그 모드**(M3)로 보존한다.

### G2 ★★★ 데몬 자동 기동·디스커버리 없음
- Sol §4 첫 줄 `loom daemon start --bind 127.0.0.1 --port 0` 외에 자동 기동 언급이 없다(문서 전체에 "자동 기동/auto" 0회). 에이전트가 매 세션 첫 호출 전에 데몬을 띄우고, 실패·중복을 처리해야 한다 → 스킬이 길어지고 에이전트가 자주 실수하는 지점이 된다.
- 보완: 첫 `loom uart …` 호출 시 descriptor(사용자별, 0600/ACL) 확인 → 없거나 죽었으면 detach 기동 → 최대 3 s 대기. `LOOM_NO_AUTOSTART=1`이면 기동하지 않고 전용 종료 코드. Sol의 "runtime lock으로 중복 실행 방지"와 합친다.

### G3 ★★★ 짧게 사는 CLI 호출에서 에이전트 정체성(=커서 소유자)이 정의되지 않음
- Sol: 커서는 `(session_id, client_id, RX offset)`, "client_id/role/origin은 **서버가 부여**". 그런데 CLI는 호출마다 새 연결이다 → 서버가 연결마다 부여하면 **다음 호출에서 같은 client_id를 받을 방법이 없다**. 예시의 `--client a1`, `c_a1_dut`는 누가 어떻게 정하는지 빠져 있다.
- 결과적으로 "에이전트별 마지막으로 본 이후" 기능이 매 호출 `--cursor <opaque>`를 에이전트가 들고 다니는 방식으로만 동작한다. 에이전트는 JSON에서 `next_cursor`를 꺼내 다음 명령에 넣어야 하고, 한 번 놓치면 끝이다.
- 보완: **안정 정체성 = `LOOM_CLIENT` 환경변수 → 없으면 기본 `agent`**. 커서는 `(channel, client_name)`에 서버가 저장하고 `read`가 암묵적으로 전진한다. Sol의 opaque `next_cursor`·revision 충돌은 **고급 옵션**으로 유지한다(두 에이전트 동시·응답 유실 재조회용). origin(agent/user) 자체는 Sol 말대로 서버가 role로 부여하고 위조를 금지한다 — 정체성 이름과 권한(role)을 분리하면 된다.

### G4 ★★★ 재연결 시 expect 종료 기본값이 대표 레시피를 깬다
- Sol §4: "reconnect generation 변화 … **기본 expect는 CONNECTION_CHANGED로 종료**". §8: 리셋 민감 profile은 자동 재open off, §14: **unknown 보드는 reset-sensitive로 간주, 자동 재open off**.
- 네이티브 USB CDC 보드(ESP32-S2/S3/C3 USB-JTAG-Serial, STM32 VCP, RP2040, nRF52840)는 **리셋하면 USB가 재열거**된다. "리셋 → 부팅 로그 기다리기"라는 가장 흔한 에이전트 작업이 ① expect가 즉시 종료되고, ② unknown 보드는 재open조차 안 되어 사용자 화면이 DISCONNECTED에 멈춘다. 브리프의 "뽑았다 꽂기 재연결"과도 충돌한다.
- 보완: (a) **읽기형 대기(expect/read)는 재연결을 넘어 계속**이 기본, 응답에 `generation`·`disconnects[]`를 넣고, `--fail-on-disconnect`로 Sol 동작을 선택한다. (b) **TX·lease·승인은 세대 변경 시 취소**(Sol 안 그대로). (c) 자동 재open 기본 = **serial 번호 정확 일치 시 ON**, location만 일치하면 사전 허용 시에만, reset-sensitive profile은 opt-out. "unknown = reset-sensitive"는 너무 보수적이다 — USB 전원 보드는 재연결 시점에 이미 전원이 다시 들어온 상태라 DTR glitch로 잃을 것이 없다.

### G5 ★★ 에이전트 동사가 저수준뿐 — 실제 작업 레시피가 길고 깨지기 쉬움
- 없는 것: `run`(명령 출력만 추출: 에코·프롬프트 제거), `send --expect`(원자적 송신+대기 — Sol의 `--after-request q1`은 에이전트가 JSON에서 request_id를 꺼내 다음 명령에 넘겨야 함), `grep`(대용량 로그를 컨텍스트에 붓지 않고 검색), `mark`/`--since mark:boot-1`, 상대 시간 `--since 30s`(Sol은 timezone 필수 RFC3339만 허용 — 에이전트가 현재 시각을 계산해야 함).
- `read --timeout 1s`의 의미가 불명확하다: 새 데이터가 없으면 timeout까지 **기다리는가**, 즉시 빈 결과인가? `--tail`과 결합하면 무엇을 기다리는가? 명시해야 한다.
- `send` 기본 eol = none(B5). 에이전트가 `--eol cr`을 빠뜨리면 명령이 실행되지 않고 **조용히** 실패한다(장치는 입력을 버퍼에 두고 기다림). 이건 에이전트 실수 1순위가 될 것이다.

### G6 ★★ 외부 도구 공존(포트 넘겨주기) 경로 없음
- Sol §1: 펌웨어 플래셔는 비범위(동의). 그러나 **포트를 잠시 넘기는 방법**이 없어서, esptool/stm32flash/dfu-util을 쓰려면 `close`해야 한다 → 세션이 끝나고(§4 "close/reopen은 새 session"), 사용자 뷰어가 끊기고, 커서는 STALE이 된다.
- 보완: `release/reclaim`(세션·뷰어·커서 유지, 뷰어에 `RELEASED` 배너, R2). 핵심 루프 연속성의 문제다.

### G7 ★★ 스킬 본문·레시피 부재
- Sol §12 끝 한 단락(SKILL.md 요소 나열, references 6개 파일명)뿐이다. **SKILL.md 초안 본문, 레시피(부팅 대기→로그인→명령→파싱, 리셋 후 부트로그 캡처, U-Boot 중단), 종료 코드별 에이전트 행동 규칙**(예: exit 5에서 재시도 루프 금지)이 없다. 공통 지시의 "스킬 패키징" 쟁점과 R1 Opus 관점 항목이다.
- Sol의 좋은 원칙 두 가지 — "장치 출력은 신뢰할 수 없는 데이터(prompt injection)", "runtime policy를 스킬 선의에만 맡기지 않는다" — 는 SKILL.md 본문 규칙으로 꼭 들어가야 한다(§4 합의).
- Codex 호환: 양쪽 모두 발견 경로 미검증. R3 전에 확인할 항목으로 남긴다.

### G8 ★★★ 상태 모델 빈틈 (요청 중점)
| # | 빈틈 | 문제 상황 | 보완안 |
|---|---|---|---|
| S1 | **raw 모드 사용자 lease가 영구 유지** ("첫 키부터 `Ctrl-] y`까지") | 사용자가 프롬프트를 보려고 Enter 한 번 누르면 에이전트는 사용자가 yield를 기억할 때까지 **무기한 차단**된다. 기본 attach가 raw 모드라 사실상 늘 발생한다. 게다가 agent send는 "즉시 거절(HUMAN_ACTIVE)"이라 에이전트가 재시도 루프에 빠지거나 매번 사용자에게 물어야 한다 | **줄 완결 + 짧은 유휴 시 자동 반환**: 사용자의 마지막 바이트가 CR/LF/^C/^U이고 1.5 s 유휴면 입력권 자동 반환. **미완결 줄은 절대 자동 반환하지 않는다**(Sol의 지적이 맞고, 내 R1의 "3초 무입력 후 송신"은 틀렸다). 에이전트 send는 즉시 거절 대신 `--timeout`까지 **대기**(`deferred_ms`), 그래도 안 되면 `E_USER_ACTIVE`와 함께 사용자가 쓰던 내용 길이 정도만 반환 |
| S2 | 사용자 lease 보유 중 **뷰어 종료/크래시/herdr 패인 닫힘** | 명시 없음 → 입력권이 고아가 되면 에이전트가 영구 차단될 수 있다 | 뷰어 연결 종료 = 즉시 lease 해제 + `input_owner` 이벤트. hold(명시적 차단)만 연결과 무관하게 유지하고 상태바·`status`에 표시 |
| S3 | **세션 = open 1회** ("close/reopen은 새 session, 이전 cursor STALE") + 뷰어는 session_id에 결합 | 에이전트가 baud를 바꾸려고 close→open(M1에는 configure 없음)하면 사용자 뷰어가 닫힌 세션에 남고 커서는 STALE이 된다 → "사용자 화면이 에이전트 세션과 분리"되는 상황이 **설계상 생긴다** | 뷰어·커서는 **채널 별칭(dut)** 에 붙고 세션 교체를 따라간다(헤더에 `gen`/`session` 변경 배너). `configure`(baud 변경)를 M1 또는 M2 초에 둬서 close/reopen 필요를 줄인다 |
| S4 | 재연결 **STOPPED**(2분 후) 이후 | 세션이 닫힌 건지, 커서·뷰어·기록은 어떻게 되는지 불명 | STOPPED = 세션 유지·포트 없음(뷰어 배너, `open --resume`/`reconnect --extend`로 재개). 닫힘과 구분 |
| S5 | **OPENING 실패·CLOSING·RELEASED·degraded** 상태 부재 | Sol 상태열은 재연결 경로만 있다. open 실패(PORT_BUSY)·close 진행 중·외부 도구 사용 중·기록 실패(degraded)가 상태 표시에 안 나타난다 | 상태기 = {OPENING, OPEN, DISCONNECTED, MATCHING, REOPENING, AMBIGUOUS, STOPPED, RELEASED, CLOSING, CLOSED, ERROR(code)} + **직교 플래그** {recording: ok/degraded/failed, input_owner: none/user/agent, hold} |
| S6 | **에이전트 lease TTL 10 s, 상한 30 s** 와 CLI 관계 | 매 send가 lease를 자동 취득하는지, 여러 단계 시퀀스(U-Boot 중단 → 여러 setenv)를 어떻게 보호하는지 불명. 30 s 상한이면 부팅 대기 + 명령 시퀀스를 하나로 묶을 수 없다 | 기본: send는 **암묵적 단발 lease**(송신 동안만). 명시 `lock --ttl`은 갱신 가능(각 호출이 TTL 연장)하고 사용자가 언제든 탈취(이미 양쪽 합의) |
| S7 | **기록 실패 시 TX 차단 범위** ("디스크 full/기록 실패에는 TX를 차단") | 사용자 타이핑까지 막으면 사용자가 장치를 복구하지도 못한다 | 에이전트 TX만 차단(감사 불가 상태에서 자동화 금지). 사용자 TX는 허용하되 빨간 `REC FAILED` 표시 |
| S8 | **데몬 재시작 복구** | 세션·커서·승인·진행 중 expect가 어떻게 되는지 없음 | 재시작 시 세션 자동 재open은 하지 않는다(DTR 위험). 커서·마크는 디스크에서 복원하고, `status`에 `recovered_from_crash`. 진행 중 요청은 클라이언트에 연결 끊김으로 실패(종료 코드 7) |
| S9 | **여러 뷰어 간 입력권** ("하나만 입력, 나머지 watch") | 두 번째 뷰어가 입력하려면? 승계 규칙이 없다 | 뷰어 간에는 마지막 키를 친 쪽이 소유(사람끼리는 같은 사람이라고 가정) 또는 `Ctrl-] u`로 획득. watch-only는 `--readonly` 명시일 때만 |

### G9 ★★ 안전 정책이 에이전트를 쓸모없게 만들 위험
- Sol §10: "unknown shell/file/hex TX는 안전한 명령이라고 분류하지 않고 **R2 이상 profile 검토**를 요구", R1 = "승인된 status/help", §14 기본 = "R0 + 명시 status/help profile부터".
- 그러면 에이전트가 디버깅하며 `dmesg | tail`, `cat /proc/meminfo`, `ls /sys/...`처럼 **처음 보는 명령마다 승인**이 필요하다. 핵심 루프의 "쓰기"가 사실상 막힌다. "문자열 검사는 보안 경계가 아니다"는 옳지만, 그렇다고 allowlist를 기본으로 하면 사용성을 잃는다.
- R3("flash/erase/메모리 쓰기 … **기본 거절, 현재 자동화 범위 밖**")와 R4(비가역) 구분이 없다. 브리프는 "부트로더 진입, 펌웨어 쓰기 명령 문자열 **처리**"를 요구한다(금지가 아니라 처리). U-Boot `tftp; sf write`처럼 사용자가 의도한 재플래시를 에이전트가 사용자 확인하에 하는 것은 정당한 작업이다.
- 사용자가 승인하는 **UX가 없다**: "user approve client"라는 표현뿐, 승인 화면·명령·만료 표시가 없다.
- 보완 → B6(세션 신뢰 모드 + denylist 확인 + R4 차단 + 뷰어 팝업 승인).

### G10 ★ 사용자에게 "에이전트가 지금 무엇을 기다리는지"가 안 보임
- Sol 화면에는 TX 줄과 `owner:` 표시는 있지만, **에이전트의 대기 상태**(`expect /login:/ 37s left`, "사용자 줄 완결을 기다리는 send")가 없다. 사용자는 화면이 조용할 때 에이전트가 멈춘 건지 기다리는 건지 모른다.
- 에이전트 쪽 반대 방향도 없다: 뷰어가 0개일 때 에이전트에게 `no_viewer_attached` 경고를 줘서 사용자에게 attach를 권하게 하는 장치가 없다(핵심 원칙 4요소 중 "사용자 실시간 표시"를 런타임이 보장하는 유일한 수단).

### G11 ★ 기타
- 키 바인딩: herdr prefix(`Ctrl-B`)와의 충돌 검토가 없다(나는 `herdr --default-config`로 확인). 뷰어 쪽 break/리셋/hold 키가 없다.
- §2 시나리오는 표 한 줄씩이라 "구체적으로" 요구에 비해 얇다(명령·화면은 §0 E2E에만 있음).
- `loom uart lines dut --dtr off`(Sol)와 `loom uart signal dut dtr off`(Opus) 등 동사 이름은 R3에서 통일이 필요하다.

---

## 3. 더 나은 선택

평가 기준: 단순성 / 안정성 / 에이전트 사용성 / 사용자 가시성 / Windows 호환 / 구현 비용. ◎ 우수, ○ 보통, △ 약함.

| # | 쟁점 | Opus 안 | Sol 안 | 단순 | 안정 | 에이전트 | 사용자 | Win | 비용 | 추천 |
|---|---|---|---|---|---|---|---|---|---|---|
| B1 | 터미널 화면 | 패스스루(herdr가 VT 처리) + 상태바, 로그 모드는 별도 | 줄 단위 이벤트 로그, M1 escaped text, M2 SGR subset | O◎ / S○ | O○(escape 위험) / S◎ | 동일 | **O◎ / S△**(메뉴·줄편집 깨짐) | O○(콘솔 VT 필요) / S◎ | O◎ / S○ | **Opus 패스스루 + Sol의 escape 차단 목록 = "정제 패스스루"**. Sol 화면은 로그 모드로 |
| B2 | 입력 혼재 | 타이핑 가드(대기), hold, 탈취 가능한 에이전트 락 | 사용자 lease(raw는 명시 yield까지), agent 즉시 거절 | O○ / S○ | O△(미완결 줄 3 s 후 송신 = 버그) / S◎ | **O◎ / S△** | O◎ / S○ | 동일 | 비슷 | **혼합**: 미완결 줄 자동 반환 금지(Sol) + 완결 줄·1.5 s 유휴면 자동 반환, 에이전트는 대기 후 실패(Opus) |
| B3 | 커서 | 이름 있는 암묵 커서(`agent`/`LOOM_CLIENT`), read가 전진 | 명시 `cursor create`, opaque next_cursor, ack, revision | O◎ / S△ | O○ / S◎ | **O◎ / S△** | — | 동일 | O◎ / S○ | **Opus 기본 + Sol의 next_cursor·revision은 고급 옵션** |
| B4 | 경쟁 없는 대기 | `send --expect`(한 호출) | `expect --after-request q1` | O◎ / S○ | 동일 | O◎ / S○ | — | — | 비슷 | **둘 다 제공**, 스킬은 `send --expect`를 가르친다 |
| B5 | send 줄끝 기본 | profile 기본, 없으면 `cr`; hex/file/ctrl은 none | 항상 none | O○ / S◎ | O○ / S◎(놀람 없음) | **O◎ / S△**(빠뜨리면 조용한 실패) | 동일 | — | 동일 | **Opus**. 단 응답에 실제 송신 바이트(escaped)를 항상 포함해 놀람을 없앤다(Sol의 투명성 원칙) |
| B6 | 안전 분류 | R0–R4, denylist 문자열 → R3 매번 확인, R4 차단, 뷰어 팝업 승인 | allowlist profile, 미분류 R2+, R3 기본 거절, 문자열은 경고 보조 | O○ / S○ | O○ / S◎ | **O◎ / S△** | O◎(팝업) / S△(UX 없음) | 동일 | 비슷 | **세션 신뢰 모드**: `observe`(R0만) / **`interactive`(기본: 임의 텍스트 R1, denylist R3 확인, R4 차단)** / `strict`(Sol의 allowlist). 승인은 Sol의 digest 바인딩 + Opus의 뷰어 팝업 |
| B7 | 재연결 기본 | serial 일치 시 자동, expect 계속 | unknown 보드 자동 off, expect 종료 | O◎ / S○ | O○ / S◎ | **O◎ / S△** | O◎ / S△ | 동일 | 동일 | **Opus 기본 + Sol의 세대 변경 시 TX/승인 취소, VID/PID 변경 = 다른 장치** |
| B8 | 데몬 배치 | 도메인별 프로세스 | 단일 사용자 데몬 내 UART 모듈 | O○ / S◎ | O◎ / S○ | 동일 | 동일 | 동일 | O○ / S◎ | **메인 결정 사항**. UART는 둘 다 수용 가능. CAN-TP 타이밍 격리 근거로 도메인별을 계속 권하되, Sol 말대로 CAN vendor worker를 분리할 거면 단일 데몬도 합리적 |
| B9 | regex 안전 | 언급 없음(내 누락) | subprocess worker 50 ms | — | S◎ | — | — | S○(프로세스 비용) | S△ | **`regex` 패키지 인프로세스 + `timeout` + literal 기본**(F6 확인). 워커 프로세스는 불필요 |
| B10 | 비밀값 기록 | bytes.bin에 원본, 텍스트만 마스킹 | secret TX는 digest·길이만, RX 에코는 profile 정책 | O◎ / S○ | **O△ / S◎** | 동일 | 동일 | 동일 | 비슷 | **Sol** |
| B11 | 시각 | mono 1개 + 시작 시점 wall에서 파생 | 청크마다 mono+wall, clock_id | O◎ / S○ | **O△(sleep 시 틀림) / S◎** | — | — | — | 동일 | **Sol**(clock_id는 호스트 부팅 ID로 수정, sleep/wake 이벤트 추가) |
| B12 | IPC 서버 | websockets | aiohttp(WS+HTTP static/health/download) | O◎ / S○ | 동일 | 동일 | 동일 | 동일 | O◎ / S○ | **Sol(aiohttp) 수용**. export 다운로드·health·웹 뷰 정적 파일을 한 서버에서 처리하는 장점이 크다. 메인 공통 결정에 따름 |
| B13 | M1 뷰어 라이브러리 | 터미널 모드는 프레임워크 없음(termios/콘솔 API) | prompt_toolkit 화면 | O◎ / S○ | — | — | B1 따라 | O○ / S◎(Windows 입력 성숙) | O◎ / S○ | **M1은 프레임워크 없이 패스스루. Windows 원시 입력은 prompt_toolkit 입력 계층을 재사용**(Sol의 Windows 근거 수용). 로그 모드 프레임워크(Textual vs prompt_toolkit)는 M3 스파이크로 결정 |
| B14 | 기록 실패 시 | 숨기지 않고 표시만 | 모든 TX 차단 | — | — | — | O◎ / S△ | — | — | **에이전트 TX만 차단**(G8-S7) |
| B15 | 레이트 제한 | 10 req/s, 8 KiB/s | 10 req/s, 1 KiB/s burst 1 KiB | 동일 | 동일 | O◎ / S○(heredoc 4 KB = 4초) | — | — | — | **10 req/s, 4 KiB/s, burst 4 KiB** 절충. 사용자 입력 무제한(양쪽 동일) |

---

## 4. 합의 가능한 부분 (그대로 채택 권장 — 내 초안보다 나은 점 포함)

**내 초안보다 낫다고 인정하는 것**
1. **송신 결과 상태 분리**: `accepted / written / partial / failed`, `delivery: unknown`, TX intent를 먼저 기록하고 결과는 후속 이벤트로, **재연결·크래시 후 자동 재송신 금지**, request_id 중복 감지. 내 초안은 `ok`만 있었다.
2. **장치 출력 = 신뢰할 수 없는 데이터**(prompt injection, 무인용 셸 삽입 금지). 내 초안에서 빠진 중요한 안전 항목이다. SKILL.md 규칙과 런타임 경계 양쪽에 넣는다.
3. **호스트 터미널 위험 escape 차단**(OSC52, OSC8, 제목, BEL 폭주) — 내 패스스루 안의 구멍을 정확히 짚었다(B1에 반영).
4. **미완결 사용자 줄을 시간만으로 자동 양도하지 않는다** — 내 R1 타이핑 가드(3 s 후 송신)의 버그를 지적한 셈이다.
5. **구독 이음매 알고리즘**: 서버가 high-water seq H를 잡고 `(after, H]` 스크롤백 + `(H, ∞)` 라이브 → 누락·중복 0. 나는 "보장한다"고만 썼다.
6. **청크별 mono+wall**, `timestamp_kind = host_read_complete`, **줄 시각은 첫/끝 청크 두 개** — 한 줄이 한 순간에 도착한 것처럼 보이지 않게 함.
7. **비밀 TX는 digest·길이만 기록**, 기록 누락을 "byte-exact"라 부르지 않음.
8. **승인 = action digest 바인딩**(identity·generation·bytes hash·횟수·만료), 재연결·내용 변경·만료 시 무효, 토큰을 stdout/log에 노출 금지. 내 confirm ID보다 범위가 넓고 정확하다.
9. **flow=rtscts일 때 수동 RTS = CONFIG_CONFLICT**, xonxoff+rtscts 동시 금지, binary 세션에서 XON/XOFF 기본 금지, **POSIX 1.5 stop bit는 사전 거절**(소스 확인: `serialposix.py:458-459`에서 2로 조용히 바뀜).
10. **앱→부트로더로 VID/PID가 바뀌면 다른 장치로 취급**(명시 alias + 승인 필요).
11. **원시 기록 디스크 현실성**: 3 Mbaud RX 하루 25.9 GB/포트 → 회전·quota·`durable_through_seq` 표시. 내 "30일/5 GiB"는 고속 포트에서 비현실적이었다.
12. **스파이크 실증**: pty에서 DTR/RTS가 `ENOTTY`(Errno 25) → 시그널 테스트는 pty가 아니라 **인프로세스 가짜 전송 계층**에서 해야 한다(내 SimTransport 안과 합치되는 실측 근거).
13. **Windows CI는 드라이버 없는 in-memory 백엔드 필수, com0com은 전용 러너 선택** — 같은 결론이고 근거(서명/Secure Boot 미확인)가 더 구체적이다.
14. **서버가 role/origin 부여, 클라이언트 자기신고 불신**, Origin/Host allowlist, 토큰을 URL query에 두지 않기(내 안은 URL fragment — 둘 다 query는 피함).
15. **reader 설계 세부**: `read(1)` 첫 바이트 대기 후 `in_waiting` drain(최대 16 KiB), 무제한 `call_soon_threadsafe` 대신 bounded 큐 + 합쳐진 wakeup.
16. **화면 생략(view skip) ≠ RX 손실 ≠ 기록 손실**을 서로 다른 카운터로 표시.
17. **명령 응답·TX 결과는 드롭 대상에서 제외**(데이터 이벤트만 드롭).

**양쪽이 이미 일치 — 확정 후보**
- 데몬 단독 소유, JSON-RPC 2.0 over WebSocket(127.0.0.1 + 토큰 descriptor), 이벤트 = notification, base64 기본·binary는 실측 후.
- pyserial 3.5 + 포트당 blocking reader 스레드 + 단일 TX actor, pyserial-asyncio 계열은 보류.
- pexpect 비채택, expect는 데몬 공통 버퍼 위에서 구현.
- 탈출키 `Ctrl-]`, 두 번이면 0x1D 리터럴, Ctrl-C/D/Z는 장치로 전달.
- 재연결 매칭 1순위 VID/PID+serial, 후보 2개 이상이면 AMBIGUOUS + 수동 bind.
- M1 = 걷는 뼈대, 웹 뷰·viz 추출·XMODEM 후순위.
- 사용자 Ctrl-C 최우선 처리, 사용자 입력은 문자열 필터 대상 아님.
- viz 요구: long-format `{ts, series, value, unit}` + source 참조(Sol의 `quality`·gap marker·NaN 금지를 내 안에 합침).

---

## 5. 핵심 원칙 점검 (r1-sol.md 대상)

| # | 체크리스트 | 판정 | 근거 | 보완안 |
|---|---|---|---|---|
| 1 | 맨 앞 "핵심 루프" 표 | **충족** | §0 표: 4요소 × UART 구현 × M1 증명. 숨은 연결 금지 단락 포함 | — |
| 2 | 종단 간 시나리오(송신→응답→화면 ms→개입) | **부분** | 명령·화면·Ctrl-C 개입은 있다. 그러나 ① M1 시나리오가 M2 기능(expect)을 쓴다(F4) ② 화면이 줄 단위 로그라 "터미널에서 무엇이 보이는지"와 다르다(G1) ③ **개입 이후 에이전트가 무엇을 받는지**(다음 호출 응답, lease 상태)가 없다 — raw 모드 lease 규칙상 Ctrl-C 후 에이전트는 사용자가 yield할 때까지 차단되는데, 시나리오가 이 결과를 보여주지 않는다 ④ ms 수치는 "제안 예시"라고 정직하게 표기했지만 **지연 예산 분해**가 없다 | 시나리오 끝에 "에이전트 다음 호출 → 응답 JSON(warnings/lease)"을 추가한다. 예산 분해표(아래 3번)를 붙인다 |
| 3 | 실시간 목표 + 고부하 정직성 | **충족(근거 보강 필요)** | p95 < 100 ms·p99 < 200 ms(host-read-complete → stdout write), 30 Hz 렌더, viewport 최신 행, `render lag`·`view skipped`·`RX gap` 분리, 느린 구독자 격리, 기록 실패 표시, escaped-summary 모드. 고부하 동작은 내 초안보다 세밀하다 | **예산 근거**: `read(1)`는 데이터가 오면 즉시 반환하므로 reader 대기는 지연에 거의 더해지지 않는다. 최악 경로 ≈ IPC 배치 16 ms + 렌더 프레임 33 ms(30 Hz) + 루프·직렬화 수 ms ≈ **55 ms**, 여기에 측정 범위 밖 USB latency timer(FTDI 기본 16 ms)가 더해져 사용자 체감 ≈ 70 ms. 100 ms 목표는 타당하지만 여유가 크지 않으므로 921600에서 JSON 직렬화가 예산을 넘는지 M1에서 반드시 측정해야 한다. 측정은 F2대로 같은 호스트 monotonic을 그대로 비교하면 된다. "escaped-summary mode"가 사용자 화면에서 어떻게 보이는지 목업이 필요하다 |
| 4 | M1 = 걷는 뼈대 | **부분** | 범위가 핵심 4요소에 집중돼 있고, "화면을 미루는 M1은 불합격" 원칙이 좋다. 수용 기준(late attach 누락·중복 0, RX↔기록 hash, 921600 p95/p99, agent-only 경로 0)도 강하다. **빠진 것**: ① expect(F4) — 경계 있는 대기 없이 `read --tail` 폴링만으로는 "부팅 대기→로그인" 같은 기본 루프가 어색하다 ② **최소 입력 중재가 M2(O2)** — M1 데모가 "사용자 입력 + 에이전트 send"를 동시에 보여주는데 중재가 없으면 바로 줄 섞임이 생긴다 ③ **최소 안전 게이트 부재** — 핵심 원칙의 "쓰기(안전 규칙 안에서)"에 비해 M1에 쓰기 가드가 없다. sim만 쓰므로 위험은 낮지만 나중에 붙이면 CLI 응답 형식이 바뀐다 | M1에 추가: expect(literal+regex, timeout), 최소 중재(원자 send + 미완결 줄 보호 + hold), 최소 SafetyGate(R3/R4 denylist → 확인 필요 응답, dry-run). 반대로 M1의 **"921600 p99 < 200 ms"는 M1 필수에서 "측정·보고"로 낮추고 p95만 gate**로 하는 것이 현실적이다(prompt_toolkit/패스스루 선택 전 p99 보장은 과함) |
| 5 | 사용자 화면 ≠ 분리, 숨은 연결 금지 | **부분** | §0 우회 금지, §3 단독 handle·registry 중복 검사, §4 snapshot→live 이음매, §5 `attach COM3`/`attach dut` 동일 session 표시 — 좋다. **구멍**: ① 뷰어가 **session_id에 결합**되고 close/reopen이 새 session을 만든다 → 에이전트가 재open하면 사용자 화면이 끊긴다(G8-S3) ② POSIX 잠금이 실제 장벽이 아니다(F5 — Sol 본인도 인정) → 에이전트가 `screen /dev/cu.x`로 숨은 연결을 만들면 macOS에서 성공할 수 있다 ③ 뷰어 0개일 때 에이전트에게 알리는 장치가 없다(G10) | 뷰어·커서를 채널 별칭에 결합. 스킬 규칙 "직접 open 금지" 명시 + 데몬 응답 `no_viewer_attached` 경고. M3 HW gate에 "실 USB 시리얼에서 2차 open 차단 여부(flock/TIOCEXCL)" 시험 추가 |
| 6 | 비기여 기능 후순위/비범위 명시 | **충족** | §1 웹·숫자 추출·XMODEM/YMODEM·프리셋 UI·고급 hex 편집 후순위, 플래셔·RS485·RFC2217·probe 비범위, §11 M4 | (참고) "능동 probe 비범위"는 baud 모름 문제를 사용자에게 넘긴다. 후순위로 두는 편이 낫다 |

---

## 6. 쟁점 목록 (리드 결정용)

| # | 쟁점 | opus 안 | sol 안 | 내 추천 | 근거 |
|---|---|---|---|---|---|
| I1 | 사용자 화면 렌더 방식 | 패스스루 터미널 모드(+로그 모드) | 줄 단위 이벤트 로그, ANSI escaped → M2 subset | **정제 패스스루**(CSI/SGR 통과, OSC/DCS/APC/PM/RIS 차단·표시) + 로그 모드는 M3 | 브리프 "터미널 수준", readline/U-Boot 메뉴 충실도, Sol의 보안 우려 수용 (G1, B1) |
| I2 | 입력권 모델 | 타이핑 가드(시간), hold, 락 탈취 | 사용자 lease, raw는 명시 yield, agent 즉시 거절 | **미완결 줄 = 반환 안 함, 완결 줄 + 1.5 s 유휴 = 자동 반환**, agent send는 timeout까지 대기 후 `E_USER_ACTIVE`, hold는 명시적 | 영구 lease는 에이전트를 무기한 막는다(G8-S1). 시간만으로 반환하면 줄이 섞인다(Sol 지적) |
| I3 | 커서·에이전트 정체성 | 암묵 이름 커서(`LOOM_CLIENT`/`agent`) | 명시 cursor create + opaque next_cursor + ack | **암묵 기본 + opaque/revision은 고급** | 짧게 사는 CLI에서 서버 부여 client_id는 유지되지 않는다(G3) |
| I4 | 재연결 중 expect | 계속 대기(기본) | CONNECTION_CHANGED 종료(기본) | **계속 대기 기본, `--fail-on-disconnect` 옵션, TX/lease/승인은 취소** | 네이티브 USB 보드의 리셋 = 재열거 (G4) |
| I5 | 자동 재open 기본 | serial 일치 시 ON | unknown 보드 OFF | **serial 정확 일치 ON, location만 일치하면 사전 허용 시에만, reset-sensitive profile opt-out** | 브리프의 재연결 요구, USB 전원 보드는 이미 리셋된 상태 |
| I6 | 안전 분류 기본 | denylist → R3 확인, R4 차단 | allowlist profile, 미분류 R2+, R3 기본 거절 | **세션 신뢰 모드 3단(observe/interactive=기본/strict)**, 승인은 digest 바인딩 + 뷰어 팝업 + 채팅 둘 다 | 탐색 디버깅을 막지 않으면서 Sol의 정확한 승인 범위를 유지 (G9, B6) |
| I7 | R3 플래시·erase 명령 범위 | 매번 확인 후 허용 | 기본 거절, 자동화 범위 밖 | **매번 확인 후 허용(R3), efuse/OTP 등은 R4 차단** | 브리프 "위험 명령 처리", 사용자가 의도한 재플래시는 정당 |
| I8 | send 기본 줄끝 | profile 기본(없으면 CR) | none | **profile 기본(CR)**, hex/file/ctrl은 none, 응답에 실제 송신 바이트 표시 | 에이전트 조용한 실패 방지 (G5, B5) |
| I9 | 고수준 동사 | `run`, `send --expect`, `grep`, `mark`, 상대 `--since` | `--after-request`, RFC3339 `--since` | **Opus 동사 채택 + `--after-request`도 유지** | 에이전트 레시피 단순화·컨텍스트 보호 |
| I10 | 데몬 자동 기동 | 첫 CLI 호출 시 자동 | 수동 `loom daemon start` | **자동 기동 + runtime lock(Sol)** | 에이전트 첫 단계 실패 제거 (G2) |
| I11 | M1 범위 | expect·중재·안전 최소형 포함 | open/send/read + attach, expect·lease·안전은 M2 | **M1에 expect·최소 중재·최소 안전 포함, 921600 p99는 측정·보고(게이트 아님)** | 걷는 뼈대가 실제 에이전트 루프(대기→명령)를 돌려야 함, M1 데모의 줄 섞임 방지 |
| I12 | 뷰어·커서 결합 대상 | 세션 별칭(세션이 끊김·재연결을 견딤) | session_id (close/reopen = 새 세션) | **채널 별칭에 결합, 세션 교체 시 배너** | 사용자 화면 분리 방지 (G8-S3, 핵심 원칙 5) |
| I13 | 외부 도구 공존 | `release/reclaim` | 없음(플래셔 비범위만) | **`release/reclaim` 채택(R2)** | 세션·뷰어 연속성 (G6) |
| I14 | regex 안전 | (누락) | subprocess worker 50 ms | **`regex` 패키지 인프로세스 + timeout + literal 기본** | F6 실측, 비용·지연 |
| I15 | 비밀값 기록 | 원본 유지·텍스트 마스킹 | digest·길이만 | **Sol** | 자격증명 유출 방지 |
| I16 | 시각 기록 | 시작 wall에서 파생 | 청크별 mono+wall | **Sol + clock_id = 호스트 부팅 ID + sleep/wake 이벤트** | F2 |
| I17 | IPC 서버 라이브러리 | websockets | aiohttp | **aiohttp**(메인 결정 우선) | HTTP health·export 다운로드·정적 파일을 한 서버에서 (B12) |
| I18 | M1 뷰어 구현 기반 | 프레임워크 없음(termios/콘솔) | prompt_toolkit | **패스스루는 프레임워크 없이, Windows 원시 입력은 prompt_toolkit 입력 계층 재사용, 로그 모드 프레임워크는 M3 스파이크로 결정** | B13 |
| I19 | 데몬 배치 | 도메인별 프로세스 | 단일 데몬 내 모듈 | **메인 결정**(UART는 양쪽 수용). 도메인별 권장은 유지 | CAN-TP 타이밍 격리 vs 운영 단순성 (B8) |
| I20 | 종료 코드 | 7 = 데몬 연결 불가, 6 = 확인 필요/정책 | 7 = gap/한도, 데몬 불가 없음 | **0 ok, 1 일반, 2 사용법, 3 timeout, 4 연결, 5 사용자/락, 6 정책·확인, 7 데몬 불가, 8 gap/한도** — 도메인 공통으로 메인에 제안 | 양쪽 합집합 |
| I21 | 기록 실패 시 TX | (표시만) | 전 TX 차단 | **에이전트 TX만 차단, 사용자 TX 허용 + 경고** | G8-S7 |
| I22 | 레이트 제한 | 10 req/s, 8 KiB/s | 10 req/s, 1 KiB/s | **10 req/s, 4 KiB/s, burst 4 KiB** | B15 |
| I23 | break 길이 | `--ms` | `--duration` | **< 250 ms는 `break_condition` on/sleep/off, ≥ 250 ms는 동일 방식(정확도 일관), 결과에 실측 지속시간 반환** | F1 |
| I24 | 숨은 연결 방지 수단 | TIOCEXCL(틀림) | flock 보조 | **설계(공개 open 경로 없음) + 스킬 규칙 + `no_viewer_attached` 경고 + M3 HW gate로 실 드라이버 배타성 측정** | F5 실험: macOS pty에서 TIOCEXCL 무효 |
