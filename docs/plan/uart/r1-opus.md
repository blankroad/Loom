# UART 계획 초안 R1 — uart-opus

- 작성: uart-opus (Opus 5.5), 2026-11-09
- **docs/plan/02-core-principle.md 준수 (최우선)** — §0 핵심 루프, 끝의 "핵심 원칙 자체 점검" 참조.
- 무게: 아키텍처 · 에이전트/사용자 UX · 스킬 설계 · 안전 규칙. 라이브러리·플랫폼·성능의 깊은 검증은 uart-sol 초안(r1-sol.md)과 합쳐서 R2에서 확정.
- 근거 표기: **[확인: 출처 날짜]** 는 직접 확인한 사실, **[추정]** 은 확인 전 가정, **[제안]** 은 결정이 필요한 설계안.
- 이번 라운드에 spikes/uart/ 코드는 쓰지 않았다 (pty/pyserial 스파이크는 uart-sol 담당). 대신 로컬 환경에서 herdr CLI(`pane split`, `pane run`, prefix 키)와 PyPI 최신 버전을 직접 확인했다.

### 핵심 결정 요약 (리드 검토용)

| # | 쟁점 | 추천안 | 대안 |
|---|---|---|---|
| D1 | 데몬 배치 | 도메인별 프로세스(`loomd uart`), 공통 프레임워크 `loom.core.daemon` 공유 | 단일 `loomd`에 모든 도메인 |
| D2 | IPC | JSON-RPC 2.0 over WebSocket, 127.0.0.1 임의 포트 + 토큰, 이벤트는 JSON-RPC notification | JSON Lines over unix socket/named pipe + 별도 웹 게이트웨이 / HTTP+SSE |
| D3 | 입력 혼재 | "하이브리드": send 단위 원자성 + 사용자 타이핑 중 에이전트 send 지연 + 사용자 hold(일시 차단) + 에이전트 시퀀스 락(사용자가 언제든 탈취) | 자유 혼합 / 배타 락 |
| D4 | 사용자 뷰어 | `loom uart attach` 한 명령에 2개 모드: **터미널 모드**(원시 패스스루, herdr 패인이 VT 에뮬레이션, 하단 1줄 상태바) + **로그 모드**(Textual, 타임스탬프/hex/검색/출처 필터). `Ctrl-]` 접두키 | Textual 단일 화면에 pyte로 VT 에뮬레이션 |
| D5 | 에이전트 읽기 | 이름 있는 커서(기본 `agent`), `read`는 커서 이후 + 커서 전진, `--tail/--since`는 커서 불변, 출력 상한(기본 200줄/16 KiB, 머리+꼬리 보존) | 커서 없이 tail만 |
| D6 | 고수준 동사 | `send --expect`(송신과 대기를 원자적으로), `run`(명령→프롬프트까지 출력만 추출) | send/expect만 제공 |
| D7 | 위험 등급 | R0 읽기 / R1 일반 입력 / R2 교란(리셋·break·baud 변경) / R3 위험 문자열(플래시·erase·saveenv) / R4 비가역(efuse/OTP) — R2 정책 기반 확인, R3 항상 확인, R4 기본 차단 | 에이전트 송신 전부 확인 |
| D8 | 파일 전송 | XMODEM/YMODEM은 **후순위(M4)**, `xmodem` 패키지 사용, 항상 R3 | 범위 제외 |
| D9 | 웹 뷰 | **후순위(M4)**. 데몬이 xterm.js 정적 페이지를 같은 WS 포트로 서빙 | 처음부터 웹 우선 |
| D10 | 첫 마일스톤 | **M1 = 걷는 뼈대**: pty 가짜 디바이스 위에서 직접 접근·읽기·쓰기·사용자 실시간 표시+개입 최소형 전부 | 기능 계층별(에이전트 먼저, 뷰어 나중) |

---

## 0. 핵심 루프 (docs/plan/02-core-principle.md 준수 — 최우선)

> 에이전트가 COM 포트에 **직접** 붙어 **읽고**, **쓰고**, 사용자는 **같은 바이트를 같은 순간에** 같은 화면에서 보며 언제든 끼어든다. 이 문서의 모든 다른 기능은 이 루프의 부수 기능이다.

| 요소 | UART에서의 구현 | 에이전트/사용자가 쓰는 것 | M1(걷는 뼈대) 최소형 | 상세 |
|---|---|---|---|---|
| **1. 직접 접근** | `loomd-uart` 데몬이 실제 포트(`/dev/cu.*`, `/dev/ttyUSB*`, `COMn`)를 pyserial로 **유일하게** 연다. 에이전트는 사람 손 없이 `loom uart ports` → `loom uart open` 으로 스스로 장치를 고르고 연다. 데몬은 첫 CLI 호출 때 자동 기동 | `loom uart ports`, `open --match vid=…`, `status` | pty 가짜 디바이스(`loom uart sim start bootshell --pty`)의 경로를 **실제 포트와 같은 pyserial 경로로** 연다. Windows CI는 `sim://` | §3.1, §3.4, §3.8, §4.2 |
| **2. 읽기** | 세션 링버퍼(바이트 오프셋 좌표) 위의 이름 있는 커서. 모든 읽기는 경계 있음(커서 이후/tail/since, 타임아웃, 출력 상한) | `read`, `expect`, `send --expect`, `run`, `grep` | `read`(커서·tail·상한·ANSI 제거), `expect`(다중 패턴·타임아웃), `send --expect` | §3.3, §3.6, §4.4–4.6 |
| **3. 쓰기** | writer 태스크가 send 단위로 원자 송신. SafetyGate(위험 등급) + InputArbiter(사용자 우선) 통과 후에만 | `send`(텍스트/hex/ctrl, 줄끝), `signal`(break/DTR/RTS) | `send`(텍스트/hex/`--ctrl`, `--eol`), R3/R4 패턴 차단 + `--confirm`, 레이트 제한 | §3.5, §3.7, §4.3, §10 |
| **4. 사용자 실시간 표시** | `loom uart attach dut`가 **같은 데몬의 같은 스트림**을 구독(rx·에이전트 tx·사용자 tx가 한 좌표계). 열자마자 직전 스크롤백 + 라이브. 출처 강조(agent 청록/user 기본/device 기본), 상태바에 에이전트 동작·지연(LAG) 표시. 사용자 키는 원시 그대로 장치로(Ctrl-C 포함) | herdr 패인의 `loom uart attach dut` | 터미널 모드 패스스루 + 1줄 상태바 + 에이전트 에코 틴트 + 백로그 + 원시 키 입력 + 타이핑 가드. 목표 **rx→화면 p95 < 100 ms** | §2.0, §3.11, §3.12, §5 |

**하나의 스트림 원칙**
1. 포트를 여는 코드 경로는 **데몬 하나뿐**이다. 에이전트용 "직접 열기" 옵션·라이브러리 API·숨은 세션은 없다(§3.12).
2. 에이전트 송신, 사용자 송신, 장치 수신은 **같은 `off` 좌표의 단일 스트림**에 순서대로 들어가고, 뷰어는 그 스트림을 그대로 그린다 — 에이전트가 본 것과 사용자가 본 것이 다를 수 없다.
3. 표시는 **라이브 경로**(구독)로만 한다. 기록 파일은 사후 분석·손실 복구용 보조다.
4. 사용자는 보는 화면에서 바로 개입한다(키 입력, Ctrl-C, hold). 에이전트는 개입을 응답의 `deferred_ms`/`warnings`/exit 5로 알게 된다.

---

## 1. 범위 / 비범위

### 범위 (In)
1. **포트 관리**: 포트 목록(경로, VID:PID, 시리얼 번호, 제조사, 설명, USB location), 열기/닫기, 시리얼 파라미터(baud, data bits 5–8, parity N/E/O/M/S, stop 1/1.5/2, flow none/rtscts/dsrdtr/xonxoff), 런타임 재설정, 여러 포트 동시 세션.
2. **데몬 소유 모델**: UART 데몬 1개가 모든 포트를 연다. 에이전트 CLI·사용자 뷰어·(선택) 웹 뷰어는 클라이언트. 데몬은 첫 CLI 호출 시 자동 기동.
3. **재연결**: USB-시리얼 분리/재연결 자동 감지, 동일 장치 식별(시리얼 번호 → USB location → 포트 이름 순), 재연결 시 기록에 간극 표시.
4. **에이전트 동작**: `send`(텍스트/hex/파일/환경변수 비밀값, 줄끝 CR/LF/CRLF/none), `read`(커서/tail/since), `expect`(정규식, 다중 패턴, 타임아웃), `run`(명령 실행 후 프롬프트까지 출력 추출), `grep`, `mark`, 시그널(break, DTR/RTS, 명명된 리셋 시퀀스), 락, 출력 크기 상한, ANSI/바이너리 처리, `--json`.
5. **사용자 뷰어**: herdr 패인에서 `loom uart attach dut`. 원시 키 전달, 타임스탬프 토글, hex 보기, 에이전트 송신 강조, 스크롤백 검색, 로그 저장, 에이전트 활동 표시, 확인 팝업.
6. **기록**: 모든 세션 자동 기록(원시 바이트 + 청크 타임스탬프, 줄 단위 텍스트 로그, 제어 이벤트), 공통 이벤트 레코드로 export.
7. **시뮬레이션**: 인프로세스 `sim://` 전송, POSIX pty 페어, 가짜 디바이스 시나리오(부팅 로그/U-Boot/로그인/쉘/센서 출력/분리 주입), HW 없는 3-OS CI.
8. **안전**: 위험 등급, 확인 흐름(채팅 또는 뷰어 팝업), 레이트 제한, dry-run, 위험 문자열 패턴, 감사 기록.
9. **스킬**: `.claude/skills/loom-uart/` SKILL.md + references, Codex에서도 같은 파일 사용.
10. **viz 연동 요구사항**: UART 텍스트에서 숫자 추출 → `loom.viz` 플롯 (요구 초안을 §6.7에 작성).

### 비범위 (Out) — 이유 포함
| 항목 | 이유 / 대체 |
|---|---|
| ZMODEM, Kermit | 현장 수요 낮음, 구현·테스트 비용 큼. 필요 시 `release`로 포트를 넘겨 외부 도구(lrzsz) 사용 |
| 칩 벤더 플래셔 재구현(esptool, stm32flash, dfu) | 외부 도구 영역. 대신 **`release/reclaim`**(포트를 잠시 놓아주기)으로 공존 |
| 원격 포트 서버(RFC2217/ser2net) 운영 | pyserial `rfc2217://`·`socket://` URL을 열 수는 있게 하되(전송 계층 덤), 서버 운영·인증은 비범위 |
| LIN, RS-485 반이중 방향 제어(RTS 토글형) | RS-485 RTS 자동 토글은 M3 이후 검토 |
| 터미널 에뮬레이터 자체 구현 | 터미널 모드는 herdr/호스트 터미널이 VT를 처리. 로그 모드는 텍스트 렌더(ANSI 색만 해석) |
| 멀티 사용자/원격 접속 | 로컬 전용(공통 계약 §3-3) |
| 1 Mbaud 초과 장시간 바이너리 스트림 분석 | 기록은 하되 실시간 렌더 품질은 보장 안 함(§13 리스크) |
| hex 편집기, 매크로 버튼 패널, 스크립트 언어(TeraTerm TTL류) | 핵심 루프에 기여 없음. 에이전트가 CLI로 대신함 |

### 후순위 (M4 — 핵심 루프에 직접 기여하지 않음, 02-core-principle 기준)
| 기능 | 후순위 이유 |
|---|---|
| 웹 뷰(xterm.js) | herdr 패인 터미널 모드가 이미 실시간 표시·개입을 충족. IPC를 WS로 정해 두어 나중에 추가 비용 낮음 |
| XMODEM/YMODEM | 파일 전송은 루프의 부가 기능. 급하면 `release` + 외부 도구 |
| `probe`(baud 자동 탐색) | 편의 기능. 수동 `configure --baud`로 대체 |
| `extract` + viz 라이브 플롯 | 사용자 표시의 확장이지만 원시 스트림 표시가 먼저. 요구사항(§6.7)은 지금 작성 |
| `replay://`, `audit`, msgpack 협상, OS 장치 이벤트 재연결 | 품질/성능 개선. 폴링·JSON으로 M1–M3 충분 |
| 로그 모드 고급 기능(마크 목록, 활동 패널, 다중 포맷 저장) | M3. M1은 터미널 모드 최소형으로 4요소 충족 |

---

## 2. 사용 시나리오

### 2.0 종단 간 시나리오 E1 — 에이전트 송신 → 장치 응답 → 사용자 화면 → 사용자 개입

환경: herdr 왼쪽 패인 = 에이전트(Claude/Codex), 오른쪽 패인 = 사용자 뷰어. 장치 = `dut`(115200 8N1, 임베디드 리눅스 쉘). M1에서는 같은 흐름을 pty 가짜 디바이스 `bootshell`로 재현한다(§11 M1 수용 기준).

**① 에이전트가 쓰고 있는 도중 사용자가 뷰어를 연다 (숨은 세션 없음 확인)**
```
[agent pane]
$ loom uart send dut 'ping -c 1000 192.168.0.1' --expect 'bytes from' --timeout 5
sent 26 bytes (+CR) at off 520114
matched /bytes from/ after 0.31s

[user pane]  ← 이 시점에 처음 연다
$ loom uart attach dut
```
attach는 연결 즉시 링버퍼의 직전 64 KiB를 그리고(같은 스트림의 과거 부분) 이어서 라이브로 전환한다. 화면 ⓐ:
```
┌─ herdr: pane 2 ─────────────────────────────────────────────────────────────┐
│root@dut:~# ping -c 1000 192.168.0.1          ← 청록: 에이전트 송신의 에코     │
│PING 192.168.0.1 (192.168.0.1): 56 data bytes                                │
│64 bytes from 192.168.0.1: icmp_seq=0 ttl=64 time=0.912 ms                   │
│64 bytes from 192.168.0.1: icmp_seq=1 ttl=64 time=0.874 ms                   │
│64 bytes from 192.168.0.1: icmp_seq=2 ttl=64 time=0.901 ms   ← 1초마다 라이브 │
│▌dut ● OPEN 115200 │ agent ▶ "ping -c 1000…⏎" 3s ago │ lag 12ms │ ^] menu    │
└─────────────────────────────────────────────────────────────────────────────┘
```

**② 장치 응답이 사용자 화면에 뜨기까지 (115200 baud, 1줄 ≈ 60바이트)**
| 구간 | 예상 | 근거 |
|---|---|---|
| 장치가 줄을 송신 (60 B × 86.8 µs/B) | ~5.2 ms | 115200 8N1 = 11520 B/s [계산] |
| USB-시리얼 칩 → 호스트 | 1–16 ms | FTDI latency timer 기본 16 ms, CDC-ACM은 ~1 ms [추정] |
| reader 스레드 read 반환 (timeout 10 ms) | ≤ 10 ms | §3.5 |
| 루프 홉 + 링버퍼/기록 큐 + 팬아웃 병합 (≤ 10 ms) | ≤ 11 ms | §3.11 |
| WS 로컬 전송 + 뷰어 디코드 | < 2 ms [추정] | |
| 터미널 모드: stdout write → herdr 렌더 | 1 프레임 (~16 ms) [추정] | |
| **합계(마지막 바이트 도착 기준)** | **전형 20–40 ms, 목표 p95 < 100 ms** | M1 수용 기준에서 측정 |

