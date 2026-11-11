# CAN + 범용 시각화 R1 독립 초안 — can-sol

작성일: 2026-11-09, 한국어 독립 계획 초안. 구현 승인이나 완성된 제품 명세가 아니다.
우선순위: `02-core-principle.md` > `r1-addendum-core.md` > 공통/컴포넌트 브리프.
공통·CAN·UART·UDS 브리프와 `_xteam`의 현재 두 초안을 읽었다.
다른 워커의 R1 파일은 읽지 않았고, 다른 팀 파일은 수정하지 않았다.
이 문서의 수치는 **실측**, **산술 추정**, **수용 목표**, **미확인**을 구분한다.
근거용 코드·원시 결과는 `spikes/can/`에만 두었다. 제품 구현은 시작하지 않았다.

## 0. 핵심 루프

**결론: 에이전트와 사용자가 하나의 CAN 세션을 공유하고, 같은 프레임을 라이브로 본다.**
에이전트 직접 접근은 CLI가 소유 데몬에 명령하여 실제 인터페이스를 여는 것을 뜻한다.
사람이 CANoe를 먼저 열어 주거나 로그를 복사해 주어야 하는 구조를 기본값으로 삼지 않는다.
데몬 경유는 장치 소유권·타이밍을 위한 것이며, 에이전트 전용 숨은 연결을 만들지 않는다.

| 핵심 요소 | CAN에서의 구현 | 확인 가능한 결과 |
|---|---|---|
| 직접 접근 | `loom can open --backend vector --app Loom --channel 0 ...`; CAN worker가 XL/PCAN/SocketCAN 핸들 소유 | 장치 ID·실효 설정·세션 ID가 CLI와 뷰어에 동일하게 표시 |
| 읽기 | `loom can trace --session s1 --tail 100 --json`, `signal get/wait-for` | 같은 `session_id/seq`를 가진 RX/TX·사용자 프레임까지 반환 |
| 쓰기 | `loom can send --session s1 --id 0x100 --data ...`; 공통 TX 중재기 | `origin=agent`, 요청/접수/확인 상태를 사용자 Trace에 표시 |
| 사용자 실시간 표시·개입 | `loom can attach s1` 또는 `loom viz live --session s1`; TUI 송신창/웹 IG | agent/user/device를 색+문자로 구분하고 user 송신을 에이전트도 읽음 |

M1은 virtual 위에서 이 네 요소가 모두 동작하는 walking skeleton이다.
M0은 문서·스키마·시험 준비일 뿐 별도 제품 기능 마일스톤으로 세지 않는다.
사용자가 나중에 뷰어를 열어도 같은 세션의 최근 버퍼와 이후 스트림을 즉시 받는다.
뷰어가 닫혀도 기록과 세션은 유지되며, 사용자의 접속은 추가 장치 핸들을 열지 않는다.
파일 변환, 고급 DBC, 멋진 플롯, CANoe 제어는 이 루프 이후다.

## 1. 범위 / 비범위

### 1.1 결정

- 패키지는 Python ≥3.11의 `loom.can`, `loom.viz`, 공통 `loom.core`를 사용한다.
- 하나의 사용자 진입점 `loomd`가 도메인 worker를 감독하는 구조를 제안한다.
- CAN 인터페이스는 CAN worker 한 프로세스만 소유한다.
- ISO-TP FC/CF 상태기는 그 CAN 프로세스 내부에 둔다.
- 대용량 DBC decode, 기록 변환, 다운샘플링, HTTP/WS UI는 별도 프로세스로 분리한다.
- CLI·TUI·웹은 같은 권한 검사·TX 중재기·이벤트 원장을 사용한다.
- 핵심 실HW는 Windows Vector XL/PCAN, Linux SocketCAN이다.
- macOS 개발·시뮬레이션은 정식 지원, macOS PCAN 실HW는 조건부 지원이다.
- 웹은 장기 주 뷰어, M1 최소 라이브 화면은 구현 부담이 작은 TUI로 먼저 제공한다.
- CANoe 전체 대체가 아니라 지정한 Trace/Graphics/IG/Statistics/Replay 작업 수준을 목표로 한다.

### 1.2 M1 필수

1. virtual 세션 열기/닫기, 단일 프레임 송신, 경계 있는 트레이스 조회.
2. 같은 세션의 라이브 TUI와 사용자 직접 송신.
3. 가짜 ECU 응답, 출처·순서·세션 표시, 늦게 붙은 뷰어의 버퍼 복구.
4. 자동 기록, 화면 생략/기록 손실의 명확한 분리, 송신 정지.
5. 인증된 loopback IPC와 입력 검증, 재연결 시 자동 재송신 금지.

### 1.3 M1 이후 범위

- CAN CC 11/29bit, CAN FD/데이터 비트레이트/BRS, 다채널.
- DBC 심볼릭 decode/encode, 값 테이블, multiplexing, 신호별 주기 송신 갱신.
- Trace, Graphics, Data/State, IG, Statistics, 파일 Replay.
- BLF/ASC 입출력, 선택 의존성에 의한 MF4 입출력.
- CSV/JSONL/Parquet와 CAN 파일·라이브 이벤트를 받는 범용 `loom.viz`.
- CAN-TP PDU 서비스와 UDS 팀 어댑터. UDS 서비스 의미는 UDS 팀 소유.
- ARXML의 검증된 CAN 데이터베이스 부분 읽기; 지원 범위를 별도 표기.
- 최소 `.claude/skills/loom-can/` 및 `loom-viz/` 배포안. 이번 라운드에는 만들지 않는다.

### 1.4 후순위 / 비범위

- CANoe COM/MCP/XIL 연동은 후순위 플러그인: M1 및 CAN 물리 접근의 전제가 아니다.
- ARXML 전 범위·AUTOSAR 시스템 편집, DBC 편집 GUI는 후순위다.
- LDF를 통한 실제 LIN 통신·스케줄러는 비범위다. DB 변환 후보만 조사한다.
- CAPL 실행 환경, HIL 실시간 보장, CAN XL, FlexRay, Automotive Ethernet 분석은 비범위다.
- ECU flash/security algorithm/UDS 서비스 자동화는 UDS 팀 판단이며 CAN 팀이 구현하지 않는다.
- 차량 제어 폐루프를 Python/브라우저에 맡기거나 100µs급 STmin을 보장하지 않는다.
- 사후 로그만 생성하는 기능을 핵심 루프 완료로 인정하지 않는다.

## 2. 사용 시나리오 (에이전트 관점, 사용자 관점 각각 3개 이상, 구체적으로 — 실제 명령/화면 흐름 수준)

### 2.1 종단 간 핵심 시나리오 — M1 필수 수용 시험

아래 명령은 제안 인터페이스이며 현재 실행 가능한 제품 명령이 아니다.
M1에는 DBC가 없어도 되도록 ECU와 payload를 고정한다.
가짜 ECU는 수신 ID `0x100`의 첫 바이트 값을 ID `0x101`로 echo한다.

```bash
loom can open --backend virtual --channel demo --name bench --json
# {"session_id":"s1","channel":"ch0","backend":"virtual","recording":true}
loom can sim start --session s1 --profile echo --request-id 0x100 --reply-id 0x101
loom can send --session s1 --id 0x100 --data 012A --timeout 1s --json
# {"request_id":"r1","state":"accepted","origin":"agent","seq":"41"}
loom can trace --session s1 --id 0x101 --since-seq 41 --timeout 1s --limit 10 --json
```

사용자가 별도 herdr 패인에서 `loom can attach s1`을 실행한다.
최근 버퍼 `seq=41`의 agent TX와 뒤따른 device RX를 같은 화면에 본다.
Trace에는 `agent → 100 01 2A`, `device ← 101 01 2A`를 색+라벨로 표시한다.
목표: 수신 enqueue 시각부터 TUI 표시까지 p95 <200ms, 정상 부하 p99 <500ms.
TX 의도는 즉시 보이되 실제 전송 확인과 구분한다. virtual은 HW 확인이라고 표시하지 않는다.
사용자가 TUI의 송신 키 `t` → ID `0x100` → hex `0137` → Enter로 직접 개입한다.
그 프레임은 동일한 TX 중재기를 지나 `origin=user`, `actor_id=viewer-u1`로 기록된다.
에이전트는 다음 조회로 사용자의 프레임과 ECU 응답을 읽는다.

```bash
loom can trace --session s1 --origin user --since-seq 41 --tail 10 --timeout 1s --json
# {"events":[{"seq":"44","origin":"user","payload":{"id":256,"data_hex":"0137"}}]}
```

M3의 웹에서는 같은 세션을 `loom viz live --session s1`로 연다.
IG 패널에서 같은 송신을 할 수 있고, Graphics의 `echo.value`는 10Hz 이상 갱신을 목표로 한다.
원시 payload→숫자 매핑은 M3에 정해진 decoder metadata로 제공한다.
실ECU 시험은 동일 순서를 승인된 벤치 프로파일과 읽기/echo용 ID로 대체한다.
에이전트와 사용자의 별도 bus open이나 별도 가짜 스트림은 실패로 판정한다.

### 2.2 에이전트 관점 A — 장비 식별 후 bounded 읽기

```bash
loom can devices --backend vector --json
loom can open --backend vector --app Loom --channel 0 --mode shared --name bench --json
loom can trace --session bench --tail 100 --id 0x180 --timeout 2s --json
```

응답에 adapter serial, physical channel, 실효 CC/FD 설정과 init 권한을 표시한다.
viewer URL/attach 명령을 함께 반환해 사용자가 즉시 같은 트레이스를 열 수 있게 한다.
모르는 bitrate를 추정해 기존 CANoe 측정을 재설정하지 않는다.
응답이 없으면 RX=0을 보고하고 자동으로 송신·탐색 프레임을 만들지 않는다.

### 2.3 에이전트 관점 B — DBC 신호 대기와 주기 프레임 갱신

```bash
loom can db load --session bench --file vehicle.dbc --channel ch0 --json
loom can signal get --session bench --name Engine.RPM --max-age 500ms --json
loom can signal wait-for --session bench --name Engine.RPM --op gt --value 1000 --timeout 5s --json
loom can tx create --session bench --message Command --values '{"Enable":1,"Target":50}' --period 20ms --duration 5s --dry-run
loom can tx start --session bench --plan p17 --grant g1 --json
loom can tx update --job j1 --set Target=55 --expected-revision 1 --json
loom can tx stop --job j1 --json
```

`tx update`는 같은 메시지의 보존된 전체 signal state에 patch한 후 한 번 encode한다.
수신 상태를 송신 기본값으로 묵시 복사하지 않는다. 초기 전체 값이 필요하다.
Revision 충돌·mux 불일치·범위 초과는 원자적으로 거절한다.
agent가 바꾼 Target과 user가 바꾼 Enable을 IG 이력에서 각각 확인한다.

### 2.4 에이전트 관점 C — 기록 조사 및 사용자 플롯 열기

```bash
loom can record status --session bench --json
loom can record start --session bench --format blf --name incident --json
loom can trace --session bench --since 2026-11-09T12:00:00Z --tail 200 --json
loom can record stop --recording incident --timeout 10s --json
loom viz show incident.blf --dbc vehicle.dbc --y Engine.RPM,Engine.Temp --json
loom viz show data.csv --x ts --y a,b --json
```

정지는 선택 export만 닫고 공통 자동 기록은 계속한다.
`viz show`는 viewer ID/URL을 반환하고 CLI는 끝난다. 브라우저 실행 실패 시 URL만 반환한다.
큰 파일은 ingest job을 반환하고 진행률을 사용자 화면에 표시한다.

### 2.5 사용자 관점 A — 진행 중 작업에 늦게 참가

`loom can sessions` → `loom can attach bench`로 같은 세션을 고른다.
snapshot high-water seq와 이후 live subscription을 원자적으로 연결해 중복·틈을 막는다.
출처별 TX/RX, 연결 상태, 기록 상태, 마지막 agent 작업과 현재 TX job을 즉시 본다.
Pause는 화면 스크롤만 멈춘다. 수신·기록 중단 버튼과 분리한다.

### 2.6 사용자 관점 B — 웹 IG로 개입하고 송신 정지

Trace 행의 DBC 메시지 → IG에 추가 → 필수 초기 신호 입력 → dry-run byte 확인 → 송신.
기존 주기 job에 접근하면 소유자·revision·남은 lease가 보인다.
일반 patch는 revision 기반, 사용자 takeover는 명시 동작으로 소유권을 넘긴다.
상단 Stop TX는 사용자/agent 주기 job 및 ISO-TP 새 송신을 차단하고 처리 결과를 표시한다.
이 정지는 벤치 전원 차단 기능이 아니며, 이미 버스에 나간 프레임은 취소할 수 없다.

