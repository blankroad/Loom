# UART R1 독립 초안 — Sol

작성/근거 확인: 2026-11-09 KST. 계획 문서이며 제품 구현은 하지 않았다. 최우선은 [02-core-principle.md](../02-core-principle.md), 다음은 [공통 브리프](../00-brief.md), [UART 브리프](brief.md), R1 및 긴급 core addendum이다. `[확인]`은 문서/메타데이터/로컬 실험으로 확인한 사실, `[제안]`은 설계·수용 기준, `[추정]`은 아직 검증하지 못한 성능·플랫폼 동작을 뜻한다. CLI/API/스키마 예시는 전부 제안이며 현재 사용 가능한 제품 명령이 아니다.

## 0. 핵심 루프

| 필수 요소 | UART 구현 제안 | M1에서 증명할 것 |
|---|---|---|
| 직접 접근 | 에이전트가 `loom uart open PORT` 실행 → 사용자 손을 거치지 않고 데몬의 pyserial 어댑터가 실제 포트를 연다. CI에서는 동일 어댑터로 PTY slave를 연다. | CLI 요청으로 가짜 장치가 연결되고 session/실제 경로가 반환됨 |
| 읽기 | 데몬이 단 한 번 읽은 RX를 기록·공유 버퍼·클라이언트에 분배. 에이전트는 유한한 `read`/`expect`로 같은 바이트를 읽는다. | `read --tail 20 --timeout 1s --json` 결과가 장치 응답과 일치 |
| 쓰기 | 에이전트 `send`와 사용자 키 입력이 같은 포트 TX actor를 경유한다. 출처·부분 송신·실패를 공개한다. | `send --text status --eol cr`가 장치에 실제 도착하고 응답 발생 |
| 사용자 실시간 표시·개입 | herdr 패인의 `attach`가 같은 session의 RX/TX 이벤트를 구독. origin 강조, 직전 스크롤백, 사용자 입력과 Ctrl-C 전달. | 장치 RX 수신→사용자 화면 쓰기 p95 <100 ms, agent TX도 즉시 표시, 사용자 Ctrl-C로 장치 작업 중단 |

**에이전트 전용 숨은 연결은 금지한다.** CLI, 스킬, expect, 파일 송신, TUI 어느 곳에도 데몬을 우회해 `serial.Serial`을 여는 경로를 두지 않는다. pyserial 의존성은 데몬 어댑터에만 둔다. 사용자가 에이전트 작업 도중 attach하면 이미 열린 동일 세션과 직전 스크롤백을 즉시 받는다. direct access는 데몬을 통한 장비 조작이며, 사람에게 포트를 열어달라고 요구하는 흐름이 아니다.

### 종단 간 예: 에이전트 진단 → 사용자 중단

아래는 M1 수용 시나리오다. 시뮬레이터 CLI는 향후 구현안이며 R1 스파이크와 별개다.

```sh
# 개발/CI: master에서 장치를 흉내 내고 slave 경로를 한 줄 출력
loom uart sim start --profile boot-shell --seed 7
# {"port":"/dev/ttys046","sim_id":"sim-1"}

# 에이전트: 사용자 대행 없이 직접 open
loom uart open /dev/ttys046 --name dut --baud 115200 --reconnect off --json
# {"session":"u1","channel":"dut","state":"open","generation":1}
loom uart send dut --text status --eol cr --timeout 2s --json
# {"request_id":"q1","status":"written","written_bytes":7,"tx_seq":"41"}
loom uart expect dut --literal 'temp=' --after-request q1 --timeout 2s --json
# {"matched":true,"text":"temp=42.5 rpm=1200\r\n","rx_seq":"42"}

# 사용자: 에이전트 실행 중 herdr 패인에서 같은 연결에 attach
loom uart attach dut --scrollback 100
# 실제 Windows 예: 이미 COM3로 열린 session에 loom uart attach COM3
```

```text
UART dut  /dev/ttys046 115200 8N1   LIVE  gen=1   log:OK
14:10:00.000 [agent a1 TX written q1] status<CR>       ← 청록+문자 태그
14:10:00.018 [device RX #42] temp=42.5 rpm=1200
14:10:00.040 [display #42]                            ← 검증용 계측, 평상시 숨김
14:10:00.100 [user TX] monitor<CR>
14:10:00.120 [device RX] sample=1
14:10:00.200 [user TX] ^C (03)
14:10:00.218 [device RX] interrupted\r\ndut>
owner:user  rendered:#46  newest:#46  skipped:0  RX gap:0
Ctrl-] then q: detach | y: yield input | t: time | h: hex
```

위 시각은 `[제안 예시]`다. RX→화면 쓰기 22 ms가 실제 측정됐다는 뜻이 아니다. M1에서 monotonic 계측으로 p95 <100 ms를 측정한다. 921600 baud 연속 출력에서도 30 Hz 렌더, 제한된 화면 행, 누락/지연 표시로 UI를 계속 조작할 수 있어야 한다(§3, §5, §11). 원시 기록은 지원 부하 내 무손실을 수용 조건으로 삼고, 과부하·디스크 장애에서의 불가피한 손실을 숨기지 않는다.

## 1. 범위 / 비범위

**M1 범위**: 데몬 단독 소유, PTY와 pyserial 경로, open/list/close, 최소 send/read, attach 스크롤백·실시간 표시·사용자 입력·Ctrl-C, origin, 자동 원시 기록, 출력·큐 상한. 첫 납품은 네 요소가 연결된 걷는 뼈대다. 라이브러리 표·스키마만 완성하고 화면을 미루는 M1은 불합격이다.

M1 이후 필수 범위: macOS/Linux/Windows 물리 UART, 다중 포트, 장치 신원에 따른 재연결, bounded expect와 에이전트별 커서, 문자열/hex/파일 송신, 안전한 ANSI/hex 보기, 검색·export, break/DTR/RTS와 보드별 검증된 시퀀스. 터미널 수준 목표에는 CR 덮어쓰기·backspace·색상·키 전달을 포함한다. curses/전면 화면 호환은 별도 수용 시험 후 확대한다.

후순위: 웹 뷰, 숫자 추출 플롯, XMODEM/YMODEM, 프리셋 편집 UI, 고급 hex 편집. XMODEM/YMODEM은 M1–M3 비범위이며, 추후 체크섬/재전송/사용자 개입 정책이 검증된 전송 플러그인으로만 검토한다. 펌웨어 플래셔·보드별 부트로더 구현·RS485 방향제어·원격 RFC2217·장비 자동 탐색용 능동 probe·GUI 드라이버 설치는 본 계획 비범위다. 바이트 파일 send를 XMODEM 지원이라고 부르지 않는다.

## 2. 사용 시나리오

| 주체 | 구체적 작업 | 성공 판정 |
|---|---|---|
| 에이전트 A1 | USB VID/PID/serial로 보드를 찾아 115200 8N1 open, 부팅 `login:`을 expect, 승인된 테스트 계정 로그인 | 포트 선택·TX·응답이 동일 사용자 화면에 보이고 expect가 제한시간 내 종료 |
| 에이전트 A2 | `send --hex '01 03 00 00' --eol none`, `read --tail-bytes 256 --format hex --timeout 1s` | 00/ff를 포함한 원시 바이트 보존, ANSI/인코딩으로 오염되지 않음 |
| 에이전트 A3 | 채널 dut와 aux를 동시에 관찰하고 client별 cursor로 새 데이터만 읽기 | 서로의 커서를 소비하지 않으며, ring 만료는 명시적 GAP 반환 |
| 에이전트 A4 | 상태 명령 송신 후 장치 USB 탈착 발생 | 부분 송신/연결 세대 변경을 감지하고 명령을 자동 재전송하지 않음 |
| 사용자 U1 | herdr 패인에서 `loom uart attach COM3`, 에이전트가 보낸 status와 장치 응답 관찰 | 기존 session에 즉시 붙고 직전 100행+새 이벤트를 순서대로 표시 |
| 사용자 U2 | 에이전트 작업 중 로컬 prefix로 입력권 회수 후 키 입력·Ctrl-C 전송 | agent 잔여 송신이 정지되고 사용자 바이트가 사용자 출처로 보임 |
| 사용자 U3 | 타임스탬프 토글·hex 보기·검색으로 부팅 오류 찾기, 로그 export | 보기는 바뀌어도 RX 원본과 에이전트 수신 결과는 동일 |
| 사용자 U4 | 921600 baud 폭주 시 화면 일부만 보고 일시정지/스크롤 | 화면 지연·생략 수가 표시되고 기록은 독립적으로 계속됨 |

로그인 비밀번호는 profile의 secret reference로 주입하고 화면에 `TX 12 bytes [secret]`로 표시한다. 비밀번호를 CLI 인자/모델 출력/원시 파일에 무조건 남기는 설계는 금지한다(§6, §10).

## 3. 아키텍처

```text
Agent skill → finite CLI ──────┐
herdr → TUI attach ────────────┼─ loopback IPC, authenticated client
later web/viz ────────────────┘          │
                         loom daemon (사용자별 한 인스턴스)
                         ├─ registry / policy / RPC / event subscriptions
                         ├─ session dut: RX reader → stamp/seq → record queue
                         │                            ├→ bounded ring → read/expect
                         │                            └→ per-client queue → TUI/CLI
                         │   arbitration → TX actor → pyserial handle ─→ DUT
                         └─ session aux: 독립 handle/queues/TX actor ───→ AUX
                                      │
                              raw.bin + events.jsonl + index
```