**③ 사용자가 Ctrl-C로 개입**
사용자가 오른쪽 패인에서 Ctrl-C → 뷰어가 0x03을 `uart.input`(role=user)으로 보냄 → writer가 즉시 송신(사용자 입력은 대기열 앞) → 장치가 ping 통계와 프롬프트 출력 → 양쪽 모두 같은 바이트를 본다. 화면 ⓑ:
```
│64 bytes from 192.168.0.1: icmp_seq=7 ttl=64 time=0.889 ms                   │
│^C                                            ← 사용자 입력(기본색)            │
│--- 192.168.0.1 ping statistics ---                                          │
│8 packets transmitted, 8 packets received, 0% packet loss                    │
│root@dut:~# █                                                                │
│▌dut ● OPEN 115200 │ you ▶ ^C 0.4s ago │ agent idle │ lag 9ms │ ^] menu        │
```
에이전트의 다음 호출:
```
$ loom uart read dut --json
{"ok":true,"cmd":"read","port":"dut","data":{"text":"64 bytes from …icmp_seq=7…\n^C\n--- 192.168.0.1 ping statistics ---\n8 packets transmitted, 8 packets received, 0% packet loss\nroot@dut:~# "},
 "warnings":["user_input_since_last_call: 1 send by user (\\x03) at off 521877"],"range":{"from":521310,"to":521990},"truncated":false}
```
→ 에이전트는 "사용자가 ping을 멈췄다"를 알고 계획을 조정한다.

**④ 사용자가 타이핑 중일 때 에이전트 송신**
사용자가 `ifconfig eth` 까지 치고 멈춤(Enter 전). 그 순간 에이전트가 `loom uart run dut 'ip -br addr'` 호출 → 타이핑 가드로 대기. 화면 ⓒ:
```
│root@dut:~# ifconfig eth█                                                    │
│▌dut ● OPEN │ ⏳ agent waiting for your line: "ip -br addr" (1.4s) │ ^] k cancel │
```
사용자가 `0⏎` 입력 → 사용자의 `ifconfig eth0` 실행 결과가 먼저 출력 → 프롬프트 후 에이전트 명령이 송신되고 그 에코가 청록으로 보인다. 에이전트 응답 JSON에 `"deferred_ms":2310`. 사용자가 에이전트를 아예 멈추려면 `Ctrl-] h`(HOLD) → 에이전트의 다음 send는 exit 5 `E_HELD_BY_USER`, 상태바 빨간 `HOLD`.

### 에이전트 관점
**A1. 부팅 대기 → 로그인 → 명령 → 결과 파싱 (임베디드 리눅스 보드)**
```
loom uart open --match serial=A10KXYZ --as dut --baud 115200 --profile linux-shell
loom uart signal dut reset --seq rts-pulse          # R2: 정책상 허용돼 있으면 즉시, 아니면 확인 요청
loom uart expect dut 'login:' --timeout 90
loom uart send dut root --expect 'Password:' --timeout 5
loom uart send dut --secret-env DUT_PASS --expect '# $' --timeout 10
loom uart run dut 'cat /proc/meminfo | head -3' --json
```
→ 마지막 결과는 `{"output":"MemTotal: ...\nMemFree: ...\n...","prompt_matched":"root@dut:~# ","rc":null}`. 에이전트는 MemFree를 파싱해 보고. 사용자는 같은 과정을 attach 화면에서 실시간으로 보고, 에이전트가 친 명령 에코는 청록색으로 보인다.

**A2. 리셋 후 부트로그 전체 캡처 + 오류 검색 (ESP32)**
```
loom uart mark dut boot-1
loom uart signal dut reset --seq esp32-run
loom uart expect dut 'app_main|Guru Meditation' --timeout 20
loom uart grep dut 'E \(|panic|Guru' --since mark:boot-1
loom uart log export dut --since mark:boot-1 --format txt -o /tmp/boot-1.txt
```
→ 에이전트 컨텍스트에는 grep 결과 몇 줄만, 전체 로그는 파일로.

**A3. U-Boot 자동부팅 중단 후 환경변수 조회**
```
loom uart signal dut reset --seq rts-pulse
loom uart expect dut 'Hit any key to stop autoboot' --timeout 30
loom uart send dut --hex 20 --eol none --expect '=> ' --timeout 3
loom uart run dut 'printenv bootargs' --prompt '=> $'
```
→ 만약 에이전트가 `saveenv`를 보내려 하면 R3 패턴에 걸려 `E_CONFIRM_REQUIRED`(exit 6), 사용자 확인 후에만 송신.

**A4. 장시간 테스트 감시 (분리/재연결 포함)**
```
loom uart expect dut 'TEST DONE|FAIL' --timeout 1800 --json
```
→ 도중에 USB를 뽑았다 꽂아도 expect는 계속 대기(기본), 결과 JSON에 `"disconnects":[{"at":"...","gap_s":3.2}]`가 포함된다.

**A5. baud 모를 때 탐색 (후순위 M4)**
```
loom uart probe dut --bauds 9600,57600,115200,921600 --listen 2s
```
→ 각 baud에서 수신 바이트의 출력 가능 문자 비율을 보여주고 최적값 추천(자동 적용은 `--apply`).

### 사용자 관점
**U1. 에이전트 작업 지켜보기**: herdr 오른쪽 패인에서 `loom uart attach dut`. 평소 minicom처럼 보이고, 하단 상태바에 `agent: expect /login:/ 37s left` 같은 에이전트 현재 동작이 뜬다. 에이전트가 보낸 명령의 에코는 청록색.

**U2. 직접 개입**: 에이전트가 기다리는 동안 사용자가 직접 `dmesg | tail` 입력. 사용자가 타이핑하는 동안 에이전트의 다음 send는 사용자가 Enter를 칠 때까지 자동 지연(최대 에이전트 timeout까지). 사용자가 아예 멈추게 하려면 `Ctrl-] h`(hold) → 에이전트 쓰기 전부 거부, 상태바에 `HOLD (user)` 표시.

**U3. 부트 로그 사후 분석**: `Ctrl-] v`로 로그 모드 전환 → `/panic` 검색, `F2` 타임스탬프(절대/상대/델타), `F3` hex, `F4` 출처 필터(에이전트 송신만 보기), `F5` 선택 구간 저장.

**U4. 확인 요청 승인**: 에이전트가 `sf erase 0 0x100000`(R3)을 보내려 하면 attach 화면에 팝업이 뜬다. 사용자가 보낼 바이트를 그대로 확인하고 `y`/`n`. 채팅으로 답하는 것보다 빠르다.

**U5. 외부 플래셔와 공존**: `loom uart release dut` (또는 `Ctrl-] R`) → 데몬이 포트를 닫고 뷰어는 `RELEASED — external tool may use port` 배너 표시 → `esptool.py write_flash ...` → `loom uart reclaim dut`. 세션·기록·커서는 유지.

---

## 3. 아키텍처

### 3.1 프로세스 구성도
```
 herdr pane (user)            herdr pane (agent: Claude/Codex)          browser (후순위 M4)
 ┌──────────────────┐         ┌───────────────────────────────┐        ┌───────────────┐
 │ loom uart attach │         │ loom uart send/read/expect ...│        │ xterm.js page │
 │  (viewer client) │         │  (짧게 실행되는 CLI 클라이언트)  │        │               │
 └────────┬─────────┘         └──────────────┬────────────────┘        └──────┬────────┘
          │  WebSocket (JSON-RPC 2.0, 127.0.0.1:<port>, token)               │
          └───────────────────────┬──────────────────────────────────────────┘
                     ┌────────────▼────────────────────────────────────────┐
                     │ loomd-uart  (asyncio 메인 루프)                      │
                     │  ├─ RPC server / subscriber fan-out (bounded queues) │
                     │  ├─ PortSession "dut"  ┌─ Transport (pyserial|sim) │
                     │  │   ├─ reader thread ─┘   rx 청크 + ts            │
                     │  │   ├─ writer task (InputArbiter: lock/hold/typing)│
                     │  │   ├─ RingBuffer (seq/offset, 기본 16 MiB)        │
                     │  │   ├─ Cursors {agent, viewer..}                   │
                     │  │   ├─ ExpectEngine (대기 중 expect들)              │
                     │  │   ├─ SafetyGate (등급 판정, 확인 대기)            │
                     │  │   └─ Recorder (bytes.bin, chunks.jsonl, text.log)│
                     │  ├─ PortSession "gps" ...                           │
                     │  └─ DeviceWatcher (list_ports 폴링, 재연결 매칭)      │
                     └──────────────────────────────────────────────────────┘
                               │ 직접 OS 핸들 (유일한 소유자)
                       /dev/cu.usbserial-A10KXYZ, COM3, sim://bootshell ...
```

### 3.2 데몬 배치 (D1) — 대안 비교
| | A. 단일 `loomd`(uart+can+uds 모듈) | B. 도메인별 프로세스 `loomd uart` / `loomd can` (공통 프레임워크) |
|---|---|---|
| 장애 격리 | USB 드라이버 hang/예외가 CAN-TP 타이밍까지 영향 | 격리됨 |
| 타이밍 | 3 Mbaud UART 팬아웃이 같은 이벤트 루프·GIL 공유 → CAN-TP FC 지연 위험 | 독립 |
| 운영 단순성 | 프로세스 1개, 엔드포인트 1개 | 프로세스 N개, 디스커버리 파일 N개 |
| 웹 뷰어 | 한 포트로 모두 | 각자 포트, 또는 얇은 게이트웨이 |
| 교차 기능(UART 이벤트로 CAN 기록 트리거 등) | 쉬움 | IPC 경유 |

**추천: B** [제안]. 이유: CAN-TP가 ms 단위 응답이 필요한데(공통 계약 §3-8), UART 고속 팬아웃·USB 재열거 처리와 같은 루프를 쓰는 건 불필요한 위험. 공통 코드(`loom.core.daemon`: RPC 서버, 인증, 디스커버리, 기록 기반, 안전 게이트 골격)는 공유하므로 메인이 A로 정하더라도 UART 모듈은 그대로 얹힌다. 디스커버리 파일 형식(§3.10)만 맞추면 된다.

### 3.3 세션 · 포트 · 커서 모델
- **Device(장치 식별자)**: 물리 장치를 가리키는 매칭 규칙. `{vid, pid, serial, location, path_hint}`. 경로는 바뀔 수 있으므로 식별의 주키가 아니다.
- **PortSession(세션)**: 데몬 안에서 열린 하나의 포트. 사용자/에이전트가 붙이는 **별칭(alias)** 으로 지칭(`dut`, `gps`). 별칭이 없으면 자동(`ttyUSB0`, `COM3`, `usbserial-A10KXYZ`). 세션은 포트가 끊겨도 살아 있다(버퍼·커서·기록 유지).
  - 필드: `alias, device, config(baud..), state, profile, ring, cursors, lock, hold, typing, recorder, stats(rx/tx bytes, errors)`.
- **스트림 좌표**: 세션당 rx/tx 바이트를 **도착 순서대로 하나의 논리 스트림**에 붙이고, 각 청크에 단조 증가 `off`(바이트 오프셋)와 `seq`(청크 번호)를 매긴다. rx와 tx가 같은 좌표계를 쓰므로 "에이전트가 보낸 직후부터"를 정확히 지칭할 수 있다.
  - 청크 = `{seq, off, len, t_mono_ns, t_wall, dir: rx|tx, origin: device|agent|user|script, client}`.
  - 텍스트 뷰(read/expect)는 기본적으로 **rx만** 대상으로 한다(에코는 장치가 rx로 돌려준다). `--include-tx`로 tx 포함.
- **커서**: `(session, cursor_name) → off`. 이름 규칙:
  - 에이전트 기본 `agent`. 여러 에이전트가 같은 포트를 쓰면 `LOOM_CLIENT=codex-1` 또는 `--cursor NAME`으로 분리.
  - 뷰어는 커서를 쓰지 않는다(라이브 구독).
  - 커서는 데몬 메모리 + `cursors.json`에 저장(데몬 재시작 시 복원, 단 버퍼는 기록 파일에서 재구성).
- **마크**: `mark:<label>` = 이름 붙은 오프셋. `--since mark:boot-1`처럼 사용. 사용자도 `Ctrl-] m`으로 찍는다. 마크는 기록·viz에 수직선 이벤트로 남는다.
- **위치 지정자(공통 문법)**: `cursor`(기본), `now`, `start`, `off:12345`, `mark:LABEL`, 기간 `30s|5m|2h`(현재 시각 기준), 시각 `@23:05:01`, `send:last`(이 클라이언트의 마지막 송신 시작점).

### 3.4 포트 상태 머신
```
              open()                 성공
   CLOSED ───────────▶ OPENING ───────────────▶ OPEN ◀──────────────┐
     ▲                    │ 실패(busy/권한)        │  │                │ reclaim()
     │                    ▼                       │  │ release()      │
     │                 ERROR ──(retry/close)──┐   │  └──────▶ RELEASED┘
     │                                         │   │ I/O 오류/장치 사라짐
     │   close()                               │   ▼
     ├───────────── CLOSING ◀───────────────── DISCONNECTED ──(장치 재등장 + 매칭 성공)──▶ OPENING
     │                                          │  (reconnect=auto, 500ms 폴링)
     │                                          ├─ 매칭 후보 2개 이상 ─▶ AMBIGUOUS (사용자 선택 필요)
     └──────────────────────────────────────────┘  (reconnect=off 또는 close())
```
- 상태 전이는 모두 `state` 이벤트로 방송되고 `events.jsonl`에 기록된다.
- DISCONNECTED 중 `send`는 기본 `E_DISCONNECTED`(exit 4). `--wait-connected 30s`를 주면 재연결까지 대기 후 송신.
- DISCONNECTED 중 `expect`/`read`는 계속 동작(버퍼는 남아 있음). expect 타이머도 계속 흐름. `--fail-on-disconnect`면 즉시 실패.
- **ERROR 원인 코드**: `E_PORT_BUSY`(Windows 타 프로그램 점유 등), `E_PERMISSION`(Linux dialout 그룹), `E_NOT_FOUND`, `E_CONFIG_UNSUPPORTED`(예: 드라이버가 해당 baud 미지원).
- RELEASED: 데몬이 OS 핸들을 닫은 상태. 뷰어/커서 유지. reclaim 시 같은 Device 매칭으로 다시 연다.

### 3.5 데이터 흐름
**수신(rx)**: reader 스레드가 `serial.read(max(1, in_waiting))` (timeout 10–20ms) → 청크에 `time.monotonic_ns()` + wall clock 부여 → `loop.call_soon_threadsafe`로 메인 루프 큐에 투입 → (1) RingBuffer 추가 (2) Recorder 비동기 append (3) ExpectEngine 증분 검사 (4) 구독자별 bounded 큐에 팬아웃.
- reader를 스레드로 두는 이유: pyserial의 asyncio 계열은 Windows에서 이벤트 루프 통합이 약하다 [추정 — sol 확인 대상]. 스레드 + 블로킹 read는 3-OS 동일 동작.
- 고속 시 청크 병합: 구독자 전송은 최대 10ms 또는 16 KiB 단위로 묶는다(타임스탬프는 원 청크 단위로 기록에 남음).

**송신(tx)**: CLI → RPC `uart.send` → SafetyGate(등급 판정·확인) → InputArbiter(락/hold/타이핑 가드) → writer 태스크 큐 → `serial.write` + `flush` → tx 청크를 rx와 같은 스트림에 기록·방송(origin 포함) → 응답에 `{off_start, off_end, deferred_ms}` 반환.
- 사용자 키 입력은 뷰어 → RPC `uart.input`(작은 바이트열) → Arbiter → writer (우선 처리, 아래 3.7).

### 3.6 버퍼 · 커서 · 출력 상한
- RingBuffer: 세션당 메모리 16 MiB(설정 가능). 115200 baud 연속 출력 기준 약 27분 분량, 921600 기준 약 3.4분 [계산]. 이보다 오래된 범위는 기록 파일(bytes.bin + chunks.jsonl 인덱스)에서 읽는다 → 에이전트 입장에선 **간극이 생기지 않는다**(디스크 읽기라 느릴 뿐). 기록이 꺼져 있거나 파일이 순환 삭제된 경우에만 `gap` 경고.
- `read` 기본 동작: 커서 이후 rx 텍스트 → 출력 상한 적용 → **커서를 끝까지 전진**(생략분은 응답에 오프셋 범위로 알려줌). 이유: 에이전트가 생략분을 다시 받는 루프에 빠지지 않게. 필요하면 `--since off:A --until off:B`로 정밀 조회.
- 출력 상한(기본, 에이전트 컨텍스트 보호): `--max-lines 200`, `--max-bytes 16384`, 한 줄 최대 512자(초과분 `…[+N chars]`). 초과 시 **머리 20줄 + 꼬리 나머지** 보존, 가운데에
  `… [omitted 4,812 lines / 391 KB: off 10240..402116 — use 'loom uart grep' or 'loom uart log export'] …` 삽입.