### 2.7 사용자 관점 C — 트레이스에서 그래프로 조사

심볼릭 Trace에서 Engine.RPM 클릭 → Graphics 추가 → 구간 확대 → 두 커서 Δt/Δvalue.
Data/State는 raw 값, 물리값, enum, 단위, 마지막 변경/수신 시각과 stale 상태를 보여준다.
필터가 바뀌어도 원본 기록은 유지된다. 검색 결과 건수와 화면 생략 건수는 별도다.
파일 Replay로 전환하면 LIVE/REPLAY 배지를 크게 바꾸고 실HW 송신은 기본 꺼짐이다.

## 3. 아키텍처 (프로세스 구성도, 데이터 흐름 — ASCII 다이어그램 권장)

### 3.1 프로세스 결정

**공통 supervisor + 도메인별 장치 worker + 공용 기록/시각화 서비스**를 제안한다.
단일 giant asyncio 프로세스는 GIL 부하가 FC와 UART/DoIP까지 전파될 위험이 있다.
프로세스를 나눠도 사용자 관점의 세션·명령·권한·이벤트 스키마는 하나다.

```text
agent CLI -------- authenticated local RPC ---------+
user TUI ------------------------------------------+--> loomd supervisor / gateway
user web -------- HTTP + WS same-origin -----------+       | session registry / grants
UDS session worker ---- PDU RPC ---------------------------|
                                                        private local IPC
                  +----------------------+----------------+-----------------+
                  |                      |                                  |
             CAN worker              UART worker                       UDS/DoIP worker
             Bus / Notifier           COM owner                         DoIP owner
             TX arbiter              (UART team)                       (UDS team)
             ISO-TP threads
                  |
           bounded raw batches + monotonic seq
                  |
           recorder process ---- journal / manifest / sealed chunks
                  |
           decode / query workers ---- DBC cache / Parquet / downsampling
                  |
           gateway ---- filtered WS batches ---- web/TUI same-session clients
```

CAN worker만 CAN 장치를 연다. UDS·viz worker는 동일 장치를 다시 열지 않는다.
Linux의 kernel ISO-TP는 후속 옵션이며 도입 시에도 소켓 소유자는 CAN worker다.
Windows 생성 방식은 `spawn`; 열린 DLL 핸들을 자식에게 fork/복사하지 않는다.
멀티채널 Vector의 경우 하나의 VectorBus가 여러 채널을 열 수 있다.
운영 기본은 채널별 논리 객체이며 backend 제약에 따라 하나의 실제 핸들로 묶는다.
이 선택은 채널별 상태/seq/작업 권한을 합치는 이유가 되지 않는다.

### 3.2 수신 경로

1. 드라이버 수신 → `Notifier` 단일 소비자. 별도 `CanStack.recv()` 경쟁을 금지한다.
2. Notifier callback은 ISO-TP 큐와 bounded raw 큐에 넣고 즉시 반환한다.
3. callback에서 DBC decode, JSON, 파일 압축, HTTP, 사용자 코드 실행을 하지 않는다.
4. raw batch를 기록기로 넘기고 이벤트 ID를 부여한다. ISO-TP는 별도 내부 thread에서 처리한다.
5. decode worker는 관심 신호/트리거를 계산하고 조회용 열형 저장소를 만든다.
6. gateway는 구독별 필터·배치·표시 제한을 적용한다.
7. 화면 ack는 수신/기록 완료의 조건이 아니다.

관측 필터와 장치 필터를 구분한다. 사용자 Trace 필터로 ISO-TP FC/RX ID를 제거하면 안 된다.
장치 수신 필터는 모든 소비자의 union이며 raw 기록 전체 요구가 있으면 전체 수신을 우선한다.
관측 이후 디코드 실패는 raw frame을 버리는 이유가 되지 않는다.

### 3.3 송신 경로와 출처

CLI/UI 인증 → 권한·범위·lease 검사 → TX 요청 원장 → 중재기 → backend send.
`tx.requested`, `tx.accepted`, `tx.confirmed`/`tx.failed`를 구분한다.
지원하지 않는 HW TX confirmation은 `confirmation=unavailable`이다.
`send()` 반환이 ECU 수신/응답 성공을 뜻하지 않는다.
TX echo를 payload/ID/시각만으로 합치면 외부 ECU 프레임을 잘못 숨길 수 있다.
backend receipt와 correlation이 있을 때만 연결하고 불확실하면 별도 관측으로 남긴다.
외부 CANoe가 송신한 프레임의 actor는 raw CAN만으로 식별할 수 없다.
이 경우 `origin=device`, `external_actor=unknown`으로 표시한다.
서버가 client role에서 origin을 정한다. 클라이언트가 user를 자칭하지 못하게 한다.

### 3.4 IPC

- 외부 local endpoint는 `127.0.0.1:<allocated-port>` 하나, HTTP+WebSocket same-origin.
- 요청은 JSON-RPC 2.0, 이벤트는 subscription ID가 있는 notification envelope.
- 브라우저가 직접 붙으므로 Unix socket만을 공통 외부 프로토콜로 택하지 않는다.
- Unix socket/named pipe는 supervisor↔worker 및 endpoint discovery의 내부 선택지다.
- loopback도 인증한다. 세션별 token, Origin/Host 검증, 크기·요청 수 제한을 둔다.
- CLI token은 사용자 전용 파일 권한으로 보관한다. URL query token을 장기 저장하지 않는다.
- 웹은 일회성 bootstrap 교환 후 HttpOnly same-site cookie와 WS Origin 검증을 사용한다.
- 정적 UI는 로컬 배포물에서 제공한다. 세션 데이터/DBC를 CDN에 업로드하지 않는다.
- 구독 재시작에는 `after_seq`와 retention gap 응답을 사용한다.
- 타임아웃 난 mutation을 자동 재시도하지 않고 idempotency key로 상태부터 조회한다.

### 3.5 실시간성·과부하 목표

아래는 구현 후 검증할 수용 목표이며 이번 spike의 달성 수치가 아니다.

| 구간 | 목표 | 정의 |
|---|---|---|
| device ingest → Trace 표시 | 정상 부하 p95 <200ms, p99 <500ms | host 수신 enqueue→화면 paint, HW clock을 직접 빼지 않음 |
| agent/user TX 요청 표시 | p95 <100ms | gateway accepted→동일 세션 화면 |
| Graphics | 정상 10–20Hz, 표시 age p95 <250ms | raw 수집률과 화면 redraw 주기는 분리 |
| PDU ready→UDS 전달 | p95 ≤10ms 목표 | transport timestamp와 IPC delay를 따로 보고 |
| 늦게 붙은 viewer | p95 1초 내 첫 최근 화면 | 최근 1,000행 또는 2MiB 상한 |
| overload 상태 배지 | 탐지 후 1초 안 | lag·display drop·recorder health를 구분 |

정상 검증 프로파일은 1ch 5,000frames/s, 두 viewer, 한 ISO-TP 대화다.
고부하 검증은 다채널 합계 20,000frames/s와 물리 버스 부하 100%를 별개 조건으로 시험한다.
frame rate를 bus load와 동일시하지 않는다. FD 크기·bitrate에 따라 관계가 달라진다.
기본 WS 배치는 최대 40ms 또는 200프레임 또는 64KiB 중 먼저 도달한 조건이다.
한 프레임씩 WebSocket/DOM 갱신하지 않고 25회/s 안팎의 batch를 보낸다.
구독별 outbound 대기열은 2MiB, visible trace는 최근 5,000행으로 제한한다.
한도를 넘으면 가장 오래된 화면용 raw batch를 버리고 `display.gap`을 먼저 알린다.
화면은 **“N 프레임 화면 생략 · 원본 기록 정상/불완전”**, seq 범위와 누적 횟수를 표시한다.
자동 생략을 검색 필터나 수신 0건으로 표현하지 않는다.
plot에는 `downsampled 1,000,000→4,000 points` 및 원본 범위를 표시한다.
과부하 시 Trace 5–10Hz, Graphics 5Hz로 줄이되 상태·오류·사용자 정지 이벤트는 우선한다.
화면이 따라잡지 못하면 LIVE 배지를 `지연 2.1초` 상태로 바꾼다.
record queue와 display queue는 별개이며 화면 생략은 기록에 영향을 주지 않는다.
파일 기록은 지정한 처리량·보존 자원 안에서 무손실 유지가 수용 기준이다.
CAN 수신은 backpressure로 멈출 수 없으므로 무한 부하/디스크 장애의 무손실은 보장할 수 없다.
recorder 한도 초과 시 `recording_incomplete`, 알려진 누락 수/범위 또는 `count=unknown`을 남긴다.
고수위에서 신규 TX·export·무거운 decode를 억제하고 사용자에게 즉시 경보한다.
무손실 기록 시험을 통과하지 못하면 M1/해당 HW 지원을 완료로 표시하지 않는다.

## 4. CLI / API 명세 초안 (명령, 인자, 출력 예시 — `loom can ...`, `loom viz ...`, 데몬 RPC 메서드/이벤트)

### 4.1 공통 출력 계약

- 비대화형 명령 기본 timeout 5초, 최대 응답 256KiB, 기본 100행, 최대 1,000행.
- `--json`은 버전 있는 단일 객체. stdout=결과, stderr=진행/진단.
- `--max-bytes`는 하향 조정 가능하며 서버 상한을 넘기지 못한다.
- 잘린 응답은 `truncated`, `next_cursor`, `matched_count`의 known/unknown을 반환한다.
- `trace --follow` 같은 무한 agent stream은 제공하지 않는다.
- 인간용 `attach`만 명시적 대화형 장기 구독이며 Ctrl-C로 viewer를 종료한다.
- `since-seq`는 마지막 관측 이후, wall-clock `since`는 timezone 명시 필터다.
- 종료 코드: 0 성공, 2 인자/스키마, 3 timeout, 4 권한, 5 장치/연결, 6 자원/부분 손실.
- 에러에 `retryable`, `request_id`, backend 원본 code를 포함한다.

### 4.2 명령 표

| 명령 | 핵심 인자 | 결과/경계 |
|---|---|---|
| `loom can devices` | `--backend`, `--timeout` | 식별 가능한 장치 목록; 탐색의 HW 부작용은 capability로 명시 |
| `loom can sessions` | `--json` | 동일 daemon의 열린 세션/최근 기록 |
| `loom can open` | backend/channel/name/mode/bitrate/fd/data-bitrate/app/serial | 세션·실효 설정·capabilities·viewer 접속법 |
| `loom can close` | session, `--timeout` | job/ISO-TP 정리·기록 seal 결과 |
| `loom can send` | session/id/data/extended/fd/brs/timeout/dry-run/grant | 단발 TX 요청 및 접수 상태 |
| `loom can trace` | tail/limit/id/mask/origin/dir/since/since-seq/timeout | 유한 이벤트와 다음 cursor |
| `loom can signal get` | name/channel/max-age | 값·단위·raw·quality·시각 |
| `loom can signal wait-for` | name/op/value/timeout/debounce | 조건을 만족한 새 샘플 또는 timeout |
| `loom can db load/list/unload` | session/file/channel/expected-revision | DB hash·diagnostics·revision |
| `loom can tx create/start` | message/values/period/duration/count/grant | 검증 plan 및 bounded job |
| `loom can tx update/stop/list` | job/set/expected-revision | 원자적 갱신/정지 상태 |
| `loom can stats` | session/window/timeout | FPS/오류/부하/추정·실측 구분 |
| `loom can record start/stop/status` | session/format/name/timeout | 추가 export 수명, 자동 기록과 분리 |
| `loom can replay` | file/session/speed/duration/target/dry-run | 기본 가상 timeline; HW target은 별도 승인 |
| `loom can attach` | session/trace-only | 인간 TUI, read+직접 송신 |
| `loom can stop-tx` | session, `--all-jobs` | 사용자 우선 정지, pending 정리 결과 |
| `loom viz show` | file/x/y/long/series/schema | viewer ID/URL/ingest job |
| `loom viz live` | session/preset/series | 공통 세션 구독 view |
| `loom viz export` | view/format/from/to/max-points | 제한된 CSV/PNG/SVG 데이터·뷰 export |