UART reader는 포트마다 하나, writer/control actor도 하나다. pyserial handle은 RX와 TX를 병행하되 close/reconfigure의 수명주기는 session 상태기가 직렬화한다. 동일 COM3, 별칭 dut, symlink를 별도 장치로 열지 않도록 canonical path와 USB identity를 registry에서 중복 검사한다. 데몬 중복 실행을 사용자 runtime lock으로 막고 POSIX `exclusive=True`를 보조로 사용한다. `[확인]` Windows backend는 `CreateFile` share mode 0으로 COM을 배타적으로 연다. POSIX pyserial exclusive는 flock이므로 비협조적 외부 프로그램의 open까지 막는 보장은 없다. [Win32 소스](https://github.com/pyserial/pyserial/blob/a5c48d445fbc1943d4fabf8d9090a50fda3172fd/serial/serialwin32.py), [POSIX 소스](https://github.com/pyserial/pyserial/blob/a5c48d445fbc1943d4fabf8d9090a50fda3172fd/serial/serialposix.py).

### 읽기 루프와 성능 예산

추천 기본은 **pyserial blocking reader thread/port + bounded asyncio bridge**, 전용 TX worker다. `read(1)`로 첫 바이트 대기(timeout 20 ms), 이어 `in_waiting`을 참고해 최대 16 KiB까지 drain한다. 항상 `read(16384)`로 가득 차길 기다려 저속 프롬프트를 지연시키지 않는다. callback 하나/바이트, 줄이 올 때까지 무제한 축적, 메인 event loop에서 디스크 fsync·regex 실행은 피한다. bridge는 무제한 `call_soon_threadsafe` 예약 대신 포트당 bounded byte queue와 합쳐진 wakeup을 쓴다. 미세 설정은 M1 실측으로 결정한다.

| 8N1 부하 | 계산상 RX 데이터 | RX+TX 최대 합계 | 의미 |
|---|---:|---:|---|
| 115200 baud | 11,520 B/s | 23,040 B/s | 대화형 기준 |
| 921600 baud | 92,160 B/s | 184,320 B/s | 연속 로그 수용 시험 |
| 3,000,000 baud | 300,000 B/s | 600,000 B/s | USB/driver/HW 지원 전제 |
| 8포트 × 3 Mbaud | 2.4 MB/s | 4.8 MB/s | `[추정]` 지원 목표, 아직 보장하지 않음 |

계산식은 baud/(start 1 + data 8 + parity 0 + stop 1). 7E2 등은 다시 계산한다. 3 Mbaud에서 1바이트의 선로 시간은 약 3.33 µs지만, 데몬 시각은 USB packet/OS read가 끝난 시각이므로 그 정밀도로 바이트 도착을 측정할 수 없다. Python `monotonic_ns()`의 정수 단위와 장비 시간 정확도를 혼동하지 않는다. chunk마다 read 반환 직후 `mono_ns`, `wall_ns`를 수집하고 timestamp_kind=`host_read_complete`로 기록한다. 추정 바이트 시각을 만드는 기능은 기본 제외한다.

초기 상한 `[제안]`: 포트 8개, ring은 포트당 payload 16 MiB 및 event 32,768개 중 먼저 도달한 상한, 전체 payload 128 MiB. client outbound는 encoded 2 MiB/구독, writer queue 8 MiB/포트, TX queue 64 KiB/포트, expect window 64 KiB, 활성 expect 8개/포트. 이벤트·오브젝트·문자열 overhead를 별도 계측한다. 16 MiB는 RX만 3 Mbaud 약 55.9초, full duplex 약 28.0초에 해당하지만 event 개수 제한 때문에 실제 보존시간은 더 짧을 수 있다. 사용자에게 `retained_from`, `retained_age_ms`를 실제 값으로 보여준다.

### 팬아웃·기록·고부하

RX chunk timestamp/seq를 한 번 정하고 동일 불변 객체를 기록·ring·구독으로 보낸다. IPC는 16 ms 또는 raw 16 KiB 중 먼저 찰 때 배치, TUI는 최대 30 fps, 기본 화면 2,000행·scrollback 100,000행/32 MiB 중 작은 상한. 3 Mbaud × 4 clients의 raw 복사만 약 1.2 MB/s/포트이고 base64는 약 4/3 배다. 8포트 × 4 clients RX는 base64만 약 12.8 MB/s이므로 baud 자체보다 JSON 직렬화·터미널 렌더가 병목일 수 있다. 직렬화 재사용, tail 구독, 포트별 제한을 먼저 쓰고 binary IPC는 실측 후 결정한다.

느린 TUI는 수신을 무한 대기시키지 않는다. client queue 초과 시 해당 구독에 `stream.gap {from_seq,to_seq,reason:"slow_client"}`를 내고 최신으로 복귀하거나 재구독을 요구한다. UI에는 `화면 생략 84 chunks / 원본 기록 OK`를 표시한다. 구독 drop은 장치 RX 손실과 다른 카운터다. 명령 응답과 TX 결과는 드롭 대상에서 제외하고 전달 불가능하면 연결을 종료한다. 모든 클라이언트에 순차 await하는 fanout은 쓰지 않는다. [WebSocket backpressure 근거](https://websockets.readthedocs.io/en/stable/topics/broadcast.html).

**원시 기록은 정상 지원 부하에서 무손실**을 M1/M3 gate로 둔다. 디스크 writer가 8 MiB 큐를 소진할 때: 검증된 RTS/CTS 또는 XON/XOFF가 설정돼 있으면 해당 흐름제어에 맡기고, 없으면 bounded queue로 무손실을 보장할 수 없음을 인정한다. overload 이벤트/카운터를 별도 예약 공간에 남기고 TX 중지·세션 degraded 표시, 가능한 정확한 drop bytes와 unknown driver loss를 구분한다. 디스크 full/기록 실패에는 TX를 차단하고 `recording_failed`를 화면/CLI에 즉시 알린다. 무한 메모리·조용한 overwrite로 성공을 꾸미지 않는다. UART RX는 RX queue 포화만으로 장치를 멈출 수 없다.

### 입력 중재

| 대안 | 장점 | 문제 | 판단 |
|---|---|---|---|
| 각 입력을 도착순 전송 | 구현 간단 | 사용자 `rebo`와 agent `status`가 합쳐질 수 있음 | 기각 |
| agent 독점 lease | 자동화 예측 가능 | 사용자가 개입하지 못하면 핵심 원칙 위반 | 사용자 회수 기능 필수 |
| 사용자 우선 lease + TX transaction | 출처 명확, 문자 혼합 방지 | raw 입력의 경계를 명시해야 함 | 추천 |

단순 attach는 관찰이며 입력권을 빼앗지 않는다. line mode에서 첫 키에 사용자 lease, 로컬 편집 중 agent send는 기본 `HUMAN_ACTIVE`로 즉시 거절한다. Enter 시 완성 줄을 한 transaction으로 보내며, 사용자가 `yield`할 때 agent에게 반환한다. 2초 무입력만으로 미완성 명령을 자동 양도하지 않는다. raw mode에서는 첫 키부터 `Ctrl-] y`까지 사용자 lease를 유지한다. 여러 사용자 뷰어 중 입력권은 하나이며 나머지는 watch다.

agent lease는 사용자 편집 중이 아닐 때만 획득, 기본 10초 TTL·상한 30초, 종료/연결 해제 시 잔여 TX 취소. 사용자의 `Ctrl-] u` 또는 Ctrl-C는 즉시 회수 요청이다. 현재 driver write에 들어간 바이트는 취소 불가능하므로 다음 ≤1 KiB chunk 경계에서 agent 송신을 멈추고, Ctrl-C는 사용자 `03`으로 우선 전송한다. 부분 송신 바이트 수와 장치 상태 unknown을 공개하며 자동 재시도하지 않는다. 일반 키를 agent lease 동안 몰래 큐잉했다가 뒤늦게 보내지 않고 `입력권 회수 필요`로 알려준다. Ctrl-C의 원격 처리 자체가 장치 OS에 달렸다는 점도 표시한다. 중재 규칙은 daemon에서 강제한다.

## 4. CLI / API 명세 초안

### 탐색·세션·파라미터

```sh
loom daemon start --bind 127.0.0.1 --port 0
loom uart ports --vid 0403 --pid 6001 --json
# {"ports":[{"device":"COM3","description":"USB Serial Port",
#  "vid":1027,"pid":24577,"serial_number":"FT123","location":"1-2",
#  "interface":"0","identity_quality":"serial"}]}
loom uart open COM3 --name dut --baud 921600 --data-bits 8 --parity none \
  --stop-bits 1 --flow rtscts --dtr keep-profile --rts keep-profile \
  --reconnect same-device --json
# {"session":"u1","channel":"dut","state":"open","generation":1,
#  "effective":{"baud":921600,"flow":"rtscts"},"warnings":[]}
loom uart open /dev/cu.usbserial-FT123 --name aux --baud 115200 --flow none
loom uart sessions --json
loom uart status dut --json
loom uart close dut --timeout 2s
```

open은 즉시 시작하고 최대5초 내 반환한다. 오류는 PORT_BUSY/NOT_FOUND/PERMISSION/UNSUPPORTED_PARAM/IDENTITY_AMBIGUOUS로 구분한다. `keep-profile`은 소프트웨어가 요구한 상태를 적용한다는 뜻이며, open할 때 물리 레벨이 유지된다는 보장은 아니다. 초기 프로파일이 없으면 dtr/rts=true라는 pyserial 기본을 명시해 반환한다. open 전 보드 프로파일로 원하는 상태를 세팅해도 glitch 가능성을 표시한다(§8). 자동 probe 명령을 장치에 보내지 않는다. 재설정 `configure`는 향후 M2에서 사용자 lease와 위험 정책을 통과한 명시적 작업으로만 허용하며, attach가 baud를 바꾸지 않는다.

### 송신·읽기·expect

```sh
loom uart send dut --text 'help' --encoding utf-8 --eol crlf --timeout 2s --json
loom uart send dut --hex '00 ff 1b 5b 41' --eol none --timeout 2s --json
loom uart send dut --file ./probe.bin --eol none --max-file-bytes 1048576 --timeout 10s
loom uart read dut --tail 50 --timeout 1s --max-bytes 16384 --json
loom uart read dut --tail-bytes 4096 --format hex --timeout 1s --json
loom uart read dut --since 2026-11-09T14:00:00Z --timeout 2s --json
loom uart read dut --cursor c_a1_dut --timeout 2s --max-bytes 16384 --json
loom uart expect dut --regex '(?m)^dut> ?$' --cursor c_a1_dut --timeout 5s --json
loom uart expect dut --literal 'OK' --after-request q1 --timeout 2s --json
```

text/hex/file는 정확히 하나만. eol은 none/cr/lf/crlf 중 하나, 기본 none; text만 UTF-8 기본이며 hex/file에 암묵적 줄끝을 붙이지 않는다. NUL은 text argv 제약 대신 hex/file 사용. file은 CLI 로컬에서 읽고 bounded 업로드(64 KiB RPC message 이하, 서버 파일 경로 열기 금지), 기본 상한 1 MiB·명시 상한 16 MiB, chunk 1 KiB 송신·속도 제한·lease 유지·부분 취소. 업로드 후 send 시작을 분리해 불완전 업로드가 장치에 도착하지 않게 한다. 파일 전송은 M2 이후다.

```json
{"request_id":"q1","status":"partial","accepted_bytes":6,"written_bytes":3,
 "tx_seq":"41","generation":1,"delivery":"unknown","error":"DISCONNECTED"}
```

`written`은 OS write가 받은 바이트 수이지 장치가 명령을 실행했다는 확인이 아니다. `accepted`/`written`/`partial`/`failed` 상태를 분리한다. TX 의도는 송신 전에 기록하고 실제 결과를 후속 이벤트로 남긴다. request_id로 같은 세대·같은 payload의 중복 제출을 감지하지만, 데몬 crash 후 exactly-once를 약속하지 않는다. reconnect 시 미완료 명령은 결과 unknown, 자동 재송신 금지다.

read 기본은 RX, `--include-tx`는 별도 선택. `--tail N`은 텍스트 논리행 N개와 마지막 부분행, `--tail-bytes N`은 원시 RX N바이트이며 둘은 배타적이다. `--since`는 timezone 필수 RFC3339 wall 기준으로 검색하고 시계 보정 경고를 반환한다. 순서 판정과 새 데이터 소비는 cursor/seq를 추천한다. 원시 RX byte offset을 기준으로 cursor를 만들고 view/ANSI 필터 후에도 원본 위치로 매핑한다.

```json
{"session":"u1","generation":1,"client_id":"a1","from_seq":"42","to_seq":"44",
 "raw_b64":"T0sNCg==","text":"OK\r\n","next_cursor":"c_a1_dut_104",
 "returned_raw_bytes":4,"truncated":false,"timed_out":false,
 "gap":null,"recording":"ok","more_available":false}
```

커서는 `(session_id, client_id, RX offset)`에 묶고 agent마다 독립이다. 초기 생성 `loom uart cursor create dut --client a1 --at now --json`; `read --cursor` 성공 시 실제 반환한 바이트까지만 전진한다. response 유실 재조회는 반환된 opaque next_cursor를 명시하는 stateless 방식도 제공하며, 서버 저장 커서 전진은 별도 ack로 확정한다. expect는 일치 끝까지만 전진, timeout은 기본 유지. 같은 이름을 두 프로세스가 갱신하면 revision 충돌을 반환한다. session close/reopen은 새 session이고 이전 cursor는 STALE_CURSOR; reconnect generation 변화는 이벤트로 알리고 기본 expect는 CONNECTION_CHANGED로 종료한다. ring 만료는 CURSOR_GAP와 earliest_cursor를 반환하며 조용히 최신으로 이동하지 않는다.

명령 기본 출력 raw 상한16 KiB, 허용최대 raw32 KiB; JSON serialized 총64 KiB(메타데이터 예약 포함), human 출력32 KiB. base64/escaping이 커지면 raw 반환량을 줄이고 `truncated=true`, 이어 읽을 cursor를 반환한다. read 기본 timeout1초, expect 기본5초·상한60초. CLI 무한 stream은 없고 대화형 attach만 지속 연결이다. exit 0 성공, 2 인자 오류, 3 timeout, 4 disconnect, 5 busy/lease, 6 policy, 7 gap/한도. stderr는 진단, stdout은 JSON 한 객체로 고정한다.

expect는 pexpect를 COM에 직접 붙이지 않고 데몬 RX window에서 수행한다. literal 기본 권장, regex는 명시 옵션, pattern 최대2 KiB·window64 KiB·ANSI 제거 여부 명시·incremental UTF-8 state 유지. 줄/escape/다중 바이트 문자가 chunk 사이에서 나뉘는 경우를 지원한다. Python `re` timeout 없는 호출을 event loop에서 실행하지 않는다. regex는 제한된 worker subprocess에 맡겨 평가당50 ms 초과 시 worker 종료·PATTERN_TOO_EXPENSIVE; 동시 worker 상한을 둔다. 무한 길이 lookbehind/임의 과거 검색은 지원하지 않고 window 초과를 알린다. 대안은 timeout 가능한 regex 패키지인데, 새 의존성 조사·라이선스 검증 후 교체 여부를 결정한다.

### 신호 및 reset 시퀀스

```sh
loom uart break dut --duration 250ms --timeout 2s --dry-run --json
loom uart lines dut --dtr off --timeout 2s --dry-run --json
loom uart lines dut --rts on --timeout 2s --dry-run --json
loom uart sequence run dut --profile board-a-reset --timeout 5s --dry-run --json
# {"risk":"R2","steps":[{"dtr":false,"hold_ms":100},{"dtr":true}],
#  "requires_authorization":true,"timing":"best_effort"}
```

승인 후 동일 action digest에 묶인 authorization_id로 실행한다(§10). break/line toggle/sequence도 TX actor와 동일 lease를 사용한다. flow=rtscts일 때 수동 RTS, flow=dsrdtr일 때 수동 DTR은 CONFIG_CONFLICT. 반복 pulse·boot strap 시퀀스는 보드별 profile 승인 없이는 실행하지 않는다.

### IPC 후보 비교와 권장 wire format

| 후보 | 장점 | 비용/한계 | 제안 |
|---|---|---|---|
| JSON-RPC 2.0 over WebSocket | RPC+events 한 연결, 브라우저 직접 구독, 다중 포트 multiplex | base64/큐 관리·auth를 정해야 함 | 공통 기본 추천 |
| HTTP REST + SSE | curl·프록시 친화, 요청/이벤트 분리 | 양방향 제어 별도, resume 상태 이원화 | 차선 |
| UDS/named pipe + length-prefixed JSON | OS 권한 활용·TCP 포트 없음 | 브라우저 gateway와 Windows 별도 구현 | headless 대안 |

서버 구현은 aiohttp 추천(HTTP health/static와 WS를 한곳에서 제공). websockets 단독은 더 작은 의존성·WS 집중 대안, FastAPI는 OpenAPI/REST 통합이 필요할 때의 대안이며 M1 필수 의존성으로 만들지 않는다. 최종 전송 선택은 메인 결정에 따른다. UART 도메인 API는 transport와 분리한다.

```json
{"jsonrpc":"2.0","id":"q1","method":"uart.send","params":{
 "api_version":1,"session":"u1","generation":1,"encoding":"base64",
 "data":"c3RhdHVzDQ==","lease_id":"l1","request_id":"q1","timeout_ms":2000}}
{"jsonrpc":"2.0","id":"q1","result":{"status":"written","written_bytes":7,"tx_seq":"41"}}
{"jsonrpc":"2.0","method":"uart.event","params":{"subscription":"sub1",
 "session":"u1","seq":"42","kind":"data","dir":"rx","origin":"device",
 "ts":{"mono_ns":"123000000","wall_ns":"1790000000000000000"},
 "payload":{"encoding":"base64","data":"dGVtcD00Mi41DQo="}}}
```

`uart.subscribe {session,after_seq,tail_events}`는 서버가 high-water seq H를 잡아 `(after,H]` 스크롤백과 `(H,∞)` 라이브를 중복/누락 없이 잇는다. seq는 문자열 uint64, session 내 단조 증가. gap/status도 별도 kind이며 데이터인 척하지 않는다. subscribe/ack/unsubscribe, server capabilities/schema version, ping20초·dead60초, message최대64 KiB·구독최대32개/연결을 둔다. 장기 export는 RPC 본문 전체 반환 대신 bounded download다.

로컬 전용127.0.0.1, 랜덤 포트·사용자별 runtime descriptor(0600 또는 Windows 사용자 ACL). capability token으로 인증하고 client_id/role/origin은 서버가 부여한다. JSON에서 origin=agent라고 썼다고 신뢰하지 않는다. 웹은 same-origin static+엄격한 Origin/Host allowlist, 임의 웹페이지의 localhost 접속을 차단한다. 토큰을 URL query나 로그에 남기지 않는다. CORS만으로 WS 인증을 대신하지 않는다.

## 5. 사용자 뷰어 UX

`loom uart attach COM3`는 COM3가 가리키는 기존 session에 붙는다. 미개방이면 상태와 `open` 예시를 보여주며 원치 않는 baud/DTR 변경을 몰래 수행하지 않는다. 에이전트는 사용자 조작 없이 open할 수 있다. `attach dut`와 `attach COM3`가 같은 session_id임을 헤더에 표시한다.

```text
┌ UART dut(u1) COM3 · 921600 8N1 RTS/CTS · LIVE · user:watch ┐
│ 14:10:00.000 A:a1 → status<CR>              written q1     │
│ 14:10:00.018 D    ← temp=42.5 rpm=1200                     │
│ 14:10:00.100 U:me → monitor<CR>                            │
│ ...                                                      │
├ paused:no | render lag:37ms | view skipped:120 | RX gap:0 ┤
│ recording:OK  cursor:#986  latest:#986  session gen:1      │
│ Input:user owns   > _                                    │
└ ^] q detach · t time · h hex · / search · s save · y yield┘
```

origin은 색상뿐 아니라 A/U/D와 방향으로 표시한다. TX annotations는 장치 terminal byte stream에 삽입하지 않고 별도 gutter/event pane에 둔다. 장치 echo는 `device RX`로 두어 agent TX와 이중 표시될 수 있음을 알리며, echo를 추측해서 삭제하지 않는다. `written`과 pending/partial 표시를 구분한다.

기본 attach raw 입력에서는 Ctrl-C=0x03, Ctrl-D=0x04, Ctrl-Z=0x1a가 원격 전송이다. viewer 종료는 **Ctrl-] 다음 q**만 사용한다. Ctrl-] 자체는 두 번 누르면0x1d 전송, prefix 후 잘못된 키는 로컬 도움말·미전송. line mode는 명시 선택이며 입력줄 편집과 전송 Enter의 eol을 헤더에 표시한다. raw mode Enter 기본 CR, arrows는 선택 terminal profile의 ANSI sequence, bracketed paste의 외곽 제어는 로컬 해석하고 paste 데이터만 정책에 따라 전송한다. 단말 API가 원래 바이트와 의미 키를 구분하지 못하는 조합은 capability로 표시하고 지원 시험을 한다. 콘솔 close 이벤트는 포트 close 대신 detach다.