- 줄 분리: `\r\n`, `\n`, 단독 `\r`(진행률 표시 덮어쓰기) 모두 줄 경계로 취급하되, 단독 `\r` 연속 덮어쓰기는 마지막 상태만 남김(`--keep-cr`로 원형 유지).
- ANSI: 에이전트 출력 기본 `--ansi strip`(CSI/OSC 제거). `keep`(원형), `escape`(`\x1b[31m` 문자 그대로). 
- 비출력 문자: `\xNN`로 표기, `\t` 유지. 최근 창에서 비출력 비율 > 30%면 경고 `"likely_binary_or_wrong_baud": true` + 힌트(`--hex` 또는 `probe`).
- 인코딩: 기본 UTF-8 `errors=backslashreplace`, 세션별 `--encoding latin-1` 등.

### 3.7 입력 혼재 규칙 (D3)
대안 비교:
| | A. 자유 혼합 | B. 배타 락(쓰기 토큰) | **C. 하이브리드 [추천]** |
|---|---|---|---|
| 사용자 개입 | 즉시 | 락 해제 기다려야 함 → 사용자 불만 | 즉시, 항상 가능 |
| 장치 라인버퍼 오염(사용자 반쯤 친 줄 + 에이전트 명령) | 자주 발생 | 없음 | 타이핑 가드로 방지 |
| 에이전트 다단계 시퀀스 보호 | 없음 | 있음 | 에이전트 락(사용자 탈취 가능) |
| 구현 | 쉬움 | 보통 | 보통 |

**C 규칙 상세**
1. **원자성**: 하나의 `send` 페이로드는 다른 입력과 섞이지 않고 연속으로 써진다(writer 큐 직렬화). 파일/대용량 send는 "전송 세션"으로 묶여 그동안 사용자 키는 보류되고 뷰어에 `TRANSFER 42% — Ctrl-] k abort` 표시.
2. **사용자 우선 + 타이핑 가드**: 사용자가 Enter/Ctrl-C 없이 키를 친 뒤 **3초 이내**이고 그 줄이 끝나지 않았으면 "타이핑 중". 이때 에이전트 send는 큐에서 대기(`deferred`)하고, 사용자가 줄을 끝내거나 3초 무입력이 되면 송신. 에이전트의 `--timeout`(기본 10s)을 넘기면 `E_USER_ACTIVE`(exit 5). 응답에는 `deferred_ms`가 들어가 에이전트가 "사용자가 개입했다"는 걸 안다.
   - 예외: 에이전트의 `--ctrl c`(인터럽트, 0x03)는 가드를 무시하지 않는다 — 여전히 대기. 사용자 입력을 존중하는 게 원칙.
3. **사용자 hold**: 뷰어 `Ctrl-] h` 또는 `loom uart hold dut`. hold 중 모든 에이전트 쓰기(R1 이상)는 즉시 `E_HELD_BY_USER`(exit 5). 읽기·expect는 계속 가능. 해제할 때까지 유지(TTL 없음). 상태바 빨간색 `HOLD`.
4. **에이전트 락**: `loom uart lock dut --ttl 120s --reason "u-boot env 수정"`. 락 동안 다른 에이전트 클라이언트의 send는 `E_LOCKED`. 사용자가 키를 치면 바로 보내지 않고 뷰어가 한 줄 프롬프트를 띄운다:
   `Agent holds lock: "u-boot env 수정" (87s left). Type anyway and break lock? [y/N]` → `y`면 락 해제, 해당 키 송신, 에이전트의 다음 호출은 `E_LOCK_BROKEN`(exit 5). `N`이면 키 폐기. **사용자는 언제나 이긴다.**
5. **출처 표시**: 모든 tx 청크에 `origin`과 `client`가 붙어 기록·방송된다. 뷰어 표시는 §5.4.
6. **Ctrl-C 등 제어키**: 사용자의 Ctrl-C는 0x03으로 장치에 그대로 간다(뷰어 종료 아님). 에이전트는 `send --ctrl c`로 같은 바이트를 보낸다(R1).

사용자가 무엇을 보게 되는지 — 상황별 표:
| 상황 | 장치 측 결과 | 사용자 화면 | 에이전트가 받는 것 |
|---|---|---|---|
| 에이전트 send, 사용자 가만히 | 정상 | 에코가 청록색, 상태바 `agent ▶ "ls⏎"` | `ok`, `deferred_ms:0` |
| 사용자 타이핑 중 에이전트 send | 사용자 줄 끝난 뒤 실행 | 상태바 `agent waiting for you to finish line (2.1s)` | `ok`, `deferred_ms:2100` |
| 사용자가 10초 넘게 반쯤 친 상태 | 에이전트 명령 안 감 | 상태바 `agent send timed out (you were typing)` | exit 5 `E_USER_ACTIVE` |
| hold 중 | 아무것도 안 감 | 상태바 `HOLD` + `agent send rejected` 토스트 | exit 5 `E_HELD_BY_USER` |
| 에이전트 락 중 사용자 키 | 확인 후 송신 | 탈취 프롬프트 | 다음 호출 exit 5 `E_LOCK_BROKEN` |
| R3 송신 시도 | 아무것도 안 감(승인 전) | 확인 팝업 | 승인 대기 또는 exit 6 |

### 3.8 재연결과 동일 장치 식별
매칭 우선순위(세션 open 시 저장한 Device 기준):
1. `vid:pid + serial` 일치 (FTDI·CP210x·CDC-ACM 대부분 시리얼 있음) → 확정.
2. 시리얼 없음/중복(CH340 계열은 시리얼 번호가 없는 경우가 흔함 [추정]) → `vid:pid + location`(USB 허브 포트 경로) 일치 → 확정.
3. 위 둘 다 불가 → `vid:pid`가 같은 후보가 **정확히 1개**이고 이전 경로 이름과 같으면 확정. 후보가 여러 개면 **AMBIGUOUS** → 자동 연결 안 함, 뷰어/CLI에 선택 요청(`loom uart open dut --pick 2`).
4. `sim://`, `rfc2217://` 등 URL은 URL 문자열 자체가 식별자.
- 감지: DISCONNECTED 진입 후 500ms 간격 `list_ports.comports()` 폴링(백오프 최대 2s). OS 이벤트(udev/IOKit/WM_DEVICECHANGE)는 후순위(M4) 최적화.
- 재연결 성공 시: 같은 config로 재오픈, DTR/RTS 초기 상태는 프로필 값으로 다시 설정(§10.6), 스트림에 `--- reconnected after 3.2s (path changed: COM5 → COM7) ---` 마커 이벤트, 뷰어 배너.
- 재연결 직후 장치가 리셋돼 부트로그를 뿜는 경우가 많다 — 기록은 재오픈 즉시 시작하므로 놓치지 않음 (단 USB 열거~open 사이 수십~수백 ms 출력은 OS 버퍼에 따라 유실 가능 [추정]).

### 3.9 IPC 프로토콜 제안 (D2)
| | **A. JSON-RPC 2.0 over WebSocket (TCP loopback + 토큰) [추천]** | B. JSON Lines over unix socket / named pipe + 웹은 별도 게이트웨이 | C. HTTP REST + SSE |
|---|---|---|---|
| 웹 뷰어 직접 접속 | 가능(브라우저 기본) | 게이트웨이 필요 | 가능 |
| 양방향 스트림(사용자 키 입력) | 자연스러움 | 자연스러움 | 입력은 별도 POST, 지연↑ |
| 3-OS 동일 구현 | 동일 | named pipe 별도 구현 | 동일 |
| 보안 | 토큰 필요(TCP) | 파일 권한으로 충분 | 토큰 필요 |
| 라이브러리 | websockets 17.4 [확인: PyPI 2026-10-30] | stdlib | FastAPI/uvicorn |

추천 근거: 브리프가 "웹 뷰어 직접 접속 유리"를 명시했고, 사용자 입력이 키 단위 양방향 스트림이라 WebSocket이 가장 단순하다. 로컬 보안은 128-bit 토큰을 0600 권한 디스커버리 파일에 두는 것으로 충분하다고 본다(같은 사용자 계정 내 신뢰 모델). websockets는 unix socket 서빙도 지원하므로 POSIX에서 추가 엔드포인트를 여는 건 선택사항 [추정 — API 확인 필요].

**연결 수립**
```json
→ {"jsonrpc":"2.0","id":1,"method":"hello","params":{"proto":"loom/1","token":"…","role":"agent","client":"claude-7f3a","pid":4121,"tool":"loom-cli 0.1.0"}}
← {"jsonrpc":"2.0","id":1,"result":{"daemon":"loomd-uart","version":"0.1.0","proto":"loom/1","server_time":{"mono_ns":81234567890123,"wall":"2026-11-09T23:05:01.234567+09:00"}}}
```
`role` ∈ `agent | user | script | web`. origin은 role에서 파생되며 클라이언트가 임의로 바꿀 수 없다(뷰어만 `user` 역할을 받도록 attach가 별도 플래그로 연결).

**요청/응답 메서드 (uart 네임스페이스)**
| 메서드 | 주요 params | 결과 |
|---|---|---|
| `uart.ports` | `all?` | 포트 목록 + 사용 중 세션 |
| `uart.open` | `target`(path/url/match), `alias`, `config`, `profile`, `reconnect` | 세션 정보 |
| `uart.close` / `uart.release` / `uart.reclaim` | `port` | 상태 |
| `uart.status` | `port?` | 세션 상세(상태, config, 락/hold, 커서, 통계, 대기 중 expect) |
| `uart.configure` | `port`, 부분 config | 새 config (R2) |
| `uart.send` | `port`, `data_b64`, `eol`, `secret?`, `expect?`, `timeout`, `pace?`, `confirm_id?`, `dry_run?` | `{off_start, off_end, deferred_ms, risk, expect_result?}` |
| `uart.input` | `port`, `data_b64` (뷰어 키 입력 전용, role=user) | ack |
| `uart.read` | `port`, `from`, `until?`, `tail_lines?`, `cursor?`, `advance`, `limits`, `render`(text/hex/raw) | `{text|hex, from, to, omitted?, gap?}` |
| `uart.expect` | `port`, `patterns[]`, `from`, `timeout`, `cursor` | `{matched_index, match, groups, before, from, to}` 또는 timeout 에러 |
| `uart.run` | `port`, `command`, `prompt`, `timeout`, `strip_echo` | `{output, prompt_matched}` |
| `uart.grep` | `port`, `pattern`, `from`, `until`, `max_matches`, `context` | 매치 목록(오프셋, 시각, 줄) |
| `uart.signal` | `port`, `kind`(break/dtr/rts/seq), `value`/`ms`/`seq` | 실행 결과 (R2) |
| `uart.mark` | `port`, `label` | 오프셋 |
| `uart.lock` / `uart.unlock` / `uart.hold` / `uart.unhold` | `port`, `ttl`, `reason` | 상태 |
| `uart.confirm.respond` | `confirm_id`, `approve` | 결과 (role=user만) |
| `uart.subscribe` / `uart.unsubscribe` | `port`, `from`(live/off:N/backlog_bytes:N), `kinds[]` | `sub_id` |
| `uart.log.path` / `uart.log.export` | `port`, 범위, `format` | 파일 경로 |

**이벤트(서버 → 클라이언트 notification)**
```json
{"jsonrpc":"2.0","method":"uart.event","params":{"sub":"s1","port":"dut","kind":"data","seq":1042,"off":81920,"dir":"rx","origin":"device","t_mono_ns":81234567890123,"t_wall":"2026-11-09T23:05:01.234567+09:00","b64":"cm9vdEBkdXQ6fiMg"}}
{"jsonrpc":"2.0","method":"uart.event","params":{"sub":"s1","port":"dut","kind":"data","seq":1043,"off":81936,"dir":"tx","origin":"agent","client":"claude-7f3a","b64":"bHMNCg=="}}
```
`kind` 목록: `data`, `state`, `config`, `signal`, `mark`, `lock`, `hold`, `typing`(사용자 타이핑 가드 on/off), `agent_activity`(expect 대기 시작/종료, 남은 시간 — 뷰어 상태바용), `confirm_request`, `confirm_result`, `gap`(느린 구독자 드롭), `safety`(차단 기록), `transfer`(진행률).

**백프레셔**: 구독자별 큐 상한 4 MiB. 넘으면 해당 구독자에게만 `gap{lost_off_from, lost_off_to}` 보내고 드롭. **reader 스레드와 기록은 절대 구독자 때문에 막히지 않는다.** 뷰어는 gap을 받으면 `[viewer lagged: 1.2 MB skipped — see log]` 표시.

**바이너리 인코딩**: JSON + base64(오버헤드 ~33%). 3 Mbaud ≈ 300 KB/s → 400 KB/s JSON, 로컬 WS로 충분하다고 판단 [추정 — sol 성능 측정으로 확인]. 부족하면 `hello`에서 `"encoding":"msgpack"` 협상(후순위 M4).

**장시간 요청**: `expect`/`run`은 요청 하나가 timeout까지 열려 있다. 클라이언트 연결이 끊기면(에이전트 CLI가 kill됨) 데몬은 해당 expect를 취소한다. 진행 상황은 `agent_activity` 이벤트로 뷰어에 방송.

### 3.10 디스커버리 · 자동 기동 · 디렉터리
- 런타임 디렉터리: `platformdirs.user_runtime_dir("loom")` (macOS `~/Library/Caches/TemporaryItems/...` 계열이 될 수 있어 실제 경로 확인 필요 [추정]) — 대안으로 `~/.loom/run/` 고정. **[제안] `~/.loom/` 고정**이 사람이 찾기 쉽다: `~/.loom/run/uart.json`, `~/.loom/records/uart/…`, `~/.loom/config/uart.toml`.
- `uart.json` (0600): `{"pid":4120,"url":"ws://127.0.0.1:51733/rpc","token":"…","proto":"loom/1","version":"0.1.0","started":"…"}`.
- CLI 동작: 파일 없음/pid 죽음 → `loomd uart`를 detach 기동(POSIX: double-fork/`start_new_session`, Windows: `DETACHED_PROCESS`) → 최대 3s 대기 → 연결. `LOOM_NO_AUTOSTART=1`이면 기동 안 하고 exit 7.
- `loom uart daemon start|stop|status|logs`. stop은 열린 세션이 있으면 `--force` 필요.
- 데몬 수명: 세션 0개 + 클라이언트 0개 상태가 10분 지속되면 자동 종료(설정 가능).

### 3.11 실시간성 목표와 고부하 동작 (화면이 멈추거나 거짓말하지 않기)

**목표치 [제안]**
| 지표 | 목표 | 측정 방법 |
|---|---|---|
| 장치 rx 마지막 바이트 → 사용자 화면(터미널 모드) | **p95 < 100 ms, p50 < 40 ms** (≤ 921600 baud) | sim 장치가 줄마다 송신 시각(monotonic) 삽입 → 테스트 뷰어 클라이언트가 수신 시각과 비교(같은 호스트 monotonic) |
| 같은 경로, 로그 모드(Textual) | p95 < 150 ms (렌더 스로틀 포함) | 스냅샷 + 타이밍 훅 |
| 사용자 키 → 장치 송신 | p95 < 20 ms | 가짜 장치에서 수신 시각 측정 |
| 에이전트 send RPC → 장치 송신 시작 | p95 < 20 ms (가드/확인 대기 제외) | 동일 |
| attach 후 첫 화면(백로그) | < 300 ms | 64 KiB 백로그 기준 |
| 원시 기록 손실 | **0 바이트** (호스트가 읽은 모든 바이트) | sim 송신 바이트열 == bytes.bin rx 부분 해시 비교 |