실HW `open` 기본 `--mode shared`는 설정 변경을 요구하지 않는 모드다.
미설정 채널은 `--mode configure --bitrate ...`로 명시해서 연다.
`open --fd`에는 arbitration/data bitrate 또는 검증된 실효 기존 설정이 필요하다.
FD data 길이는 0–8,12,16,20,24,32,48,64 중 하나로 검증하며 RTR+FD를 거절한다.
ID는 11/29bit 범위를 검증하고 extended flag를 ID 크기만으로 임의 추측하지 않는다.
기본 signal stale 기준은 cycle metadata×3 또는 1초이며 사용자가 지정할 수 있다.
`wait-for`는 최신값 검사와 future subscription의 seq 경계를 원자적으로 잡는다.
기본은 새 샘플 기준, `--accept-current`만 유효한 현재값을 허용한다.
조건식은 제한된 연산자/AST이며 Python `eval`을 사용하지 않는다.

### 4.3 JSON 결과 예

```json
{
  "schema_version": 1,
  "session_id": "bench",
  "events": [
    {"seq": "501", "domain": "can", "channel": "ch0", "dir": "tx",
     "origin": "user", "payload": {"id": 256, "extended": false,
     "data_hex": "0137", "fd": false, "status": "accepted"}}
  ],
  "truncated": false,
  "next_cursor": "bench:epoch1:501",
  "recording_health": "ok"
}
```

```json
{"error":{"code":"ERR_TX_RESERVED","request_id":"r9","retryable":false,
"detail":{"channel":"ch0","id":2016,"owner":"isotp:tp1"}}}
```

### 4.4 RPC 메서드/이벤트

`core.hello`, `core.sessions`, `core.subscribe`, `core.unsubscribe`, `core.snapshot`.
`can.open/close/devices/capabilities/send/trace.query/stats`.
`can.db.load/list/unload`, `can.signal.get/wait`, `can.tx.plan/start/update/stop/list`.
`can.record.start/stop/status`, `can.replay.plan/start/pause/seek/stop`, `can.stop_tx`.
`viz.ingest/view.create/view.patch/query/export`와 `job.status/cancel`.
`isotp.*`는 §15의 제안으로 한정한다.

```json
{"jsonrpc":"2.0","id":"q4","method":"core.subscribe","params":{
"session_id":"bench","topics":["can.frame","can.state","tx.*"],
"after_seq":"500","batch_ms":40,"max_batch_bytes":65536}}
```

```json
{"jsonrpc":"2.0","method":"core.events","params":{
"subscription_id":"sub1","seq_first":"501","seq_last":"700",
"events":[],"display_dropped":0,"recording_health":"ok"}}
```

위 `events:[]`는 envelope 형태 예시이며 실제 seq 범위에 맞는 이벤트를 채운다.
이벤트 topic: `can.frame`, `can.state`, `can.stats`, `can.decode_error`, `signal.sample`.
제어 topic: `tx.requested/accepted/confirmed/failed`, `tx.job_state`, `policy.denied`.
운영 topic: `display.gap`, `record.gap`, `record.health`, `session.closed`, `job.progress`.
`gap`·상태·TX 승인 이력은 화면용 frame 생략 정책의 대상에서 제외한다.
API에서 세션 변경·구독·job 시작은 응답 완료와 실제 작업 완료를 분리한다.

## 5. 사용자 뷰어 UX (터미널/웹, 화면 구성 — 웹 vs 데스크톱(PySide/pyqtgraph) vs TUI 비교 후 결정)

### 5.1 선택 비교

| 선택 | 장점 | 비용/제약 | 결정 |
|---|---|---|---|
| 웹+uPlot+가상화 표 | OS 공통, 늦게 참가, 같은 WS, 범용 viz 재사용 | 대용량 전송/DOM/정렬·zoom 구현 필요 | 장기 주 뷰어 |
| PySide6+pyqtgraph | NumPy 연동, 로컬 고속 곡선·데스크톱 조작 | Qt 배포 크기·라이선스 의무·별도 설치/업데이트 | 성능 비교 후보, 기본 배포 제외 |
| TUI | herdr에 맞음, M1 빠름, 키보드 개입 | 다중 그래프·밀집 신호 분석 한계 | M1 필수 최소 Trace |

웹 성능이 좋다는 주장만으로 pyqtgraph보다 빠르다고 결론 내리지 않는다.
동일 downsample 결과·8신호·1M/10M 원본으로 redraw latency/RSS/CPU를 비교한다.
M1은 TUI만으로 핵심 루프를 닫고, 웹 개발은 그 검증을 지연시키지 않는다.

### 5.2 웹 화면

```text
[bench LIVE | ch0 FD 500k/2M | record OK | lag 54ms | agent 작업 | Stop TX]
[Trace: raw/symbolic | filter | freeze | origin | seq | recording gaps]
[Graphics: series tree | time axes | cursor A/B | zoom | decimation badge]
[Data/State: value/unit/raw/enum/stale] [IG: owner/revision/period/lease]
[Statistics: fps/errors/load quality] [Replay: file/time/speed/target OFF]
[Activity: agent/user actions | approval scope | failures | dropped count]
```

세션 selector는 새 연결을 만들지 않는다. `open hardware`는 별도 명시 동작이다.
origin 색은 agent=보라, user=파랑, device=회색을 기본으로 하되 텍스트/아이콘도 쓴다.
script keepalive는 접을 수 있지만 카운터·기록에는 남는다.
Trace 열: 상대/절대 시간, Δt, 채널, ID/name, dir/origin, length/FD/BRS/ESI, data, seq.
raw/symbolic 토글, ID/channel/origin/dir/신호값 필터, 고정/자동 스크롤을 제공한다.
화면 freeze 시 상단에 누적 새 프레임 수와 현재 지연을 표시한다.
Graphics는 단위별 축, 동기 시간 커서, zoom history, 범위 선택, snapshot export를 제공한다.
비동기 신호를 묵시 선형 보간하지 않는다. 디지털/enum은 step/state renderer다.
IG는 full initial values와 mux branch, encoded bytes, 예상 rate, 위험 프로파일을 보여준다.
오류/거절 TX도 Activity와 Trace의 관련 행에서 확인한다.
Statistics는 bus-off/error-passive/overrun을 프레임 수와 별도로 보여준다.
장치 미지원 정보는 0이 아니라 `지원 안 됨/미확인`이다.

### 5.3 TUI

M1 TUI는 상단 세션/기록/lag, 중앙 bounded Trace, 하단 명령/오류 영역으로 구성한다.
키: `space` 화면 freeze, `/` 필터, `t` 직접 송신, `s` TX 정지, `q` viewer 종료.
출처 필터·tail buffer·기록 손실 배지를 제공한다. attach 자체는 장치를 닫지 않는다.
TUI 송신의 사용자 role은 신뢰된 로컬 인터랙티브 접속에서 발급받는다.
M1에서 임의 shell 실행이나 매크로 언어는 넣지 않는다.

### 5.4 범용 viz 입력과 타팀 요구

CSV wide: `ts,a,b`; long: `ts,series,value,unit`를 지원한다.
JSONL은 공통 envelope와 일반 long record를 구분해 ingest adapter를 선택한다.
숫자 열이면 시계열, CAN envelope면 Trace+신호, 문자열/enum이면 State로 기본 렌더한다.
자동 추론이 애매하면 UI에서 열/시간 형식/단위를 선택하고 sidecar preset으로 저장한다.
Parquet는 schema metadata를 우선하고 CAN BLF/ASC/MF4는 CAN adapter로 읽는다.
UART 숫자 추출은 제한된 regex named group과 단위/time mapping으로 받는다.
UART ANSI/raw 바이트 터미널 재현은 UART viewer 소유다. viz는 추출 결과를 표시한다.
UDS V-UDS-1: 요청/응답 transaction 표, NRC·pending 수·P2·원시 hex 펼치기를 우선 수용한다.
V-UDS-2: DID long/wide 시계열·문자열 State를 지원한다.
V-UDS-4: 공통 상태/진행 widget으로 세션·TesterPresent·단계 진행을 받는다.
V-UDS-3 DTC diff, V-UDS-5 ISO-TP 그룹 주석은 후순위 renderer다.
UDS 해석을 CAN/viz가 다시 구현하지 않고 UDS payload와 metadata를 그대로 받는다.

## 6. 데이터/기록 포맷

### 6.1 원본 이벤트

공통 이벤트는 session 단위 `epoch/seq`, 시각·출처·payload를 가진다.
JS에서 정밀도를 잃는 64bit seq/ns는 JSON 문자열로 직렬화한다.

```json
{"schema_version":1,"session_id":"bench","epoch":"e1","seq":"501",
"event":"can.frame","ts":{"mono_ns":"12000000000","wall_ns":"1794220000000000000"},
"domain":"can","channel":"ch0","dir":"rx","origin":"device",
"actor_id":null,"request_id":null,
"payload":{"id":384,"extended":false,"fd":true,"brs":true,"esi":false,
"dlc_code":9,"data_len":12,"data_hex":"000102030405060708090A0B",
"remote":false,"error":false,"backend_timestamp_s":1794220000.0,
"timestamp_source":"hardware","timestamp_quality":"mapped"}}
```

`ts.mono_ns`는 host ingest monotonic으로 명확히 정의한다.
device timestamp는 별도 값/clock domain/동기 품질로 보존한다.
가능하면 raw driver tick·주기/offset도 보존하되 python-can이 float로 변환한 경우 원 tick은 미확인이다.
다른 adapter의 HW clock을 동기화 없이 빼서 ECU 응답시간을 만들지 않는다.
시스템 wall clock이 바뀌어도 event 순서는 epoch/seq로 유지한다.
과거 파일은 원본 capture time과 ingest time을 분리한다.
`dir` 공통값은 rx/tx를 유지하고 상태 이벤트에는 nullable 허용을 §15에 요청한다.
CAN DLC code와 python-can `Message.dlc`의 바이트 길이 의미를 구분해 정규화한다.

### 6.2 저장 계층

- M1 canonical journal은 길이 제한 JSONL chunk+manifest, append 후 seal 방식이다.
- M2 이후 raw binary chunk/Arrow IPC 여부를 동일 성능 시험 후 결정한다.
- 파생 signal 데이터는 Parquet로 저장하여 time/channel/series 범위만 읽는다.
- manifest: 세션, schema, OS/Python/backend/driver, 채널 설정, DB hash, chunk seq/시간 범위.
- DB 원본은 선택 첨부하고 hash/버전은 필수다. 독점 DB를 보고서에 자동 공유하지 않는다.
- chunk는 예를 들어 64MiB/60초 중 먼저 도달한 조건에 회전한다. 값은 시험으로 확정한다.
- seal 전 임시 확장자, close 후 fsync/원자 rename과 checksum, 마지막 불완전 JSONL 복구를 설계한다.
- 자동 기록은 capture open 전 준비한다. 기록 불가능하면 기본 fail-closed로 새 HW TX를 막는다.
- `record stop`은 추가 export만 정지한다. 전체 자동 기록 중단은 세션 종료와 연결한다.
- 일일/세션 quota와 보존기간을 설정하고 삭제 정책을 사전에 명시한다.
- 관측 손실·기록 손실·decode 실패·화면 생략은 각각 별도 카운터다.

### 6.3 표준 CAN 포맷

| 포맷 | 읽기/쓰기 | 용도 | 제약 |
|---|---|---|---|
| ASC | python-can 기본 reader/writer | 사람이 보는 교환 | 버전/FD 변형 검증, 메타데이터 원장 별도 |
| BLF | python-can 기본 reader/writer | 압축 raw CAN 교환 | 지원 CAN object만, Vector 임의 object 전체 보존 약속 금지 |
| MF4 CAN bus log | python-can+asammdf 선택 의존성 | ASAM bus logging 교환 | reader는 CAN bus logging 대상, writer append 미지원 |
| MF4 측정 신호 | asammdf adapter | CAN 외 측정 channel 시계열 | raw CAN reader와 다른 ingest 경로 |
| CSV/JSONL/Parquet | 범용 ingest/export | viz·타팀 데이터 | 시간/단위/schema를 sidecar로 보완 |