스크롤/검색 중에도 수신·기록은 지속, 화면 `PAUSED + new 4520 bytes` 표시. timestamp toggle은 데이터 변형 없이 gutter만 바꾼다. hex는 offset/hex/안전 ASCII, text는 incremental decoder. 로그 export는 `Ctrl-] s` 로컬 메뉴 또는 `loom uart log export dut --format text --output ./dut.log --timeout 10s`; 사용자 로컬 파일 overwrite는 별도 선택이다.

터미널 출력은 장치 ANSI를 host terminal에 그대로 통과시키지 않는다. M1은 escaped text+기본 CR/backspace 처리, M2는 안전한 SGR/cursor subset을 화면 모델에 반영한다. OSC52(clipboard), OSC8 외부 링크, title/파일/장치제어 escape, BEL flood는 차단·가시화한다. rich terminal emulator(pyte 등) 선택은 §7의 추가 검증 gate를 통과해야 한다. raw log는 그대로 보존하고 원본/표시 차이를 알려준다.

실시간 목표 `[제안]`: RX host-read-complete→TUI stdout write p95<100 ms, p99<200 ms; TX accepted→화면 pending p95<100 ms, 실제 write 완료는 별도 상태. join 후 100행 스크롤백+라이브 시작 p95<500 ms. 계측 범위에 포함되지 않는 USB FIFO 이전 지연과 실제 디스플레이 scanout은 별도다. 921600 baud에서는 렌더30 Hz, viewport 최신행만 표시하고 중간 화면 생략 수를 공개한다. UTF-8/ANSI 파서와 기록은 화면 샘플링과 독립이며 parsing budget 초과 시 escaped-summary mode로 전환하고 알린다. 디스크/수신 drop을 화면 생략과 혼동하지 않는다. 사용자 입력 반응 p95<100 ms를 같은 부하에서 시험한다.