**경로별 원칙**
1. **reader 스레드는 아무것도 기다리지 않는다**: read → 타임스탬프 → 메인 루프에 넘기기만. 기록·구독자·expect 어느 것도 reader를 막지 못한다.
2. **기록이 표시보다 먼저**: 청크는 링버퍼와 기록 큐에 먼저 들어가고, 그 다음 구독자에게 팬아웃. 기록 큐(기본 64 MiB)마저 넘치면(디스크 정지 등) 숨기지 않고 `record_degraded` 이벤트 + 상태바 `REC ⚠` 표시.
3. **팬아웃 병합**: 구독자별로 최대 10 ms 또는 16 KiB 단위로 묶어 보냄(921600 baud ≈ 92 KB/s → 초당 약 100 메시지).
4. **구독자 큐 상한 → 드롭 + 명시**: 뷰어가 못 따라오면(큐 4 MiB 초과) 오래된 미전송분을 버리고 `gap{from_off,to_off,bytes}` 이벤트. 뷰어는 그 자리에 반전색 한 줄 `⟪viewer skipped 1.2 MB (off 8.1M–9.3M) — fully recorded, Ctrl-] w to save⟫` 를 삽입. **빠진 걸 이어 붙여 정상처럼 보이게 하지 않는다.**
5. **지연 표시(LAG)**: 뷰어는 화면에 마지막으로 그린 청크의 `t_mono_ns`와 현재 시각 차이를 상태바에 `lag 12ms`로 상시 표시, 250 ms 초과 시 노랑, 1 s 초과 시 빨강 `LAG 1.4s`. 사용자는 "지금 보는 게 실시간인지"를 항상 안다.
6. **렌더 스로틀**: 터미널 모드는 받은 바이트를 그대로 stdout에 쓰되 쓰기를 최대 60 Hz로 묶는다(호스트 터미널이 ~MB/s 처리 [추정]). 로그 모드는 행 추가를 30 Hz 배치로, 화면 밖 행은 가상화(보이는 행만 렌더). 스로틀 중임을 상태바 rx 속도(`92 KB/s`)로 표시.
7. **일시정지 정직성**: `Ctrl-] p`(화면 정지) 중에는 상태바 `PAUSED +3,214 lines buffered` — 정지 화면을 라이브로 오인하지 않게. 해제 시 밀린 분량을 이어서 그리거나(≤ 4 MiB) gap 표시로 건너뜀.
8. **기기 쪽 손실은 모른다고 말한다**: OS/드라이버 수신 버퍼 오버런은 pyserial이 일반적으로 보고하지 못한다 [추정 — sol 확인]. 흐름제어 없는 고속 연속 출력에서는 상태바에 `no flow control @ 921600 — device-side loss undetectable` 경고를 1회 표시.
9. **에이전트 쪽 보호**: 폭주 중 `read`는 출력 상한(머리+꼬리)과 `omitted` 범위를 돌려주고, `expect`는 증분 스캔이라 폭주에도 놓치지 않는다(링버퍼 밖으로 밀린 구간은 기록 파일에서 스캔).

**폭주 테스트 (M1 수용 기준에 포함)**: sim `flood` 시나리오가 921600 baud 상당 속도(92 KB/s)로 60초 연속 출력 + 줄마다 시퀀스 번호. 확인: (a) 뷰어 lag p95 < 100 ms 또는 gap 표시가 정확한 오프셋으로 나옴, (b) 화면이 멈추지 않음(상태바 시계 1 Hz 갱신 지속), (c) bytes.bin이 송신 바이트와 일치, (d) 동시에 에이전트 `expect 'seq=5000\b'`가 매치.

### 3.12 사용자 화면과 에이전트 세션의 비분리 (숨은 연결 금지)
- **직접 포트 열기 경로 없음**: `loom` CLI 어디에도 `--direct`/`--no-daemon` 옵션을 두지 않는다. `loom.uart` 파이썬 패키지는 `Transport`를 공개 API로 내보내지 않으며(내부 모듈), 에이전트가 쓰는 공개 표면은 CLI와 데몬 RPC뿐이다.
- **데몬 배타 점유**: 데몬은 POSIX에서 `exclusive=True`(TIOCEXCL)로 열어 에이전트가 pyserial/screen으로 같은 포트를 따로 여는 시도 자체가 실패한다. Windows는 OS가 원래 배타.
- **스킬 규칙**: SKILL.md 첫 줄에 "포트를 pyserial/screen/minicom으로 직접 열지 말 것, 항상 `loom uart`" 명시(§4.8).
- **`release` 예외의 가시성**: 외부 도구에 포트를 넘기는 `release`는 R2 + 뷰어에 `RELEASED` 배너 + 기록 이벤트. 이 동안 데이터가 안 보인다는 사실 자체를 사용자 화면에 표시한다.
- **attach 즉시 같은 스트림**: 뷰어는 `uart.subscribe(from="backlog_bytes:65536")`로 붙어 직전 스크롤백 → 라이브로 끊김 없이 이어진다(백로그 마지막 off와 라이브 첫 off가 연속임을 데몬이 보장). 로그 모드는 링버퍼 전체(16 MiB)까지 스크롤백.
- **기록 끄기와 무관**: `--no-record`는 디스크 기록만 끈다. 라이브 표시 경로는 끌 수 없다.
- **뷰어 없음 알림**: 에이전트가 포트를 열었는데 뷰어가 0개면 `open`/`send` 응답 `warnings`에 `no_viewer_attached: run 'loom uart attach dut'`를 넣어, 스킬이 사용자에게 뷰어를 띄우도록 유도(또는 herdr로 직접 띄움, R-8).

---

## 4. CLI / API 명세 초안

### 4.1 공통 규약
- 형식: `loom uart <verb> [PORT] [args]`. **PORT**는 세션 별칭/경로. 세션이 하나만 열려 있으면 생략 가능, 아니면 `LOOM_UART_PORT` 환경변수 → 없으면 `E_AMBIGUOUS_PORT`.
- 모든 명령 비대화형. 무한 스트림 없음 — 대기형 명령은 `--timeout` 필수 기본값 보유(`expect` 30s, `send --expect` 10s, `run` 30s, 최대 3600s).
- `--json`: 아래 공통 봉투. 사람용 출력은 stdout, 진단은 stderr.
```json
{"ok":true,"cmd":"read","port":"dut","data":{...},"range":{"from":81920,"to":84211},"truncated":false,"warnings":[]}
{"ok":false,"cmd":"expect","port":"dut","error":{"code":"E_TIMEOUT","message":"no match for /login:/ within 90s","hint":"last 5 lines are in data.tail; check baud or power"},"data":{"tail":"...","scanned_bytes":10233}}
```
- 종료 코드(공통 계약 후보, §15):
| code | 의미 |
|---|---|
| 0 | 성공 |
| 1 | 일반 오류 |
| 2 | 사용법 오류 |
| 3 | 타임아웃(expect/run/send --expect) |
| 4 | 포트 없음/닫힘/끊김 (`E_NOT_OPEN`, `E_DISCONNECTED`, `E_PORT_BUSY`) |
| 5 | 사용자/락 충돌 (`E_USER_ACTIVE`, `E_HELD_BY_USER`, `E_LOCKED`, `E_LOCK_BROKEN`) |
| 6 | 안전 게이트 (`E_CONFIRM_REQUIRED`, `E_DENIED`, `E_FORBIDDEN`, `E_RATE_LIMIT`) |
| 7 | 데몬 연결 불가 |

### 4.2 포트/세션
```
loom uart ports [--all] [--json]
```
```
PORT                        VID:PID    SERIAL      DESCRIPTION                     SESSION
/dev/cu.usbserial-A10KXYZ   0403:6001  A10KXYZ     FT232R USB UART                 dut (OPEN 115200 8N1)
/dev/cu.usbmodem14201       2e8a:000a  E6614C3...  Board CDC (Raspberry Pi Pico)   -
/dev/cu.SLAB_USBtoUART      10c4:ea60  0001        CP2102 USB to UART              gps (DISCONNECTED 3s)
(2 non-USB ports hidden; --all to show)
```
`--json` 항목: `{"path","vid","pid","serial","manufacturer","product","description","location","hwid","session":null|{"alias","state"}}`.

```
loom uart open TARGET [--as ALIAS] [--baud 115200] [--data 8] [--parity N|E|O|M|S] [--stop 1|1.5|2]
               [--flow none|rtscts|dsrdtr|xonxoff] [--profile NAME] [--reconnect auto|off]
               [--dtr on|off|keep] [--rts on|off|keep] [--encoding utf-8] [--no-record]
TARGET := 경로(/dev/cu.x, COM3) | URL(sim://bootshell, rfc2217://host:port) | --match k=v[,k=v]
          (--match vid=0403,pid=6001,serial=A10K | --match desc~CP210)
```
```
$ loom uart open --match vid=0403 --as dut --baud 115200 --profile linux-shell
opened dut  /dev/cu.usbserial-A10KXYZ  115200 8N1 flow=none  reconnect=auto  rec=~/.loom/records/uart/2026-11-09/dut-230501
viewer: loom uart attach dut
```
- 이미 열린 같은 장치를 다시 open → 기존 세션 반환(멱등), config가 다르면 `E_CONFIG_CONFLICT`(변경은 `configure`로).
- `--match` 후보가 여럿이면 목록 출력 후 exit 2.

```
loom uart close PORT | loom uart release PORT | loom uart reclaim PORT
loom uart configure PORT --baud 921600        # R2
loom uart status [PORT] [--json]
```
```
$ loom uart status dut
dut  OPEN  /dev/cu.usbserial-A10KXYZ  115200 8N1 flow=none  profile=linux-shell
  up 00:42:10  rx 1.82 MB  tx 2.1 KB  errors 0  reconnects 1 (last 23:31:02, gap 3.2s)
  input: hold=off  lock=none  user typing=no   viewers=1 (user)
  cursors: agent @ off 1,903,221 (12 lines behind)
  pending: expect /TEST DONE|FAIL/ by claude-7f3a, 1714s left
  record: ~/.loom/records/uart/2026-11-09/dut-230501 (1.9 MB)
```

### 4.3 송신
```
loom uart send PORT [TEXT] [--hex "1b 5b 41" | --file PATH | --secret-env VAR | --ctrl c|d|z|]|...]
               [--eol cr|lf|crlf|none] [--escapes] [--pace-ms N] [--chunk N]
               [--expect REGEX ... [--timeout 10]] [--dry-run] [--confirm ID] [--json]
```
- 줄끝 기본: 프로필 값, 프로필 없으면 **`cr`**(터미널에서 Enter가 보내는 바이트와 동일 — Linux tty는 ICRNL로 처리, U-Boot도 CR 수용) [제안 — §14 질문].
- `--hex`/`--ctrl`/`--file`은 기본 `--eol none`.
- `--escapes`: `\r \n \t \xNN \\` 해석. 기본은 꺼짐(에이전트가 쉘 이스케이프와 헷갈리지 않게).
- `--secret-env VAR`: 값이 에이전트 컨텍스트·명령줄·기록 텍스트에 남지 않는다. 기록 bytes.bin에는 원본이 들어가지만 text.log·뷰어·export에서는 `«secret:8»`으로 마스킹하고 해당 청크에 `secret:true` 플래그 [제안]. 장치 에코도 같은 바이트열이면 마스킹(비밀번호 프롬프트는 보통 에코 없음).
- `--pace-ms`: 글자 사이 지연(흐름제어 없는 느린 MCU 쉘용). `--file`은 기본 `--chunk 256 --pace-ms 0`, 전송 세션(§3.7-1)으로 처리.
- `--expect`: **송신 직전 오프셋부터** 매칭 → send 후 expect 사이 경쟁 조건 제거.
```
$ loom uart send dut 'reboot' --expect 'login:' --timeout 120
sent 7 bytes (+CR) at off 1903221
matched /login:/ after 41.2s  (off 1955012)
--- before (last 15 of 312 lines) ---
[   3.112] systemd[1]: Started Journal Service.
...
dut login:
```
```
$ loom uart send dut 'rm -rf /data' --dry-run --json
{"ok":true,"cmd":"send","data":{"dry_run":true,"bytes_hex":"72 6d 20 2d 72 66 20 2f 64 61 74 61 0d","escaped":"rm -rf /data\\r","risk":"R3","rule":"linux.rm_recursive","would":"require confirmation"}}
```

### 4.4 읽기
```
loom uart read PORT [--new (기본) | --tail N | --since SPEC] [--until SPEC] [--cursor NAME] [--peek]
               [--max-lines 200] [--max-bytes 16384] [--hex] [--ansi strip|keep|escape]
               [--ts none|rel|abs] [--include-tx] [--json]
```
- `--new`: 커서 이후, 커서 전진. `--peek`이면 전진 안 함.
- `--tail N` / `--since`: 커서 불변(구경만). `--since` 문법은 §3.3 위치 지정자.
- `--ts rel`: 각 줄 앞에 `+12.345s` (첫 줄 기준). 에이전트가 타이밍 판단할 때.
```
$ loom uart read dut --ts abs
23:05:01.234 U-Boot 2024.01 (Oct 02 2026)
23:05:01.240 DRAM:  512 MiB
… [omitted 380 lines, off 1210..40122 — loom uart read dut --since off:1210 --until off:40122] …
23:05:09.871 dut login:
(cursor agent → off 41022, 0 bytes pending)
```
```
$ loom uart read dut --tail 4 --hex
off 40990  6c 6f 67 69 6e 3a 20 0d 0a 1b 5b 30 6d 64 75 74  |login: ...[0mdut|
```

### 4.5 대기
```
loom uart expect PORT REGEX [REGEX ...] [--timeout 30] [--from cursor|now|SPEC] [--cursor NAME]
                 [--context 15] [--fail-on-disconnect] [--ignore-case] [--json]
```
- 여러 패턴 → 먼저 매치된 것의 인덱스 반환(`matched_index`). 실패 패턴을 같이 거는 관용구: `expect dut 'login:' 'Kernel panic' 'Guru Meditation'` 후 인덱스로 분기.
- 매칭 대상: ANSI 제거·줄끝 정규화된 텍스트(기본), `--raw`면 원 바이트에 bytes regex.
- 매치 성공 시 커서 = 매치 끝. 타임아웃 시 커서 불변 + 마지막 `--context`줄 반환.
- 정규식 다중행: Python `re.MULTILINE` 기본. `$`는 "현재 버퍼 끝"이 아니라 줄 끝 → 프롬프트(`# $`)는 개행이 오지 않으므로 **버퍼 끝도 줄 끝으로 간주**하도록 엔진이 처리 [제안].
```json
{"ok":true,"cmd":"expect","port":"dut","data":{"matched_index":0,"pattern":"login:","match":"login:","groups":[],"elapsed_s":41.2,"before":"...(15 lines)...","disconnects":[]},"range":{"from":1903221,"to":1955012}}
```

### 4.6 명령 실행
```
loom uart run PORT COMMAND [--prompt REGEX] [--timeout 30] [--no-strip-echo] [--rc] [--max-lines 200] [--json]
```
- = `send COMMAND --expect PROMPT` + 결과 정제: 첫 줄의 명령 에코 제거, 마지막 프롬프트 제거, ANSI 제거.
- `--prompt` 기본: 프로필의 prompt 정규식(`linux-shell`: `[#$] $`, `uboot`: `=> $`, `esp-idf-console`: `> $`).
- `--rc`(POSIX 쉘 전용): `COMMAND; echo __LOOM_RC=$?` 를 보내고 rc를 파싱해 반환. 사용자 화면에도 그 꼬리가 보이므로 기본 off.
```
$ loom uart run dut 'uname -a'
Linux dut 6.6.21 #1 SMP PREEMPT Tue Oct  1 10:00:00 UTC 2026 aarch64 GNU/Linux
```