근거: [python-can File IO](https://python-can.readthedocs.io/en/stable/file_io.html),
[MF4 구현 v4.6.1](https://github.com/hardbyte/python-can/blob/v4.6.1/can/io/mf4.py).
FD raw data, channel, direction, timestamps, error/remote frame을 fixture로 비교한다.
모든 포맷이 origin/request ID/정책 이력을 보존하지 않으므로 canonical sidecar가 필요하다.
BLF/ASC/MF4 export를 canonical 자동 기록의 유일한 crash recovery 수단으로 삼지 않는다.
MF4 writer가 최종 저장 전에 메모리/임시 자원을 어떻게 쓰는지 별도 대용량 시험한다.
부하 높은 세션의 MF4 변환은 live CAN process 밖에서 봉인된 chunk를 대상으로 한다.

### 6.4 Replay

기본 target은 가상 세션이며 agent/user 과거 origin은 `original_origin`으로 보존한다.
재생 실행 주체는 `origin=script`, `replay_id`, `source_seq`로 따로 표시한다.
속도·seek·pause는 원본 capture time에 적용하고 사용자 화면 clock을 분리한다.
실HW replay는 채널 매핑·ID allowlist·기간·rate 상한과 dry-run 계획을 먼저 만든다.
파일의 RX 전체를 무조건 TX로 바꾸지 않는다. 선택된 원본 방향/ID만 송신한다.
원본 ECU와 충돌할 ID는 기본 차단한다. 버스 부하 추정과 기존 주기 job 예약도 검사한다.

## 7. 라이브러리 선택과 근거 (버전, 라이선스, 플랫폼 지원, 유지보수 상태 — 가능한 한 실제 확인)

### 7.1 조사 기준과 선택

버전은 2026-11-09 PyPI JSON API 및 npm registry의 stable/latest 결과를 직접 확인했다.
stable 문서와 upstream main은 버전이 다를 수 있어 backend 동작은 **v4.6.1 소스**를 기준으로 했다.
Python 후보 메타데이터는 `spikes/can/package-evidence.json`에 보관했다.
backend/MF4 및 플롯 후보 원문 근거는 `spikes/can/source-evidence.json`에 보관했다.
라이선스는 패키지 선언/원본 LICENSE의 확인 결과이며 드라이버 재배포 승인을 뜻하지 않는다.

| 후보 | 확인 버전·최근 배포 | 라이선스 | 플랫폼·유지보수 확인 | 선택 |
|---|---|---|---|---|
| python-can | 4.6.1, 2025-08-12 | LGPL-3.0-only | Python≥3.9; backend별 OS; upstream 2026-11-08 commit 확인 | CAN 공통 계층 |
| can-isotp | 2.0.7, 2025-05-14 | MIT | Python≥3.7; pure Python은 OS 공통, kernel wrapper는 Linux; v2.x 2026-08-22 commit | CAN 내 ISO-TP |
| cantools | 45.1.2, 2026-10-29 | MIT | Python≥3.10; python-can≥4.6.0 요구; 최근 release 확인 | DBC runtime |
| canmatrix | 1.3.3, 2026-10-21 | BSD-2-Clause | Python≥3.8; 여러 DB 형식, 일부 extras/native XML 의존 | 선택 변환 도구 |
| asammdf | 8.8.32, 2026-11-03 | LGPLv3+ 선언 | Python≥3.10; NumPy≥2, native deps; 최근 release 확인 | MF4 선택 adapter |
| tsdownsample | 0.1.5.1, 2026-06-01 | MIT | Python≥3.8, Rust extension·wheel 확인 필요 | LTTB 후보 |
| pyqtgraph | 0.14.0, 2025-11-16 | MIT | Python≥3.10; NumPy와 Qt binding 필요 | 데스크톱 비교 후보 |
| PySide6 | 6.12.0, 2026-10-08 | LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only 선언 | Python3.10–3.15; 대상 wheel/Qt 모듈 조건 별도 | 기본 배포 제외 |
| pyarrow | 26.0.1, 2026-11-03 | Apache-2.0 | Python≥3.11; native wheel 필요 | Parquet/query 후보 |
| FastAPI | 0.162.2, 2026-11-07 | MIT | Python≥3.10, ASGI; 최근 release 확인 | 로컬 gateway 후보 |
| uPlot | 1.6.32, npm latest 확인 | MIT | browser Canvas2D; upstream 문서·registry 확인 | 주 시계열 renderer |
| Plotly.js | 4.2.0, npm latest 확인 | MIT | browser SVG/WebGL; 문서·registry 확인 | 후순위 rich chart |
| Apache ECharts | 6.1.0, npm latest 확인 | Apache-2.0 | browser Canvas/SVG; registry 확인, 실제 제품 비교 미실시 | 후순위 dashboard |

Python 버전/라이선스 출처:
[python-can 4.6.1](https://pypi.org/project/python-can/4.6.1/),
[can-isotp 2.0.7](https://pypi.org/project/can-isotp/2.0.7/),
[cantools 45.1.2](https://pypi.org/project/cantools/45.1.2/),
[canmatrix 1.3.3](https://pypi.org/project/canmatrix/1.3.3/),
[asammdf 8.8.32](https://pypi.org/project/asammdf/8.8.32/).
추가 출처:
[tsdownsample](https://pypi.org/project/tsdownsample/0.1.5.1/),
[pyqtgraph](https://pypi.org/project/pyqtgraph/0.14.0/),
[PySide6](https://pypi.org/project/PySide6/6.12.0/),
[pyarrow](https://pypi.org/project/pyarrow/26.0.1/),
[FastAPI](https://pypi.org/project/fastapi/0.162.2/).
npm 출처:
[uPlot](https://registry.npmjs.org/uplot/latest),
[Plotly.js](https://registry.npmjs.org/plotly.js/latest),
[ECharts](https://registry.npmjs.org/echarts/latest).
유지보수의 commit 확인:
[python-can](https://github.com/hardbyte/python-can/commit/2a45350e3c23cd6ddeb71fc1d1c58dbaf311f263),
[can-isotp v2.x](https://github.com/pylessard/python-can-isotp/commit/5593c219f35e739554d743a98d3af68c9332da31).
최근 release/commit은 장기 지원 약속이나 해당 HW의 정상 동작을 증명하지 않는다.

### 7.2 python-can backend별 사실

| backend | FD/데이터 bitrate | 다채널 모델 | timestamp 사실 | 주기 송신 구현 |
|---|---|---|---|---|
| vector | `fd`, `data_bitrate`, BitTimingFd | channel list, app logical channel/serial mapping | XL event ns tick를 float초+offset 변환 | 4.6.1 기본 thread fallback |
| pcan | FD 지원 HW+`InitializeFD` bit timing | 일반적으로 PcanBus 하나당 PCAN channel 하나 | PCAN timestamp/FD µs를 host 기준으로 변환 | 기본 thread fallback |
| socketcan | Linux netdev를 외부에서 FD 설정; bus는 fd=True | channel별 socket, all-channel 수신도 가능 | 현재 구현의 SO_TIMESTAMPNS는 kernel SW timestamp | BCM kernel scheduler, callback 옵션은 fallback 주의 |
| virtual | FD flags/data를 전달할 수 있음, 물리 검증 없음 | 같은 프로세스 같은 channel의 Bus끼리 | host software clock | 기본 thread fallback |
| kvaser | fd/data_bitrate, CANlib 및 HW 의존 | 보통 Bus 하나당 CANlib channel, 여러 Bus 구성 | CANlib timestamp+offset, 해상도는 설정/HW 의존 | 기본 thread fallback |

출처:
[Vector 설정/기능](https://python-can.readthedocs.io/en/stable/interfaces/vector.html),
[Vector v4.6.1 source](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/vector/canlib.py),
[PCAN source](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/pcan/pcan.py),
[SocketCAN source](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/socketcan/socketcan.py),
[Kvaser source](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/kvaser/canlib.py),
[Virtual 설명](https://python-can.readthedocs.io/en/stable/interfaces/virtual.html).

`send_periodic()`라는 API 이름만으로 HW cyclic 송신을 주장하면 안 된다.
v4.6.1의 vector/pcan/kvaser에는 `_send_periodic_internal` override가 없음을 설치 소스에서도 확인했다.
일반 경로는 [BusABC thread 구현](https://github.com/hardbyte/python-can/blob/v4.6.1/can/bus.py)이다.
SocketCAN BCM은 kernel offload이지 CAN controller firmware scheduler라는 뜻은 아니다.
`modifier_callback`을 매 주기 호출하면 BCM 경로 대신 Python fallback을 타는지 시험해야 한다.
신호 수정은 가능한 경우 task `modify_data`로 반영하고 callback 없는 scheduler를 유지한다.
추가 HW scheduler는 vendor 전용 API/다른 backend별 capability로 나중에 도입한다.
Vector/PCAN HW 지원 자체와 python-can에서 해당 기능을 노출하는지를 구분한다.

### 7.3 Vector XL와 CANoe 공존

Vector 공식 제품 페이지에서 **XL Driver Library 26.30.16, 2026-07-23, Windows**를 확인했다.
상용 vendor 라이선스이며 무료 다운로드가 OSS를 뜻하지 않는다.
출처: [Vector XL Driver Library](https://www.vector.com/de/business-unit/testing-validation-diagnostics/products/network-interfaces/xl-driver-library/).
DLL/driver·HW firmware의 현장 설치 버전은 미확인이고 `doctor`가 별도로 보고해야 한다.

설정 절차:

1. Vector Hardware Configuration에서 `Loom` application을 만든다.
2. Loom logical CAN 0을 CANoe가 사용하는 것과 같은 physical HW/channel에 배정한다.
3. CANoe 설정 소유 모드인지 Loom 설정 소유 모드인지 명시한다.
4. CANoe가 먼저 측정을 시작한 상태에서 Loom shared open을 시험한다.
5. 수신·단발 TX·FD·close·역순 open을 별개로 검증한다.

앱 채널 할당은 logical→physical mapping이며 자체 배타 잠금이 아니다.
CAN multi-client 공유와 채널 파라미터를 바꾸는 **init access**를 구분한다.
실제 적용 근거는 python-can의 `permission_mask`, `_has_init_access`, 설정 검사 코드다.
초기 요청 mask는 bitrate/fd/timing/listen_only를 지정했을 때 만들어진다.
반환된 permission mask와 교집합인 채널에만 bitrate/FD/output 설정을 쓴다.
init 권한이 없어도 포트를 열고 기존 설정과 요청값을 비교하는 경로가 존재한다.
따라서 같은 채널 공유 가능성이 있지만 **모든 CANoe/driver/HW 조합에서 검증 완료는 아니다**.
공식 manual 링크는 확인했으나 이번 조사에서 PDF 본문을 추출하지 못했다.
리드는 설치된 XL manual의 multi-application/init access 절과 실제 벤치 결과를 대조해야 한다.
공식 원문 위치: [XL manual](https://www.vector.com/dam/jcr:37a98de5-a2ca-4333-b6c7-ead8b088c937/XL_Driver_Library_Manual_EN.2026-09-12-16-40-28.pdf).

CC shared에서는 bitrate/timing/listen_only를 생략해 init 요청을 피하는 경로가 있다.
FD는 `fd=True` 자체가 init 요청을 만들므로 **“FD이면 항상 init 권한 없이 연다”라고 약속하지 않는다**.
기존 FD 설정과 반환 권한을 검사하는 adapter spike가 필요하다.
shared mode에서 예상치 않게 init 권한을 얻으면 설정 변경 전에 fail-closed로 거절하는 방안을 시험한다.
stock constructor가 설정까지 수행하므로 엄격한 무변경 shared-FD는 얇은 adapter/상류 변경이 필요할 수 있다.
권한 없음은 RX-only와 같은 뜻이 아니다. TX 허용은 Loom 정책으로 별도 통제한다.
`listen_only`는 물리 채널 상태를 바꿀 수 있어 shared 모드의 기본값으로 사용하지 않는다.
하드웨어 silent와 Loom TX 차단을 별도 capability로 보고한다.
CANoe가 먼저 죽었을 때 Loom이 자동으로 bitrate를 재초기화하거나 송신을 재개하지 않는다.
일반 Stop TX에 Vector `flush_tx_buffer()`를 무비판적으로 호출하지 않는다.
해당 API의 backend 구현/문서에 추가 프레임 관련 경고가 있으므로 별도 검증 전 금지한다.

### 7.4 PCAN-Basic와 macOS PCBUSB/MacCAN

PEAK 공식 목록에서 Windows PCAN-Basic **5.1.0**, Linux **5.0.1**을 확인했다.
공식 지원 OS는 Windows/Linux, 라이선스는 vendor EULA다.
5.x CAN XL 지원은 vendor API 사실이며 python-can 4.6.1의 CAN XL 지원을 뜻하지 않는다.
동일 physical channel을 여러 PEAK 앱이 사용하는 기능과 수신 timestamp 최대 µs급 정밀도는 공식 설명에 있다.
출처: [PCAN-Basic](https://www.peak-system.com/products/software/development-packages/pcan-basic/),
[공식 버전 목록](https://www.peak-system.com/support/software-information/).

python-can 4.6.1의 Darwin 분기는 `find_library("PCBUSB")`로 라이브러리를 찾는다.
이는 macOS backend 통로가 실제 존재한다는 근거이며 macOS PEAK 공식 driver를 뜻하지 않는다.
출처: [PCAN loader](https://github.com/hardbyte/python-can/blob/v4.6.1/can/interfaces/pcan/basic.py).
MacCAN PCBUSB 최신 GitHub release는 **v0.13, 2024-07-28**로 확인했다.
프로젝트는 user-space driver, Intel/Apple silicon, PCAN-USB/PCAN-USB FD와 제한된 장치 집합을 명시한다.
README의 target은 Intel macOS10.13+, Apple silicon 포함 macOS12+다.
모든 PEAK 장치/다채널 Pro 계열의 지원으로 확대 해석하지 않는다.
PCANBasic와 “거의 호환”인 API이며 지원 parameter·이벤트 차이가 있다.
소스 공개가 아닌 binary 배포, 무료 사용 가능하지만 자체 EULA이고 무지원 조건이다.
재배포 시 원본 전체·notice/EULA 동봉 조건을 확인한다. MacCAN wrapper의 BSD를 driver에 적용하지 않는다.
출처: [PCBUSB README](https://github.com/mac-can/PCBUSB-Library),
[v0.13 release](https://github.com/mac-can/PCBUSB-Library/releases/tag/v0.13),
[driver EULA](https://github.com/mac-can/PCBUSB-Library/blob/main/LICENSE),
[MacCAN](https://mac-can.com/).
현재 macOS27/사용자 장치에서 실제 enumerate·FD·receive event·timestamp 동작은 **미확인**이다.
따라서 macOS 기본 경험은 virtual/file; PCAN은 별도 선택 설치와 인증한 HW/OS 목록으로 제공한다.

### 7.5 DB 라이브러리

cantools는 DBC, ARXML3/4, KCD/SYM/CDD parsing과 simple/extended multiplexing을 명시한다.
값 테이블은 raw 수치·physical 수치·choice label을 함께 보존하는 모델로 감싼다.
기본 decode는 plot용 numeric choice와 UI용 label을 분리한다.
DB key는 `(channel, extended, frame_id, db_revision)`이며 ID만으로 찾지 않는다.
ARXML은 CAN communication 추출의 검증된 subset만 지원하고 전 AUTOSAR round-trip은 약속하지 않는다.
컨테이너/보안/E2E/rolling counter는 별도 fixture·플러그인 검증이 필요하다.
근거: [cantools 프로젝트](https://github.com/cantools/cantools),
[API와 지원 형식](https://cantools.readthedocs.io/en/latest/).

canmatrix는 다수 DB import/export 및 diff/convert 도구를 제공한다.
ARXML export는 프로젝트가 basic implementation이라고 설명하므로 무손실 변환으로 홍보하지 않는다.
LDF import가 있어도 LIN 물리 통신은 이 컴포넌트의 범위가 아니다.
근거: [canmatrix](https://github.com/ebroecker/canmatrix).
runtime decode 경로는 cantools 하나로 고정해 두 엔진의 rounding/mux 의미 충돌을 줄인다.

### 7.6 MF4 의존성과 라이선스

**asammdf는 이번 확인 결과 LGPLv3+이며 GPL 전용이라고 단정하지 않는다.**
PyPI 선언 `LGPLv3+`와 upstream LICENSE의 LGPLv3 본문을 확인했다.
출처: [asammdf metadata](https://pypi.org/project/asammdf/8.8.32/),
[LICENSE](https://github.com/danielhrisca/asammdf/blob/master/LICENSE).
wheel/sdist에 포함되는 정확한 notice·native dependency license는 릴리스 시 SBOM으로 재검사한다.
동적 사용·수정 여부·배포 형태에 따른 의무를 제품 배포 검토에 포함한다.
MF4 지원은 `[mf4]` extra로 분리하며 기본 CAN/TUI 실행을 막지 않는다.
**현재 asammdf8.8.32는 `canmatrix[arxml,dbc]==1.2`를 요구한다.**
따라서 같은 환경에서 canmatrix1.3.3을 강제 pin하면 resolver 충돌이 난다.
기본안은 MF4 호환 조합에서 canmatrix1.2를 사용하고, 최신 변환기는 별도 uv tool 환경으로 격리한다.
python-can4.6.1의 asammdf≥6 선언은 최신8.8.32 조합의 동작 검증을 대신하지 않는다.
실제 MF4 read/write·FD·시각·chunk 부하 시험 통과 전 optional 기능을 stable로 표시하지 않는다.

### 7.7 ISO-TP 선택과 타이밍

can-isotp2.0.7 `NotifierBasedCanStack`를 사용하고 ISO-TP를 직접 재구현하지 않는다.
python-can4.6.1과 동일 프로세스 virtual round trip이 이번 spike에서 실제 성공했다.
Python3.11/3.12/3.13, Windows·실HW 호환은 이번 실행으로 검증되지 않았다.
구형 `CanStack`는 직접 recv하여 다른 소비자를 굶길 수 있으므로 사용하지 않는다.
프로젝트 v2 문서는 내부 thread·blocking IO를 통한 timing 개선을 설명한다.
공식 문서의 수십 µs 예시는 Loom의 보증값으로 채택하지 않는다.
출처: [can-isotp implementation](https://can-isotp.readthedocs.io/en/latest/isotp/implementation.html),
[v2.0.7 source](https://github.com/pylessard/python-can-isotp/blob/v2.0.7/isotp/protocol.py).

| Loom 파라미터 | can-isotp 대응 | 기본 제안·주의 |
|---|---|---|
| `stmin_rx` | `stmin` | 0; 0x00–0x7F 및 0xF1–0xF9 의미 검증 |
| `blocksize_rx` | `blocksize` | 제품 기본8, 0은 상대 무제한 burst 허용; UDS 초안0과 조정 요청 |
| `override_tx_stmin_s` | `override_receiver_stmin` | null; 상대 요구를 기본 준수, override는 시험 전용 |
| `n_bs_ms` | `rx_flowcontrol_timeout` | 1000ms, ECU profile로 제한 |
| `n_cr_ms` | `rx_consecutive_frame_timeout` | 1000ms, CF 수신 timeout |
| `wft_max` | `wftmax` | 0, 허용 wait 정책을 명시 |
| `tx_dl` | `tx_data_length` | CC8, FD는 지원 길이 집합 |
| `tx_padding` | `tx_padding` | null, ECU 요구 시0xCC/0x55 명시 |
| `max_pdu` | `max_frame_size` | 기본4095, 초기 상한64KiB 제안 |
| `can_fd/brs` | `can_fd/bitrate_switch` | 채널과 일치해야 함 |

STmin은 CF 간 **최소** 간격이며 FC 응답 deadline과 다르다.
N_Bs/N_Cr와 UDS P2/P2*를 별도 timer로 둔다.
can-isotp 문서의 consecutive timeout N_CS 표기는 표준 용어 대조가 필요하다.
Loom 계약은 `n_cr_ms`의 수신 CF timeout 의미를 명확히 하고 명칭 오기를 전파하지 않는다.
GIL·OS scheduler·driver buffering 때문에 sub-ms CF 간격이나 수 ms FC를 항상 보장할 수 없다.
기본 `wait_func=time.sleep`; busy-wait를 활성화하면 CPU/GIL 경쟁을 늘릴 수 있어 기본 금지다.
Linux kernel ISO-TP는 선택 성능 경로 후보이며 Windows의 공통 기본 구현으로 사용할 수 없다.
주소/FD/functional 개요 근거: [Linux ISO-TP 문서](https://docs.kernel.org/networking/iso15765-2.html).

### 7.8 CANoe 제어: COM, Python, XIL, 최신 MCP

XL raw CAN 접근과 CANoe Application 제어는 서로 다른 adapter다.
COM wrapper 후보 `py-canoe`는 **26.4.1, 2026-08-30, MIT, Python≥3.10, pywin32**다.
프로젝트 소유자는 auto-py-utils이며 Vector 공식 Python SDK로 표기하면 안 된다.
출처: [py-canoe PyPI](https://pypi.org/project/py-canoe/26.4.1/),
[프로젝트](https://github.com/auto-py-utils/py-canoe).
PyPI의 `canoe`0.1.1은 2013년 별개 프로젝트이며 이름만으로 설치하지 않는다.
현재 공개 자료에서 CANoe 일반 제어용 **Vector 공식 PyPI Python 패키지는 확인하지 못했다**.
이는 Vector에 Python 연동이 전혀 없다는 뜻이 아니다. 설치 SDK/사내 배포는 미확인이다.
COM을 도입하면 Windows STA thread·message pumping·설치 버전 typelib 차이를 격리한다.
지원 초안은 기존 cfg attach, measurement 상태/start/stop, system variable get/set 정도로 제한한다.
Signal write가 곧 raw 프레임 송신이라는 보장은 없다. CANoe simulation node/설정 의미를 검증한다.
CAPL compile/test execution은 별도 고위험/후순위 작업이다.
ASAM XIL은 HIL/testbench 표준 연동 후보지만 설치 API·라이선스·port 지원·Python binding은 미확인이다.
일반 CAN 송수신을 위해 XIL을 요구하지 않는다.

**추가 발견:** Vector 공식 사이트는 CANoe20 SP2의 CANoe MCP Server와 AI Package를 명시한다.
이 공식 경로를 조사하지 않고 비공식 COM wrapper만 기본안으로 고정하면 안 된다.
출처: [Vector CANoe AI Features](https://www.vector.com/en/product/canoe/canoe-ai-features/),
[CANoe 제품/설치 버전](https://www.vector.com/en/product/canoe/).
공식 MCP의 API 범위·이벤트 처리량·승인/감사 통합·설치 entitlement는 아직 미확인이다.
사용자 CANoe 버전이20 SP2 이상이면 후속 spike 우선순위는 공식 MCP→COM fallback이다.
CANoe가 이미 장치를 소유한 adapter mode에서는 Loom이 같은 장치를 이중 제어하지 않게 모드를 명시한다.
MCP 호출 가능성은 raw CAN/ISO-TP 타이밍 제공 여부와 별개다.
이 조사는 M1의 virtual→실HW 직접 접근 완료를 지연시키지 않는다.

### 7.9 플롯과 LTTB

uPlot은 작은 Canvas2D 시계열 뷰어로 선택하되 scatter/annotation/rich dashboard는 범위를 제한한다.
project benchmark는 특정 장비의 저자 측정이며 Loom/pyqtgraph 성능으로 인용하지 않는다.
출처: [uPlot 프로젝트](https://github.com/leeoniya/uPlot).
Plotly.js scattergl은 큰 점 수 예제가 있고 browser WebGL context 한계가 있다.
따라서 여러 Graphics pane에 임의로 WebGL chart를 하나씩 만들지 않는다.
출처: [Plotly WebGL vs SVG](https://plotly.com/javascript/webgl-vs-svg/).
ECharts는 범용 dashboard 후보이나 이번에는 동일 데이터셋 벤치마크를 실행하지 않았다.
pyqtgraph 역시 수백만 포인트 raw 무제한 갱신을 자동 해결한다고 가정하지 않는다.

기본 query: time-range predicate→시계열별 min/max overview→화면 폭에 맞춘 MinMaxLTTB.
`tsdownsample`는 Rust 기반 LTTB/MinMaxLTTB/M4 및 NaN 대응 변형을 제공한다.
출처: [tsdownsample 알고리즘/API](https://github.com/predict-idlab/tsdownsample).
LTTB 원본은 [Sveinn Steinarsson 학위논문](https://skemman.is/handle/1946/15343)의 알고리즘 계열이다.
직접 재구현 대신 검증된 라이브러리 우선, wheel 부재 시 M4/minmax fallback을 제공한다.
순수 Python LTTB로 10M 포인트를 request thread에서 순회하는 경로는 금지한다.
1M 포인트의 float64 x+y만 약16MB, 10M이면160MB다. Python 객체·복사 비용은 추가다.
브라우저에는 기본 series당 최대 `min(4×width_px,20000)`점, 전체 표시점 상한을 둔다.
각 신호의 x가 다르면 union alignment가 큰 배열을 만들 수 있어 차트 그룹을 분리하거나 제한한다.
같은 x로 묶어 보내는 adapter는 재표본화 여부/방법을 명시한다.
LTTB는 모든 spike·최댓값·상태 전이를 보존하지 않는다.
alarm/wait-for/statistics는 원본 sample로 평가하고 plot 근사값을 사용하지 않는다.
gap/NaN에서 선을 끊고 enum·디지털 상태는 transition-preserving 방식으로 처리한다.
cursor는 근사 점만 보고 값을 확정하지 않고 해당 범위 원본 sample을 별도 조회한다.

### 7.10 실제 spike와 해석

재현: `spikes/can/.venv/bin/python spikes/can/r1_probe.py`.
원시 결과: `spikes/can/r1-results.json`. 단일 실행, baseline/load 각각50 PDU 왕복이다.
환경: macOS27.0.1 arm64, CPython3.14.1, python-can4.6.1, can-isotp2.0.7, cantools45.1.2.
venv 안에만 설치했으며 전역/사용자 설정은 변경하지 않았다.
pure virtual은 **동일 프로세스**이고 물리 전송시간·중재·USB·Windows scheduler가 없다.
load는 별도 Python thread가3신호 DBC decode를 반복하여 GIL 경쟁을 만든 조건이다.
이 조건은 100% 물리 버스 부하 재현이 아니며 부하의 실제 크기를 등가 FPS로 주장하지 않는다.

| 측정 | idle | GIL decode load | 의미 |
|---|---|---|---|
| 256B ISO-TP RTT p50/p99 | 1.895/9.566ms | 105.265/253.774ms | 요청+echo 응답, STmin0·BS0 |
| FF send→FC send p50/p99/max | 0.068/1.486/2.557ms | 9.225/51.863/60.503ms | 두 bus send hook의 host 시각, wire FC latency 아님 |
| 1ms cyclic 수신 간격 p99/max | 2.828/4.333ms | 14.494/48.349ms | 수신 observer scheduling도 포함 |
| 10ms cyclic 수신 간격 p99/max | 13.050/15.064ms | 30.301/41.743ms | HW 송신 jitter라고 부르면 안 됨 |
| transport 오류 | 0 | 0 | 1000ms timeout 안에서 성공; tight ECU 보증 아님 |

1ms cyclic의 idle 간격 표본700개, load698개; 10ms는120/121개다.
표본 수가 작아 p99는 불안정하며 단일 실행을 SLA로 사용하지 않는다.
load의 1ms 간격 p50≈0.0014ms는 수신 backlog를 연속 drain한 결과를 포함한다.
따라서 송신 bunching의 크기나 실제 wire spacing을 이 값만으로 확정하지 않는다.
실HW 평가는 send hook, driver RX timestamp, 외부 bus monitor의 세 시각을 분리한다.
다른 프로세스로 decode를 옮겨도 OS 경쟁이 남으므로 다음 spike에서 개선 여부를 재측정한다.

DBC3신호 numeric decode 200,000회는 **0.443초, 약451,855frames/s**였다.
복잡한 mux/enum/ARXML, 여러 소비자·저장·UI를 포함하지 않은 단일 메시지 microbenchmark다.
200개의 작은 CAN 이벤트 JSON batch는39,091B, 직렬화 평균0.371ms였다.
5,000frames/s이면25batch/s → 약0.977MB/s, 직렬화 CPU 약9.27ms/s의 **산술 추정**이다.
WS/TLS/IPC/브라우저·fan-out·FD64B payload·signal decode 비용은 포함하지 않았다.
동일 stream의 viewer 수만큼 fan-out 비용이 늘며 GIL hot path에서 JSON을 만들면 안 된다.
최적화 첫 단계는 frame별 JSON/DOM 제거와 배칭이며, binary protocol은 측정 후 도입한다.
대용량 실제 browser/pyqtgraph/다운샘플 벤치는 미실시로 명시한다.

## 8. HW/OS 지원 매트릭스

정식/조건부는 계획상의 목표 등급이다. 실HW 검증 완료를 뜻하지 않는다.

| OS/경로 | CC | FD | 다채널 | timestamp/주기 | 목표 등급 |
|---|---|---|---|---|---|
| macOS virtual | 가능 | flag/data 모델 | 동일 process 여러 channel | SW/thread | M1 정식 |
| Windows virtual | 가능 | 모델 | 동일 process | SW/thread | M1 정식, CI 필요 |
| Linux vcan/socketcan | 가능 | kernel 지원 설정 | 여러 netdev | SW/BCM | M2 정식 |
| Windows Vector XL | HW 의존 | FD 지원 장치 | channel list | XL clock/thread | M2 핵심 실HW |
| Windows PCAN-Basic | HW 의존 | FD 지원 장치 | Bus별 channel | HW clock/thread | M2 핵심 실HW |
| Linux PCAN SocketCAN | kernel driver 의존 | 장치/driver 의존 | netdev별 | 현재 python-can SW/BCM | M2 정식 후보 |
| Linux PCAN-Basic | vendor 설치 필요 | 장치/API 조합 | Bus별 channel | vendor clock/thread | 조건부 선택 |
| macOS PCBUSB v0.13 | 지정 장치 | PCAN-USB FD 등 확인 목록만 | device 수 제한·실제 채널 확인 | vendor clock/미검증 | 실험적 |
| Windows/Linux Kvaser | CANlib 설치 | 지원 장치 | Bus별 channel | CANlib clock/thread | 후속 인증 |
| macOS Vector XL | 미지원 | 미지원 | 미지원 | 해당 없음 | 비범위 |
| macOS SocketCAN/vcan | native 미지원 | 미지원 | 미지원 | Linux VM은 별도 환경 | 비범위 |
| CANoe COM | Windows/설치 필요 | cfg/툴 의존 | cfg 의존 | raw bus 계층과 별개 | M5 후순위 |

Kvaser macOS는 MacCAN 계열 별도 드라이버/adapter 가능성을 정식 CANlib 지원과 혼동하지 않는다.
ETAS/NI/neoVI/gs_usb 등의 python-can backend 존재는 자동 제품 지원 약속이 아니다.
장치별 capability는 fd, hw_timestamp, tx_confirmation, periodic_engine, shared_open,
listen_only, bus_load, error_frames, hardware_filters, isotp_addressing을 별개 값으로 보고한다.
`unknown`과 `false`를 구분하며 unsupported에 조용히 software emulation을 넣지 않는다.
Windows ARM64 CANoe/XL DLL·Python 조합은 미확인이다. 1차 인증은 Windows x64다.
Mac→Windows 원격 사용은 M1 비범위다. 후속에는 SSH port forward 등 인증된 tunnel만 검토한다.
웹을 Windows daemon에서 제공하더라도 `0.0.0.0` 공개가 기본이 되어서는 안 된다.

## 9. 시뮬레이션 & 테스트 전략 (HW 없는 CI 포함)

### 9.1 M1 종단 간 자동 시험

1. 하나의 daemon 안 virtual Bus 두 개로 fake ECU와 CAN owner를 연결한다.
2. CLI가 세션 생성→단발 request, fake ECU가 response를 송신한다.
3. viewer client는 동일 seq의 agent/device 이벤트를 제한 지연 안에 받는다.
4. viewer가 user request를 송신하고 CLI trace가 origin=user를 읽는다.
5. 늦게 참가한 viewer가 snapshot+live를 중복/누락 없이 받는다.
6. 두 viewer 중 느린 하나만 gap이 발생하고 다른 viewer/기록/송신이 계속 동작한다.
7. journal에서 모든 수락된 request와 관측 raw frame·사용자 개입을 대조한다.
8. viewer 종료·재접속과 daemon 종료를 구분하고 모든 thread/job가 정리되는지 확인한다.

python-can virtual은 여러 프로세스의 버스를 이름만으로 연결하지 못한다.
fake ECU를 별도 process로 분리할 때는 Linux vcan 또는 명시적 transport bridge를 쓴다.
virtual에는 arbitration/ACK/error physics/bandwidth가 없으므로 물리 bus load 검증에 사용하지 않는다.

### 9.2 가짜 ECU·프로토콜 fixture

- DBC fixture: 주기 RPM/temperature/state, mux 분기, Intel/Motorola bit order, signed/float/enum.
- 신호 patch fixture: 필수 초기값, 비활성 mux, 범위 초과, revision race, padding bit 보존 정책.
- CAN fixture: CC 11/29bit, FD 길이/BRS/ESI, RTR/error frame, 동일 ID 다른 channel.
- ISO-TP fixture: SF/FF/CF/FC, BS0/8, STmin0/1/10 및0xF1–F9 best-effort.
- fault: FC 누락/Overflow/Wait 한도, CF 순번 오류/누락, 초과 PDU, padding 정책, 중복 주소.
- UDS 팀 제공 echo/0x22 fake ECU와 공동 계약 시험을 실행한다.
- security/flash fake 흐름은 UDS 팀 소유이며 CAN은 PDU/원시 frame 관측을 검증한다.
- fake data에는 고정 random seed·명시 time source·시험 종료 deadline을 둔다.

### 9.3 OS와 CI

| Job | 환경 | 검사 |
|---|---|---|
| portable unit/contract | Linux/macOS/Windows, Python3.11–3.13 | 스키마/CLI/virtual 핵심 루프·정리 |
| extra compatibility | wheel 존재 대상 Python3.14 | 이번 spike와 dependency resolver 재현 |
| Linux integration | vcan 가능한 runner | 별도 process·BCM·kernel ISO-TP 선택 경로 |
| format round-trip | portable fixtures+MF4 extra | ASC/BLF/MF4 CAN/FD/시간/손실 보고 |
| web integration | headless browser | 동일 세션 사용자 개입·gap 표시·xss·zoom |
| nightly performance | 기준 머신 | 5k/20kfps·viewer2개·1M/10M points·journal 무손실 |
| hardware gated | 관리된 Windows x64 bench | XL/PCAN·CANoe 공존·wire timing |

Linux runner 준비 명령 예: `modprobe vcan`, `ip link add dev vcan0 type vcan`, `ip link set vcan0 up`.
이 명령은 CI 관리자 권한 준비 단계이며 macOS 개발자에게 실행을 요구하지 않는다.
vcan 모듈/CAP_NET_ADMIN이 없는 CI는 해당 job을 명시 skip하고 virtual 결과로 대체했다고 쓰지 않는다.
성능 시험은 개발자의 순간 부하에 민감한 universal unit assertion으로 만들지 않는다.
기준 머신/OS·driver·부하·샘플 수·percentile·실패 횟수를 artifact에 기록한다.
HW 없는 PR에서 실제 Vector/PCAN 지원을 통과로 표시하지 않는다.

### 9.4 로그·장애·성능 시험

기록 replay fixture로 seek·speed·timestamp jump·불완전 tail·지원 안 되는 object를 검사한다.
디스크 full/permission/I/O slowdown, recorder process crash, client flood를 주입한다.
검사 대상은 유실이 숨겨지지 않는지, 승인 없는 송신 재개가 없는지, 다른 viewer가 멈추지 않는지다.
정상 5kfps1시간과 burst20kfps10분을 baseline으로 두고 journal seq/카운터를 대조한다.
실HW 100% bus load는 별도 generator로 만들고 ISO-TP/Trace/기록 경로의 경쟁을 측정한다.
수신 이벤트 N개에 대해 관측→recorded→displayed→display gap을 독립적으로 집계한다.
외부 USB/driver overflow로 정확한 N을 모르면 `unknown`으로 남기는지 시험한다.
million plot은 ingest·첫 paint·pan/zoom query·cursor 원본 조회·RSS를 따로 측정한다.
목표: 준비된1M series 첫 view p95<2초, 캐시 zoom<200ms;10M import는 progress job이다.
이 수치는 아직 미측정 목표이며 raw10M 브라우저 push를 수용 조건으로 삼지 않는다.

## 10. 안전 / 가드레일

### 10.1 동작 등급과 집행

| 등급 | 예 | 정책 기본값 |
|---|---|---|
| R0 | trace/stats/db inspect/viz 파일 | 세션 읽기 권한으로 허용 |
| R1 | virtual TX, 승인된 벤치의 안전 ID 단발/주기 | 세션 프로파일·rate·기간 범위 내 사전 권한 재사용 |
| R2 | 실HW 새로운 ID/주기·IG takeover·HW replay·bitrate 변경 | 대상/bytes/기간을 concrete plan으로 제시하고 명시 권한 요구 |
| R3 | 알려진 reset/actuator/security/flash를 담은 raw/ISO-TP | UDS/벤치 정책으로 승격, 전용 절차 없으면 거절 |

모든 raw CAN payload의 위험을 ID만으로 완전히 판별할 수 없다.
따라서 실HW default read-only와 대상 벤치 profile·ID allowlist를 결합한다.
기존 사용자의 유효 권한은 범위·만료·plan hash가 맞으면 반복 확인하지 않는다.
approval은 서비스가 검증하고 agent 프롬프트/`--yes` 문자열만으로 우회되지 않는다.
origin=user라도 안전 rate/대상 잠금은 적용한다. user takeover는 감사 이력을 남긴다.
구체 제안: 실HW 초기50frames/s, job 기본5초·최대30초 lease, 설치자가 profile로 조정.
이 숫자가 모든 차량에서 안전하다는 뜻은 아니다. 실제 벤치에서 확정해야 한다.
bus load rate guard는 ISO-TP payload bitrate와 CAN overhead를 구분한다.
처음부터 모르는 bitrate로 active probing하거나 bus reset/auto recovery를 반복하지 않는다.

### 10.2 송신 경쟁·실패

동일 `(physical channel, extended, tx_id)`에 한 active frame producer만 예약한다.
ISO-TP 예약 tx_id에 대한 raw/IG/replay 송신은 기본 **차단**한다.
ISO-TP 수신 ID를 내 producer가 사용하게 하는 충돌도 검사한다.
다른 addressing extension의 중첩 수신은 ambiguity를 검사하고 지원 가능한 demux만 허용한다.
서로 다른 ID의 agent/user 송신은 공통 rate budget 안에서 처리한다.
같은 주기 메시지 갱신은 revision CAS; 취소/Stop TX는 신규 일반 TX보다 우선한다.
client 단절은 즉시 혹은 lease 만료 시 job을 정지하도록 소유자 유형별 규칙을 둔다.
한 번 실행 후 종료되는 CLI socket의 종료를 상주 UDS owner 종료와 혼동하지 않는다.
crash/reconnect에서 주기 TX·TesterPresent·replay를 자동 복구하지 않는다.
정지 시 host queue 삭제와 이미 driver queue에 들어간 잔여 frame을 구분해 보고한다.
kernel/HW scheduler가 crash 이후 남는지 시험하고 안전한 cancel 방법을 지원 조건으로 둔다.

### 10.3 데이터·플러그인·스킬

DBC/CSV/ARXML/JSONL을 비신뢰 입력으로 취급한다. XML entity/network resolution은 차단한다.
대형 파일·중첩 깊이·문자열 길이·PDU/WS 크기·regex 실행시간에 상한을 둔다.
파일 이름/choice label/UDS decoded 문자열은 HTML로 실행하지 않는다.
뷰 preset에서 임의 JS/Python 실행을 허용하지 않는다.
plugin checksum/명시 설치를 사용하고 ECU 데이터가 지시문처럼 보여도 실행 권한을 주지 않는다.
감사 원장은 actor/request/승인 scope/실효 bytes/결과·거절까지 기록한다.
UDS seed-key·token 등 비밀은 공용 Trace/웹·export에 원문 노출하지 않는다.
보안 데이터 redaction은 별도 규칙으로 기록하며 임의 누락과 구별한다.
향후 `loom-can` SKILL.md는 discover→shared read→bounded query→dry-run→authorized TX→verify 순서를 짧게 둔다.
안전 profile/명령/OS driver 상세는 references로 분리한다.
Codex 호환은 동일 CLI/문서 내용을 재사용하고 installer가 skill 경로만 선택하도록 계획한다.
이번 계획 작업은 사용자 설정/skill 설치 파일을 쓰지 않는다.

## 11. 마일스톤 (M0, M1, … 각각 수용 기준)

### M0 — 준비/계약 고정, 구현 기능 선행 금지

- 이벤트/seq/time/출처·장치 소유권·RPC 에러·권한 scope 초안에 타팀 합의.
- virtual fixture와 CI 실행 환경 정의, 이번 spike 보존/재현 확인.
- CANoe integration·DB 확장·포맷 수집을 이유로 M1 착수를 미루지 않는다.

### M1 — walking skeleton: 핵심 네 요소 모두

- daemon+virtual+가짜 ECU+`loom can send/trace`+라이브 TUI+사용자 송신을 완성한다.
- §2.1의 agent→device→화면→user 개입→agent 읽기 전체를 한 세션으로 통과한다.
- Trace p95<200ms, 출처·seq 일치, 늦게 참가한 사용자 snapshot+live가 통과한다.
- 자동 journal·정상 무손실·느린 viewer gap 표시·명시 Stop TX·bounded CLI를 통과한다.
- DBC 고급 기능·웹 미관·MF4·CANoe 제어가 없어도 이 조건이 충족돼야 한다.
- 로그만 남거나 사용자 직접 송신이 없으면 M1 실패다.

### M2 — 기본 실HW/DBC/주기 송신

- Windows x64 Vector/PCAN 한 종류 이상과 Linux vcan 경로를 검증한다.
- 장치 open→agent TX→실ECU 응답→동일 화면→user TX를 실HW에서 재현한다.
- CC/FD·두 채널·DBC simple mux·enum·신호 wait·주기 patch·rate/lease를 검증한다.
- CANoe 공유는 인증한 조합만 capability를 켠다. FD shared는 별도 판정한다.
- driver/raw timestamp·TX 확인 제한·jitter 실측을 게시한다.
- ASC/BLF export와 원본 metadata sidecar를 round-trip 검증한다.

### M3 — 웹 Trace/Graphics와 범용 viz

- M1과 동일한 세션의 웹 Trace/IG 개입, Data/State·Statistics를 제공한다.
- CSV/JSONL long/wide·Parquet·BLF/ASC→한 줄 `viz show`를 검증한다.
- 정상5kfps·viewer2개, overload20kfps에서 lag/gap/downsample 배지가 사실과 일치한다.
- 1M/10M dataset의 query·plot memory·zoom 목표를 기준 머신에서 판정한다.
- UART numeric series와 UDS transaction/DID 예제 입력으로 실제 렌더한다.

### M4 — UDS용 ISO-TP와 선택 MF4

- UDS 합의 계약으로 PDU open/send/rx/close, timestamp quality, 오류·cancel·lease를 검증한다.
- BS/STmin·timeout·주소 방식·FD·raw TX 예약 충돌을 계약 시험으로 통과한다.
- CPU/display 부하에서 FC/PDU 지연을 측정하고 ECU profile 지원 한계를 명시한다.
- MF4 optional dependency 조합·라이선스 notice·실제 bus log 및 신호 ingest를 검증한다.
- optional MF4 미설치 상태에서도 기본 CAN/viz 기능은 동작한다.
- UDS 팀 일정상 필요하면 M2 이후 transport work를 병렬 시작하되 M1 조건은 바꾸지 않는다.

### M5 — 후순위 확대

- 인증 장치/OS 확대, ARXML subset·advanced mux/E2E plugin, replay UX 개선.
- 필요 확인 시 공식 CANoe MCP spike, COM fallback, XIL 타당성 재평가.
- pyqtgraph 대체 frontend는 웹 성능 목표를 못 맞춘 측정 근거가 있을 때 선택한다.
- CANoe full suite·LIN runtime·CAN XL·hard realtime은 이 milestone에도 자동 포함하지 않는다.

## 12. 구현 단계 작업 분해 — 병렬 가능한 단위, 각 작업에 추천 담당(Opus 워커 / Sol 워커)과 의존성

아래는 **향후 구현 배정안**이며 이번 독립 R1에서 워커를 추가 생성하지 않았다.
M1 critical path는 T0→T1/T2→T3→T4다. 같은 이벤트 계약 뒤에서 병렬화한다.

| ID | 작업·완료 산출 | 추천 담당 | 의존성·병렬성 |
|---|---|---|---|
| T0 | 공통 세션/이벤트/권한/IPC schema와 M1 시험 명세 | Opus 워커 | 최우선, 타팀 리드 합의 |
| T1 | supervisor·endpoint discovery·bounded JSON-RPC·세션 registry | Sol 워커 | T0; T2와 병렬 |
| T2 | virtual Bus·단일 Notifier·가짜 ECU·TX 중재·journal 큐 | Sol 워커 | T0; T1과 인터페이스 mock 병렬 |
| T3 | CLI send/trace와 사용자 TUI 송신·출처·snapshot+live | Opus 워커 | T0,T1,T2; frontend/API 분리 가능 |
| T4 | M1 종단 간·느린 viewer·기록/Stop TX·정리 시험 | Sol 워커 | T1–T3 통합; M1 gate |
| T5 | Vector/PCAN capabilities·shared/init·driver 오류 adapter | Sol 워커 | M1,T0; T6/T7과 병렬 |
| T6 | cantools state model·DB revision·mux·signal wait | Sol 워커 | M1,T0; T5와 병렬 |
| T7 | TX job revision/lease/프로파일·IG 의미·safety UX | Opus 워커 | M1,T0; runtime은 T6 연동 |
| T8 | ASC/BLF adapter·chunk manifest·crash 복구 | Sol 워커 | M1 journal; T5/T6 병렬 |
| T9 | 웹 Trace/IG/Data/State·공통 renderer schema | Opus 워커 | M1 contract; T6/T7/T10 병렬 |
| T10 | 범용 ingest·Parquet query·MinMaxLTTB·viewport cache | Sol 워커 | T0,T8; T9와 mock data 병렬 |
| T11 | 웹 Graphics·커서·단위/상태·downsample/gap 표시 | Opus 워커 | T9,T10 |
| T12 | ISO-TP transport·주소 예약·PDU events·timestamps | Sol 워커 | M1,T5,UDS 계약; T9/T10과 병렬 |
| T13 | UDS adapter 공동 계약·functional group·keepalive 의미 | Opus 워커 | T12,UDS 팀; 확대 기능은 별도 gate |
| T14 | MF4 dependency lock·license notice·raw/signal adapter | Sol 워커 | T8,T10; optional, M1 비차단 |
| T15 | 실HW 공존·wire timing·부하/기록/뷰 성능 인증 | Sol 워커 | T5,T7,T8,T12; 도입 단계별 수행 |
| T16 | SKILL.md/references·예제·설치/OS 지원 문서 | Opus 워커 | M1부터 작성, T15 결과로 지원표 확정 |
| T17 | CANoe 공식 MCP/COM·XIL 조사 및 선택 integration | Sol 워커 | M5, 사용자 설치/필요 확인 후 |

한 워커가 공통 core 구현을 독단적으로 확정하지 않는다. T0은 제안→타팀 합의 gate다.
TUI/웹은 이벤트 소비자로 개발해 HW 부재가 UI 개발을 막지 않게 한다.
HW 시험이 지연돼도 M1을 먼저 완료하고 미인증 backend는 experimental로 둔다.
protocol hot path와 plot/query 수정은 별도 모듈·프로세스 ownership으로 병렬 작업 충돌을 줄인다.
리뷰 기준은 행 수/기능 수보다 실제 agent-user shared loop 통과 여부다.

## 13. 리스크

| 위험 | 근거/영향 | 완화·판정 기준 |
|---|---|---|
| GIL 경쟁으로 FC 지연 | 이번 load p99≈51.9ms | decode/JSON 분리, 긴 callback 금지, HW deadline 시험 |
| Vector 공유 오해 | init권한·FD constructor 설정 경로 | shared mode·permission/실효 설정 검사, 인증 조합만 지원 |
| CANoe 중복 송신 | XL raw와 simulation node가 같은 ID 생성 | 모드/ID 예약, 외부 앱 행위 식별 한계 표시 |
| 주기 송신 jitter | vector/pcan/kvaser thread fallback | BCM/전용 scheduler capability, 최소 주기 프로파일, wire 실측 |
| timestamp 허위 정밀도 | backend float·offset·다른 clock | ingest/HW/driver 시각 분리, quality/오차·TX 확인 상태 |
| 무손실 과장 | 외부 CAN은 backpressure 불가 | 처리량 예산·queue 수위·unknown gap·지원 부하 gate |
| UI가 오래된 값을 LIVE로 표시 | 배치/느린 viewer·decode 지연 | lag/stale/downsample/gap 명시, 상태 이벤트 우선 |
| macOS PCBUSB 중단/비호환 | 마지막 확인 release2024·binary EULA | 실험적·지원 HW 목록·virtual 기본·독립 smoke |
| MF4 의존성 충돌 | canmatrix==1.2 pin·native deps | optional 환경·resolver lock·round-trip fixture |
| parser/DB 해석 차이 | mux·ARXML subset·E2E | cantools 단일 runtime, DB hash·golden byte fixture |
| 잘못된 신호 patch | 불완전 초기값·user/agent race | full state·CAS·mux 명시·dry-run encoded bytes |
| ISO-TP 세션 충돌 | 동일 RX/TX 예약·raw 송신 | 기본 차단·owner lease·명시 takeover |
| browser 메모리 폭증 | raw millions+alignment 복사 | server query·point budget·column arrays·worker |
| 원격 접속 범위 확장 | mac→Windows 요구 가능 | M1 local 한정, 후속 인증 tunnel·동일 가시성 유지 |
| 기능 과대 범위 | CANoe 수준의 포괄적 해석 | M1 shared loop 우선, §1 비범위/후순위 고정 |

실패 보고에는 “미지원”, “미검증”, “장치 없음”, “설정 불일치”, “실측 실패”를 구분한다.
성능 미달을 timeout을 크게 늘려 성공처럼 보이게 하지 않는다.
현재 개발 머신에서 성공한 Python3.14 virtual 테스트는 Windows/Python3.11 인증을 대체하지 않는다.
정확한 HW timestamp가 없어도 공유 루프는 가능하지만 UDS timing 판정의 신뢰도를 낮춰 보고해야 한다.

## 14. 사용자에게 물어볼 질문 (각 질문에 기본값 제안)

현재 라운드를 중단하는 질문이 아니다. 기본값으로 계획을 진행하고 실HW 연결 전에 필요한 것만 확정한다.

| 질문 | 미응답 기본값 | 결정 영향 |
|---|---|---|
| 실제 Vector/PEAK 모델·serial·FD 여부는? | Windows x64 Vector1ch+PCAN-USB FD1ch를 인증 후보로 둠 | 장치별 driver/FD/채널 수 |
| Windows에서 agent/daemon을 실행할 수 있는가? | CAN HW와 동일 Windows host, mac은 개발/로그 분석 | 원격 bridge 필요 여부 |
| CANoe 설치 버전·라이선스와 주 목적은? | raw CAN/공유 관측 우선, CANoe 제어 후순위 | 공식 MCP20 SP2/COM 선택 |
| CANoe 측정을 유지한 shared 사용이 필수인가? | 기존 측정 유지, 설정 변경 없는 접근 우선 | init ownership·FD shared 검증 |
| 개발·실행 OS와 Python 배포 제약은? | Python3.11–3.13 정식, 3.14 별도 호환 | wheel/driver/CI 범위 |
| 목표 채널 수/FPS/기록 시간은? | 정상1ch5kfps, 고부하 합계20kfps;1시간 기록 gate | queue·디스크·UI budget |
| DBC/ARXML·E2E/rolling counter가 있는가? | DBC 우선, checksum plugin 없으면 관련 TX 차단 | 신호 IG 초기화·encode 범위 |
| 벤치에서 안전한 TX ID·rate·기간은? | 실HW read-only, 승인 profile 없으면 TX 차단 | 실제 송신 승인 범위 |
| ISO-TP ECU 주소/패딩/STmin/timeout은? | normal11, padding 없음, BS8,STmin0,N_Bs/N_Cr1000ms | ECU별 profile 검증 |
| 상시 기록 quota/보존/비밀 정보 규칙은? | 로컬 journal+manifest, 비밀 redaction, quota 명시 후 확정 | 보존·디스크 장애 정책 |
| 기본 사용자 화면은? | M1 herdr TUI, M3 웹;별도 Qt 설치 제외 | 설치/운영 UX |
| MF4가 초기 필수 교환 포맷인가? | M4 optional, BLF/ASC 먼저 | native dependency·납품 순서 |

기본값은 실제 장치의 존재나 사용 허가를 추정하는 것이 아니다.
미확정 장치에서는 virtual loop/기록/viz를 진행하고 실HW TX만 확정 후 수행한다.

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 공통 core에 요청

1. **프로세스 계약 보강:** 단일 `loomd` 진입점 아래 도메인 worker를 허용한다.
   장치당 단일 owner를 유지하며 ISO-TP hot path는 CAN worker 내부에 둔다.
2. **이벤트 계약 보강:** session/epoch/seq, event type, actor/request/correlation,
   timestamp source/quality, optional nullable dir를 공통으로 정의한다.
3. **시각 계약 보강:** ingest monotonic과 원본 HW clock을 분리한다. JS64bit는 문자열이다.
4. **손실 계약 보강:** capture/record/display gap과 unknown count, retention expired를 구분한다.
5. **작업 계약 보강:** grant scope/hash, TX owner reservation, lease, idempotency·cancel·reconnect 규칙.
6. **IPC 제안:** local HTTP+JSON-RPC/WS를 공통 외부 프로토콜로 택하고 내부 IPC는 OS별 선택한다.
7. **viewer 계약:** 원자적 snapshot+subscribe와 same-session late attach를 필수로 한다.
8. **기록 계약:** stop export와 stop automatic session journal을 분리한다.

이 요청은 `_xteam`을 수정한 합의가 아니다. 리드가 통합 결정 후 문서를 갱신한다.

### 15.2 UDS 초안 v0.1에 대한 수용·수정 제안

읽은 문서: `docs/plan/_xteam/can-uds-isotp.md` (UDS 리드 초안 v0.1).
CAN 내부 stack, PDU-only IPC, 원시 frame 공개, Notifier 공유 방향에 동의한다.
`isotp.open/close/list/send`와 tx/rx/error/state 이벤트 이름은 우선 수용한다.
다음 세부사항은 구현 전 합의가 필요하다.

| 쟁점 | CAN 제안 |
|---|---|
| 패딩 | 기본 null, ECU profile에서0xCC/0x55 지정 |
| BS 기본 | 8 권장, 초안0은 무제한 burst의 queue 영향 검증 후 허용 |
| raw 같은 TX ID | 경고만이 아니라 기본 차단, ID 예약·명시 takeover |
| `max_pdu` | 기본4095, 초기 서비스 상한64KiB;2^32−1 allocation 금지 |
| functional SF 길이 | CC7/FD62는 normal addressing의 경우;extended/mixed 주소 byte로 가용량 감소 |
| unknown responder | 초기 SF 관측만, 새 주소로 자동 FC 송신/stack 생성 금지 |
| periodic PDU | 가능하되 UDS 상주 owner·거래 serialization·lease로 제어 |
| 시각 | ready/submit/confirmed와 timestamp quality 분리;wire 시간인 척하지 않음 |

특히 `tx_done.ts_last_frame`이 **wire TX 완료**라고 기본 가정하면 안 된다.
python-can send callback 완료는 driver 접수 시각일 수 있으며 CAN ACK/ECU 수신 시각과 다르다.
RX 첫 frame의 backend timestamp를 transport PDU에 연결하려면 frame→PDU 추적 hook이 필요하다.
can-isotp `recv()`만 호출해서 첫/마지막 frame HW timestamp를 자동 얻는다고 가정하지 않는다.
동일 addressing별 FF/SF/CF 추적과 send sequence mapping을 adapter에서 검증한다.
불확실한 추적은 `timestamp_quality=estimated`로 보고한다.

```text
isotp.open({
  can_channel:"ch0", addressing:"normal_11", tx_id:0x7E0, rx_id:0x7E8,
  func_tx_id:0x7DF, params:{can_fd:false,tx_dl:8,bitrate_switch:false,
  stmin_rx:0,blocksize_rx:8,n_bs_ms:1000,n_cr_ms:1000,
  tx_padding:null,wft_max:0,max_pdu:4095,override_tx_stmin_s:null},
  owner_id:"uds-session-u1",lease_ms:10000
}) -> {isotp_id:"tp1",effective_params,capabilities,timestamp_quality}

isotp.send({isotp_id:"tp1",data_hex:"22F190",functional:false,
  req_id:"r42",deadline_mono_ns:"...",idempotency_key:"..."})
  -> {accepted:true,transfer_id:"x42"}
isotp.cancel({transfer_id:"x42"}) -> {state,residual_tx_possible}
isotp.recv({isotp_id:"tp1",after_pdu_seq:"8",timeout_ms:1000,max_pdus:1})
isotp.close({isotp_id:"tp1",reason:"user"})
```

바이트는 RPC에서 명시적으로 hex 또는 base64 한 형식으로 정하고 Python bytes를 JSON에 넣지 않는다.
`isotp.rx`에는 pdu_seq, source IDs/addressing, payload, first/last ingest/HW 시각·clock quality를 붙인다.
RX 응답에 req_id를 무조건 붙이지 않는다. ISO-TP 자체에는 UDS transaction 식별자가 없다.
UDS 세션 관리자가 요청 serialization·NRC0x78·pending 응답과 correlation을 담당한다.
CAN worker는 수신을 소유한 채 PDU를 FIFO 보관하며 큐 초과를 명시 오류로 보고한다.
완료/오류 이벤트는 raw frame display 생략 큐와 분리하여 보존한다.

P2는 UDS 팀의 semantic timer이며 송신 마지막 frame과 응답 첫 frame 관측으로 평가한다.
그러나 IPC 때문에 deadline 시점에 아직 RX event가 도착하지 않았을 수 있다.
“timestamp만 쓰면 IPC 지연이 판정에 영향 없음”은 충분하지 않다.
단조 시간 deadline과 전송 여유·watermark/drain barrier를 합의하고 late event를 구분해야 한다.
TX 확인이 없는 backend는 submit 기준 보수적 timer와 측정 uncertainty를 결과에 표시한다.
CAN은 UDS P2/P2*를 임의로 늘리거나 NRC0x78 의미를 해석하지 않는다.

주소 방식은 normal11/29부터 시작하고 normal_fixed29/extended11/29/mixed11/29를 fixture별로 확장한다.
ID 쌍뿐 아니라 channel·address extension·source/target의 수신 demux 중첩을 검사한다.
`open_group`은 기존 멤버 physical stack들의 유한 그룹으로 제안한다.
functional request는 한 번만 송신하고 응답은 member별 수신/FC 상태기로 처리한다.
기능 SF 용량은 addressing/tx_dl별 라이브러리 검증을 사용한다.
FD62B라는 단일 상수를 모든 주소 형식에 적용하지 않는다.
임의 responder를 보고 자동 FC를 보내면 장비에 새로운 쓰기가 생기므로 allowlist를 요구한다.

`isotp.periodic_start`는 PDU queue에 주기 요청을 넣되 이미 진행 중인 UDS 거래를 방해하지 않는다.
기본 job은 TesterPresent 용도로 제한하고 arbitrary flash PDU 반복은 허용하지 않는다.
연결 단절 기준은 ephemeral CLI가 아니라 상주 UDS session owner의 heartbeat/lease다.
owner 종료·session close·bus-off·Stop TX에서 즉시 신규 keepalive를 중단한다.
출처는 `origin=script`, `initiator_origin=agent/user`, `purpose=tester_present`로 공통 enum을 유지한다.
`agent:tp-keepalive`처럼 origin enum을 임의 확장하지 않는다.
UDS CLI는 이 상주 manager를 경유하여 여러 agent/user가 한 ECU 세션을 공유한다.

### 15.3 UART/UDS→viz 계약 제안

현재 `viz-requirements.md`에는 UDS 요구가 있고 UART 요구 절은 아직 없다.
따라서 UART 숫자/단위/문자열 요구는 공통 브리프에 근거한 잠정안이다.
타팀 producer가 제공할 최소 metadata:

```text
series_id, display_name, value_type(number|enum|string|bytes), unit,
time_source, quality, source_domain, session_id, source_seq, schema_version
transaction: request_id, ecu, transport, service, req/resp, nrc, pending_count,
             t_request/t_response, timing_quality, risk, state
```

CAN 팀은 입력 adapter/renderer/query만 소유하고 UART line parsing·UDS DID/NRC 의미는 각 팀이 소유한다.
TUI 최소 UDS trace는 UDS 팀, 공통 web transaction renderer는 CAN/viz가 제공하는 분담에 동의한다.
진단 요청/응답↔CAN frame↔UART event를 연결할 `correlation_id/source_seq_range`를 선택 필드로 제안한다.
서로 다른 host/clock의 단순 timestamp 정렬은 정밀 동시성 보증이 아니므로 정렬 품질을 표시한다.
기능/필드 합의는 리드가 `_xteam`에 반영하고 각 팀 R2에서 versioned fixture로 검증한다.

## 핵심 원칙 체크리스트 자기점검

- [x] **맨 앞 핵심 루프 표**: §0에서 직접 접근/읽기/쓰기/사용자 실시간 표시·개입을 명령·구성요소·결과에 매핑했다.
- [x] **종단 간 구체 시나리오**: §2.1에 agent send→ECU 응답→200ms Trace 목표→user TUI/IG 송신→agent의 origin=user 조회를 적었다.
- [x] **실시간 목표와 고부하 진실성**: §3.5·§9.4에 Trace<200ms 목표·배칭·N프레임 생략·다운샘플 명시·정상 무손실 기록과 장애 gap을 정의했다.
- [x] **M1 walking skeleton**: §11 M1과 §12 T0–T4는 virtual daemon+send/trace+live TUI+사용자 송신·자동 기록을 최우선으로 둔다.
- [x] **같은 세션/스트림, 숨은 연결 금지**: §0·§2.5·§3.1/3.4·§9.1에서 단일 장치 owner와 원자 snapshot+live·늦은 viewer 접속을 필수로 했다.
- [x] **부수 기능의 후순위/비범위 표시**: §1.3/1.4·§11 M2–M5에 고급 DB/포맷/미려한 UI/CANoe·XIL·Qt 확대를 M1 이후로 구분했다.

위 체크는 **계획서 반영 여부**의 자기점검이며 제품 구현/성능 검증 완료 표시가 아니다.
<!-- END R1 -->