웹 뷰는 후순위다. 선택 시 읽기 전용 trace+수치 플롯부터, 같은 subscription으로 붙이고 xterm.js 등 브라우저 terminal의 escape 정책과 라이선스·버전을 별도 조사한다. 새 웹 서버가 별도 COM을 열지 않는다.

## 6. 데이터/기록 포맷

모든 session은 자동 기록한다. `[제안]` 세션 디렉터리 `manifest.json`, `raw.bin`, `events.jsonl`, 선택 index, export된 `text.log`. raw.bin에는 RX/TX 바이트를 append하고, JSONL에는 offset·길이·seq·시각·출처를 남긴다. TX intent만 있고 write 결과가 없으면 완료라고 해석하지 않는다. disk 정책은 처음부터 노출한다(기본 session 1 GiB rotation, 사용자 전체 quota20 GiB·free1 GiB 경고, 사용자가 정한 retention만 자동 삭제; 보존 실패 시 §3 degraded).

```json
{"schema":"loom.event/1","session":"u1","seq":"42","generation":1,
 "ts":{"mono_ns":"123000000","wall_ns":"1790000000000000000",
       "clock_id":"daemon-boot-8","kind":"host_read_complete"},
 "domain":"uart","channel":"dut","dir":"rx","origin":"device",
 "kind":"data","payload":{"encoding":"raw_ref","file":"raw.bin",
 "offset":"1024","length":18,"rx_offset":"320","request_id":null,
 "identity_id":"usb-ft123-if0"}}
```

| 필드 | 정의 |
|---|---|
| ts.mono_ns / wall_ns | read 완료 또는 write 결과 관측 시각. 정수는 JSON 문자열로 JS 정밀도 보존 |
| clock_id | monotonic은 데몬 재시작마다 다른 epoch; 다른 머신/프로세스 숫자를 직접 비교 금지 |
| domain/channel | uart / 사람이 붙인 채널명; manifest에 실제 path·USB 신원·설정 이력 |
| dir/origin | data는 rx/tx와 device/agent/user/script. origin_id는 인증된 client와 연결 |
| seq/generation | session 전체 이벤트 순서 / reconnect 횟수. raw RX byte offset은 별도 |
| payload | recording은 raw_ref, IPC/export JSONL은 base64. 길이는 디코드된 바이트 수 |
| kind | data, tx_intent, tx_result, state, gap, control, annotation |

공통 ts/domain/channel/dir/origin/payload를 유지한다. 상태 이벤트는 dir=null·origin=script를 제안하며 공통 계약 확정 필요(§15). RX raw에는 clock interpolation을 하지 않는다. CR/LF/CRLF와 불완전 줄을 보존하고, 파생 line text는 `first_seq,last_seq,start_rx_offset,end_rx_offset,encoding,decode_errors,partial`을 추가한다. line timestamp는 첫/끝 chunk 시각 두 개를 저장해 한 줄이 한 순간에 도착한 것처럼 보이지 않게 한다.

write ordering은 raw append→해당 offset의 JSONL 메타데이터 commit, 주기 flush·선택 fsync(기본1초)다. crash recovery는 JSONL의 유효 마지막 offset으로 잘린 raw tail을 판정하며 파일 회전은 pair 단위 manifest로 관리한다. 전원 차단까지 무손실을 기본 약속하지 않고 `durable_through_seq`를 보여준다. 디스크 읽기/압축은 별도 worker이며 live 수신을 막지 않는다. 3 Mbaud RX만 하루25.92 GB/포트(십진), full duplex51.84 GB이므로 quota/회전 없이 지속 기록을 약속할 수 없다.

보안 예외: 기본 원시 기록은 byte-exact지만, 명시적 secret TX span은 값 대신 길이·digest·redacted=true를 기록한다. secret echo RX는 알 수 없는 장치 출력을 자동 완전 마스킹할 수 없으므로 로그인 profile이 해당 수신 구간 기록 정책(일시 redaction·보호된 저장)을 명시해야 한다. 기록 누락을 raw-byte-exact라고 표시하지 않는다. 예외가 없는 RX 데이터는 항상 원문을 보존한다. session 파일 권한0600/사용자 ACL, 보존 정책은 사용자 환경에서 설정한다.

### viz 요구사항 초안

이 절만 리드가 `_xteam/viz-requirements.md` UART 절로 옮긴다. 타팀 파일은 직접 수정하지 않았다. 기존 UDS long-format 요구와 정합성을 맞춘다.

1. 로그 `temp=42.5 rpm=1200`에서 선언적 named regex와 단위 매핑을 적용해 숫자 series 생성. 임의 Python eval 금지, 매칭 시간·줄길이 상한. 예: `(?P<temp>-?\d+(?:\.\d+)?)`, unit=degC, source channel/seq/line offset 보존.
2. 출력은 `{"ts":...,"series":"dut.temp","value":42.5,"unit":"degC","quality":"valid","source_seq":"42"}` 형태의 long JSONL/CSV와 동일 live 이벤트. text parsing 실패·reconnect·gap은 NaN으로 이어 그리지 않고 quality/gap marker로 표현한다.
3. trace 표는 wall/상대시간·channel·origin·dir·text/hex·세대·송신 결과, plot 클릭 시 source_seq로 원문 찾기. device RX와 TX를 숫자 추출 대상으로 혼동하지 않도록 기본 RX만 처리.
4. 예시 향후 CLI: `loom uart extract ./dut-session --profile ./temp-rpm.yaml --output ./signals.jsonl --timeout 10s`, `loom viz show ./signals.jsonl --long --series series --y value`. 후순위지만 raw schema의 source 참조는 M1부터 예약.
5. decimation은 렌더층에서만, 원본·최대/최소와 gap 유지, live backlog·view drop 카운터를 표시. 브라우저는 같은 데몬 이벤트를 받아 장치를 다시 열지 않는다.