### 4.7 검색 · 마크 · 시그널 · 락 · 기록
```
loom uart grep PORT REGEX [--since 10m|mark:X] [--max-matches 50] [-C 2] [--json]
loom uart mark PORT LABEL
loom uart signal PORT break [--ms 250]
loom uart signal PORT dtr|rts on|off|pulse [--ms 100]
loom uart signal PORT reset [--seq NAME]          # 프로필에 정의된 명명 시퀀스
loom uart lock PORT [--ttl 120s] [--reason TEXT] | loom uart unlock PORT
loom uart hold PORT | loom uart unhold PORT       # 사용자용(뷰어 단축키와 동일)
loom uart approve CONFIRM_ID | loom uart deny CONFIRM_ID   # 사용자가 다른 터미널에서 승인할 때
loom uart log path PORT
loom uart log export PORT [--since SPEC] [--until SPEC] --format raw|txt|jsonl|csv -o FILE
loom uart extract PORT --re 'temp=(?P<temp>[-\d.]+)' [--since 10m] -o temp.csv   # viz 연동(§6.7) — 후순위 M4
loom uart probe PORT --bauds 9600,115200,921600 [--listen 2s] [--apply]            # 후순위 M4
loom uart attach PORT [--mode term|log] [--escape '^]'] [--readonly]               # 사용자 뷰어 (핵심 루프, M1)
loom uart sim list | loom uart sim start SCENARIO [--as dut]                       # 시뮬레이션
loom uart daemon start|stop|status|logs
```
명명 리셋 시퀀스(프로필 TOML):
```toml
[profile.esp32]
eol = "crlf"
prompt = '> $'
open_dtr = false          # 열 때 리셋 방지
open_rts = false
[profile.esp32.seq.esp32-run]       # 일반 리셋
steps = [ {rts=true}, {wait_ms=100}, {rts=false} ]
[profile.esp32.seq.esp32-download]  # 부트로더(다운로드 모드) 진입 → R3
steps = [ {dtr=false, rts=true}, {wait_ms=100}, {dtr=true, rts=false}, {wait_ms=50}, {dtr=false} ]
risk = "R3"
```
(ESP32 DTR/RTS 극성은 보드의 자동 리셋 회로에 따라 다름 [추정] — 프로필은 기본값, 사용자 수정 가능)

### 4.8 에이전트 스킬 패키징 (`loom-uart`)
**배치**: 저장소 단일 원본 `skills/loom-uart/` → 설치 스크립트(`loom skills install`)가 프로젝트의 `.claude/skills/loom-uart/`와 Codex용 경로(`.agents/skills/` 또는 `~/.codex/skills/` — Codex의 SKILL.md 지원 경로는 확인 필요 [추정])에 심링크/복사. 본문은 **CLI만** 사용하고 Claude 전용 도구명을 언급하지 않아 두 에이전트에서 동일하게 동작.

**SKILL.md 초안 (짧게)**
```markdown
---
name: loom-uart
description: Read and write serial/UART/COM ports (embedded boards, boot logs, U-Boot, device shells) through the Loom daemon while the user watches live. Use for opening ports, waiting for boot/login prompts, running commands on a device console, capturing boot logs, resetting boards via DTR/RTS.
---
# loom-uart

The user sees the same port live in their viewer. NEVER open a serial port with pyserial/screen/minicom/cat — always use `loom uart` (the daemon owns the port; direct opens fail and would hide your work from the user).
If a response warns `no_viewer_attached`, ask the user to run `loom uart attach dut` (or open it via herdr, see recipes R-8).

## Start
1. `loom uart ports` → pick by VID:PID/serial. `loom uart open <path|--match ...> --as dut --baud <B> [--profile linux-shell|uboot|esp32]`
2. Tell the user: "Run `loom uart attach dut` in a pane to watch." (herdr: see references/recipes.md#viewer)

## Core verbs (all bounded; add --json to parse)
- `loom uart read dut` — new output since your last read (cursor). `--tail 50`, `--since 30s|mark:X`.
- `loom uart expect dut 'login:' 'panic' --timeout 60` — exit 3 on timeout; check `matched_index`.
- `loom uart send dut 'cmd' --expect '# $'` — send and wait atomically. Prefer this over send+expect.
- `loom uart run dut 'cmd'` — returns only the command output.
- `loom uart grep dut 'ERROR' --since mark:boot` / `loom uart log export ... -o file` for large logs.
- Passwords: `--secret-env VAR`, never put secrets in the command line.

## Rules
- Exit 5 = the user is typing/holding the port: stop and ask the user; don't retry in a loop.
- Exit 6 = confirmation required: show the user the exact bytes (from the error) and ask; resend with `--confirm <id>` only after they approve (or they approve in the viewer).
- Before reset/break/baud change/bootloader/flash/erase commands, say what you will do in one line.
- Garbled output → wrong baud: `loom uart probe dut`. Never guess-send to a garbled console.
- Keep reads small; don't dump whole logs into context.

References: cli.md, recipes.md, safety.md, json.md, profiles.md, troubleshooting.md
```

**references 목록과 개요**
| 파일 | 내용 |
|---|---|
| `references/cli.md` | 모든 동사, 인자, 위치 지정자 문법, 종료 코드 표 |
| `references/json.md` | `--json` 봉투와 동사별 data 스키마(JSON Schema 발췌), 예시 |
| `references/recipes.md` | 아래 레시피 R-1~R-8 전체 명령 |
| `references/safety.md` | 위험 등급표, 확인 흐름, 에이전트가 해야 할 말 예시 |
| `references/profiles.md` | 내장 프로필(linux-shell, uboot, esp32, stm32-vcp, zephyr-shell, generic), 리셋 시퀀스, 프롬프트 정규식, 사용자 정의 방법 |
| `references/troubleshooting.md` | 깨진 글자(baud), 무출력(TX/RX 교차, 전원, 흐름제어), `E_PORT_BUSY`(Windows 다른 프로그램 점유), Linux 권한(dialout), macOS cu vs tty, CH340 시리얼 없음으로 AMBIGUOUS, 열 때 보드가 리셋됨(DTR) |

**레시피 (recipes.md 요지)**
- R-1 부팅→로그인→명령→파싱: §2 A1 그대로 + `run --json`의 `output` 파싱 팁(줄 단위, `--ansi strip`).
- R-2 리셋 후 부트로그 캡처: `mark` → `signal reset` → `expect '<부팅 완료 문자열>' '<실패 문자열>'` → `grep --since mark:` → 필요 시 `log export`.
- R-3 U-Boot 진입: `expect 'Hit any key'` → `send --hex 20 --eol none --expect '=> '`.
- R-4 장시간 대기: `expect ... --timeout 1800`; 에이전트 도구 호출 타임아웃이 더 짧으면 **짧은 expect를 반복**하되 `--from cursor`로 이어 붙이기(커서 덕분에 놓치는 구간 없음).
- R-5 인터럽트: `send dut --ctrl c --expect '# $' --timeout 5`.
- R-6 baud 모름: (후순위 M4) `probe` → `configure --baud`. M1–M3에선 흔한 baud를 `configure`로 바꿔가며 `read --tail` 확인.
- R-7 외부 플래셔 공존: `release` → (사용자 확인 후) 외부 도구 → `reclaim` → `expect` 부팅.
- R-8 뷰어 띄워주기(herdr): `herdr pane split --current --direction right` 로 새 패인 ID를 얻고 `herdr pane run <ID> loom uart attach dut` [확인: 로컬 herdr CLI `pane split`/`pane run` 존재, 2026-11-09]. herdr가 없으면 사용자에게 명령만 안내.
- R-9 (후순위 M4) 숫자 추출 플롯: `loom uart extract dut --re '...' -o x.csv && loom viz show x.csv --x ts --y temp`.

---

## 5. 사용자 뷰어 UX

### 5.1 모드 구조 (D4)
| | **A. 터미널 모드 패스스루 + 로그 모드 Textual [추천]** | B. Textual 단일 앱 + pyte VT 에뮬레이션 |
|---|---|---|
| 터미널 충실도(색, 커서 이동, vi/top on device, U-Boot 메뉴) | herdr/호스트 터미널이 처리 → 최고 | pyte 품질에 좌우, pyte 0.8.2 마지막 릴리스 2023-11 [확인: PyPI] |
| 타임스탬프/hex/검색 | 로그 모드에서 풍부하게 | 한 화면에서 가능 |
| 구현 난이도 | 두 모드 전환(Textual `suspend`) 필요 | VT 에뮬 위젯 구현 부담 |
| herdr 패인 적합성 | 좋음(일반 CLI처럼 동작) | 좋음 |

**추천 A**: `loom uart attach dut`는 기본 **터미널 모드**로 시작(minicom/picocom 느낌). `Ctrl-] v`로 **로그 모드**(Textual 전체화면) 전환, `Esc`/`Ctrl-] v`로 복귀. 두 모드는 같은 구독을 공유하고, 터미널 모드 복귀 시 마지막 64 KiB를 다시 그려 화면을 복원.

### 5.2 터미널 모드 화면
```
┌─ herdr: pane 2 ───────────────────────────────────────────────────────────────┐
│[    2.104] mmc0: new HS200 MMC card at address 0001                            │
│[    3.112] systemd[1]: Started Journal Service.                                │
│                                                                               │
│dut login: root                       ← 청록색: 에이전트 송신의 에코             │
│Password:                                                                      │
│root@dut:~# uname -a                   ← 청록색                                 │
│Linux dut 6.6.21 #1 SMP PREEMPT ... aarch64 GNU/Linux                          │
│root@dut:~# dmesg | tail -3            ← 기본색: 사용자 입력                     │
│...                                                                            │
│root@dut:~# █                                                                  │
│▌dut ● OPEN 115200 8N1 │ agent: expect /TEST DONE/ 1714s │ REC │ ts:off │ ^] menu│
└───────────────────────────────────────────────────────────────────────────────┘
```
- 하단 1줄 상태바: 스크롤 영역(DECSTBM)으로 마지막 줄을 예약(picocom/tmux 상태줄과 같은 방식). 터미널 크기 변경 시 재설정. 상태바 끄기 `Ctrl-] S`.
- 상태 색: OPEN 초록, DISCONNECTED 노랑 깜빡임(`reconnecting… 4s`), HOLD 빨강, RELEASED 회색, 에이전트 락은 `🔒agent: reason`.
- 타임스탬프 토글(`Ctrl-] t`): 줄 시작마다 `[23:05:01.234] `를 흐린 색으로 삽입(off → abs → rel(+Δ) 순환). 커서 이동 시퀀스가 많은 화면(vi 등)에서는 깨질 수 있으므로 로그 모드 권장 안내.
- hex 토글(`Ctrl-] x`): 이후 수신을 `hexdump -C` 형식으로 출력.

### 5.3 로그 모드 화면 (Textual)
```
 dut · /dev/cu.usbserial-A10KXYZ · 115200 8N1 · OPEN · REC ●         origin: all ▾   ts: abs   hex: off
 ───────────────────────────────────────────────────────────────────────────────────────────────────────
 23:05:01.234   D   U-Boot 2024.01 (Oct 02 2026 - 10:11:12 +0000)
 23:05:03.101   D   Hit any key to stop autoboot:  2
 23:05:03.140   A▶  ␠                                                          [claude-7f3a]
 23:05:03.150   D   =>
 23:05:04.002   U▶  printenv bootargs⏎
 23:05:04.010   D   bootargs=console=ttyS0,115200 root=/dev/mmcblk0p2 rw
 ──────────── ◆ mark: boot-1  (agent) ────────────────────────────────────────────────────────────
 23:05:09.871   D   dut login:
 ──────────── ⚡ reconnected after 3.2s (COM5 → COM7) ───────────────────────────────────────────────
 ───────────────────────────────────────────────────────────────────────────────────────────────────────
 ┌ agent activity ──────────────────────────────┐
 │ 23:05:03 send " " (R1)            ok         │
 │ 23:05:04 expect /=> $/            matched 0.0s│
 │ 23:05:09 expect /TEST DONE|FAIL/  1714s left │
 └──────────────────────────────────────────────┘
 /panic  [3/12]  n/N  │ F2 ts  F3 hex  F4 origin  F5 save  F6 marks  F7 activity  Space follow  Esc terminal
```
- 행 = 줄(rx) 또는 tx 이벤트. 열: 시각 / 출처(D=device, A=agent, U=user, S=script) / 내용. tx는 `▶` + 보이지 않는 문자 기호화(`⏎`=CR, `␊`=LF, `␠`, `^C`).
- `Space`: 팔로우(자동 스크롤) on/off. 스크롤 중엔 상단에 `▼ 214 new lines` 표시.
- `/` 검색(정규식, 대소문자 토글 `Alt-c`), `n/N` 이동, 매치 하이라이트. 검색 대상은 메모리 버퍼 + (옵션) 기록 파일 전체(`Ctrl-/`).
- `F3` hex: 행마다 `0000  55 2d 42 ...  |U-B...|`.
- `F4` 출처 필터: all / device only / agent tx only / user tx only.
- `F5` 저장: 현재 화면 범위·선택 범위·마크 사이를 `txt/raw/jsonl`로 저장, 경로는 `~/.loom/records/uart/exports/`.
- `F6` 마크 목록 점프, `m` 마크 추가.
- `F7` 에이전트 활동 패널 토글.
- 로그 모드에서도 입력 가능: 하단 입력줄(`i` 로 진입)에 줄 단위로 쓰고 Enter로 송신(줄끝 설정 따름). 원시 키 입력은 터미널 모드에서.

### 5.4 에이전트 송신 강조 규칙
- 터미널 모드: tx 바이트 자체는 출력하지 않는다(장치 에코와 중복되므로, 일반 터미널과 동일). 대신
  1. **에코 매칭 틴트**: 에이전트 tx 직후 2초 이내 rx에서 tx 텍스트와 앞부분이 일치하는 구간을 청록색으로 칠한다(줄끝 문자는 제외하고 비교).
  2. 에코가 안 오면(비밀번호, 에코 off 장치, 바이너리) 그 자리에 `⟨agent: 6 bytes⟩` 흐린 주석을 1회 삽입(`Ctrl-] a`로 주석 표시 끔/켬).
  3. 상태바에 최근 에이전트 동작.
- 로그 모드: tx 행이 별도로 보이므로 확실. 출처 색: agent 청록, user 노랑, script 보라, device 기본.
- 사용자 입력은 강조하지 않는다(익숙한 터미널 느낌 유지).

### 5.5 키 바인딩
**접두키 `Ctrl-]`** [제안]. 근거: herdr prefix가 `Ctrl-B` [확인: `herdr --default-config`, 2026-11-09], picocom/minicom/screen의 `Ctrl-A`는 장치 쪽 bash/readline에서 줄 처음 이동으로 자주 쓰임. `Ctrl-]`는 telnet 탈출키로 익숙하고 장치에서 거의 안 씀. `--escape` 옵션/설정으로 변경 가능.

| 키 | 동작 |
|---|---|
| 그 외 모든 키 | 그대로 장치로(Ctrl-C=0x03, Ctrl-D=0x04, Ctrl-Z=0x1A, Ctrl-\=0x1C, 화살표 → VT 시퀀스) |
| `Ctrl-] Ctrl-]` | 0x1D 리터럴 송신 |
| `Ctrl-] q` | 뷰어 종료(포트·세션은 유지) |
| `Ctrl-] ?` | 도움말 오버레이 |
| `Ctrl-] v` | 터미널 ↔ 로그 모드 |
| `Ctrl-] t` | 타임스탬프 off/abs/rel |
| `Ctrl-] x` | hex 표시 토글 |
| `Ctrl-] e` | 로컬 에코 토글 |
| `Ctrl-] l` | Enter 키가 보낼 줄끝 순환(CR/LF/CRLF) |
| `Ctrl-] h` | hold 토글(에이전트 쓰기 차단) |
| `Ctrl-] k` | 진행 중 에이전트 전송/대기 중 send 취소 |
| `Ctrl-] b` | break 송신(확인 없이 — 사용자 동작) |
| `Ctrl-] r` | 리셋 시퀀스 메뉴(프로필의 seq 목록) |
| `Ctrl-] d` / `Ctrl-] s` | DTR / RTS 토글 (`s`는 RTS — 상태바에 현재 레벨 표시) |
| `Ctrl-] m` | 마크 찍기(라벨 입력) |
| `Ctrl-] w` | 최근 N분 로그 저장 |
| `Ctrl-] p` | 화면 일시정지(수신은 계속 버퍼링) |
| `Ctrl-] R` | release/reclaim 토글 |
| `Ctrl-] S` | 상태바 숨김/표시 |
| `Ctrl-] a` | 에이전트 주석 표시 토글 |

- 사용자 쪽 위험 동작(break, 리셋)은 확인 없이 실행 — 사람이 직접 누르는 것이므로. 단 모두 기록에 `origin=user`로 남는다.
- Windows 콘솔: `ENABLE_PROCESSED_INPUT` 끄고 원시 입력 + VT 입력 모드 사용 [추정 — 구현 시 prompt_toolkit 입력 계층 재사용 검토].

### 5.6 확인 팝업 (R2/R3 승인)
```
┌ Agent request · claude-7f3a · R3 ─────────────────────────────────────────┐
│ send to dut:  sf erase 0 0x100000⏎                                        │
│ hex: 73 66 20 65 72 61 73 65 20 30 20 30 78 31 30 30 30 30 30 0d           │
│ rule: uboot.flash_erase   reason (agent): "기존 이미지 지우고 재기록"        │
│                                                                           │
│  [y] approve once   [a] approve rule for 10 min   [n] deny   (expires 60s)│
└───────────────────────────────────────────────────────────────────────────┘
```
- 터미널 모드에서는 상태바 위에 3–5줄 오버레이로 그렸다가 응답 후 화면 복원(뒤 스크롤 영역 보존).
- 여러 뷰어가 붙어 있으면 모두에게 뜨고 먼저 응답한 쪽이 유효.
- R4는 팝업으로도 승인 불가(정책 파일 수정 필요) — 팝업은 "차단됨" 알림만.

### 5.7 여러 뷰어 · 읽기 전용 · herdr 연동
- 같은 포트에 뷰어 여러 개 가능. 모두 동일 스트림, 입력도 모두 가능(origin=user, client=viewer-id로 구분).
- `--readonly`: 키 입력을 보내지 않는 관전 모드(실수 방지, 다른 사람에게 보여줄 때).
- herdr 패인 제목: 뷰어가 시작 시 OSC 2로 창 제목 `uart:dut` 설정, `herdr pane report-metadata` 사용 가능 여부 검토 [추정].

### 5.8 웹 뷰 검토 (D9)
- 동일 WS 프로토콜에 브라우저가 붙는다. 데몬이 `GET /` 로 정적 페이지 서빙(xterm.js + 상태바 + 로그 표). URL: `http://127.0.0.1:51733/#token=…`(토큰을 fragment로 → 서버 로그·리퍼러에 안 남음), `loom uart web dut` 명령이 URL 출력/브라우저 열기.
- 장점: 넓은 스크롤백, 마우스 선택, viz 패널과 한 화면 합치기 가능(CAN 팀 viz가 웹이면 자연스럽게 통합).
- **추천: 후순위(M4).** 사용자 기본 동선은 herdr 패인이고, 터미널 모드가 충실도 면에서 이미 충분하다. 단 IPC를 WS로 정해 두면 언제든 추가 가능 — 이게 D2 추천의 핵심 이유.

---

## 6. 데이터 / 기록 포맷

### 6.1 디렉터리
```
~/.loom/records/uart/2026-11-09/dut-230501/
  session.json        # 메타: 장치 식별, 시작 시각, mono↔wall 기준점, 프로필, 버전
  bytes.bin           # rx+tx 원시 바이트, 도착 순서대로 이어붙임 (off = 파일 오프셋)
  chunks.jsonl        # 청크 인덱스 (아래 6.2)
  events.jsonl        # 제어 이벤트: 상태/config/시그널/마크/락/hold/안전/확인
  text.log            # 사람이 읽는 줄 단위 로그 (6.3)
  cursors.json        # 커서 스냅샷
```
- 파일 순환: `bytes.bin` 256 MiB마다 `bytes.0001.bin`… (`chunks.jsonl`에 파일 번호 포함).
- 보존: 기본 30일 또는 전체 5 GiB 초과 시 오래된 세션부터 삭제(설정 가능), 마크가 있는 세션은 `--pin`으로 보존.
- 기록 쓰기는 별도 writer 태스크, 1s마다 flush/fsync는 하지 않음(성능), 정상 종료 시 fsync.

### 6.2 `session.json` / `chunks.jsonl` 필드
```json
{"schema":"loom.uart.session/1","alias":"dut","device":{"path":"/dev/cu.usbserial-A10KXYZ","vid":"0403","pid":"6001","serial":"A10KXYZ","location":"20-1.2","description":"FT232R USB UART"},
 "config":{"baud":115200,"bytesize":8,"parity":"N","stopbits":1,"flow":"none"},"profile":"linux-shell",
 "clock":{"mono_ns_at_start":81200000000000,"wall_at_start":"2026-11-09T23:05:01.000000+09:00","host":"ctm-mbp"},
 "loom_version":"0.1.0"}
```
```json
{"seq":1042,"f":0,"off":81920,"len":16,"t":81234567890123,"dir":"rx","o":"device"}
{"seq":1043,"f":0,"off":81936,"len":4,"t":81234570012345,"dir":"tx","o":"agent","c":"claude-7f3a","r":"R1"}
{"seq":1044,"f":0,"off":81940,"len":9,"t":81234571000000,"dir":"tx","o":"agent","c":"claude-7f3a","secret":true}
```
- `t` = monotonic ns(호스트가 read() 반환을 받은 시각). wall = `wall_at_start + (t - mono_ns_at_start)`. 짧은 키로 줄 크기 축소(청크 1개당 ~90바이트).
- 타임스탬프 정밀도 한계: USB-시리얼 칩의 지연 타이머(FTDI 기본 16ms 등)와 OS 스케줄링으로 수 ms 오차 [추정 — sol 측정]. `session.json`에 `ts_source:"host_read"` 명시.

### 6.3 `text.log`
```
2026-11-09 23:05:01.234 D| U-Boot 2024.01 (Oct 02 2026 - 10:11:12 +0000)
2026-11-09 23:05:03.140 A> ␠                                   {client=claude-7f3a}
2026-11-09 23:05:04.002 U> printenv bootargs⏎
2026-11-09 23:05:04.010 D| bootargs=console=ttyS0,115200 root=/dev/mmcblk0p2 rw
2026-11-09 23:05:09.000 #| mark boot-1 (agent)
2026-11-09 23:31:02.100 #| disconnected (EIO)
2026-11-09 23:31:05.300 #| reconnected gap=3.2s path=/dev/cu.usbserial-A10KXYZ
```
- 줄 시각 = 그 줄 **첫 바이트**의 청크 시각. ANSI 제거, 비출력 문자 `\xNN`, secret 마스킹.

### 6.4 공통 이벤트 레코드 매핑 (00-brief §3-6)
`loom uart log export --format jsonl` 출력(1청크 = 1레코드, 또는 `--lines`면 1줄 = 1레코드):
```json
{"ts":{"mono_ns":81234567890123,"wall":"2026-11-09T23:05:01.234567+09:00"},"domain":"uart","channel":"dut","dir":"rx","origin":"device","kind":"data","seq":1042,"payload":{"b64":"VS1Cb290IDIwMjQuMDE=","text":"U-Boot 2024.01"}}
{"ts":{...},"domain":"uart","channel":"dut","dir":"tx","origin":"agent","client":"claude-7f3a","kind":"data","payload":{"b64":"IA==","text":" "},"risk":"R1"}
{"ts":{...},"domain":"uart","channel":"dut","dir":null,"origin":"device","kind":"state","payload":{"state":"DISCONNECTED","reason":"EIO"}}
{"ts":{...},"domain":"uart","channel":"dut","dir":"rx","origin":"device","kind":"line","payload":{"text":"temp=41.5 rpm=1200","off":90211}}
```
- `channel` = 세션 별칭. 공통 필드 외 추가 제안: `kind`, `seq`, `client`, `risk` (§15).

### 6.5 Export 포맷
| format | 용도 |
|---|---|
| `raw` | rx 바이트만 이어붙인 `.bin` (다른 도구 재생용) |
| `txt` | text.log 형식(옵션: 시각 끄기, rx만) |
| `jsonl` | 공통 이벤트 레코드 |
| `csv` | 줄 단위 `ts_wall,ts_rel,origin,text` (스프레드시트/viz) |
| (후순위 M4) `teraterm` 타임스탬프 로그 호환 | 사용자 요청 시 |

### 6.6 재생(replay) — 후순위 M4
`loom uart sim start replay://~/.loom/records/uart/.../dut-230501 --speed 1.0` — 기록을 가짜 장치로 재생(rx만, 원래 타이밍). 파서/expect 레시피 회귀 테스트에 사용.

### 6.7 viz 요구사항 초안 (리드가 `_xteam/viz-requirements.md`로 옮김)
구현은 후순위(M4)지만 요구는 지금 고정한다. 단, **UART의 1차 실시간 표시는 viz가 아니라 `attach`** 다(§0).
1. **입력 형식**: `loom uart extract`가 만드는 CSV/JSONL — 열 `ts_wall`(RFC3339 µs), `ts_rel_s`, 그리고 정규식 명명 그룹별 숫자 열(`temp`, `rpm`…). 숫자 변환 실패는 빈 값.
2. **자동 key=value 파싱**: `--auto-kv` 모드는 `k=v`, `k: v`, `k=v unit` 패턴을 자동 추출 → viz도 같은 규칙으로 텍스트 로그를 직접 받을 수 있으면 좋음(텍스트 + 추출 규칙을 viz 입력 스펙으로).
3. **라이브 스트림 소스**: viz가 UART 데몬 WS에 `uart.subscribe(kinds=["line"])`로 붙고, 추출 규칙(정규식/명명 그룹)을 viz 쪽 설정으로 받아 실시간 플롯. → viz의 "라이브 소스 플러그인" 인터페이스가 필요(CAN과 공통).
4. **이벤트 오버레이**: 마크·리셋·재연결·에이전트 송신을 시간축 수직선/마커로(입력: events.jsonl 또는 공통 이벤트 레코드 `kind in {mark,signal,state}`).
5. **로그 역참조**: 플롯 점 클릭/호버 → 원본 줄 텍스트와 `off` 표시, 가능하면 "뷰어에서 이 위치 열기".
6. **규모**: 1 kHz 이하 샘플 × 수 시간(~10^7 점) → 다운샘플링 필요(CAN 쪽 LTTB와 공통).
7. **시간축**: wall clock 기본, 상대(초) 토글. 여러 UART 포트와 CAN 신호를 같은 시간축에 겹치기(동일 호스트 monotonic 기준 정렬).
8. **에이전트 한 줄 호출**: `loom viz show temp.csv --x ts_wall --y temp,rpm` 만으로 사용자 화면에 뜰 것.

---

## 7. 라이브러리 선택과 근거
버전/날짜는 PyPI JSON API로 직접 확인 [확인: PyPI 2026-11-09]. 유지보수·Windows 동작의 깊은 검증은 r1-sol.md 참조.

| 용도 | 후보 | 최신(릴리스일) | 라이선스 | 판단 |
|---|---|---|---|---|
| 시리얼 I/O | **pyserial** | 3.5 (2020-11-23) | BSD | **채택**. 사실상 표준, 3-OS, `list_ports`(VID/PID/serial/location), break/DTR/RTS, URL 핸들러(`loop://`, `socket://`, `rfc2217://`). 5년간 릴리스 없음 → 리스크(§13), 포크 대비 래퍼 계층(`Transport`)으로 격리 |
| asyncio 래퍼 | pyserial-asyncio | 0.6 (2021-09-30) | BSD | 비채택(정체) |
| | pyserial-asyncio-fast | 0.16 (2025-03-27) | BSD-3 | 후보. 그러나 **전용 reader 스레드 방식 추천**(3-OS 동일, 단순) [제안] |
| IPC | **websockets** | 17.4 (2026-10-30) | BSD-3 | **채택**. 순수 asyncio, 서버/클라이언트 모두, py≥3.11 |
| | aiohttp | 3.14.5 (2026-10-11) | Apache-2.0/MIT | 대안(정적 파일 서빙 쉬움). 웹 뷰(후순위 M4)에서 필요 시 |
| | FastAPI/uvicorn | 0.162.2 / 0.54.0 | MIT/BSD | 과함(REST 불필요) |
| JSON | **msgspec** | 0.22.0 (2026-09-29) | BSD-3 | 채택 후보(빠른 JSON + 스키마 검증 + msgpack 겸용). 대안: stdlib json |
| TUI(로그 모드) | **Textual** | 8.2.8 (2026-06-30) | MIT | **채택**. 활발, 스냅샷 테스트 도구, `App.suspend()`로 터미널 모드 전환 [추정 — API 확인] |
| | prompt_toolkit | 3.0.53 (2026-07-26) | BSD | 터미널 모드의 Windows 원시 입력 처리에 일부 재사용 후보 |
| | urwid | 5.1.0 (2026-11-06) | LGPL-2.1 | 비채택(LGPL, Windows 지원 약함 [추정]) |
| VT 에뮬 | pyte | 0.8.2 (2023-11-12) | LGPL-3.0 [추정] | 비채택(D4-A면 불필요) |
| expect | pexpect | 4.9.0 (2023-11-25) | ISC | **비채택(엔진 직접 구현)**. pexpect는 자기 프로세스가 fd를 읽는 모델 → 데몬 버퍼 위 다중 클라이언트 expect와 안 맞음, Windows 제약. 정규식+증분 스캔은 수백 줄 규모 |
| XMODEM/YMODEM | xmodem | 0.5.0 (2026-03-04) | MIT | 후순위(M4) 채택 후보(YMODEM 지원 범위 확인 필요 [추정]) |
| 경로 | platformdirs | 4.14.5 (2026-11-04) | MIT | 선택(§3.10에서 `~/.loom` 고정 제안) |
| 설정 | tomllib(stdlib) | — | — | 프로필 TOML 읽기 |

---

## 8. HW / OS 지원 매트릭스
| 항목 | macOS (개발) | Linux | Windows (실 HW 유력) |
|---|---|---|---|
| 포트 이름 | `/dev/cu.*` 사용(`tty.*`는 DCD 대기로 open이 막힐 수 있음 → 목록에서 숨김) [추정 — sol 확인] | `/dev/ttyUSB*`, `/dev/ttyACM*`, 안정 경로 `/dev/serial/by-id/*` | `COMn` |
| 동일 장치 식별 | serial, location | serial, by-id, location | serial, location(`hwid`의 LOCATION) |
| 배타 점유 | 기본 아님 → 데몬이 `exclusive=True`로 열어 다른 프로그램 실수 방지 | 동일(TIOCEXCL) | OS가 배타 → 다른 프로그램(TeraTerm 등)이 열고 있으면 `E_PORT_BUSY` |
| break / DTR / RTS | 지원 | 지원 | 지원 |
| 권한 | 없음 | `dialout`/`uucp` 그룹 필요 → troubleshooting 안내 | 없음 |
| 사용자 뷰어 | herdr 패인 | herdr 패인 | Windows Terminal(herdr 지원 여부 미확인 [추정]) |
| 시뮬레이션 | `sim://`, pty | `sim://`, pty, socat(선택) | `sim://`, com0com(수동/선택) |
| 칩 | FTDI(내장 드라이버), CP210x, CH34x(드라이버 필요할 수 있음), PL2303, CDC-ACM | 커널 내장 | 벤더 드라이버/CDC 내장 |
| 우선순위 | M1 1급(pty 가짜 디바이스 + 가능하면 실 FTDI) | M1부터 CI(pty) | M1 CI(`sim://`), M3 실HW 스모크 |

- **보드 리셋 주의**: 많은 보드(Arduino, ESP32 DevKit)는 포트 open 시 DTR/RTS 변화로 리셋된다 → 프로필의 `open_dtr/open_rts`를 open 전에 적용(pyserial은 open 전 속성 설정 가능 [추정 — sol 스파이크로 확인 요청]).

---

## 9. 시뮬레이션 & 테스트 전략