## 7. 라이브러리 선택과 근거

`[확인: 2026-11-09]` 아래 버전은 실제 **PyPI info.version의 안정판**, 날짜는 해당 버전의 **첫 artifact upload UTC**다. GitHub latest release나 최근 push 시각과 구분한다. 원본 요약은 [r1-sol-pypi-evidence.json](r1-sol-pypi-evidence.json). 유지보수 평가는 확인한 release/commit과 archived=false에서 추론했으며 안전성 보증이 아니다. GitHub push는 branch/tag/bot 변경일 수 있어 대표 branch 최근 commit도 따로 확인했다.

| 후보·역할 | PyPI 버전 / 날짜(UTC) | 라이선스 근거 | Windows 지원 | 유지보수 관측·판정 |
|---|---|---|---|---|
| [pyserial](https://pypi.org/project/pyserial/) / I/O | 3.5 / 2020-11-23 | BSD, 원문3조건 확인 | 공식 Win32 backend, macOS/POSIX | [repo](https://github.com/pyserial/pyserial) 대표 commit 2026-02-27, push 2026-05-19. 배포는 오래됐으나 곧바로 abandoned로 판정하지 않음 |
| serial.tools.list_ports | pyserial 3.5에 포함 / 같은 날짜 | pyserial 배포물, 별도 패키지 아님 | SetupAPI/registry, macOS IOKit, Linux sysfs | 같은 유지보수. VID/PID/serial/location은 None 가능 |
| [pyserial-asyncio](https://pypi.org/project/pyserial-asyncio/) / async | 0.6 / 2021-09-30 | BSD(PyPI), 정확 SPDX는 `[추정: BSD-3]` | 문서상 포함, polling으로 느릴 수 있음 | [repo](https://github.com/pyserial/pyserial-asyncio) 대표 commit2022-10-30/push2023-11-26. 배포·활동 모두 오래됨 |
| [pyserial-asyncio-fast](https://pypi.org/project/pyserial-asyncio-fast/) / 대안 | 0.16 / 2025-03-27 | BSD-3-Clause(PyPI SPDX) | 소스 os.name==nt polling 분기 확인, `[추정]` 최신 Windows 실사용 지원은 실측/CI 미확인 | [repo](https://github.com/home-assistant-libs/pyserial-asyncio-fast) commit/push2026-10-23. 활동은 개선, 이름만으로 고성능 판정 불가 |
| [Textual](https://pypi.org/project/textual/) / TUI | 8.2.8 / 2026-06-30 | MIT(PyPI/repo) | Win10/11, macOS/Linux classifiers | [repo](https://github.com/Textualize/textual) 대표 commit2026-10-09/push2026-11-09, 활동 있음. terminal emulator 자체는 아님 |
| [prompt_toolkit](https://pypi.org/project/prompt_toolkit/) / TUI | 3.0.53 / 2026-07-26 | PyPI BSD classifier, repo BSD-3-Clause | 공식 README에 Windows 지원·최근 Win10 VT100 권장 | [repo](https://github.com/prompt-toolkit/python-prompt-toolkit) commit/push2026-10-20. 활동 있음 |
| [urwid](https://pypi.org/project/urwid/) / TUI | 5.1.0 / 2026-11-06 | LGPL-2.1-only(PyPI SPDX) | README: Windows10+ **부분 지원**, fd/loop 제약·Python3.15 curses wheel 제약 | [repo](https://github.com/urwid/urwid) commit/push2026-11-09. 활동 높지만 제약 검증 필요 |
| [pexpect / fdpexpect](https://pypi.org/project/pexpect/) / expect | 4.9.0 / 2023-11-25 | ISC(PyPI) | spawn/run은 불가, PopenSpawn/fdspawn은 별도 조건 | [repo](https://github.com/pexpect/pexpect) commit/push2025-04-11. 안정적이지만 release 공백 큼, COM 직접 wrapper로 부적합 |
| [websockets](https://pypi.org/project/websockets/) / IPC | 17.4 / 2026-10-30 | BSD-3-Clause(PyPI/repo) | OS independent, Python≥3.11 | [repo](https://github.com/python-websockets/websockets) commit2026-10-30/push2026-10-31. 최근 배포·활동 |
| [aiohttp](https://pypi.org/project/aiohttp/) / IPC+HTTP | 3.14.5 / 2026-10-11 | PyPI **Apache-2.0 AND MIT**; GitHub 주면허만 Apache-2.0로 보임 | PyPI Windows/macOS/POSIX, Python≥3.10 | [repo](https://github.com/aio-libs/aiohttp) commit/push2026-11-09. 종속·번들 MIT 고지도 포함해야 함 |
| [FastAPI](https://pypi.org/project/fastapi/) / IPC+REST | 0.162.2 / 2026-11-07 | MIT(PyPI/repo) | OS independent, ASGI server 별도, Python≥3.10 | [repo](https://github.com/fastapi/fastapi) commit/push2026-11-08. 활동 높음, UART에 필요한 범위보다 큼 |
| [pyte](https://pypi.org/project/pyte/) / 후순위 화면 모델 | 0.8.2 / 2023-11-12 | PyPI license 문자열 공란. `[추정: 정확 면허 미확인]`, 채택 gate | 순수 Python 화면 파서라는 점은 확인, Windows QA는 `[추정]` | [README](https://github.com/selectel/pyte) VT 계열 화면 모델 확인; `[추정: 유지보수 상태]` 최근 commit은 미확인. M1 필수 아님 |

라이선스 판독 근거: [pyserial LICENSE](https://github.com/pyserial/pyserial/blob/a5c48d445fbc1943d4fabf8d9090a50fda3172fd/LICENSE.txt), 각 표의 PyPI와 repo. `NOASSERTION`/빈 license metadata는 무면허·public domain을 뜻하지 않는다. 최종 lock 때 **선택 버전의 sdist/wheel LICENSE 전체와 transitive notices**를 확인한다. 이 표는 공급망 감사 완료 선언이 아니다.

**선택 추천**: pyserial3.5 + bounded reader/TX threads + prompt_toolkit3.0.53 + aiohttp3.14.5, Python≥3.11/uv lock. M1 UI는 최소 prompt_toolkit 화면과 입력으로 핵심 루프를 우선한다. Textual은 풍부한 widget·mouse UX에 유리하지만 raw key 전달·렌더 부하 검증을 더 요구한다. urwid는 부분 Windows 지원·LGPL 배포 의무·loop 통합이 추가 비용이다. 개발 생산성 근거로 prompt_toolkit을 우선하되 raw Ctrl-C/Enter/arrows/paste가 Windows Terminal과 herdr에서 실패하면 Textual과 비교 스파이크 후 결정한다. 프레임워크 선택이 단말 에뮬레이터 기능을 자동 해결하지 않는다.

async 대안 비교: POSIX에서는 pyserial-asyncio/fast가 add_reader 기반이라 포트 수가 많을 때 thread 감소 이점이 있다. Windows는 둘 다 polling 경로를 가지며, fast 소스의0.5 ms 예약값이 OS에서0.5 ms 지연을 보장하지 않는다. [공식 asyncio 설명](https://pyserial-asyncio.readthedocs.io/en/latest/), [fast 구현](https://github.com/home-assistant-libs/pyserial-asyncio-fast/blob/a658ed8c16c4d3c28351e9e13cba5e629e849975/serial_asyncio_fast/__init__.py). 한 백엔드로 3 OS를 먼저 증명하고 M3에서8포트 CPU/latency를 측정해 async 전환을 결정한다. pyserial3.5의 Python3.11+ 조합은 오래된 metadata만 믿지 않고 CI한다.

pexpect는 비교·가짜 쉘 테스트에 활용 가능하지만 production RX consumer로 채택하지 않는다. `[확인]` [Windows 문서](https://pexpect.readthedocs.io/en/stable/overview.html#pexpect-on-windows)는 PopenSpawn/fdspawn의 조건부 사용을 설명한다. 이것이 Win32 COM handle을 POSIX fd처럼 select할 수 있다는 뜻은 아니다. [fdpexpect 소스](https://github.com/pexpect/pexpect/blob/fc8f062518b40bd0862aae870cdedf5d9c0c7fc3/pexpect/fdpexpect.py)는 os.read/select와 platform 조건에 의존한다. 별도 reader가 같은 COM을 읽으면 fanout 계약도 깨진다. expect 의미론만 데몬 공통 ring 위에서 구현하는 편이 안전하다.

## 8. HW/OS 지원 매트릭스

| 항목 | macOS | Linux | Windows |
|---|---|---|---|
| 물리 포트 | `/dev/cu.*` 우선, paired tty 중복 제거 | `/dev/serial/by-id/*` 있으면 우선, ttyUSB/ttyACM fallback | COM3, COM10+를 pyserial에 전달 |
| 탐색 | list_ports IOKit | list_ports sysfs, udev symlink | list_ports SetupAPI/registry |
| 점유 | pyserial exclusive=flock 보조 | 같은 POSIX 제약 | COM 기본 배타 점유, 다른 terminal 열려 있으면 PORT_BUSY |
| 권한 | 장치 node/driver 권한 | dialout 등 distro 정책, permission 오류 안내 | 사용자 장치 접근/driver 필요 |
| 기본 HW 후보 | USB CDC·FTDI·CP210x·CH34x | 동일, kernel driver 지원 전제 | vendor signed driver/CDC 전제 |
| 921600 / 3 Mbaud | adapter/driver별 수용 시험 | adapter/driver별 수용 시험 | adapter/driver별 수용 시험 |
| PTY CI | os.openpty/socat | os.openpty/socat | 지원하지 않음; fake byte backend/TCP fixture |
| BREAK·DTR·RTS | ioctl, 물리 신호 시험 필요 | ioctl, 물리 신호 시험 필요 | Win32 API, driver/장치별 물리 시험 필요 |

위 HW 가족은 `[추정: 지원 후보]`다. 실제 보드/driver/OS 조합을 아직 시험하지 않았으므로 무조건 지원 표시를 하지 않는다. `[확인]` [Apple 공식 안내](https://developer.apple.com/library/archive/documentation/DeviceDrivers/Conceptual/WorkingWSerial/WWSerial_SerialDevs/SerialDevices.html)는 일반적으로 callout `/dev/cu.*`를 사용하고 dial-in `/dev/tty.*`는 incoming-call 용도라고 설명한다. cu/tty는 같은 장치의 두 면이므로 동시에 열어야 하는 별개 포트가 아니다. Linux [systemd serial rules](https://github.com/systemd/systemd/blob/main/rules.d/60-serial.rules)는 USB serial 정보와 interface 번호에 따라 by-id/by-path를 만든다. 모든 시스템에 by-id가 존재하는 것은 아니다.

### 재연결과 신원

`[확인]` [list_ports 문서](https://pyserial.readthedocs.io/en/latest/tools.html)는 USB의 VID/PID/serial_number/location 및 OS별 열거를 제공한다. 이 정보가 모든 장치에서 채워지거나 고유하다는 보장은 없다. `description`은 화면용이며 신원 키로 신뢰하지 않는다.

추천 매칭: (1) VID/PID+serial_number+interface 번호/사용자 지정 channel, (2) serial 없는 경우 VID/PID+USB location을 사용자가 사전 허용한 경우만, (3) 둘 다 없거나 후보2개 이상이면 reconnect 대기·수동 bind. 같은 serial이 복제된 adapter도 ambiguity로 다룬다. interface 문자열만으로 다중 UART 채널이 구분되지 않으면 OS backend의 interface/port index 보강이 필요하며 미확인 필드는 null로 표시한다. location은 같은 물리 USB topology를 뜻하므로 hub 이동 시 달라지고 동일 장비의 영구 ID로 쓰면 안 된다.

path(COM3/ttyUSB0)는 unplug/replug 시 바뀌거나 다른 장치에 재할당될 수 있다. path 일치만으로 다시 쓰지 않는다. 1초 주기 열거·exponential backoff0.25→0.5→1→2→5초, 기본2분 대기 후 stopped, 사용자가 extend 가능. 상태는 OPEN→DISCONNECTED→MATCHING→REOPENING→OPEN(generation+1)/AMBIGUOUS/STOPPED. OS hotplug hook은 후순위 최적화다.

RX 관찰의 자동 재연결은 사전 허용된 exact identity에만, 재open도 DTR glitch 위험이 있으므로 reset-sensitive profile은 자동 재open을 끈다. TX lease·미완료 send·bootloader approval은 세대 변경 시 취소한다. 앱→부트로더로 VID/PID가 바뀌는 장치는 명시 identity alias profile+위험 승인 없이는 다른 장치로 취급한다. baud/flow는 검증된 기존 profile만 복원하고 control sequence·명령 문자열을 자동 replay하지 않는다.

### 라인 신호와 흐름제어

`[확인]` [pyserial API](https://pyserial.readthedocs.io/en/latest/pyserial_api.html)는 높은 baud가 많은 장치에서 동작하나 비표준 값 거절 가능, DTR/RTS open glitch, 일부 플랫폼 dsrdtr 무시, xonxoff+rtscts 동시 사용 제약, POSIX1.5 stop bit→2 fallback을 명시한다. 따라서1.5 stop은 Windows만 후보, POSIX에서는 unsupported로 사전 거절하여 조용한 변환을 막는다. 설정 성공 반환과 실제 baud/waveform 정확도는 별도다.

BREAK는 UART TX를 spacing 상태로 유지하는 신호이며 `0x00` 송신과 다르다. POSIX send_break는 termios/driver별 길이 해석 차이가 있고 Windows는 SetCommBreak/ClearCommBreak를 사용한다. `break_condition`으로 on/sleep/off해도 Python scheduler·USB 지연 때문에 hard realtime이 아니다. finally에서 해제하되 device 분리/driver 실패는 unknown으로 표시한다. 본 스파이크에서20 ms 요청이403 ms 걸렸으므로 ms 단위 부트 타이밍에 API 이름만 믿지 않는다.

RTS/DTR의 true는 API active 상태이며 보드 active-low reset과 실제 전압 극성은 profile에서 정의한다. open 전에 원하는 상태를 지정해도 driver가 순간 assert할 수 있다. MCU USB CDC 가상 UART는 control request를 reset/1200-baud touch로 해석하기도 하므로 모델별 조사 없이 안전하다고 가정하지 않는다. 루프백 dongle/logic analyzer로 open/close/reconnect/pulse를 검증한다.

flow 기본 none. RTS/CTS는 실제 배선·driver 지원·상대 장치 설정이 맞을 때만, XON/XOFF는0x11/0x13이 데이터와 충돌하므로 binary session에 기본 금지한다. 둘을 동시에 켜지 않는다. CTS low로 TX가 멈추면 write_timeout과 pending byte count를 보여주고 무한 flush하지 않는다. DSR/DTR flow는 초기 비범위로 두고 수동 DTR과 의미 충돌을 피한다.

## 9. 시뮬레이션 & 테스트 전략

### R1에서 실제 수행한 짧은 검증

[pty_probe.py](../../../spikes/uart/pty_probe.py)와 [실행법/결과](../../../spikes/uart/README.md). 환경은 macOS27.0.1 arm64, Python3.15.0, pyserial3.5. 전역 설치 없이 `spikes/uart/.venv`에만 설치했다. os.openpty slave를 pyserial로 열고0x00–ff 양방향 각256바이트 일치, 두 번째 exclusive open Errno35,4 MiB 수신 hash 일치(2.2499초,1.78 MiB/s)를 확인했다. send_break(20 ms)는 API 성공이나403.190 ms 소요, break on/off 합3.751 ms, DTR/RTS는 각각 Errno25였다.

첫4 KiB 동기 master write는 reader 시작 전에 PTY 버퍼에서 대기했다. 해당 실험 프로세스를 종료하고 작은 왕복 블록256B 및 bulk nonblocking producer/select로 수정해 통과했다. 이를 “실제 UART 입력을 언제나 동기 write해도 된다”는 근거로 쓰지 않는다. PTY 속도는 baud와 무관하고, 데몬/IPC/화면/다중 포트 부하는 아직 측정하지 않았다.3 Mbaud 물리 지원·DTR/RTS·Windows·Python3.11 지원 검증으로 확대 해석하지 않는다.

### 가짜 장치 설계(향후 구현)

M1 fixture는 `os.openpty()` master 보유, slave를 실제 pyserial 어댑터에 전달한다. raw mode/echo off로 tty line discipline이 CR/LF·Ctrl-C·NUL을 변조하지 않도록 한다. stdlib openpty는 master/slave 한 쌍이며 두 프로그램이 slave 두 개를 필요로 하면 socat으로 두 PTY를 bridge한다.

```sh
# 선택적 개발 도구. CI 필수 의존성/전역 설치 요구는 아님.
socat -d -d pty,raw,echo=0,link=/tmp/loom-uart-host \
               pty,raw,echo=0,link=/tmp/loom-uart-device
# 두 경로가 콘솔에 출력됨. host를 데몬, device를 가짜 장치가 사용.
```

fixture 상태기: POWER_ON→부팅3줄(ANSI/CR 혼합)→`login:`→`Password:`→`dut>`; `status`는 temp/rpm, `monitor`는 주기 sample,03은 `interrupted`와 prompt, `binary`는00..ff. seed 기반 delay/chunk 분할, UTF-8 중간 분할, no-newline prompt, 긴1 MiB line, ill-formed ANSI, echo on/off, noise·timeout·master close·새 PTY path를 주입한다. 재연결 identity는 fake enumerator로 별도 주입한다. PTY 자체에는 실제 USB serial/location이 없으므로 열거 재연결 시험과 구분한다.

Windows 일반 CI는 **driver 없는 in-memory byte transport + 동일 IPC/CLI/TUI headless harness**를 필수로 돌린다. pyserial loop://는 byte loop 단위 검증, socket://와 localhost TCP 가짜 장치는 선택적 cross-platform serial API 검증으로 쓰되 실제 COM 기능을 검증했다고 하지 않는다. [com0com](https://sourceforge.net/projects/com0com/)은 Windows kernel virtual pair, GPLv2, 프로젝트 페이지 last update2018-04-09를 확인했다. Win11 Secure Boot/HVCI·driver signature 호환은 **미확인**이다. 공유 CI에 테스트 서명·Secure Boot 해제를 요구하지 않고 사전 설치된 전용 Windows runner에서 선택 테스트만 한다. 상용 signed virtual COM 대안은 조직 승인·가격·호환 확인 후 검토, 기본은 드라이버 없는 fake backend다.

| 시험 층 | HW 없는 필수 검증 | 선택 HW gate |
|---|---|---|
| 핵심 루프 M1 | PTY open→agent send→device reply→같은 attach 표시→user Ctrl-C, 뒤늦은 attach snapshot/live seam | Windows/macOS/Linux 실제 USB UART 각각1종 |
| byte/encoding |00..ff, CR/LF, ANSI·UTF-8 split, 파일 hash, no-newline prompt | RX/TX physical loopback |
| 중재·cursor | 사용자 미완성 입력 중 agent 거절, takeover partial, 두 agent 독립 cursor, 만료 GAP | 실제 장치 쉘 개입 |
| 실패 | full disk·slow subscriber·writer queue overflow·프로세스 crash·불완전 raw tail | 케이블 pull/replug, 이름 변경, 중복serial |
| 부하 | fake921600 pacing10분, 3M pacing·8 sessions·4 subscribers 각10분, view drop과 원본 hash 비교 |921600/3M loopback 장치+driver별10분 |
| 신호 | unsupported 오류와 제어 actor 순서, duration 요청과 결과 기록 | logic analyzer로 BREAK/DTR/RTS/open glitch·CTS stall |

CI는 Python3.11과 채택 최신 안정버전, macOS/Linux/Windows matrix. macOS/Linux PTY,3 OS fake backend·IPC·JSON 출력 상한·보안 테스트. TUI는 가짜 output sink에 쓰기 완료 시각을 계측하고 실제 herdr/Windows Terminal 키 매핑·리사이즈·detach smoke를 추가한다. latency는 RX stamp와 UI paint/write ack의 동일 clock domain 또는 offset을 보정한 timestamp로 계산하며, 임의 프로세스 monotonic 값의 차를 그대로 신뢰하지 않는다. 실제 화면 가시성은 수동 smoke/화면 캡처 타이밍과 함께 확인한다.

## 10. 안전 / 가드레일

| 등급 | 예 | 정책 |
|---|---|---|
| R0 관찰 | ports/read/expect/attach watch/export | 기존 사용자 요청으로 진행, bounded·비밀 보호 |
| R1 통상 입력/연결 | 보드 profile open, 승인된 status/help, 사용자 일반 키 | 세션/profile에 사전 허용된 범위, rate 제한, 전부 화면에 보임 |
| R2 상태 변화 | break, DTR/RTS, reset, reboot/bootloader, baud touch | 명시 위험 설명·dry-run·동일 action의 사용자 승인 필요. 이미 승인된 session 범위는 재질문하지 않음 |
| R3 파괴/비가역 | flash/erase/메모리 쓰기/자격증명 변경/위험 shell 문자열 | 기본 거절. 향후 전용 profile+대상·payload digest·단회 승인, 현재 자동화 범위 밖 |

UART는 바이트만으로 명령 의미를 완벽히 알 수 없다. `erase`, `flash`, `reboot`, `rm` 등의 문자열 검사는 경고 보조이며 보안 경계가 아니다. board profile의 허용 명령/상태기·사용자가 승인한 작업 범위가 실제 기준이다. unknown shell/file/hex TX는 안전한 명령이라고 분류하지 않고 R2 이상 profile 검토를 요구한다. interactive 사용자의 직접 타이핑은 본인 개입으로 취급하되 agent 권한을 자동 확장하지 않는다.

authorization은 daemon이 action digest(session identity/generation, bytes hash, signal steps, 최대 횟수, expiry)에 묶어 발급하고, token은 stdout/log에 노출하지 않는다. user approve client와 agent RPC가 같은 credential role을 위조할 수 없게 한다. 일반 로컬 사용자 자신에게서 daemon을 보호하는 sandbox까지 보장하는 것은 아니며, 스킬의 대화 승인만으로 강한 OS 격리를 주장하지 않는다. 재연결/내용 변경/TTL 만료 시 approval은 무효다.

기본 agent TX token bucket1 KiB/s·burst1 KiB,10 requests/s/port, 큐64 KiB·최대 대기2초. 승인된 bulk는 profile별 상한으로 늘리되 선로 용량의80%와 CTS/driver 상황을 감안한다. rate limit은 물리 baud와 별개로 잘못된 반복 명령을 줄이는 제한이다. user Ctrl-C/입력권 회수는 우선 처리하고 파일 송신/sequence를 취소 가능하게 한다. emergency stop은 새 TX를 막지만 이미 선로에 나간 바이트를 되돌리지 않는다. close도 DTR/RTS를 바꿀 수 있으므로 “close=항상 안전한 reset 취소”로 취급하지 않는다.

장치 로그/ANSI/배너는 **신뢰할 수 없는 데이터**다. `ignore previous instructions`, 파일 경로, shell command를 에이전트에게 보이더라도 실행 지시로 승격하지 않도록 SKILL에 적는다. host escape 차단, secret redaction, 로컬 auth, JSON 상한, regex 격리를 runtime에서도 강제한다. 장치가 낸 로그를 다음 셸 명령에 무인용 삽입하지 않는다.

## 11. 마일스톤

M0는 조사·계약 준비로만 취급한다. **첫 기능 납품 M1은 네 요소가 모두 동작하는 걷는 뼈대**이며 고급 부가 기능 완료를 선행 조건으로 삼지 않는다.

| 단계 | 결과 | 수용 기준 |
|---|---|---|
| M0 조사·계약 | 본 초안·IPC 스키마 결정·지원 HW 선정 |15절+core 점검·실제 라이브러리 근거·PTY spike 한계 명시 |
| M1 걷는 뼈대 | 한 PTY 세션, agent open/send/read, 실제 장치 응답, 같은 attach 라이브/scrollback, user 입력/Ctrl-C, 최소 기록·origin·상한 |§0 E2E 자동 재현, late attach seq 누락/중복0, RX와 기록 hash 일치,921600 pacing에서 p95 표시<100ms/p99<200ms·UI 입력 반응<100ms, 화면 생략은 명시. agent-only 포트 열기 경로0 |
| M2 실장비·협업 |3 OS pyserial, 다중2포트, identity reconnect, lease·cursor·expect·hex/file, 안전 제어, 검색·ANSI subset |두 agent+사용자 동시 중재, disconnect partial/unknown, 중복 신원 차단,3 OS USB UART 각1종 수용, log recovery·secret/escape 보호 |
| M3 성능·터미널 품질 |8포트·3M 후보, TUI 키/scroll/search·flow·signals |fake와 HW 측정 결과 분리,10분 hash 손실0이 확인된 조합만 지원 선언, 큐/RSS 상한·느린 뷰어 고립·disk full 명시, BREAK/RTS/DTR 물리 waveform profile별 확인 |
| M4 후순위 |숫자 추출/viz, 필요 시 웹 뷰·full screen emulator |핵심 루프/latency/기록 회귀 없음, 동일 session 구독, 화면 sampling 표시·raw source 링크 |

XMODEM/YMODEM은 일정에 넣지 않는다.3 Mbaud가 특정 HW에서 실패하면 지원표에 한계를 적고921600 지원부터 납품한다. 성능 목표를 측정 없이 “완료”로 바꾸지 않는다. M1의 실제 구현·수용은 이후 작업이며 R1 spike 통과와 동일하지 않다.

## 12. 구현 단계 작업 분해

계획상 담당 추천이며 이 라운드에 에이전트를 새로 실행하거나 제품 코드를 작성한 것은 아니다. 공통 IPC/이벤트/권한 scaffold 합의 이후 포트 어댑터·가짜 장치·TUI를 병렬로 만들고 M1에서 반드시 합친다.

| ID | 작업/산출 | 추천 담당 | 의존성 | 병렬성/완료 증거 |
|---|---|---|---|---|
| C0 |공통 envelope·session/seq·IPC auth/원점·visibility 계약 | Opus 워커 |메인 공통 결정 |Sol과 API stub 합의, schema fixture |
| S1 |fake boot-shell/PTY·byte fixture·seed·latency 계측 | Sol 워커 |C0 최소 envelope |S2/O1과 병렬, send/response/Ctrl-C fixture |
| S2 |pyserial owner·reader bridge·TX actor·bounded ring·최소 raw 기록 | Sol 워커 |C0 |S1/O1과 병렬, 데이터 hash·상한 |
| O1 |최소 finite CLI+attach live/late snapshot·origin·사용자 입력 | Opus 워커 |C0 |S1/S2 mock 기반 병렬, 실제 키 입력 sink |
| I1 |M1 E2E 통합·921600 폭주·late attach seam 검증 | Sol 워커 주, Opus 워커 리뷰 |S1,S2,O1 |4요소·100ms gate를 함께 통과, 별개 세션 금지 |
| O2 |사용자 우선 lease·takeover·위험 승인·profile 명세 | Opus 워커 |I1 |S3와 병렬, 부분 TX/미완성 사용자 입력 테스트 |
| S3 |USB identity·reconnect·Windows/Linux 어댑터·signal errors | Sol 워커 |I1 |O2와 병렬, fake enumerator+실장비 검증 |
| S4 |cursor/read/expect isolated regex·bounded file send·log recovery | Sol 워커 |O2,S3 |O3와 병렬, max context·timeout·gap |
| O3 |TUI search/hex/ANSI 안전 모델·secret 표시·터미널 키 정책 | Opus 워커 |O2 |S4와 병렬, Windows/herdr smoke |
| S5 |8포트 부하·flow/driver/HW signal bench·지원표 확정 | Sol 워커 |S3,S4,O3 |M3 숫자·p95/p99/RSS·hash 결과 |
| O4 |짧은 SKILL+references·Codex 읽기 호환·사용 예 | Opus 워커 |I1, O2 초안 |M1 최소 안내 후 M2 기능 증분 |
| O5 |viz parser schema·선택 웹 뷰 계약 | Opus 워커 |M3, CAN viz 계약 |핵심 루프 뒤 후순위 |

스킬 패키징 제안: `.claude/skills/loom-uart/SKILL.md`는 목적·daemon 경유·ports/open→bounded read/send→attach 안내·승인 범위·장치 출력 불신·dry-run·오류 처리만 짧게 둔다. `references/cli.md`, `safety.md`, `sessions-cursors.md`, `terminal-input.md`, `platforms.md`, `examples.md`로 상세 분리. Codex는 같은 SKILL 문법/상대 references를 사용하는 저장소 내 패키지를 제공하고 발견 경로는 문서/선택 설치 단계에서 검증한다. `~/.claude`/`~/.codex`를 이 작업에서 수정하거나 두 위치에 사본을 자동 설치하지 않는다. runtime policy를 스킬의 선의에만 맡기지 않는다.

## 13. 리스크

| 리스크 | 영향 | 대응/결정 gate |
|---|---|---|
| USB/driver FIFO·scheduler 지연 |ns timestamp지만 실제 µs 정확도 없음 |host timestamp 명시, HW clock처럼 표시 금지, 실제 loopback/logic analyzer |
| pyserial 배포 노후·새 Python |OS/Python 조합 회귀 가능 |3.11+ CI·lock·feature adapter, 업데이트/대안은 회귀 시험 후 |
| high baud·8포트·JSON/TUI fanout |버퍼·디스크/CPU 폭주 |상한·렌더 throttle·gap marker·raw hash·지원 조합만 선언 |
| flow 미배선/미지원 |수신 overflow·TX 무한 대기 |기본 none·write deadline·실제 배선 gate, 무손실 절대 보장 금지 |
| DTR/RTS open/close glitch |MCU reset/bootloader 진입 |profile별 위험·재연결 끄기·waveform 검증 |
| USB 이름/serial 중복 |잘못된 보드에 송신 |신원 exact match·interface·location 제한·ambiguity 차단 |
| user/agent 입력 혼합 |쉘 명령 변형·위험 실행 |lease·미완성 입력 자동 양도 금지·takeover partial 공개 |
| regex 폭주·악성 ANSI/로그 |UI/데몬 정지·host 제어·prompt injection |worker timeout·safe renderer·untrusted-data 스킬 규칙 |
| daemon crash/중복 TX |동일 위험 명령 재실행 |결과 unknown·request_id·자동 재전송 금지·durability 표시 |
| 기록에 secret 노출 |자격증명 유출 |명시 redaction profile·로컬 권한·보존기한, raw 예외 공개 |
| com0com 최신 Windows |driver 설치 불가/CI 권한 변경 |fake backend 필수·전용 runner만 optional |
| raw terminal key 지원 차이 |Ctrl-C가 로컬 종료·키 손실 |herdr/Windows Terminal 실제 smoke, prefix detach와 안전 restore |
| 뷰어가 로그만 보여 핵심 루프 누락 |제품 목적 실패 |M1 E2E·late join·사용자 개입을 출시 gate로 유지 |

## 14. 사용자에게 물어볼 질문

R1에서는 답을 기다리며 멈추지 않고 아래 기본값으로 계획한다. 실제 연결/위험 동작을 할 때 대상 장치와 승인 범위는 필요하다.

| 미결 질문 | 대안 | 기본 추천과 이유 |
|---|---|---|
|첫 HW/OS·USB adapter는? |Windows 실장비 우선 / macOS 개발 우선 |macOS PTY 개발+Windows11 실제 USB UART 병행, driver별 지원표 |
|목표 baud·포트 수는? |115200/단일 /921600·다중 /3M·8포트 |M1 단일921600 시뮬레이션, M2 두 HW 포트, M3 3M·8포트 실측 |
|장치 shell eol·encoding은? |CR/LF/CRLF, UTF-8/legacy |send 기본 none, 사용 예는 명시 CR, UTF-8 표시·원본 bytes 보존 |
|user/agent 입력권은? |자동 idle 반환 /명시 yield |명시 yield, 미완성 명령·raw screen에서 안전 |
|리셋 민감·DTR 부트 기능이 있나? |일반 adapter /reset-sensitive |unknown 보드는 reset-sensitive로 profile 확인, 자동 재open off |
|기록 유지·민감정보 정책은? |무제한 /rotation+quota /secret redaction |1GiB rotation·20GiB quota 예시, 명시 retention·secret profile 적용 |
|터미널 full-screen 앱이 필수인가? |line shell/부팅 로그 /vi·menu 등 |M1 line/raw 입력·기본 ANSI, full-screen은 별도 수용 후 M3/M4 |
|웹/XMODEM을 먼저 원하는가? |터미널 핵심 /웹·파일 프로토콜 우선 |핵심 루프 우선, 웹/전송 프로토콜 후순위 |
|agent 허용 명령 범위는? |읽기만 /status+help /reset·flash |R0+명시 status/help profile부터, reset별 승인·flash 비범위 |

## 15. 공통 계약 변경 요청 / 타팀 의존성

1. **IPC**: JSON-RPC2.0 over WS+aiohttp를 추천하되 메인 결정을 따른다. UART/CAN/UDS 공통 `session_id,seq,request_id,actor_id,generation,capabilities,error,gap` envelope·구독 resume·최대 message·인증을 C0에서 확정 요청. 요청 응답과 이벤트 구독을 모두 제공하고 웹 direct subscribe를 가능하게 한다. UART 별도 숨은 socket/reader는 만들지 않는다. 읽은 [CAN R1 Sol](../can/r1-sol.md) §3.4와 [UDS R1 Sol](../uds/r1-sol.md) §4도 loopback JSON-RPC/WS·인증·origin 검증을 제안하므로 방향은 일치한다. 다만 UART 예시의 `session`/`generation`/`origin_id`와 타팀의 `session_id`/`connection_epoch`/`actor_id`, 문자열 seq와 UDS 예시의 숫자 seq는 아직 wire 호환이 아니다. 공통 schema 확정 때 동일 이름·타입으로 바꾸며 그 전까지 호환 완료라고 선언하지 않는다.
2. **기록**: 공통 ts에 mono_ns/wall_ns/clock_id/kind를 명시, uint64/나노초는 JSON 문자열 제안. payload는 base64/raw_ref 타입, state/control/gap 레코드에 dir=null·origin=script 또는 core origin 확장을 결정 요청. UART는 per-byte hardware timestamp를 제공하지 않으므로 UDS/CAN HW 시각과 quality를 구분한다.
3. **안전·가시성**: core가 user/agent role·action authorization·durable watermark·slow subscriber 정책을 공통으로 제공하면 중복 구현을 줄인다. 정상 read/open을 매번 재승인시키는 흐름은 피하고 이미 허용한 범위를 재사용한다. R2/R3만 세대·내용·기한에 묶인 승인으로 관리한다.
4. **viz**: §6의 “viz 요구사항 초안”을 리드가 CAN 소유 문서로 전달. 기존 UDS의 long JSONL series/value/unit 구조와 같은 숫자 자료형·시각·gap quality를 쓰되 UART raw source_seq를 추가한다. UART가 CAN GUI를 따로 만들지 않는다.
5. **CAN/UDS 정합성**: 읽은 `_xteam/can-uds-isotp.md`는 시간 민감 처리를 데몬 안에 두고 결과를 이벤트화한다. UART 역시 RX/TX/신호 순서를 소유 데몬에서 처리하며, CLI round-trip에 BREAK 정확도나 바이트 시각을 의존하지 않는다. ISO-TP/UDS 엔진은 UART 범위 밖이다.
6. **프로세스·패키지**: `loom.core/uart/can/uds/viz`, Python≥3.11, uv 계약 유지. 기본은 단일 사용자 daemon 내 UART 모듈이다. CAN vendor driver가 별도 worker process를 필요로 해도 UART 물리 handle 소유자는 하나다. herdr는 단순 TTY 환경으로 가정하며 독점 API 의존성 없이 attach한다.
7. **이번 라운드 쓰기 범위**: UART 초안·근거 JSON과 `spikes/uart/`만 작성했다. 다른 팀/`_xteam`/사용자 설정 파일을 수정하지 않았다. STATUS는 리드 책임이므로 워커가 DONE으로 덮어쓰지 않는다. R1 완료는 계획 완료이며 제품 구현 완료가 아니다.

## 핵심 원칙 자체 점검

| core 체크리스트 | 충족 여부 | 위치·검증 내용 |
|---|---|---|
|앞에 핵심 루프4요소 표 |충족(계획) |§0 첫 절: 직접 접근/읽기/쓰기/사용자 실시간 표시·개입과 M1 증거 |
|구체 E2E 명령·화면·사용자 개입 |충족(계획) |§0 status→reply→표시 지연 예시→monitor/Ctrl-C, §5 origin ASCII |
|실시간 목표·고부하에도 사실 표시 |충족(계획, 성능은 미검증) |§3 bounded queues·fanout·기록, §5 p95<100ms/30Hz·화면 생략/RX gap 분리, §9 부하 시험 |
|M1=시뮬레이션 위4요소 걷는 뼈대 |충족(계획) |§1 M1 범위, §11 M1 acceptance, §12 I1 통합 gate. 고급 기능 이후 |
|동일 session·즉시 attach·숨은 포트 금지 |충족(계획) |§0 우회 금지, §3 단독 handle, §4 snapshot→live seam, §5 alias→session 결합 |
|핵심에 기여하지 않는 기능 후순위/비범위 |충족 |§1 XMODEM/YMODEM·웹·고급 hex 후순위/비범위, §11 M4 viz/웹. 핵심 루프를 선행 |

이 점검은 계획의 충족 여부이며 실행 수용은 M1 이후 별도로 통과해야 한다. 현재 실제 실험 결과는 §9/스파이크 README에 한정된다.