### 9.1 전송 계층 추상화
```
Transport (abstract): open(), close(), read(n, timeout), write(b), send_break(ms),
                      set_dtr/rts(bool), get_cts/dsr/ri/cd(), set_config(cfg)
 ├─ PySerialTransport   — 실제 포트, pyserial URL(rfc2217:// 등) 포함
 ├─ SimTransport        — 인프로세스: 가짜 장치 시나리오와 큐로 직결 (3-OS 동일, CI 기본)
 └─ (테스트) PtyTransport — POSIX pty 페어의 slave를 pyserial로 열어 실제 경로 검증
```
- `sim://<scenario>`는 데몬 안에서 가짜 장치를 돌린다 → **Windows CI에서도 com0com 없이** 전 기능 테스트.
- pty 경로(`loom uart sim start bootshell --pty`)는 실제 pyserial 코드 경로·외부 도구(screen/picocom) 호환 확인용. pty는 break/DTR/RTS/parity가 실제 UART와 다르게 동작 [추정] → 시그널 테스트는 SimTransport에서.

### 9.2 가짜 디바이스 시나리오
Python DSL(테스트 친화) + 간단 YAML(사용자 작성용) 두 층. 내장 시나리오:
| 이름 | 동작 |
|---|---|
| `bootshell` | 부팅 로그(타이밍 포함 ~200줄) → `login:` → `Password:`(에코 없음) → 쉘 `root@sim:~# `, 명령 `uname -a`, `cat /proc/uptime`, `dmesg`, `reboot`(부팅 재시작), `sleep N`, 알 수 없는 명령은 `sh: x: not found` |
| `uboot` | `Hit any key to stop autoboot: 3` 카운트다운, 아무 키 → `=> `, `printenv`, `setenv`, `saveenv`, `boot` |
| `esp32` | RTS 펄스(시퀀스) → 리셋 로그, 다운로드 모드 시퀀스 → `waiting for download` |
| `sensor` | 10 Hz `temp=41.5 rpm=1200` 줄 (viz 테스트) |
| `garbage` | baud 불일치 흉내(랜덤 고비트 바이트) — probe 테스트 |
| `ansi` | 색/커서 이동/진행률(`\r` 덮어쓰기) 출력 |
| `flaky` | N초 후 분리(전송 계층 오류) → M초 후 재등장(같은 serial 또는 다른 경로) |
| `binary` | 바이너리 프레임 스트림(hex 보기 테스트) |
| `flood` | 지정 속도(기본 92 KB/s = 921600 상당)로 `seq=N t=<mono_ns>` 줄 연속 출력 — 지연·폭주·무손실 측정(§3.11, M1) |
| `replay://path` | 기록 재생 |

YAML 예:
```yaml
name: mini-shell
on_open:
  - emit: "Booting...\r\n"
  - wait: 0.5s
  - emit: "login: "
states:
  login:   { on_line: { "root": { emit: "Password: ", goto: password } } }
  password:{ echo: false, on_line: { ".*": { emit: "\r\n# ", goto: shell } } }
  shell:
    on_line:
      "uname -a": { emit: "Linux sim 6.6 aarch64\r\n# " }
      "reboot":   { emit: "rebooting\r\n", restart: true }
on_signal:
  rts_pulse: { restart: true }
  break:     { emit: "\r\n<BREAK>\r\n" }
```

### 9.3 테스트 층
| 층 | 내용 | 도구 |
|---|---|---|
| 단위 | RingBuffer/오프셋, 커서, 줄 분리(`\r` 처리), ANSI 제거, 출력 상한(머리+꼬리), expect 증분 매칭(청크 경계에 걸친 패턴), 위험 패턴 판정, 장치 매칭 규칙 | pytest |
| 통합 | 데몬 + `sim://` + **CLI를 서브프로세스로** 실행해 종료코드·`--json` 스키마 골든 테스트 | pytest, msgspec 스키마 |
| 동시성 | 사용자 입력 시뮬레이터(가짜 뷰어 클라이언트)가 타이핑하는 중 에이전트 send → 지연/타임아웃/hold/락 탈취 시나리오 | pytest-asyncio |
| 재연결 | `flaky` 시나리오 + 가짜 `list_ports` 결과 주입 → AMBIGUOUS 포함 | pytest |
| TUI | 로그 모드 Textual 스냅샷 테스트, 터미널 모드는 pty로 뷰어 프로세스를 구동해 출력/키 전달 검증 | Textual pilot, pty |
| pty E2E (POSIX) | pty 페어 slave를 실제 pyserial로 열기 | pytest(macOS/Linux만) |
| 스킬 레시피 | references/recipes.md의 명령을 `sim://bootshell`에 대해 그대로 실행(문서-코드 동기화 테스트) | pytest |
| 실HW (수동/야간) | FTDI/CP210x/CH340/CDC-ACM 보드 체크리스트: 열기, 115200/921600 송수신, break, DTR/RTS 리셋, 뽑았다 꽂기 | 체크리스트 문서 |

CI: GitHub Actions `ubuntu-latest`, `macos-latest`, `windows-latest` × Python 3.11/3.13. HW 불필요.

---

## 10. 안전 / 가드레일

### 10.1 위험 등급
| 등급 | 정의 | 예 | 에이전트 기본 정책 |
|---|---|---|---|
| R0 읽기 | 장치 상태 불변 | ports, status, read, expect, grep, attach, log, mark | 자유 |
| R1 일반 입력 | 쉘/콘솔 일반 명령, 제어문자 | `send 'ls'`, `--ctrl c`, `run 'uname -a'` | 허용 + 레이트 제한 + 출처 표시 |
| R2 교란 | 장치 동작을 끊거나 리셋 | break, DTR/RTS, `signal reset`, `configure --baud`, `release`, `reboot`/`reset` 문자열, 대용량 `--file`(> 4 KiB) | **정책에 따름**: 기본 `ask_once_per_session`(세션 첫 R2 때 확인, 이후 같은 종류 허용) |
| R3 위험 | 저장 상태 변경/파괴, 부트로더 진입 | 플래시/erase/write, `saveenv`, `env default -a`, `fw_setenv`, `dd of=/dev/`, `mkfs`, `rm -rf`, `flash_erase`, `nand erase`, 다운로드 모드 시퀀스, XMODEM 전송 | **매번 확인** |
| R4 비가역 | 되돌릴 수 없음 | efuse/OTP/fuse 쓰기(`fuse prog`, `efuse burn`, `espefuse`), 보안부트 키, `otp write`, 락비트 | **차단**(정책 파일에서 사용자가 명시적으로 풀어야) |

- 판정은 **데몬의 SafetyGate**에서(스킬 문구에만 의존하지 않음 — 에이전트가 스킬을 무시해도 막힌다).
- 문자열 규칙은 프로필별 정규식 묶음(`linux`, `uboot`, `esp-idf`, `zephyr`, `generic`)으로 관리하고, **에이전트 송신에만** 적용(사용자 입력은 필터링하지 않음).
- 규칙은 오탐을 감수하는 쪽(안전 우선). `--dry-run`으로 판정 미리 확인 가능.
- 정책 파일 `~/.loom/config/uart-policy.toml` + 프로젝트 `loom.toml`(프로젝트가 더 엄격하게만 바꿀 수 있음 [제안]):
```toml
[uart.policy]
R2 = "ask_once_per_session"   # allow | ask | ask_once_per_session | deny
R3 = "ask"
R4 = "deny"
[uart.policy.port.dut]
R2 = "allow"                  # 이 보드는 리셋 자유
[uart.rate]
sends_per_sec = 10
bytes_per_sec = 8192          # --file 은 pace 적용
```

### 10.2 확인 흐름
```
agent: loom uart send dut 'saveenv'
  → SafetyGate: R3 (uboot.saveenv)
  → 뷰어가 붙어 있으면: 확인 팝업 방송 (§5.6)
  → CLI 동작 (두 가지 중 선택)
     (a) 기본: 즉시 exit 6 + {"confirm_id":"c-91f2","risk":"R3","bytes_hex":"…","rule":"uboot.saveenv","expires_s":300}
         → 에이전트가 사용자에게 채팅으로 묻고, 승인 시 `--confirm c-91f2`로 재호출
         → 단, 사용자가 그 사이 뷰어에서 승인/거부하면 그 결정이 confirm_id에 기록됨
     (b) `--wait-confirm 60s`: 뷰어 팝업/`loom uart approve`를 기다리며 블록 → 승인 시 송신, 거부/만료 시 exit 6
```
- `--confirm ID`는 **같은 바이트열·같은 포트**에만 유효(해시로 바인딩), 1회용, 5분 만료. 에이전트가 ID를 재사용해 다른 명령을 보낼 수 없다.
- 중요한 설계 질문: 에이전트가 채팅에서 "사용자가 승인했다"고 주장하며 `--confirm`을 쓰는 것을 막을 수는 없다(같은 신뢰 경계). 그래서 R3는 **뷰어 승인 전용 모드**(`R3 = "ask_viewer"`)를 옵션으로 제공 — 이 모드에선 confirm_id가 뷰어(role=user) 승인으로만 활성화. [제안: 기본은 `ask`(채팅/뷰어 둘 다), 엄격 모드 선택 가능]
- 모든 판정·승인·거부는 `events.jsonl`에 `kind:"safety"`로 기록(누가, 언제, 어떤 바이트).

### 10.3 사용자 개입 정책 (사용자가 타이핑 중이면 에이전트는?)
1. 데몬 수준: §3.7 타이핑 가드 → 에이전트 send 지연, 초과 시 exit 5.
2. 스킬 수준 지침: exit 5를 받으면 **재시도 루프 금지**. 사용자에게 "포트를 사용 중이신 것 같습니다. 끝나면 알려주세요" 식으로 알리고 대기. `status`로 `user typing`/`hold` 확인 가능.
3. 사용자 hold는 사용자만 해제(에이전트는 `unhold` 권한 없음 — role=user 전용).
4. 에이전트 락은 사용자가 언제든 탈취. 탈취 후 에이전트는 다단계 시퀀스를 처음부터 다시 판단(현재 상태를 read로 확인).
5. 사용자 Ctrl-C가 장치로 갔다는 사실(tx origin=user, 0x03)은 `agent_activity`와 함께 에이전트의 다음 read/expect 결과 `warnings`에 `"user_input_since_last_call": true`로 알려준다 → 에이전트가 상황 변화를 인지.

### 10.4 레이트 제한
- 에이전트 send: 기본 10 회/s, 8 KiB/s(포트별). 초과 시 대기(짧게) 후 여전히 초과면 `E_RATE_LIMIT`. 사용자 입력은 제한 없음.
- 같은 페이로드 반복 송신(동일 바이트 10회/10s 초과) 감지 시 경고 + 일시 차단 → 에이전트 재시도 루프 방지.
- R2 시그널: 최소 간격 500ms(DTR/RTS 고속 토글로 보드가 이상 상태에 빠지는 것 방지).

### 10.5 비밀값
- `--secret-env`: 값은 CLI가 환경변수에서 읽어 데몬으로 전달, 명령줄·에이전트 출력·text.log·뷰어에 노출 안 함.
- 장치 에코 마스킹(같은 바이트열이 2s 내 rx에 나타나면 text 표현에서 마스킹; bytes.bin 원본은 유지 — 원본 기록에서도 지울지 §14 질문).

### 10.6 포트 열기 시 의도치 않은 리셋 방지
- 프로필 `open_dtr/open_rts` 지정 시 open 전에 적용. 프로필 없는 경우 pyserial 기본(DTR/RTS assert) 유지하되, `ports` 출력에 칩이 ESP/Arduino 계열로 보이면 경고 [제안].

### 10.7 감사
- tx 전부(origin, client, risk, rule, confirm_id) 기록. `loom uart audit dut --since 1h` (후순위 M4)로 에이전트가 보낸 것만 요약.

---

## 11. 마일스톤

원칙: **M1 = 걷는 뼈대.** 시뮬레이션(pty 가짜 디바이스) 위에서 핵심 루프 4요소(직접 접근·읽기·쓰기·사용자 실시간 표시+개입)가 최소 형태로 **모두** 동작해야 M1이다. 멋진 UI·부가 포맷은 M1 이후.

| M | 내용 | 수용 기준 |
|---|---|---|
| **M0 합의·준비** (짧게, 구현 최소) | 메인의 IPC/디스커버리/이벤트 레코드 확정 반영, uart-sol 스파이크(pty+pyserial, break, open 전 DTR/RTS) 결과 반영, 저장소 골격(`loom.core`, `loom.uart`, uv, CI 빈 파이프라인) | 결정 문서 고정. 빈 CI가 3-OS에서 녹색 |
| **M1 걷는 뼈대 (핵심 루프)** | ① 데몬 + 최소 WS JSON-RPC + 자동 기동 ② `PySerialTransport` + **pty 가짜 디바이스**(`loom uart sim start bootshell --pty` → 경로 출력, 데몬이 실제 포트와 같은 경로로 open) + Windows용 `sim://` ③ CLI: `ports/open/close/status/send(텍스트·hex·--ctrl·--eol)/read(커서·tail·상한·ANSI 제거)/expect(다중 패턴·timeout)/send --expect` ④ **`attach` 터미널 모드 최소형**: 원시 키 양방향(Ctrl-C 포함), 64 KiB 백로그 후 라이브, 에이전트 에코 청록 틴트(안 맞으면 `⟨agent: N bytes⟩` 주석), 1줄 상태바(상태·에이전트 최근 동작·lag), `Ctrl-] q`, `Ctrl-] h`(hold) ⑤ InputArbiter 최소형: send 원자성 + 타이핑 가드 + hold ⑥ SafetyGate 최소형: R3/R4 문자열 규칙(generic+linux+uboot), `E_CONFIRM_REQUIRED`/`--confirm`(해시 바인딩), `--dry-run`, 레이트 제한 ⑦ 기록: bytes.bin + chunks.jsonl(무손실) ⑧ 뷰어 gap/lag 표시 ⑨ 스킬 v0(SKILL.md만) | **(a)** §2.0 E1 시나리오 ①–④를 pty `bootshell`에서 macOS·Linux 수동 시연 + 자동 테스트(가짜 뷰어 클라이언트), Windows CI는 `sim://`로 동일 테스트 **(b)** 115200에서 rx→뷰어 p95 < 100 ms 측정 통과 **(c)** `flood`(92 KB/s, 60 s): 뷰어 멈춤 없음, gap/lag 정확 표시, bytes.bin 무손실, 동시 expect 매치 **(d)** 에이전트가 쓰는 중 attach 시 백로그+라이브가 오프셋 연속 **(e)** 타이핑 중 에이전트 send가 장치 라인을 섞지 않음 **(f)** R3 문자열이 확인 없이 절대 송신 안 됨 **(g)** Claude가 SKILL.md만 보고 open→expect→send→read 수행 **(h)** 가능하면 macOS 실 FTDI/CP210x 1개로 같은 흐름 수동 확인 |
| **M2 에이전트 완성 + 안전** | `run`, `grep`, `mark`, 위치 지정자 전체, `--json` 스키마 고정·종료 코드 골든 테스트, 재연결(매칭·AMBIGUOUS·`flaky`), 시그널(break/DTR/RTS)·프로필·명명 리셋 시퀀스·open 시 DTR/RTS, 정책 파일·R2 `ask_once_per_session`, **뷰어 확인 팝업**, 에이전트 락/탈취, `--secret-env`, text.log/events.jsonl, 스킬 v1 + references 6종 + 레시피 동기화 테스트, Codex 설치 경로 | 레시피 R-1~R-5·R-7·R-8을 `bootshell`/`uboot`/`esp32`/`flaky` 시나리오에서 자동 통과. 재연결을 넘어 expect 매치. 뷰어 팝업 승인/거부/만료 테스트. Claude·Codex 각각 A1–A4 수행 |
| **M3 사용자 뷰어 심화 + 실HW** | 로그 모드(Textual: 타임스탬프·hex 보기·검색·출처 필터·마크·저장·에이전트 활동 패널, 모드 전환), 터미널 모드 메뉴 전체(ts/hex 토글, 리셋 메뉴, 일시정지), `release/reclaim`, export(raw/txt/jsonl/csv), 실HW 스모크(macOS FTDI/CP210x/CH340/CDC-ACM, Windows COM) | 사용자 시나리오 U1–U5 수동 시연 통과(herdr). 로그 모드 스냅샷 테스트. Windows 실 COM에서 E1 재현 |
| **M4 후순위 (핵심 루프에 직접 기여하지 않음)** | 웹 뷰(xterm.js), XMODEM/YMODEM, `probe`(baud 탐색), `extract` + viz 라이브 소스, `replay://`, `audit`, msgpack 협상, OS 장치 이벤트 기반 재연결 | 기능별 데모 + 테스트. 사용자 요청 시 우선순위 재조정 |

---

## 12. 구현 단계 작업 분해
담당: **O** = Opus 워커(설계 무게·UX·통합), **S** = Sol 워커(라이브러리·플랫폼·성능·테스트 인프라). 같은 단계 안에서 의존성 없는 것은 병렬. **M1 작업은 전부 "최소형"** — 기능 폭보다 4요소가 끝까지 이어지는 것이 우선.

| ID | 작업 | M | 담당 | 의존 |
|---|---|---|---|---|
| T00 | 저장소 골격(uv, 패키지 구조, 빈 3-OS CI) | M0 | S | – |
| T01 | `loom.core.ipc` 최소: WS JSON-RPC 서버/클라이언트, 토큰, 디스커버리 파일, 자동 기동, subscribe+backlog (타 도메인과 공용 — 메인 확정 후) | M1 | O | 메인의 IPC 확정, T00 |
| T02 | `Transport` 인터페이스 + `PySerialTransport`(reader 스레드, write, config) | M1 | S | T00 |
| T03 | 가짜 디바이스 엔진 + `bootshell`·`flood` 시나리오 + **pty 모드**(POSIX) + `sim://`(인프로세스) | M1 | S | T02 인터페이스 |
| T04 | RingBuffer/스트림 좌표/청크 + Recorder(bytes.bin, chunks.jsonl, 기록 우선 팬아웃) | M1 | S | T00 |
| T05 | PortSession + 상태 머신(OPEN/CLOSED/ERROR 우선) + 데몬 조립 + CLI `ports/open/close/status/send` | M1 | O | T01,T02,T04 |
| T06 | 텍스트 렌더 파이프라인 최소: 줄 분리(`\r`), ANSI 제거, 출력 상한(머리+꼬리) | M1 | S | T04 |
| T07 | 커서 + `read`(new/tail/peek) + ExpectEngine(증분, 다중 패턴, 청크 경계, 버퍼 끝=줄 끝) + `send --expect` | M1 | O | T05,T06 |
| T08 | InputArbiter 최소: 원자 send, 타이핑 가드, hold, 사용자 입력 우선 | M1 | O | T05 |
| T09 | 뷰어 터미널 모드 최소: 원시 입력(POSIX termios; Windows 콘솔은 M1에선 기본 동작만), 백로그→라이브, 에코 틴트/주석, 상태바(상태·에이전트 동작·lag·gap), `Ctrl-] q/h` | M1 | O | T01,T08 |
| T10 | SafetyGate 최소: R3/R4 규칙셋(generic/linux/uboot), confirm ID(해시 바인딩·1회용), dry-run, 레이트 제한 | M1 | O | T05 |
| T11 | 지연·폭주 측정 하네스(가짜 뷰어 클라이언트, lag p95, bytes.bin 해시 비교) + CI 통합 | M1 | S | T03,T04,T01 |
| T12 | 스킬 v0 SKILL.md | M1 | O | T07 (초안은 병렬) |
| T13 | `run`, `grep`, `mark`, 위치 지정자 전체 | M2 | O | T07 |
| T14 | `--json` 스키마(msgspec)·종료 코드·골든 테스트 | M2 | S | T07 |
| T15 | DeviceWatcher·재연결 매칭(serial/location/AMBIGUOUS) + `flaky` 시나리오 | M2 | S | T05 |
| T16 | 시그널·프로필(TOML)·명명 리셋 시퀀스·open 시 DTR/RTS + `uboot`/`esp32` 시나리오 | M2 | S | T05,T03 |
| T17 | SafetyGate 완성: 정책 파일, R2 정책, **뷰어 확인 팝업**, 에이전트 락/탈취, `--secret-env`, 감사 이벤트 | M2 | O | T10,T09 |
| T18 | 스킬 v1: references 6종 + 레시피 동기화 테스트 + Claude/Codex 설치 | M2 | O | T13,T17 |
| T19 | 뷰어 로그 모드(Textual) + 모드 전환 + 스냅샷 테스트 | M3 | S | T09 |
| T20 | 터미널 모드 메뉴 전체(ts/hex 토글, 리셋 메뉴, 일시정지 정직성 표시), Windows 콘솔 원시 입력 완성 | M3 | O | T09,T16 |
| T21 | `release/reclaim`, export(raw/txt/jsonl/csv), text.log | M3 | S | T05,T04 |
| T22 | 실HW 스모크 체크리스트·수행(macOS/Windows) | M3 | S | T20 |
| T23 | 웹 뷰(xterm.js) | M4 | O | T01,T17 |
| T24 | XMODEM/YMODEM + R3 연동 | M4 | S | T17 |
| T25 | `probe`, `extract` + viz 라이브 소스 어댑터 | M4 | O/S | viz 인터페이스 확정 |
| T26 | `replay://`, `audit`, msgpack, OS 장치 이벤트 | M4 | S | – |

병렬 흐름(M1): S는 T02·T03·T04·T06·T11을 동시에, O는 T01 → T05 → (T07 ∥ T08 ∥ T10) → T09 → T12. 합류 지점은 T05(데몬 조립)와 T11(측정 하네스로 M1 수용 판정).

---

## 13. 리스크
| 리스크 | 영향 | 가능성 | 완화 |
|---|---|---|---|
| pyserial 장기 무릴리스(3.5, 2020) | 신규 Python/OS에서 깨질 때 대응 지연 | 중 | `Transport` 계층으로 격리, 필요 시 포크/벤더링, sol 조사로 현황 확인 |
| 터미널 모드 상태바(DECSTBM)·에코 틴트가 장치의 전체화면 앱(vi, top, 메뉴)과 충돌 | 화면 깨짐 | 중 | 상태바 숨김 키, 전체화면 감지 시 자동 숨김, 틴트 끄기 옵션 |
| 에코 매칭 오탐(장치 출력이 우연히 같은 텍스트) | 잘못된 강조 | 저 | 시간창 2s, tx 직후 연속 구간만, 로그 모드가 정답 제공 |
| 사용자 타이핑 가드가 에이전트를 자주 막음 | 에이전트 작업 지연 | 중 | 3s 유휴 기준, 상태바로 이유 표시, 에이전트에 `deferred_ms` 피드백 |
| `$` 프롬프트 매칭 경쟁(프롬프트가 늦게 오거나 부분 도착) | run/expect 오판 | 중 | 버퍼 끝=줄 끝 규칙 + 프롬프트 뒤 50ms 정적 대기 옵션 |
| 동일 장치 식별 실패(시리얼 없는 CH340 여러 개) | 엉뚱한 보드에 송신 | 저/치명 | AMBIGUOUS 상태에서 자동 연결 금지, location 사용 |
| 포트 open 시 보드 리셋(DTR) | 사용자가 원치 않은 리셋 | 중 | 프로필 open_dtr/rts, 경고 |
| 고속(921600~3M) 팬아웃 시 Python 처리량/GIL | 뷰어 지연, 드롭 | 중 | reader 스레드는 최소 작업, 청크 병합, 구독자 드롭(gap), 기록 우선. sol 성능 측정 |
| 타임스탬프 오차(USB 지연 타이머) | 정밀 타이밍 분석 불가 | 확실 | 문서화(`ts_source`), FTDI latency timer 안내 |
| 에이전트가 `--confirm`을 사용자 승인 없이 사용 | 위험 명령 실행 | 저/치명 | 바이트 해시 바인딩·1회용, 엄격 모드(`ask_viewer`), 감사 기록 |
| 위험 문자열 규칙 우회(인코딩, 여러 send로 쪼개기) | 가드 무력화 | 중 | 줄 단위 재조립 판정(포트별 직전 미완 줄 + 현재 페이로드), hex 송신도 텍스트로 디코딩해 판정 |
| herdr의 Windows 지원 불확실 | Windows 사용자 뷰어 동선 | 중 | Windows Terminal 탭/분할로 `attach` 실행 안내, 웹 뷰(후순위 M4) |
| 실시간성 목표 미달(USB latency timer, 터미널 렌더, Python 루프) | 사용자가 늦은 화면을 실시간으로 오인 | 중 | 상시 lag 표시, M1 측정 하네스로 조기 발견, 병합 주기·스로틀 조정 |
| 데몬 단일 장애점 | 모든 세션 중단 | 저 | 데몬 로그, 자동 재기동, 커서/기록은 디스크 → 재기동 후 이어짐 |
| 로컬 WS 포트에 다른 로컬 프로세스 접근 | 무단 송신 | 저 | 토큰(0600 파일), Origin 헤더 검사, 127.0.0.1 바인드 |

---

## 14. 사용자에게 물어볼 질문 (기본값 제안)
| # | 질문 | 기본값(답 없으면 진행) |
|---|---|---|
| Q1 | 주로 쓰는 보드/칩은? (임베디드 리눅스 SoC, U-Boot, ESP32, STM32, Zephyr …) | linux-shell, uboot, esp32, generic 프로필을 M1에 포함 |
| Q2 | 실 HW는 Windows에서 쓰나요, macOS에서도 쓰나요? | macOS 1급 + Windows M2 스모크 |
| Q3 | 에이전트가 **확인 없이** 보드 리셋(DTR/RTS, break, reboot)을 해도 되나요? | 세션당 첫 1회만 확인(`ask_once_per_session`) |
| Q4 | 위험 명령(R3) 승인을 채팅으로 해도 되나요, 뷰어 팝업으로만 받을까요? | 둘 다 허용(`ask`) |
| Q5 | 기본 줄끝(Enter)은 CR로 할까요? | CR, 프로필에서 변경 |
| Q6 | 뷰어 탈출키 `Ctrl-]` 괜찮나요? | `Ctrl-]` |
| Q7 | 웹 뷰가 필요한가요, herdr 패인이면 충분한가요? | herdr 패인, 웹은 후순위(M4) |
| Q8 | XMODEM/YMODEM 파일 전송이 필요한가요? | 후순위(M4) |
| Q9 | 기록 보존 기간/용량과 위치(`~/.loom/records`)? | 30일 / 5 GiB / `~/.loom` |
| Q10 | 비밀번호를 원시 기록(bytes.bin)에서도 지워야 하나요? | 텍스트 표현만 마스킹, 원본 유지 |
| Q11 | 사용자가 타이핑 중일 때 에이전트 대기 기준 3초가 적절한가요? | 3초 |
| Q12 | 실시간 목표 rx→화면 p95 < 100 ms로 충분한가요? | 100 ms |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 변경 요청 (메인 결정 필요)
1. **§3-2 데몬 배치**: "장치 소유 프로세스 = 도메인별 데몬, 공통 프레임워크 공유"를 명시해 주길 요청(D1). 단일 데몬으로 정해도 UART는 수용 가능하나 CAN-TP 타이밍 격리 근거로 도메인별을 권장.
2. **§3-3 IPC 확정 제안**: JSON-RPC 2.0 over WebSocket, `127.0.0.1` 임의 포트, 토큰(디스커버리 파일 0600), 첫 메시지 `hello{proto, token, role, client}`, 이벤트는 `<domain>.event` notification, 바이너리는 base64(추후 msgpack 협상). 디스커버리 파일 `~/.loom/run/<domain>.json`. 백프레셔는 구독자별 드롭 + `gap` 이벤트.
3. **§3-4 CLI 공통 규약 추가**: `--json` 봉투(`ok/cmd/data/error{code,message,hint}/warnings/truncated`), 종료 코드 표(0–7, §4.1)를 도메인 공통으로. 위치 지정자 문법(`cursor|now|off:N|mark:X|30s|@HH:MM:SS`)과 이름 있는 커서 개념도 CAN 트레이스 조회에 그대로 적용 가능 → 공통화 제안.
4. **§3-5 출처 표시 확장**: `origin` 외에 `client`(어느 에이전트/뷰어) 필드 추가. 사용자 hold / 에이전트 락 / 확인 요청을 도메인 공통 개념으로(CAN 송신에도 동일 필요).
5. **§3-6 공통 이벤트 레코드 보강**: `ts`를 `{mono_ns, wall(RFC3339 µs)}`로 고정, 필드 `kind`(data/line/state/signal/mark/safety…), `seq`, `client`, `risk` 추가, 바이트 도메인 payload는 `{b64, text?}`.
6. **§3-10 위험 등급 공통화**: R0–R4 정의를 세 도메인 공통 축으로(UDS: write DID=R3, ECU reset=R2, flash/security access=R3, 비가역 OTP/VIN 등=R4 같은 매핑 가능). 확인 ID의 바이트 해시 바인딩·1회용 규칙도 공통.
7. **설정/런타임 경로**: `~/.loom/{run,records,config}` 고정 제안.

### 타팀 의존성
| 대상 | 필요한 것 | 시점 |
|---|---|---|
| 메인 | IPC·디스커버리·종료 코드·이벤트 레코드 확정 (`loom.core`) | M0 전 |
| CAN 팀(viz 소유) | §6.7 viz 요구사항 반영, 라이브 소스 플러그인 인터페이스, CSV/JSONL 입력 스펙 | M2 설계 시 |
| CAN/UDS 팀 | 위험 등급·확인 흐름·hold/lock 공통 UX 합의(뷰어 팝업 형태 통일) | M1 전 |
| UDS 팀 | (약함) UART로 ECU 디버그 콘솔을 보면서 UDS 요청을 하는 동시 시나리오 — 같은 시간축 기록 정렬(mono_ns 공통 기준) | M3 |

---

## 핵심 원칙 자체 점검 (docs/plan/02-core-principle.md 체크리스트)

| # | 항목 | 충족 | 위치 |
|---|---|---|---|
| 1 | 맨 앞 "핵심 루프" 절 — 4요소별 구현 한 표 | ✅ | §0 (표 + 하나의 스트림 원칙) |
| 2 | 종단 간 시나리오(에이전트 송신 → 장치 응답 → 사용자 화면 ms 단위 → 사용자 개입), 구체 명령·화면 | ✅ | §2.0 E1 ①–④ (명령, 화면 ⓐⓑⓒ, 지연 예산 표, Ctrl-C·타이핑 중 개입·HOLD) |
| 3 | 실시간성 목표치 + 고부하 시 멈추거나 거짓말하지 않는 방법 | ✅ | §3.11 (rx→화면 p95 < 100 ms 등 목표 표, 기록 우선·드롭+gap 표시·LAG 표시·렌더 스로틀·일시정지 표시·기기 쪽 손실 불명 고지, 921600 폭주 테스트) |
| 4 | M1 = 걷는 뼈대(시뮬레이션 위 4요소 최소 동작), 부가 기능은 이후 | ✅ | §11 M1(pty 가짜 디바이스 + 최소 attach/send/read/expect/안전), §12 M1 작업 T01–T12 |
| 5 | 사용자 화면이 에이전트 세션과 분리되지 않음, 숨은 연결 금지 | ✅ | §3.12 (직접 열기 경로 없음, 배타 점유, attach 즉시 백로그+라이브 오프셋 연속, `--no-record`와 무관, 뷰어 없음 경고), §0 하나의 스트림 원칙, §4.8 스킬 규칙 |
| 6 | 원칙에 기여하지 않는 기능은 후순위/비범위 명시 | ✅ | §1 "후순위" 표, §11 M4, 결정 요약 D8/D9 |

남은 확인 필요(추정 표시 항목): USB 칩별 지연(FTDI latency timer), 터미널 렌더 처리량, pyserial 오버런 보고 불가 여부 — uart-sol 스파이크/측정으로 R2에서 수치 확정.
