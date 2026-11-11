# ④ 탐색(Explore) 계획 — Loom에 추가로 만들 도메인

작성: explore-lead (R3 통합), 2026-11-09 · v1.1 갱신 2026-11-10(마스터 §2 D1–D14, §2.1 A1–A16 반영, §0.3, 결정 D22–D29) · **v1.2 갱신 2026-11-10**: 마스터 §2.2 **B1–B7**(사용자 답변, A·D보다 우선) 반영 — 실HW 호스트 Windows x64 + Linux x64(B1), 보유 장비 = Saleae LA·Vector/CANoe·PCAN·USB-시리얼(B2), 실차 송신 기본 차단 폐기(B4), **LA = Saleae Logic 2 자동화 1순위·P2**, PSU는 SimPsu 계약 검증만, 스코프 보류(B5). 결정 D30–D33. **v2.0 갱신 2026-11-10**: 마스터 §2.3 **C1–C8**(C > B > A > D) — 코어·장치 경로 Rust, 사용자 화면 = 웹 UI 앱(1급), 통합 테스트(EXPLORE 팀이 TEST 기획 담당 → `docs/plan/test/plan.md`). 탐색 도메인 적용은 **§0.4**가 정본이며, 아래 본문의 Python 라이브러리·entry point·TUI 서술은 §0.4로 대체된다(큰 재작성 대신 바뀌는 점만 적음). 결정 D34–D36. 충돌 시 마스터 계획이 우선이다.
입력: `r1-opus.md`, `r1-sol.md`, `r2-opus-review.md`, `r2-sol-review.md`, `r2b-opus-scope-la.md`, `r2c-sol-scope-la-check.md`, `user-notes.md`, 세 팀 `plan.md`(uart/can/uds)와 `_xteam/*`
기준: `02-core-principle.md`(최우선) → `00-brief.md` §3 → `explore/brief.md`
성격: **계획 문서다. 구현 코드는 없다.** 사실 확인 원자료는 `spikes/explore/` 아래에 있다(PyPI/GitHub/공식 문서 원본, 백엔드 6종 왕복 스파이크, R2 재확인). 지연·처리량 수치는 별도 표기가 없으면 **설계 목표**이고 실측이 아니다.

---

## 0. 핵심 루프 — 추천 도메인별 4요소

> 에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자는 같은 데이터를 같은 순간에 본다(출처 agent/user/device/script, 사용자 개입 가능).

| 추천 | ① 직접 접근 | ② 읽기 | ③ 쓰기 | ④ 사용자 실시간 표시 + 개입 |
|---|---|---|---|---|
| **1. 벤치 전원** `loom psu` (+`loom io`) — v1.2: 실HW 미보유, **SimPsu로 플러그인 계약 검증 전용**(B5), 실장비는 구매 시 | `loomd`가 VISA 리소스(LAN SCPI 우선, pyvisa-py)를 단독 소유. IO는 릴레이 보드(HID/시리얼/Modbus RTU) | `psu meas/watch --for/wait-for --timeout`: 설정값(`VOLT?`)과 실측(`MEAS:VOLT?`)을 구분, 실측 주기·age 표시 | `psu set`(한계 안), `psu out on/off`, `psu cycle`; `io set/pulse` | PSU 계기판(설정/실측/출력 램프) + 전류 플롯 + 명령 행(origin 색). 사용자: **비상 OFF 키 `!`**(확인 없음, A3), 값 편집 → origin=user |
| **2. DLT 라이브 로그** `loom dlt` | dlt-daemon TCP 3490 연결 1개를 `loomd`가 소유 | `dlt tail/wait-for/contexts`: APID/CTID/레벨 필터, MCNT 불연속은 `source.gap` | 제어 메시지 `set-level`(0x01), `get-log-info`(0x03). 결과를 `confirmed`(ACK)와 `effective_verified`/`effect_unverified`(반영 관측)로 분리(A14) | DLT 트레이스 + 컨텍스트 트리(요청 레벨/보고 레벨/대기). 사용자: 컨텍스트 레벨 변경, 마커 |
| **3. 타깃 셸** `loom shell` (SSH 먼저) | `loomd`가 SSH 연결 1개(host key 고정)를 소유, 채널 다중화 | `shell read --since/expect --timeout`, exec stdout/stderr/exit | 공유 PTY 입력(`send`, 대화형 입력 스트림 클래스 A1-③), 이름 붙은 exec 채널(`exec --argv`, 요청형) | UART 팀 공유 패키지 `loom/console/` 콘솔 위젯(A4). PTY 탭 + exec 채널 탭 자동 표시. 사용자: 같은 PTY에 직접 타이핑(입력 lease) |
| **4. 디버그 프로브** `loom debug` | `loomd` 워커가 프로브 단독 소유(pyOCD/J-Link, 비Cortex는 TRACE32 원격). 기본 **attach, halt·unlock 없음**(옵션 강제) | `debug read/watch --for/rtt read`: 심볼 단위 RAM 읽기, `coherent`·`source=device` 표시 | 허용 RAM 쓰기, RTT down 채널, halt/resume(R3 확인) | Watch 표 + 플롯 + RTT 콘솔 + 코어 상태 램프. 사용자: resume/halt, 허용 변수 편집, RTT 입력 |
| **5. 로직 애널라이저** `loom la` (v1.2: **Saleae 1순위**, sigrok 2순위; 스코프 보류) | la 워커가 Logic 2 앱(자동화 서버, gRPC)에 연결 1개를 소유하고 앱 프로세스를 감독(B5). 사용자 보유 Saleae | **캡처 단위**: 캡처(시간·트리거) 종료 후 원시 전이 + 내장 분석기 표(UART/SPI/I2C/CAN)를 내보내 회수. **캡처 중 스트리밍 불가**(B5) | 장비 설정(채널·샘플레이트·트리거·시작/정지) — 단독으로는 △(Saleae는 출력 없음). **다른 도메인의 쓰기(UART/CAN/UDS)로 자극하고 관측해 루프를 닫는다** | **캡처 중**: Loom은 캡처 상태·경과·자극 이벤트를 실시간(파형은 없음 — 배지로 명시. Logic 2 앱 창의 실시간 갱신은 미확인, M0 gate에서 확인). **캡처 후**: Loom 타임라인 디지털 레인 + 디코드 표(자극 이벤트와 같은 축). 사용자: Loom 패널에서 정지·트리거 변경·분석기 추가 |

모든 도메인 공통(§6 플러그인 계약 + 마스터 D2/D3/A14가 강제):
- 장비는 `loomd` 하나가 소유한다(gateway + 장치 worker, D1). 에이전트 CLI·TUI·웹 뷰어는 모두 같은 JSON-RPC/WebSocket 클라이언트이고, origin은 **서버가 연결 자격(role)으로 부여**한다(D2).
- 늦게 연 뷰어는 `core.subscribe`의 원자적 snapshot(`snapshot_part n/N → snapshot_end{stream_seq}`) + `after_seq` 이음매를 받는다(D2, A5). 에이전트 전용 숨은 연결은 없다.
- 쓰기는 `requested → accepted → sent → confirmed → effective_verified | effect_unverified`, 어느 단계에서든 `failed`/`outcome_unknown`(A14)을 같은 `request_id` 행으로 갱신하고, **publish-before-reply**로 뷰어에 먼저 낸 뒤 에이전트에 응답한다. `sent` = 드라이버·소켓에 넘김(wire 완료 아님).
- 모든 CLI 결과에 `others_since_last`가 붙어(D5) 사용자 개입을 에이전트가 반드시 본다.

### 0.1 종단 간 시나리오 (대표 — 도메인별 상세는 §5)

**"콜드 부팅 이상 전류를 사용자가 끊고, 에이전트가 원인 로그를 찾는다"** (PSU + DLT, 시뮬레이션 또는 실벤치 — v1.2: PSU 실장비 미보유라 M1은 SimPsu)

```
t=0      에이전트  loom psu set bench --volt 13.5 --curr 3.0
         loomd     벤치 프로파일 한계(vmax 16, imax 5) 안 → R2. env=sim이면 auto, bench는 사용자가 이 동작을 auto로 완화해 둔 경우 자동
                   (아니면 exit 5 E_CONFIRM_REQUIRED + 토큰 → loom psu confirm <tok> 또는 뷰어 Y, A8)
                   이벤트 {domain:psu, origin:agent, state:requested→accepted→sent}
                   SCPI "VOLT 13.5;CURR 3.0" → 되읽기 VOLT? 13.500 일치 → confirmed → effective_verified
         사용자 화면(웹/TUI, 수신→표시 p95 ≤ 100 ms 목표): 설정 13.50 V / 3.00 A, 행 "agent set ✓"
t=+2 s   에이전트  loom psu out bench on          (기본 R3 → 뷰어 확인 배너 → 사용자 [y])
         화면      출력 램프 ● ON (agent, approved by user), 전류 플롯 0 → 0.35 A, "poll 5.0 Hz, age 120 ms"
t=+3 s   에이전트  loom dlt wait-for ecu1 --grep 'boot complete' --timeout 20s
t=+5 s   DUT       전류 1.8 A로 상승. 화면: 플롯이 warn 1.5 A 선을 넘음 → 배너 "1.80 A > warn 1.50 A"
t=+5.4 s 사용자    PSU 패널에서 `!` (비상 OFF, 확인 없음, 인터록은 경고만 — A3) → {origin:user, cmd:out off}
t=+5.5 s 에이전트의 wait-for 반환: exit 6, error.code=E_INTERRUPTED_BY_USER
         {"ok":false,"error":{"code":"E_INTERRUPTED_BY_USER",...},"data":{"last":{"v":13.49,"i":1.80}},
          "others_since_last":{"count":1,"origins":["user"],"from_cursor":"seq:812"}}
t=+6 s   에이전트  loom dlt tail ecu1 --level warn --since -6s --max-lines 50
         → "PMIC: OV fault on BUCK2" 를 찾고, 사용자 OFF를 존중해 재인가하지 않고 보고
```

### 0.2 사용 시나리오 목록

에이전트 관점
1. **콜드 부팅 이상 전류 원인 찾기** — PSU 인가 → DLT/UART 부팅 로그 대기 → 이상 전류 시 사용자 OFF 존중 → 로그에서 원인(§0.1).
2. **슬립(암)전류 측정** — `io set kl15 off` → `psu wait-for --curr-lt 0.005 --hold 5s` → 실패 시 DLT에서 깨어 있는 앱 찾기(§5.1 b).
3. **부팅 정지 단계 좁히기** — `debug watch` + `rtt read`로 단계 변수와 로그 확인 → 사용자 승인 하에 halt → 레지스터 확인(§5.4 b).
4. **보낸 것이 선에 나갔는지 확인**(v1.2) — `la capture --trigger` → `uart send`(또는 CAN·UDS 송신) → `la wait`로 디코드와 송신 이벤트 정렬(§5.5 L-b). (스코프 전원 레일 측정 §5.5 S b는 보류)

사용자 관점
1. **뷰어에서 비상 OFF** — 에이전트 작업 중 PSU 패널 `!` → 에이전트 대기 명령이 즉시 exit 6 `E_INTERRUPTED_BY_USER`로 반환. (STOP `Ctrl-] x`는 쓰기만 잠그고 출력은 유지 — "STOP — 출력 유지 중" 배지, A3)
2. **DLT 컨텍스트 레벨을 직접 내림** — 에이전트가 올린 debug 레벨을 컨텍스트 트리에서 info로 → 에이전트가 다음 읽기에서 인지하고 덮어쓰지 않음.
3. **LA 캡처를 직접 멈추고 재디코드**(v1.2) — 에이전트가 건 루핑 캡처를 사건 직후 패널 `s`로 정지 → 직전 구간이 표시되고, baud를 바꿔 재디코드 → 에이전트가 `others_since_last`로 인지하고 사용자 설정을 그대로 사용.
4. **셸에 직접 타이핑** — 에이전트가 쓰는 같은 PTY에 사용자 명령을 입력(사용자 lease 우선) → 에이전트가 `read --since`로 결과까지 읽음.

### 0.3 v1.1·v1.2 공통 계약 정렬 — 탐색 도메인 적용표 (마스터 §2 · §2.1 · §2.2)

| 마스터 결정 | 탐색 도메인 적용 |
|---|---|
| D1 프로세스 | psu/io/dlt/shell = gateway 모듈, **debug·la = worker**(`worker=True`), AD3 = 공유 owner worker 1개 |
| D2/A5 IPC·크기 | 요청과 `core.events` 배치 1개 64 KiB 상한 → 스코프 캡처 이벤트는 데시메이션 ≤ 2k점 + 원본 `raw_ref`(같은 오리진 HTTP GET), LA 원시는 이벤트에 싣지 않음. RPC 이름 `<domain>.<verb>`/`<domain>.<noun>.<verb>` snake |
| D3/A13 이벤트 | 필수 필드 세트 사용, `metrics[{name,value,unit}]`, `quality[]`. 손실 3층: `display.gap` / `record.gap` / **`source.gap{cause}`**(DLT MCNT·buffer overflow, RTT 드롭, sigrok overrun, SCPI 응답 누락) |
| A14 쓰기 상태 | `requested→accepted→sent→confirmed→effective_verified|effect_unverified`, `failed`/`outcome_unknown`. DLT 레벨=confirmed→effective(2 s 관측창), PSU set=되읽기로 effective, 셸 send=`sent` 종단 |
| A1 쓰기 클래스 | 요청형: psu set/out, dlt level, shell exec, debug write/halt, scope arm/awg. **잡형**: psu watch 폴링은 읽기라 해당 없음, `la pattern`·`io seq`·AWG 연속 출력·`dlt` TTL 레벨은 잡(start/update/renew/stop만 게이트). 대화형 입력 스트림: shell PTY·RTT 콘솔의 사용자 키 입력(lease 시 1회 R1) |
| A2 도메인 훅 | `classify`(셸 문자열·debug 주소·IO 채널), `approval_binding`(PSU 채널+값, debug MCU ID·주소 범위·ELF 해시), `presend_check`(인터록 재확인 — 플래시 중 PSU/IO 전원 차단 시 승인 무효 + `E_LOCKED`) |
| D5/A6 CLI | `--json` 봉투, `others_since_last` 모든 결과, 하드 1,000행/64 KiB(탐색 기본 100행, `events` 기본 tail 50), 커서 `@last|seq:N|mark:X|-5s|시각`, `LOOM_CLIENT`, 종료 코드 D5 표 — 사용자 중단 6, 정책 4, 승인 5, 장치 7(`E_NO_RESPONSE` 포함), timeout 3은 조건 대기형만. `wait-for` 최대 3600 s, `--follow` 600 s |
| D6/A8 R2 | 동작 단위 `auto|token_or_viewer|viewer`. sim=auto, bench·vehicle 탐색 기본 `token_or_viewer`(사용자가 동작 단위로 auto 완화 가능, 뷰어 승인 후 — v1.2 B4로 vehicle도 bench와 같음). 모든 탐색 CLI에 `loom <domain> confirm <tok>` |
| D6/A9 R3 | 뷰어 승인만(once/session, 사용 기한 10분). 승인된 잡은 `max_job_duration`(기본 1 h) 안에서 renew |
| D6/A11 R4·차단 | R4 = 매번 뷰어 승인(scope once). `default_blocked`: debug flash·주변장치 쓰기, DLT 영속 설정·통신 인터페이스 변경, 셸 efuse 류. **`immutable_deny`**: debug RDP·보안 잠금·옵션 바이트·OTP·mass erase·auto-unlock. 하드 차단 deny list는 설정+뷰어 승인으로만 |
| ~~A10 vehicle~~ → **B4** | v1.2: vehicle의 위험 등급·R2 경로·기본값은 bench와 같다(A10 기본 deny·A8 vehicle=viewer 폐기). 남는 것: 모든 탐색 뷰어·CLI 봉투에 `env=vehicle` 배지 상시 표시, R3/R4 뷰어 승인은 환경과 무관하게 유지, 하드 차단은 사용자가 설정한 것만(기본 비어 있음) + 도메인 `immutable_deny` |
| A12 기록 불능 | agent·script의 새 R1+ 쓰기만 중단, 사용자 비상 OFF·입력은 허용 + `record.health=failed` 배너 |
| A3 STOP | 쓰기·잡 latch, 출력 유지(PSU·릴레이·AWG·halt). 도메인 패널 사용자 비상 OFF 키 `!` |
| D6 정책 2층 | 프로젝트 `loom.toml`(`[bench]`·도메인 섹션) + 사용자 `~/.loom/config/policy.toml`. 완화는 뷰어 승인 후, 강화 즉시, 해시 기록 |
| D7 기록 | `~/.loom/records`, 30일·20 GiB, `--no-record`여도 감사 기록 |
| D8/D9 데몬·뷰어 | CLI 자동 기동, 장치 자동 open 안 함. M1 뷰어 = 도메인 Textual TUI(접두키 `Ctrl-]`), 웹 패널은 CAN viz `register_panel` 경유 P3 이후. heartbeat·`LIVE/LAG/STALLED` 배지, `no_viewer_attached` 경고 |
| A15 finally | 정상 종료·on_error·agent cancel → finally(R0–R1 + `safe_state`). 사용자 STOP이면 자동 실행 안 하고 뷰어 "복구 송신" 1키. safe_state도 인터록 통과 필수, 막히면 `finally_blocked` |
| A4 콘솔 | 셸·RTT 콘솔은 UART 팀 `loom/console/` import |
| D13 스킬 | `skills/` 원본 → `loom skills sync`로 `.claude/skills`·`.agents/skills` 복사 |
| **B1 플랫폼** (v1.2) | 실HW 호스트 = **Windows x64와 Linux x64**(데몬+에이전트가 장비 PC에서 실행). macOS는 개발·시뮬레이션(Saleae 시뮬레이션 장치 포함)만. 실HW 체크리스트·수용 시험은 두 OS 모두(§5.6.1) |
| **B2 보유 장비** (v1.2) | 탐색 관련 보유 = **Saleae LA(모델 미확인)** 뿐. PSU·스코프·디버그 프로브·릴레이는 없음 → 실장비 M2는 LA만 일정에 넣고, 나머지는 "구매 시" |
| **B3 CANoe** (v1.2) | CANoe 브리지는 CAN·UDS 소유(탐색 후보 19는 CAN 귀속 확정). 탐색 영향: LA로 CAN 트랜시버 TTL쪽·UART를 디코드해 CANoe/UDS 송신을 **물리 신호로 교차 확인**하는 시나리오(§5.5 L-b) |
| **B5 LA·계측** (v1.2) | **LA 1순위 = Saleae Logic 2 자동화 API**, P2로 앞당김. 캡처 중 스트리밍 불가를 화면 설계에 반영(§5.5 L). sigrok 2순위, 스코프 보류, PSU = SimPsu 계약 검증 전용, 계측기 추가 구매 없음 |

### 0.4 v2.0 적용 — Rust 이식·UI·통합 테스트 (마스터 §2.3 C1–C8, 이 절이 본문보다 우선)

사실 근거: `docs/plan/test/r1-sol.md` §B(crates.io 2026-11-10, 출처 URL), 스파이크 `spikes/explore/v2-saleae-rs/`(Saleae proto → Rust 빌드·무앱 연결 실패 처리 성공).

**C1 언어 — 도메인별 Rust 경로 (Python 예외 없음)**
| 도메인 | v1.x (Python) | **v2.0 Rust 경로** | 버전 · 라이선스 | 남은 관문 |
|---|---|---|---|---|
| DLT | pydlt / 자체 codec | **자체 Rust 제어 codec**(TCP 프레이밍, set-log-level 0x01, get-log-info 0x03) + `dlt-core`를 verbose/FIBEX 읽기 재사용 후보로 비교 | dlt-core 0.20.2 · Apache-2.0 (제어 클라이언트는 없음 — 서비스 이름표만) | P0 DLT M0 gate를 Rust로: dlt-daemon 3.0.1 fixture로 endian·분할·ACK/반영 분리 |
| SSH | asyncssh | **`russh`** client + 같은 crate로 loopback sim 서버 | 0.65.1 · Apache-2.0 | PTY + 2 exec 동시 출력·채널 상한·host key 고정. `ssh2`(blocking 중재 필요, 서버 없음)·`openssh`(Windows 불가) 기각 |
| LA (Saleae) | logic2-automation(Python) | **`tonic`/`prost`로 공개 proto(`proto/saleae/grpc/saleae.proto`, Apache-2.0) 생성 클라이언트**, 앱 `--automation` 기동·감독도 Rust | tonic 0.14.9 · MIT, prost 0.14.4 · Apache-2.0 | Python Manager의 버전 검사·캡처 핸들 수명·오류 변환을 Rust로 옮김. 앱 GetAppInfo → F4241 캡처·내보내기 실제 왕복, Windows/Linux 빌드(M0) |
| LA (sigrok 2순위) | sigrok-cli 하위 프로세스 | 그대로(`std::process::Command`), 언어 무관 | sigrok-cli 0.7.2 · GPL-3.0+ (하위 프로세스) | 변경 없음 |
| Debug | pyOCD + pylink | **`probe-rs`**(CMSIS-DAP·ST-Link·J-Link, RTT) — 일반 `attach` + `Permissions::new()`(erase_all 금지), `attach_under_reset` 자동 대체 금지, `probe-rs run` 사용 금지 | 0.32.0 · MIT/Apache-2.0 | **일반 attach도 타깃별 init/unlock 시퀀스를 부르므로 무정지·무변경이 전 MCU에 보장되지 않음** → MCU·프로브 허용 목록별 무 halt/reset/erase 실측 통과 후에만 R0 attach. Windows J-Link는 WinUSB 필요(SEGGER 드라이버 공존 실측) |
| Debug (TRACE32) | lauterbach-trace32-rcl(Python) | **C Remote API FFI**(`hremote.c` 등 + `extern "C"`), PowerView 실행 필요, 호출은 전용 스레드에서 직렬화 | Lauterbach 자체 조건(MIT 아님) | 빌드·왕복 미확인, 수정본 배포 제약 보존 |
| PSU·스코프 (구매 시) | pyvisa-py | **raw TCP SCPI**(tokio TcpStream + deadline + 응답 상한), VXI-11/HiSLIP/USBTMC 필요 시 `visa-rs`(설치된 VISA에 링크) | tokio 1.54.1 · MIT / visa-rs 0.7.1 · MIT/Apache-2.0 | 기종 방언·binary block 실측 |
| IO (구매 시) | hidapi(Py)·pymodbus | `hidapi` crate, `tokio-modbus`(TCP·RTU, server 기능으로 sim) | 2.6.7 · MIT / 0.17.0 · MIT/Apache-2.0 | 보드 report 규약·되읽기 |
- **Python 어댑터가 필요한 탐색 도메인은 현재 없다**(결정 D34). C1의 "왜 Rust로 안 되는가" 행이 필요 없다. 벤더 런타임(Logic 2 앱, WaveForms, TRACE32 PowerView, VISA)은 외부 프로그램이며 Python이 아니다.
- **배치(C2 크래시 격리)**: 벤더 DLL·외부 앱에 묶인 경로(probe-rs USB, TRACE32 C API, VISA 링크, Logic 2 앱 감독)는 **worker 프로세스**. DLT·SSH·raw SCPI는 순수 Rust async라 gateway 안 crate 후보(최종 기준은 CORE 결정). 본문 §6.2의 Python entry point Registry는 폐기 → Rust 도메인 crate 등록 + worker 프로토콜(C3, CORE 소유). 위험 표·`safe_state`·`r2_default`·hook 의미(A1/A2, CR-X11)는 그대로 Rust 선언으로 옮긴다.

**C2 큐·버퍼 상한과 메모리 예산 (설계 기본값, M1에서 실측 조정)**
| 도메인 | 큐·버퍼 | 넘칠 때 | 메모리 예산 | soak 부하 패턴 |
|---|---|---|---|---|
| DLT | 수신 링 64k 메시지 / 32 MiB, 제어 요청 큐 16 | 화면 경로만 drop(`display.gap`), 저널은 디스크로 흘림(메모리 누적 금지), 디스크가 못 따라가면 `record.gap` + R1+ 쓰기 중단(A12) | 64 MiB | 10k msg/s 24 h + 1분마다 set-level 왕복 |
| SSH | 채널당 출력 링 1 MiB, 결과 exec 출력 64 KiB, 세션 ≤ 4 | 화면 축약 + 숨긴 바이트 수, 저널 무손실 | 64 MiB | `journalctl -f` 폭주 + PTY flood(UART flood와 같은 패턴) |
| LA (Saleae) | 캡처 1회 `max_capture_s`·버퍼 상한(프로파일), 내보내기는 디스크 파일 → **청크 단위 파싱**(전체 적재 금지), 디코드 행 이벤트 배치 ≤ 64 KiB | 프로파일 밖은 거부, 메모리 부족 조기 종료는 `quality:truncated` | worker 256 MiB | `la watch --repeat` 10 s 창 24 h(앱 메모리 증가도 관찰) |
| Debug | watch 샘플 링 100k, RTT 채널당 1 MiB | 화면 다운샘플 표시, 저널 무손실, RTT 타깃 드롭은 `source.gap` | worker 128 MiB | 4변수×20 Hz + RTT 50 KB/s 24 h |
| PSU/IO | 장치당 미완료 질의 1, 응답 상한 64 KiB | 질의 생략 + 실측 주기 표시 | 16 MiB | 5 Hz 폴링 24 h |
- worker 예산 초과 → 감독자가 그 worker만 재기동(새 epoch, 진행 중 쓰기 `outcome_unknown`), 데몬·다른 worker·저널은 유지(C2).

**C6 사용자 화면 = Loom UI 앱 (TUI는 보조)**: 본문 각 §5.x e의 "M1 = Textual TUI"는 **웹 UI 패널**로 바뀐다. 패널 내용 명세는 `_xteam/explore-ui.md`(UI 팀 템플릿 형식): `dlt.trace`+컨텍스트 트리, `la.capture`(상태·레인·디코드 표), `psu.gauge`, `shell.console`(UART `uart.terminal` 재사용), `debug.watch`(구매 시). 러너 run 패널은 UI 공통 셸의 **테스트 실행 화면**을 "run 모드"로 재사용한다(`_xteam/ui-test.md`).

**C7·C8 통합 테스트·스킬**: 탐색 도메인도 대기·조회·쓰기 결과에 증거 REF(`data.evidence[]`)를 넣는다 — `dlt wait-for` 로그 seq, `la wait` 캡처·행(`la:<dev>@capture:<n>#row:a..b`), `psu wait-for` 측정 seq, 쓰기 기준점 `psu.out.on`·`dlt.ctrl`. 도메인 스킬에 "TC 단계로 쓸 때" 절 추가, 명령 상세는 `loom <domain> help --json`이 정본. 교차 러너(§7.1)는 테스트 세션 엔진과 하나로 합친다(test/plan.md 결정 T2). SimWorld(R-b)에 UART 팀 요청 `on_external: ecu_reset`(UDS 가상 ECU 리셋 → UART sim 부팅 로그)을 넣는다. 마스터 C4.1(DoIP = PC NIC + 미디어 컨버터)은 탐색 도메인에 영향 없음.

**M1 재정의 (Rust + 웹 UI)** — 단계 배치는 마스터 v2.1에서 다시 묶임
| 도메인 | M1 걷는 뼈대 |
|---|---|
| PSU (계약 검증) | Rust SimPsu + `loom psu` CLI + 웹 `psu.gauge`에서 에이전트 set/out → 화면 → 사용자 `!` 비상 OFF → 에이전트 exit 6. 첫 도메인 crate로 CORE 계약 검증 |
| DLT | Rust codec + SimDltServer(Rust) + 웹 `dlt.trace`: 에이전트 level → `confirmed`/`effective_verified` → 사용자 레벨 변경 → `others_since_last` |
| LA (Saleae) | Rust tonic 클라이언트 + SimLa(Rust, Saleae 내보내기 형식) + 웹 `la.capture`: 캡처 상태 실시간 → 결과 레인·표 → UART 송신 정렬 → 사용자 정지·재디코드 |
| SSH (P3급) | russh client + russh sim 서버 + `uart.terminal` 재사용 패널 |

---

## 1. 요약 (결론)

**추천 5개와 착수 순서** (v1.2 — 사용자 보유 장비 B2·B5 반영, 결정 D30)

| 순위 | 도메인 | 한 줄 이유 | v1.2 위치 · 착수 gate |
|---|---|---|---|
| 1 | **DLT** `loom dlt` | 사용자가 이미 COVESA DLT 3.0.1을 쓴다(`dlt-wrapper`). 로그 읽기 + 런타임 레벨 변경이 **장비 구매 없이** 성립 | P0 **M0 gate**(네이티브 dlt-daemon 3.0.1 set-level 왕복 + 앱 반영 관측) → P2 M1 |
| 2 | **로직 애널라이저** `loom la` — **Saleae Logic 2 자동화 1순위**(B5), sigrok 2순위 | **사용자 보유 장비**. UART/CAN-TTL/SPI/I2C 물리 신호를 UART·CAN·UDS 송신과 같은 타임라인에 놓아 "보낸 것이 실제로 선에 나갔는가"를 확인. 단 **캡처 단위**(캡처 중 스트리밍 불가)라 4요소 중 읽기·표시가 △ — 이를 화면 설계와 수용 기준에 정직하게 반영(§5.5 L) | P2 M1(Saleae 시뮬레이션 장치 + 자체 SimLa), M2 = 보유 Saleae로 Windows·Linux 실기 |
| 3 | **타깃 셸** `loom shell` (SSH → ADB/telnet 후속) | Linux/QNX/Android 기반 ECU의 콘솔·journal. 구매 없음. UART 콘솔 UX 재사용으로 비용 최소 | P3. UART 팀 `loom/console/`(A4) 안정 인터페이스 |
| 4 | **벤치 전원** `loom psu` (패키지 `loom.instr`) + `loom io` | 가치는 높지만(전원 사이클·암전류) **실장비 미보유**. 작은 상태 sim으로 4요소를 가장 먼저 시연할 수 있어 **SimPsu = 첫 플러그인 계약 검증용 도메인**으로만 유지(B5) | P0 SimPsu → P1 PSU M1(sim). 실장비 M2·IO는 **구매 시** |
| 5 | **디버그 프로브** `loom debug` | 실행 중 변수 라이브 플롯 + RTT + halt/resume. 가치·위험 모두 최고, **프로브 미보유** | P4, 프로브 구매 시. 확인·인터록·`immutable_deny` 검증 후, MCU 계열 답(Q3) |
| 보류 | 스코프 `loom scope` (SCPI) | B5: 계측기 추가 구매 없음 → 계획(§5.5 S)만 보존 | 구매 시 재개 |
| 예비 | XCP (Ethernet 먼저) | 루프는 완벽하나 사용자 업무에 캘리브레이션이 있는지 미확인 | Q6 답이 "XCP slave·A2L·RAM 쓰기 권한 있음"일 때 |

**교차 기능 판정**: 시나리오 러너·통합 타임라인·녹화/재생은 새 장비 도메인이 아니라 `loom.core`/`loom.viz` 기능이다(§7). UDS 팀의 UDS 전용 러너는 폐기하지 않고, core 러너가 `uds.*` 어댑터로 부르는 방식을 요청한다.

**착수 순서**(§8, 마스터 §3.2 단계): **P0** 플러그인 계약 검토(→CORE) + DLT M0 gate + SimPsu → **P1** PSU M1(SimPsu, 첫 플러그인 계약 검증 — 세 팀 M1과 같은 단계) → **P2** DLT M1 + **LA(Saleae) M1** → **P3** 교차 러너 M1(단계 어댑터·SimWorld·run 패널·수용 시험) + SSH → **P4** Debug(프로브 구매 시). 스코프 보류, IO·PSU 실장비는 구매 시. 세 팀의 임계 경로를 막지 않는다.

**계측기 구매**(B5): 추가 구매 없음. 보유 Saleae로 LA를 하고, 저가 세트 A/B/C(§5.5 S k)는 구매를 다시 검토할 때의 참고 자료로만 남긴다.

---

## 2. 핵심 루프 적합성 기준표

### 2.1 4요소 판정 기준 (두 초안 통합 — 결정 D1)

| 요소 | ○ 성립 | △ 부분 | ✗ 불성립 |
|---|---|---|---|
| **직접 접근** | 지원 OS에서 `loomd`가 사람 손 없이 장비를 열고 끊기면 다시 연다. 벤더 SDK/드라이버 1회 설치는 허용. **OS 제한은 O 점수로 따로 감점** | 벤더 GUI 앱이 떠 있어야 함(Saleae Logic 2 등), 또는 **검증된 Python 경로가 없어 ctypes 래퍼를 새로 써야 함**(FlexRay), 또는 물리 버튼 조작 필요(6022BL 모드 전환) | 실시간 장비 없음(파일 분석), 사람이 매번 연결 |
| **읽기** | 경계 있는 명령(`--tail/--for/--timeout/wait-for`)으로 **장비에 실제 조회한** 해석 값을 얻음. 원시와 해석을 함께 기록 | 캡처 종료 후에만, 수 초 간격 스냅샷만, 또는 **호스트 명령 캐시**만(실제 조회 아님) | 기계적으로 회수 불가 |
| **쓰기** | **DUT 상태를 바꾸는** 입력: 전압·출력, 로그 레벨, 키 입력, 메모리, 릴레이, AWG/패턴 출력 | 관측기 설정만(트리거·타임베이스) — DUT 불변 | 쓰기 수단 없음(passive capture) |
| **실시간 표시** | 장비 이벤트 → 뷰어가 도메인 목표 지연(≤ 1 s) 안. 뷰어가 `loomd` 스트림을 구독하므로 에이전트와 같은 seq·origin을 봄. 뷰어에서 직접 개입 가능 | 갱신 ≥ 1 s 간격, 또는 라이브가 벤더 앱에만 있음 | 사후 파일만 |

"○" 판정은 **도메인 능력**이고, 별도 "검증 단계" 열로 실재성을 표시한다: `계획`(만들 예정) / `import`(설치·import 확인) / `sim 왕복`(시뮬레이터와 실제 왕복) / `HW`(실장비 확인).

### 2.2 점수 (결정 D2 — Opus 공식 채택)

`총점 = 30·V/5 + 30·L/8 + 10·E/5 + 10·M/5 + 5·H/5 + 5·O/5 + 10·R/5`

| 항목 | 척도 | 가중 | 비고 |
|---|---|---|---|
| V 사용자 가치 | 1–5 | 30 | 차량 임베디드 개발자의 하루(벤치·부팅 디버깅·진단·캘리브레이션) 빈도 × 절약. **사용자가 명시한 관심은 +1** |
| L 핵심 루프 | ○2 △1 ✗0, 합 0–8 | 30 | 원칙상 가치와 같은 무게 |
| E 구현 용이성 | 1–5 | 10 | |
| M 라이브러리 성숙도 | 1–5 | 10 | 최근 릴리스·라이선스·검증 가능 API |
| H HW 확보 | 1–5 | 5 | 5 = SW만/저가 |
| O OS | 1–5 | 5 | 5 = macOS + Windows (v1.2 B1: 실HW 기준은 Windows + Linux로 바뀜. 상위 후보는 모두 두 OS 경로가 있어 점수는 다시 매기지 않음) |
| R core 재사용 | 1–5 | 10 | |

Sol 공식(루프 40 + 가치 20)은 가치가 낮은 Modbus(84)가 가치 높은 릴레이(81)를 앞서는 역전을 만들어 기각했다(R2-opus §2).

### 2.3 탈락·제한 규칙 (점수보다 우선)

1. 직접 접근 ✗ 또는 실시간 표시 ✗ → 탈락.
2. ✗가 하나라도 있으면 상위 추천 불가.
3. △가 2개 이상이면 상위 추천 불가.
4. △가 1개인 후보는 상위 추천 가능하되, **M1 수용 기준에서 그 △를 보완하는 시연**(예: 관측기의 교차 자극, IO의 실제 상태 조회 보드)을 필수로 한다.
5. 기존 세 팀 범위 안이면 점수만 매기고 "귀속 요청"으로 넘긴다(이 팀이 범위를 바꾸지 않는다).
6. GPL 등 라이선스 문제는 탈락이 아니라 리스크 + 경계(하위 프로세스, 별도 배포) 대안으로 적는다. 하위 프로세스가 고지·소스 제공 의무를 없애 주지는 않는다(R2-sol F7).
7. 숫자를 채우려고 부분 루프를 상위에 넣지 않는다(R2-sol §4.1).
8. (v1.2, 결정 D31) **사용자 보유 장비 예외**: 사용자 답변으로 보유 장비를 쓰기로 정한 경우(B5 Saleae) 규칙 3을 넘어 상위에 둘 수 있다. 대신 △를 ○로 고쳐 쓰지 않고 표에 그대로 남기며, △마다 이를 보완하는 화면 설계와 M1 수용 항목을 둔다(§5.5 L). 점수는 바꾸지 않는다.

---

## 3. 범위 / 비범위

### 이 팀이 다루는 것
- UART/CAN/UDS 외 장비·프로토콜 도메인의 평가와 상위 5개 간이 계획.
- 새 도메인을 붙이기 위한 **도메인 플러그인 계약 제안**(§6) — 실제 소유는 core/viz이므로 "공통 계약 변경 요청"(§10)으로 제출.
- 교차 기능(시나리오 러너, 통합 타임라인, 녹화/재생, 공유 시뮬레이션)의 위치 판정과 UX 제안(§7).
- 새 도메인용 위험 분류와 **벤치 프로파일**(사용자 소유 한계값 파일) 제안.

### 기존 세 팀에 남기는 것
| 항목 | 팀 | 이 문서의 태도 |
|---|---|---|
| COM 포트, 입력 중재, 터미널 뷰어, expect | UART | 셸·RTT가 **콘솔 위젯과 Transport를 재사용**하도록 공개만 요청 |
| CAN 트레이스/DBC/viz/CANoe/ISO-TP, LIN·CAN XL(현재 CAN 비범위) | CAN | LIN/CAN XL은 "CAN 후속 귀속 요청"으로만. viz 패널 타입 추가 요청 |
| UDS 서비스/DoIP/플래시/UDS 시나리오 | UDS | 플래시 진행 상태를 인터록으로 노출 요청, UDS 러너는 core 러너 어댑터로 |

### 비범위 / 후순위
- 파일 분석 전용(MF4/BLF/A2L/pcap 오프라인 뷰어) — 장비 접근 없음. `loom.viz` 파일 입력이 이미 담당.
- FlexRay, CAN XL — HW·Python 경로 없음 / CAN 팀 귀속.
- passive Ethernet 캡처 — 쓰기 ✗. active SOME/IP는 후보 유지(70점).
- Modbus를 독립 도메인으로 — 릴레이·전자부하의 **전송 백엔드**로만.
- 각 도메인의 고급 기능(DLT non-verbose/FIBEX, injection, PSU 다중 기종, Debug flash, XCP DAQ/A2L, 스코프 전체 메모리 분석)은 각 도메인 M1 뒤.
- seed-key 우회, 무감독 flash, 보안 잠금 조작은 비범위. (실차 쓰기는 v1.2 B4로 bench와 같은 규칙으로 허용 — 비범위에서 뺌)

---

## 4. 후보 전체 평가표 (통합)

열: V / 루프(접근·읽기·쓰기·표시) / E / M / H / O / R / 검증 단계 / 기존 팀 중복 / 총점. 라이브러리는 2026-11-09 PyPI JSON 직접 조회(원자료 `spikes/explore/pypi-raw/`, `r2-sol/source-evidence.json`).

| # | 후보 | V | 루프 | E | 주 라이브러리 (버전 · 최근 릴리스 · 라이선스) | M | H | O | R | 검증 단계 | 중복 | **총점** |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **벤치 PSU (SCPI)** | 5 | ○○○○ | 4 | pyvisa 1.16.2 · 2026-02-27 · MIT; pyvisa-py 0.8.1 · 2025-09-04 · MIT; pyvisa-sim 0.7.1 · MIT | 5 | 4 | 4 | 4 | sim 왕복(pyvisa-sim set/query) | 없음 | **94** |
| 2 | **DLT 수신·제어** | 5 | ○○○○ | 3 | pydlt 0.3.5 · 2025-03-07 · MIT(V1 codec); COVESA dlt-daemon(MPL-2.0, 시뮬레이터용). ⚠ PyPI `python-dlt`/`dlt`는 무관 프로젝트 | 3 | 5 | 5 | 5 | codec 왕복만. **제어 왕복 미검증(M0 gate)** | 없음 | **92** |
| 3 | **타깃 셸 SSH** | 4 | ○○○○ | 4 | asyncssh 2.24.1 · 2026-10-04 · EPL-2.0 OR GPL-2.0+ (paramiko 5.1.0 LGPL-2.1 대안) | 5 | 5 | 5 | 5 | sim 왕복(로컬 SSH 1연결·2 exec 채널 라이브 출력) | UART 콘솔 UX 재사용 | **92** |
| 4 | **디버그 프로브** | 5 | ○○○○ | 2 | pyocd 0.46.1 · 2026-11-02 · Apache-2.0 + **pylink-square 1.7.0**(2.x와 공존 불가); lauterbach-trace32-rcl 1.1.6 · MIT; pyelftools 0.33 | 4 | 4 | 4 | 4 | import + QEMU GDB 정지 상태 SRAM 왕복 | 없음 | **88** |
| 5 | **관측 계측 – LA (sigrok)** (v1.2: 2순위 백엔드) | 4 | ○○△○ | 3 | sigrok-cli 0.7.2 / libsigrok 0.5.2 (GPL-3.0+, 하위 프로세스) — 정식 릴리스 정체 | 3 | 5 | 4 | 4 | 계획(sigrok 미설치, `demo` 드라이버 존재 문서 확인) | 없음 | **79** |
| 5' | 관측 계측 – 스코프 (SCPI, LAN) (v1.2: 보류) | 4 | ○○△○ (AWG 모델은 ○○○○) — 실시간 ○는 1–5 Hz 단발 캡처 반복·트리거→화면 ≤ 1 s 기준, M0 실측 전 잠정 | 3 | pyvisa(1과 공유) | 4 | 3 | 5 | 5 | pyvisa-sim `:WAV:DATA?` 왕복 | PSU 백엔드 공유 | **82** (AWG 86) |
| 5'' | 관측 계측 – Analog Discovery 3 | 4 | ○○○○ | 3 | dwfpy 1.2.0 · 2025-01-10 · MIT + WaveForms 런타임(독점, x86_64·arm64) | 4 | 3 | 4 | 4 | import만(런타임 미설치) | 없음 | **83** (LA/스코프의 백엔드) |
| 6 | 릴레이/IO 보드 | 4 | ○△○○ (실제 상태 조회 보드는 ○○○○) | 5 | pyhid-usb-relay 1.4.0 · MIT; hidapi 0.15.0; pymodbus 3.16.2 · BSD-3; pyserial 3.5; numato-gpio 0.14.0(Py<3.14 제한) | 2 | 5 | 4 | 5 | Modbus TCP 왕복만 | 없음 | **83** (조회 보드 87) → **1번에 묶음** |
| 7 | DMM / 전자부하 (SCPI) | 3 | ○○○○ | 4 | pyvisa | 4 | 3 | 4 | 5 | 계획 | 없음 | 81 → `loom.instr` 후속 |
| 8 | XCP (Ethernet 먼저) | 4 | ○○○○ | 2 | pyxcp 0.30.3 · 2026-11-06 · LGPLv3; pya2ldb 1.0.355 · **GPLv2** | 3 | 3 | 4 | 4 | 계획(slave 미검증) | XCP on CAN은 CAN 채널 공유 | 79 → **예비** |
| 9 | ADB shell/logcat | 3 | ○○○○ | 4 | adbutils 2.12.0 · MIT (adb 서버 필요) | 4 | 3 | 4 | 4 | 계획 | 셸 후속 백엔드 | 79 |
| 10 | FT232H I2C/SPI/GPIO | 2 | ○○○○ | 4 | pyftdi 0.57.2 · BSD-3 (Windows 공식 미지원) | 4 | 5 | 3 | 4 | 계획 | FTDI UART 채널 충돌 | 74 |
| 11 | HMI 영상 캡처 + 터치 주입 | 3 | ○○△○ | 3 | opencv-python, adbutils | 4 | 4 | 4 | 2 | 계획 | 없음 | 70 |
| 12 | Modbus 일반 | 1 | ○○○○ | 4 | pymodbus 3.16.2 | 5 | 3 | 5 | 4 | sim 왕복 | — | 70 → 전송 백엔드로만 |
| 13 | Bus Pirate / CH341 | 2–3 | ○○○○ | 2 | PyPI 이름 404(`pyBusPirateLite`, `ch341*`) — 다른 프로젝트 부재 증명은 아님 | 1–2 | 5 | 3 | 3 | 계획 | UART 포트 점유 | 62–70 |
| 14 | LIN | 3 | ○○○○ (Windows PLIN) / macOS ✗ | 2 | ldfparser 0.26.0 · MIT; PLIN-API(Windows DLL) | 2 | 2 | 1 | 4 | 계획 | **CAN 비범위 → 후속 귀속 요청** | 67 |
| 15 | SOME/IP active | 3 | ○○○○ | 2 | someipy 2.1.2(소스 GPL-3.0); scapy 2.8.0 GPL-2.0-only | 3 | 3 | 3 | 3 | import만 | DoIP와 NIC 공유 | 70 |
| 16 | CCP | 2 | ○○○○ | 1 | 검증된 최신 라이브러리 없음(pyXCP는 CCP 근거 아님) | 1 | 3 | 3 | 3 | 계획 | CAN | 58 |
| 17 | **Saleae Logic 2 Automation** (v1.2 갱신) | 4 | △△△△ | 3 | logic2-automation 1.0.11 · 2025-11-18 · Apache-2.0 — Logic 2 앱 실행 필요, 캡처 중 스트리밍 불가 | 3 | **5**(보유) | 5 | 3 | 계획(공식 문서 확인, `v12-saleae-check.md`) | — | **67** → v1.2: **LA 1순위**(규칙 8 보유 장비 예외, D31) |
| 18 | passive Ethernet pcap | 3 | ○○✗○ | 3 | scapy / dpkt 1.9.8 / pyshark 0.6 | 3 | 4 | 3 | 4 | — | — | 탈락(규칙 2) |
| 19 | CANoe 원격 제어 | 3 | ○○○△ | 3 | py-canoe 26.4.1 · MIT | 2 | 2 | 1 | 3 | — | **CAN 귀속** — v1.2 B3로 CAN P2 정식 산출물(`loom/canoe/`) | 63 |
| 20 | CAN XL | 2 | △△△△ | 1 | python-can 4.6.1에 XL 모델 없음 | 2 | 1 | 1 | 5 | — | **CAN 귀속** | 탈락 |
| 21 | FlexRay | 2 | △○△○ | 1 | 검증된 Python 래퍼 없음(Vector XL C API) | 1 | 1 | 1 | 3 | — | — | 탈락 |
| 22 | 파일 분석 도구 | 3 | ✗○✗✗ | 4 | asammdf, canmatrix, pydlt | 4 | 5 | 5 | 4 | — | viz와 중복 | 탈락(규칙 1) |

### 4.1 후보별 핵심 근거 (상위·경계 후보만; 나머지는 `r1-opus.md` §3.1, `r1-sol.md` §3)

- **PSU**: `VOLT/CURR/OUTP/MEAS` 공통 SCPI로 대부분 커버. 기종 방언(채널 선택, 응답 형식)은 드라이버 프로파일. 설정값 되읽기(`VOLT?`)와 물리 측정(`MEAS:VOLT?`)을 다른 quality로 라벨(R2-sol 합의).
- **DLT**: 서비스 ID 0x01/0x03/0x11/0x13/0xF04/0xF08은 사용자 로컬 COVESA 헤더와 일치. dlt-daemon 3.0.1 헤더에 V2 헤더 플래그가 있어 **M1은 V1만**. libdlt는 데몬 응답을 약 0.5 s 주기로 처리하므로(`dlt-wrapper` README) 레벨 반영을 100 ms로 약속하지 않는다.
- **SSH**: 같은 SSH 연결의 여러 exec 채널이 종료 전에 stdout을 흘리는 것을 로컬에서 확인(R2-sol F9). telnetlib는 Python 3.13에서 제거 → telnet 백엔드는 telnetlib3.
- **Debug**: pyOCD 0.46.1 API 기본값이 `connect_mode='halt'`, `auto_unlock=True`, `resume_on_disconnect=True`(설치본 `core/options.py`, R2-sol F1) → **Loom 백엔드 계약에서 반드시 덮어쓴다**. pyOCD는 Cortex-M 전용 → AURIX/RH850이면 TRACE32 원격 API.
- **LA/스코프**: 저가 장비 대부분의 "쓰기"는 장비 설정뿐(△). AWG/디지털 패턴 출력은 AD3, DSO2D15(AWG 1ch), SDS800X HD(옵션 AWG)만. 상세 §5.5 S.
- **Saleae (v1.2)**: 4요소 모두 △(앱 의존, 캡처 후 읽기, 출력 없음, 캡처 중 Loom 파형 없음)라 v1.1에서는 탈락이었다. 사용자가 보유 장비로 지정(B5)해 규칙 8로 상위에 올리되 △는 고쳐 쓰지 않는다. 상세 §5.5 L.
- **IO**: 저가 릴레이의 "상태 조회"가 래치 상태인지 접점 피드백인지 구분해야 한다. `commanded / device_reported / contact_verified / unknown` 4상태로 표시(R2-sol F6). 실제 장비 조회가 되는 보드는 읽기 ○, 호스트 캐시뿐이면 △.
- **XCP**: 루프 완벽. 단 XCP slave·A2L·RAM 쓰기 권한이 사용자에게 있는지 미확인이고 pya2ldb가 GPLv2. Ethernet 먼저, pyXCP의 CAN 백엔드가 CAN 데몬을 우회하면 채택 금지.

---

## 5. 상위 추천 간이 계획

공통 전제
- 위험 등급은 마스터 D6 + A8/A9/A11(결정 D7, D22): R0 관찰 자동 / R1 가역 저위험 자동 / **R2 확인** — 경로는 동작 단위 `auto|token_or_viewer|viewer`, sim=auto, bench·vehicle 탐색 기본 `token_or_viewer`(첫 호출 exit 5 `E_CONFIRM_REQUIRED` + 토큰 → `loom <domain> confirm <tok>` 또는 뷰어 Y; 사용자가 동작 단위로 auto 완화 가능, 완화는 뷰어 승인 후) / **R3 뷰어 승인만**(once/session, 사용 기한 10분, CLI approve 없음) / **R4 매번 뷰어 승인(once)**, 그중 `default_blocked` 항목은 `loom.toml unlock` + 뷰어 승인 후에만 승인 대상 / 하드 차단(deny list, 설정 변경 + 뷰어 승인으로만) / **`immutable_deny`**(설정으로도 해제 불가). 아래 등급표의 "immutable"이 마지막 층이다.
- 환경 축 `sim|bench|vehicle`(v1.2 **B4**, A10 폐기): vehicle의 등급·R2 경로·기본값은 bench와 같다. 실차 송신을 기본 차단하지 않는다. 남는 것은 모든 뷰어·CLI 봉투의 `env=vehicle` 배지 상시 표시, R3/R4 뷰어 승인(환경 무관), 사용자가 설정한 하드 차단(기본 비어 있음)과 도메인 `immutable_deny`.
- 승인 객체는 digest·epoch·profile hash·만료·횟수에 묶인 1회용. 도메인 훅 `approval_binding`(A2)이 장비별 필드를 더한다(PSU 채널+값, debug MCU ID·주소 범위·ELF 해시, IO 채널).
- 기록·감사 불능이면 agent·script의 새 R1+ 쓰기만 중단, 사용자 비상 OFF·입력은 허용(A12).
- 한계 초과는 **기본 거부**(clamp 아님). clamp는 명시 옵션이고 요청값/적용값을 분리해 보인다.
- 지연은 "host 수신 → 뷰어 렌더 확인(render ack)"로 측정한다. 장비 시각이 없으면 device→render는 "미확인"으로 둔다.
- 상세 화면 스케치와 확장 CLI는 `r1-opus.md` §4, `r1-sol.md` §4를 참조한다. 이 문서는 결정된 내용만 적는다.

### 5.1 벤치 전원 — `loom psu` (+ `loom io`)

**a. 핵심 루프** — §0 표 1행.

**b. 종단 간 시나리오** — §0.1. IO 변형: 에이전트 `loom io pulse relays btn_wake 200ms` → 장비 조회 상태 `device_reported on→off` 램프 + 타임라인 200 ms 막대 → DLT 레인에 "wakeup reason: BTN" → 사용자가 IO 패널에서 kl15 OFF(사용자 조작, 위험 채널은 실수 방지 대화상자) → 에이전트의 다음 명령 결과 `others_since_last`에 user 조작이 표시되고 `loom io events relays --origin user --since @last`로 상세를 읽은 뒤 슬립 전류 측정(`loom psu wait-for --curr-lt 0.005 --hold 5s --timeout 60s`)으로 진행.

**c. 실시간성 / 고부하**
- 측정 폴링 기본 **5 Hz, 장비당 미완료 질의 1개**(Sol). 실측 주기·age를 항상 표시하고 장비가 느리면 "요청 5 Hz → 실제 3.1 Hz"로 낮춘다.
- 수신 → 화면 p95 ≤ 100 ms. 장비 사건(보호 트립·앞패널 조작) → 화면은 **poll 주기 + 질의 왕복 + 렌더**가 상한이다(1 Hz 상태 폴링으로 500 ms를 약속하던 R1 표현은 폐기, R2-sol F5). SRQ/이벤트를 지원하는 기종은 그것을 쓴다.
- 타임아웃 샘플은 보간하지 않고 플롯에 gap + `source.gap{cause:"scpi_no_response"}`. "되읽기는 장비 내부 평균, 돌입전류는 스코프 필요" 고정 표기.
- 기록: 모든 명령·응답을 저널(`~/.loom/records`)에 무손실. 측정은 `metrics[{name:"current", value, unit:"A"}, …]`로 내보내 viz가 자동 플롯.

**d. CLI 초안**
```
loom psu connect <name> --visa RESOURCE [--channel 1] [--driver generic|rigol-dp|keysight-e36|…]
loom psu status <name> [--json]                 # IDN, 설정, 출력, 보호, 실측 폴 주기
loom psu meas <name> [--json]
loom psu watch <name> --for 10s [--summary|--raw --max 500]
loom psu wait-for <name> (--curr-lt A|--curr-gt A|--curr-between A,B|--volt-…) [--hold 2s] --timeout 60s
loom psu set <name> [--volt V] [--curr A] [--ovp V] [--ocp A] [--dry-run]
loom psu out <name> on|off [--dry-run]
loom psu cycle <name> --off 2s [--dry-run]
loom psu confirm <tok>                          # R2 토큰 재제출(A8)
loom psu events <name> [--origin …] [--since @last|seq:N|-5s] [--tail 50]
loom io connect <name> --driver hid-relay|numato|modbus-rtu [...]
loom io state|set|pulse|seq|wait-for|events|confirm …
```
출력 예:
```
$ loom psu watch bench --for 5s
bench ch1  13.49 V (set 13.50)  output ON (agent 14:10:02, approved by user)
current  min 0.312  max 0.388  mean 0.351 A   samples 24/25 (1 timeout)  rate 4.9 Hz
NOTE: 1 user action since your last command (seq:790, psu.output.set by user) — use --since seq:790
next: --since seq:812
```

**e. 사용자 뷰** (M1 = Textual TUI, 웹 패널은 P3 이후): 계기판(설정/실측/W, 출력 램프, 프로파일 한계), 전류·전압 스파크라인(웹에서는 `metrics` 자동 시계열), 명령 행, `LIVE/LAG/STALLED` 배지. IO는 채널 램프(이름·상태 4종·마지막 변경자). 키: **`!` 비상 OFF**(사용자 확인 없음, 인터록은 경고만 — A3), `o` 출력 토글, `v/c` 편집(적용 버튼 한 번에 한 트랜잭션), `Ctrl-] x` STOP(쓰기 latch, 출력 유지 배지 "STOP — 출력 유지 중").

**f. 라이브러리**: pyvisa 1.16.2 + pyvisa-py 0.8.1(LAN `TCPIP::…::SOCKET`/`INSTR` 우선 — macOS·Windows 같은 경로, USBTMC는 libusb 필요). NI-VISA는 Windows에 이미 있으면 선택. IO: hidapi/pyhid-usb-relay(MIT), pyserial, pymodbus.

**g. 시뮬레이션**: `SimPsu` 상태기계(설정/출력/보호, 실측은 부하 모델 + 잡음, 지연·타임아웃 주입) + `SimScpiServer`(TCP 5025로 같은 상태기계 노출 → pyvisa-py 경로까지 CI 검증). pyvisa-sim YAML은 방언 단위 테스트 보조(정적이라 동적 부하 불가 — 실측 확인). `SimRelay`는 상태 조회 있음/없음 두 모드.

**h. M1 걷는 뼈대** (SimScpiServer 위)
1. connect(IDN) → meas/watch/wait-for(경계) → set(한계 안 R2) → out on(뷰어 확인) → 뷰어 계기판·플롯·agent 행.
2. 사용자 `!` 비상 OFF → 에이전트 wait-for가 exit 6 `E_INTERRUPTED_BY_USER`로 반환, `others_since_last`에 user 조작 표시. STOP(`Ctrl-] x`) 시 출력이 유지되고 배지가 뜨는지도 시험.
3. 두 번째 뷰어를 작업 중간에 열면 snapshot + live로 같은 seq를 본다.
4. 한계 초과 `set --volt 30`은 송신 없이 거부(사유 표시). 응답 지연 주입 시 stale·gap 표시.
- IO는 PSU M1에 넣지 않는다(P2). IO M1의 루프 인정 조건: 장비 상태 조회가 되는 SimRelay 모드 + 실제 보드 1종.

**i. 안전**
| 동작 | 등급 | 규칙 |
|---|---|---|
| meas/watch/status | R0 | |
| set (프로파일 한계 안) | R2 | sim auto, bench `token_or_viewer`(사용자가 auto로 완화 가능 — 벤치 권장). 한계 밖은 확인으로도 불가(한계 완화는 정책 파일 변경 + 뷰어 승인) |
| out on | R3 | 마스터 D6 명시. session scope 승인(10분 사용 기한)으로 반복 사이클 부담 완화 |
| out off / cycle (에이전트) | R3 | `presend_check` 인터록: `uds.flashing`·`debug.flashing` 중이면 승인 무효 + `E_LOCKED`. 사용자에게 작업 취소/비상 OFF 선택지 표시 |
| 사용자 비상 OFF `!` | — | 확인 없음, 인터록은 **경고만**(A3). 이미 진행된 플래시를 되돌릴 수 없음을 배너에 표시 |
| io set 전원 계열(kl30/kl15) | R3 | 채널 등급은 `loom.toml`에서 사용자가 지정(강화만 즉시), 이름 없는 채널 기본 R3 |
| io seq / 반복 pulse | 잡형(A1) | start/stop만 게이트, 단위 토글은 worker가 승인 범위·레이트 재집행 |
| vehicle 환경 | bench와 같음(B4), `env=vehicle` 배지 | A10 폐기 |
| io 버튼·종단 | R1 | 레이트: **명령 횟수** 제한 = 채널당 ≤ 2 명령/s(펄스 1회 = 명령 1회), **전기적 최소 펄스폭**은 보드·릴레이 사양값(프로파일 `min_pulse_ms`, 기본 50 ms)으로 별도. 짧은 펄스가 화면에 보이는 지연과 실제 펄스 폭 정확도는 다른 수용 항목 |
- Registry `safe_state: true` 선언(A15): `psu.output.off`, `io`의 프로파일 지정 채널 상태(예: `kl15=off`). 이 동작도 인터록·deny list를 통과해야 하며 막히면 `finally_blocked`. 데몬 종료·연결 끊김 시 자동 적용은 하지 않음(기본 **현 상태 유지**). 재연결 시 새 epoch, 오래된 승인·대기 쓰기 자동 복원 금지.

**j. 리스크**: SCPI 방언·채널 선택 차이(보유 기종 Q1), macOS USBTMC 권한(LAN 기본 권장), 대전류 벤치 PSU(프로파일 한계 필수), 릴레이 보드 파편화·접점 피드백 부재.

### 5.2 DLT — `loom dlt`

**a. 핵심 루프** — §0 표 2행.

**b. 종단 간 시나리오** — "경고가 나는 컨텍스트를 debug로 올렸다 사용자가 내린다"
```
에이전트  loom dlt tail ecu1 --level warn --tail 20 --json      → CAMS/CAM "frame 812 late" 3건
에이전트  loom dlt level ecu1 --app CAMS --ctx CAM --level debug
          화면: 컨텍스트 트리 CAMS/CAM [요청 DEBUG][보고 INFO][대기], 트레이스 CTRL 행 [agent]
          state requested→accepted→sent, dlt-daemon 응답 → confirmed (예: 4 ms)
          debug 로그 관측 → effective_verified (관측창 최대 2 s; libdlt 0.5 s 주기 때문). 창 만료 시 effect_unverified
          debug 행이 흐름 — 수신→화면 p95 ≤ 150 ms
사용자    트리에서 CAM 우클릭 → info  → {origin:user, sid:0x01, level:info}
에이전트  (다음 명령 결과의 others_since_last에 user 1건) loom dlt events ecu1 --kind ctrl --since @last --tail 20 --json
          → 사용자 개입을 읽고, debug 로그 구간만 분석(--since/--until --max-lines 200). 사용자 변경을 자동 원복으로 덮어쓰지 않음
```

**c. 실시간성 / 고부하**
- 수신 → 화면 p95 ≤ 150 ms(1k msg/s). 제어 ACK(`confirmed`) 목표 ≤ 500 ms, 반영(`effective_verified`)은 별도 관측창 2 s. 데몬 무응답은 exit 7 `E_NO_RESPONSE` + `outcome_unknown`(A6).
- 부하 시험 프로필 10k msg/s × 256 B × 60 s(실 타깃 처리량 아님).
- **기록**: 원시 TCP 청크를 `wire.bin` + 청크 인덱스로 먼저 저장(무손실). 표준 `.dlt` export는 프레이밍 후 storage header(`DLT\x01` + 수신 시각 + ECU ID)를 생성해 따로 만든다 — TCP 수신 바이트가 곧 `.dlt` 형식은 아니다(R2-sol F3).
- 화면: 렌더 상한 초과 시 `⋯ 표시 생략 N건 (기록됨) ⋯`, 필터 푸시다운으로 생략 감소. 손실은 마스터 3층으로 나눠 표시(A13): MCNT 불연속·`MESSAGE_BUFFER_OVERFLOW`·TCP 재연결 공백 = `source.gap{cause: mcnt_discontinuity|daemon_buffer_overflow|reconnect}`, Loom 저널 손실 = `record.gap`, 화면 생략 = `display.gap`. MCNT 순환·재연결 경계에서 손실량을 모르면 `count=unknown`. 필터로 건너뛴 seq는 손실이 아니다.

**d. CLI 초안**
```
loom dlt connect <name> --tcp HOST[:3490] [--ecu-id ID]           # 직렬 DLT는 후속
loom dlt status <name> [--json]
loom dlt tail <name> [--app A] [--ctx C] [--level L] [--grep RE] [--tail 50] [--since T] [--until T] [--max-lines 200]
loom dlt wait-for <name> --grep RE [--app A] [--ctx C] --timeout 30s
loom dlt contexts <name> [--refresh] [--limit 100]                 # GET_LOG_INFO 0x03
loom dlt level <name> --app A [--ctx C] --level off|fatal|error|warn|info|debug|verbose [--dry-run]
loom dlt marker <name> [--label TEXT]       # 원격 MARKER(0xF04)는 label을 싣지 않음 → label은 로컬 annotation(mark:X 커서로도 사용)
loom dlt confirm <tok>
loom dlt events <name> --kind ctrl|gap|conn [--origin …] [--since @last|seq:N|mark:X] [--tail 50]
loom dlt export <name> --since T --until T --out file.dlt
```

**e. 사용자 뷰**: 트레이스(host 시각, ECU 타임스탬프, ECU/APID/CTID, 레벨, 페이로드, origin), 컨텍스트 트리(요청/보고/대기), 레벨별 메시지율 미니 플롯. `Pause`는 화면만 멈춤(수신·기록 계속), 뷰 필터와 장비 레벨 변경은 다른 버튼.

**f. 라이브러리**: M0에서 **pydlt 0.3.5 codec 재사용 vs 작은 자체 codec**을 비교해 결정(결정 D9). 제어 메시지·스트림 프레이밍은 Loom 어댑터가 담당. BMW `python-dlt`(MPL-2.0, ctypes/libdlt, 검증 대상 2.18.x)는 3.0.1 호환을 가정하지 않는다. PyPI `python-dlt` 0.2.1과 `dlt` 1.32.0은 무관한 데이터 적재 프로젝트 — 이름으로 설치 금지.

**g. 시뮬레이션**: **M0 gate = 네이티브 COVESA dlt-daemon 3.0.1 + 작은 APID/CTID 앱**(사용자 `dlt-wrapper`의 macOS arm64 빌드 스크립트 재사용, 격리 포트)으로 get-info/set-level 왕복과 반영 확인. **M1 CI = 순수 Python `SimDltServer`**(제어 메시지에 반응해 레벨을 바꾸고 그 결과가 로그에 보임 — Windows CI 포함). 네이티브는 통합 테스트.

**h. M1 걷는 뼈대**: SimDltServer 위 connect → tail/wait-for(경계) → level(agent) → `confirmed`/`effective_verified` 분리 표시 → 사용자 레벨 변경 → 에이전트가 `others_since_last`로 인지하고 `events --origin user`로 읽음 → 에이전트 CLI 종료 후에도 뷰어·기록 유지 → TCP 조각화·재연결 주입 시 세션 경계 표시, 제어 자동 재송신 없음. 수용: 1k msg/s render p95 ≤ 150 ms(render ack 측정), 10k msg/s에서 `wire.bin` 바이트 = 송신 바이트.

**i. 안전**
| 동작 | 등급 |
|---|---|
| tail/wait-for/contexts/stats | R0 |
| level 단일 컨텍스트(APID/CTID 허용 목록 안), marker | R1, 레이트 ≤ 5 req/s |
| level 전체(`0xF08`) 또는 verbose 광역 | R2 + 배너. TTL 레벨 변경은 잡형(A1, renew는 `max_job_duration` 안), 만료 시 원복 제안(사용자 변경은 덮어쓰지 않음) |
| injection(허용 SID) | R2 — M1 비범위 |
| injection(허용 밖) | R3 — M1 비범위 |
| STORE_CONFIG(0x05), RESET_TO_FACTORY(0x06), SET_COM_INTERFACE_*(0x07/0x08) | **R4 `default_blocked`**(영속 변경·통신 차단 가능, `unlock` 후 매번 뷰어 승인) — M1 비범위 |
| vehicle 환경 | bench와 같음(B4), `env=vehicle` 배지 |

**j. 리스크**: 양산 ECU의 non-verbose(FIBEX/ARXML 필요, Q4), dlt-daemon 다중 TCP 클라이언트 허용 여부(DLT Viewer와 동시 접속 — 미확인; 안 되면 Loom이 재송출 포트 제공), 제어 응답에 요청 ID가 없음 → ECU/서비스/APID/CTID별 직렬화, 결합 못한 응답은 `ambiguous`로 남김.

### 5.3 타깃 셸 — `loom shell` (SSH 먼저)

**a. 핵심 루프** — §0 표 3행. 결정 D10: **대화형은 공유 PTY, 비대화 명령은 같은 SSH 연결의 이름 붙은 exec 채널**(Sol). 모든 채널은 같은 이벤트 스트림에 오르고 뷰어 탭으로 자동 표시된다(Opus의 "숨은 연결 금지"를 만족).

**b. 종단 간 시나리오**
```
에이전트  loom shell open ivi --ssh root@192.168.0.20            (host key 고정, 키/agent 인증만)
에이전트  loom shell exec ivi --argv '["systemctl","restart","camd"]' --timeout 10s
          화면: 탭 ivi/exec#3 생성, "systemctl restart camd" [agent], exit 0 — 수신→화면 p95 ≤ 100 ms
에이전트  loom shell log open ivi-j --from ivi --argv '["journalctl","-f","-u","camd"]' --ttl 120s
          → expect 'camd.*(started|failed)' --timeout 15s → "failed to open /dev/video2" [device]
사용자    PTY 탭에서 직접 "ls -l /dev/video*" 입력 (user lease 획득, [user] 노랑)
에이전트  (다음 결과의 others_since_last: user 입력 1건) loom shell read ivi --since seq:880 --tail 50 → 사용자 명령과 출력(video0, video1만 존재)을 읽고 결론
```

**c. 실시간성 / 고부하**: 수신 → 화면 p95 ≤ 100 ms. 원격 프로그램 버퍼링은 SLO 밖(별도 표시). journal/logcat 폭주는 UART 정책 준용(원시 무손실 기록, 렌더 상한, 숨긴 바이트 수 표시). 기본 세션 4, 동시 쓰기 작업 1, exec 출력 상한 64 KiB.

**d. CLI 초안**: UART와 동사를 맞춘다 — `open/close/status/send/read/expect/attach/events/confirm` + 셸 전용 `exec --argv`, `log open|close --ttl`(잡형, A1), `push/pull`(후속). 사용자 입력 중재는 `loom/console/` InputArbiter 규칙을 그대로 따른다(사용자 lease 우선, 에이전트 lock TTL, Ctrl-C 항상 통과). 사용자 PTY 키 입력은 대화형 입력 스트림 클래스(A1-③): lease 획득 시 1회 R1 판정, 문자열 규칙 미적용, 감사는 줄 단위/1 s 해시. `expect`는 최대 3600 s(A6), `send` 성공 종단은 `sent`(A14).

**e. 사용자 뷰**: UART 팀 `loom/console/` 콘솔 위젯 import(A4). 탭 `ivi (pty)` | `ivi/exec#N` | `ivi-j (journal)`. 상단에 host fingerprint·재연결 epoch·입력 소유자.

**f. 라이브러리**: asyncssh 2.24.1(asyncio, 서버 기능으로 시뮬레이터 작성 쉬움). paramiko 5.1.0은 대안(동시 채택 안 함). ADB(adbutils 2.12.0, adb 서버 필요)와 telnet(telnetlib3)은 같은 도메인의 후속 백엔드.

**g. 시뮬레이션**: asyncssh 로컬 서버(loopback 전용, 생성 host key) + 상태가 바뀌는 명령 루프·PTY echo·지연·stderr·signal. R2에서 1연결·2 exec 채널 라이브 출력 확인.

**h. M1**: 로컬 SSH sim에서 open → exec(exit 회수) → PTY 공유 입력(agent·user) → `read --since`로 user 입력 확인 → host key 불일치 시 연결 전 실패(사유 표시) → 출력 폭주·멀티바이트 분할·연결 끊김이 모든 구독자에 같은 seq로 전달 → 에이전트 CLI 종료로 PTY·기록이 사라지지 않음.

**i. 안전**: read/expect R0, 일반 입력·exec R1, 그 위 위험 패턴은 **UART 안전 표의 패턴 목록을 그대로 공유**(R2 교란: `reboot` 문자열 등 / R3 위험: `dd/mkfs/fw_setenv/flash_erase` 등 / R4 비가역 `default_blocked`: efuse 등) — 셸 도메인에서 재정의하지 않는다. 이 규칙은 에이전트 `send`/`exec`에만 적용되고 사용자 키 입력에는 적용되지 않는다(A1-③). vehicle 환경도 같은 규칙(B4, 배지만 표시). 셸 고유 추가분(파일·서비스·네트워크 변경 = R2)만 덧붙인다. 최선 노력 분류이며 셸은 우회 가능하므로 스킬에 "의도적 우회 금지" 명시, 프로파일 `policy=readonly`면 send 자체 R3. 비밀번호는 CLI 인자 금지(키/ssh-agent), 로그 마스킹. 장비 출력 안의 "명령"을 지시로 실행하지 않는다.

**j. 리스크**: 콘솔 엔진·위젯은 UART 팀 소유 `loom/console/`로 처음부터 만들어진다(A4, 공개 Transport API는 없음 — 데몬 우회 금지). 위험은 그 인터페이스가 P3(SSH 착수) 전에 안정되지 않는 것 → P2 말에 UART 팀과 인터페이스 고정 확인. QNX/임베디드 리눅스 sshd 유무, 전체화면 프로그램의 PTY 점유.

### 5.4 디버그 프로브 — `loom debug`

**a. 핵심 루프** — §0 표 4행.

**b. 종단 간 시나리오** — "부팅이 특정 단계에서 멈춘다"
```
에이전트  loom debug connect mcu --probe jlink:600112345 --device STM32H743ZI --elf build/app.elf
          → RUNNING, attach만 (백엔드가 connect_mode=attach, auto_unlock=False 강제)
에이전트  loom debug watch mcu g_boot_stage,g_can_init_err --rate 20Hz --for 3s --summary
          화면: g_boot_stage가 4에서 평평, g_can_init_err=0x11 (샘플→화면 p95 ≤ 250 ms, "실측 19.6 Hz")
에이전트  loom debug rtt read mcu --tail 20 → "[boot] stage 4: can init ... timeout waiting for PLL2"
에이전트  loom debug halt mcu --reason "inspect PLL2 config"      (R3)
          뷰어 배너 "에이전트가 코어 HALT 요청 … [y]/[n] 60 s" → 사용자 y
          코어 HALTED, 레지스터·콜스택 자동 표시(pc=pll_wait_lock+0x1c), 플롯에 회색 HALTED 구간
사용자    원인 확인 후 뷰어에서 r (resume) → origin=user
에이전트  다음 명령 결과 others_since_last(user 1건) → loom debug events mcu --kind core --since @last → resume by user 확인
```

**c. 실시간성**: 시작 프로필 **변수 4개 × 20 Hz**, RTT 폴링 10 ms·RTT → 화면 p95 ≤ 100 ms 목표(SEGGER 최대 약 3.5 MB/s는 벤더 상한일 뿐). SWD 클럭으로 샘플률을 약속하지 않는다. 다중 워드 읽기 `coherent=false`, RTT 타깃 드롭은 카운터 없으면 `target_loss=unknown`, watch가 CPU를 멈췄다면 숨기지 않음, HALTED 구간은 선을 잇지 않음. RTT 폭주는 UART 콘솔 정책 준용.

**d. CLI 초안**
```
loom debug probes --timeout 2s
loom debug connect <name> --probe pyocd:UID|jlink:SN|t32:HOST:PORT [--target T|--device D] [--elf F]
loom debug status|symbols --grep RE --max 50
loom debug read <name> <expr|addr> [--len N]          # 결과에 source=device|cache|elf
loom debug watch <name> <expr,…> --rate 20Hz --for 5s [--summary|--raw --max 2000]
loom debug rtt read|send|wait-for <name> [--chan 0] …
loom debug write <name> <expr> <value> [--dry-run]
loom debug halt|resume|step|reset <name> --reason TEXT
loom debug confirm <tok>
loom debug events <name> [--kind core|cmd|confirm] [--since @last|seq:N] [--tail 50]
```

**e. 사용자 뷰**: Watch 표 + 스파크라인(웹은 P3 이후 플롯; 실측 주기·coherent·age), RTT 콘솔(`loom/console/` 위젯), 코어 상태 램프, 확인 배너. STOP은 halt 상태를 유지하며 "STOP — HALTED 유지 중" 배지(A3), 사용자 비상 키 `!` = resume(코어 정지가 위험한 벤치) 또는 reset — 프로파일에서 선택. 사용자 편집도 같은 허용 목록·정책·기록을 거치며 GUI가 프로브를 직접 열지 않는다.

**f. 라이브러리**: pyOCD 0.46.1 + **pylink-square 1.7.0 고정**(pyOCD 의존성 `pylink-square>=1.0,<2.0`; 2.1.0과 동시 고정은 resolver 실패 재현). 필요하면 J-Link 백엔드 워커 환경 분리(장치 소유권은 하나). pyOCD에 `rtt` 하위 명령 존재 확인. TRACE32 원격 API(lauterbach-trace32-rcl 1.1.6, MIT)는 비Cortex 경로(PowerView 실행 필요). pyelftools로 ELF/DWARF 심볼. pygdbmi는 2023 이후 릴리스 없음 → 후순위.

**g. 시뮬레이션**: **M1 CI = 순수 Python `SimTarget`**(심볼 JSON, 진화하는 변수, RTT 링버퍼, halt 시 진화 정지 — 툴체인·전역 설치 불필요). **M2 = QEMU/Renode GDB + 실제 ELF**(R1 스파이크: QEMU `lm3s6965evb` GDB stub 정지 상태 SRAM 4바이트 왕복 성공. 단 QEMU는 사용자 전역 Homebrew 설치본이라 CI 의존성으로 별도 명시).

**h. M1**: SimTarget에서 connect(attach) → read/watch(경계, 요약) → rtt read/send → write(허용 심볼, 되읽기) → halt 요청 → 뷰어 확인 → 사용자 resume → 에이전트가 origin=user 읽음. 거절/시간초과 경로, 주변장치 영역 읽기 기본 거부, 금지 주소는 프로브 전달 전 거부. **첫 물리 gate(M2)**: 프로브 1종 + MCU 1종에서 connect/disconnect가 halt/reset/erase를 일으키지 않음을 시험.

**i. 안전 (가장 엄격)**
| 동작 | 등급 |
|---|---|
| connect(attach), status, symbols, RAM/Flash 영역 읽기, watch, rtt read | R0 (`auto_unlock=False`, `connect_mode=attach` 강제. `--halt-on-connect`는 R3) |
| 주변장치 레지스터 읽기(읽기 부작용) | R2, SVD에서 부작용 없음 표시된 것만 |
| rtt send | R1 (대상별 `rtt_policy`) |
| RAM 쓰기(허용 심볼) | R2, 되읽기 |
| RAM 쓰기(허용 밖), halt/step/reset/브레이크포인트 | **R3**(워치독·액추에이터·통신 타임아웃 영향, 결정 D8). 벤치 프로파일 `actuators=true`면 halt도 R4 |
| 주변장치 쓰기, flash erase/program | **R4 `default_blocked`**(M1 비범위, `unlock` + 매번 뷰어 승인). flash 중 `debug.flashing` 인터록 → PSU/IO 전원 차단 `presend_check` 실패 |
| 옵션 바이트, RDP/락 비트, OTP, 보안 디버그 잠금, mass erase, auto-unlock | **`immutable_deny`**(A11) — 설정으로도 해제 불가, 필요하면 사용자가 벤더 도구 사용 |
| vehicle 환경 | bench와 같음(B4). halt R3·flash R4 뷰어 승인과 `immutable_deny`는 그대로 |

**j. 리스크**: MCU 계열(AURIX/RH850이면 TRACE32, 비용·난이도↑, Q3), IDE가 프로브 점유(동시 소유 기본 거절), 비침투 읽기도 버스 경합, 보안 디버그 잠긴 샘플, 디버거가 남긴 halt/breakpoint의 종료 시 처리.

### 5.5 로직 애널라이저 — `loom la` (v1.2: Saleae 1순위, sigrok 2순위) + `loom scope` (보류)

v1.2(마스터 B5): 사용자가 **Saleae 로직 애널라이저를 보유**(모델 미확인)했고 계측기를 더 사지 않는다. 이 절은 두 소절로 나눈다.
- **5.5 L — Saleae Logic 2 자동화(1순위, P2)**: 이 절의 정본이다. 사실 근거는 `v12-saleae-check.md`(항목마다 공식 문서·PyPI·포럼 URL)이고, 확인하지 못한 것은 "미확인"으로 적는다.
- **5.5 S — sigrok LA(2순위)와 SCPI 스코프(보류)**: v1.1 계획을 그대로 보존한다. S 안의 "§5.5 c/h/k" 같은 참조는 S 소절을 가리킨다.

#### 5.5 L — Saleae Logic 2 자동화 (1순위)

**L-a. 핵심 루프 — 정직 판정** (§2.3 규칙 8 예외 적용, 결정 D31)
| 요소 | 판정 | 이유 | 보완(수용 기준에 넣음) |
|---|---|---|---|
| 직접 접근 | △ | Logic 2 앱(독점 GUI 앱)이 떠 있어야 한다. la 워커가 앱을 `--automation`으로 띄우거나 열린 앱에 붙을 수 있어 사람 손은 필요 없지만, 앱에 의존한다. 앱 없는 네이티브 headless는 공급자 프리뷰(1.1.0/1.2.0, PyPI 1.0.11에는 없음, 신형 3종만)뿐 | 워커가 앱을 감독·재기동하고 검증된 버전 표를 둔다. headless가 정식 배포되면 재평가 |
| 읽기 | △ | 캡처가 끝난 뒤(`wait()`/`stop()`)에만 내보낼 수 있다. 캡처 중 원시 데이터나 부분 읽기 API는 없다(공급자 답변으로 확인) | 짧은 반복 캡처, 루핑 "블랙박스" 캡처(마지막 N초 확보) |
| 쓰기 | △ | Saleae에는 출력이 없다. 할 수 있는 것은 채널·샘플레이트·임계 전압·트리거 설정과 시작/정지뿐(DUT 불변) | **교차 자극 필수**: UART·CAN·UDS(CANoe 포함) 송신을 LA로 관측 |
| 사용자 실시간 표시 | △ | Loom은 캡처 상태·진행·다른 도메인 자극 이벤트를 실시간으로 보이고, 파형은 캡처 뒤에 보인다. 자동화로 캡처하는 동안 Logic 2 앱 창의 파형이 실시간으로 갱신되는지는 **미확인**이다. 공급자는 자동화가 실시간 관찰·사용자 조작 병행용이 아니라고 설명한다 | 화면 설계 L-e, 반복 캡처 + `capture_gap` 표시, M0 gate에서 앱 화면 갱신 여부 확인 |

점수(§4 #17 갱신): V4 · L4(△×4) · E3 · M3 · H5(보유) · O5 · R3 → **67**. sigrok LA(79)보다 낮다. 그래도 Saleae를 쓰는 근거는 점수가 아니라 **보유 장비와 사용자 결정**(B5)이다. 점수는 부풀리지 않는다.

**L-b. 종단 간 시나리오** — "UART로 보낸 것이 실제로 선에 나갔나" (보유 Saleae + UART 도메인)
```
에이전트  loom la capture sal --ch uart_tx,uart_rx --rate 10M --trigger uart_tx:fall --dur 2s \
            --decode async:ch=uart_tx,baud=115200 --no-wait
          loomd   R1(설정), 캡처 sal#7 시작 → 이벤트 requested→accepted→sent(앱에 캡처 시작 요청)
          화면    Loom LA 패널 "#7 WAITING_TRIGGER  uart_tx↓  2.0 s  10 MS/s [agent]"
                  배지 "캡처 중 — 파형은 종료 후 Loom에 표시"
에이전트  loom uart send dut "help\r"
          화면    타임라인 uart 레인에 tx 행 [agent] (실시간), LA 패널 "#7 CAPTURING 0.4/2.0 s" 진행 막대
장비      2.0 s 뒤 캡처 종료 → 워커가 binary·분석기 표 내보내기 → 파싱 → 기록(먼저) → 방송
          화면    "#7 READY (종료→표시 1.3 s)", 디지털 레인 2개 + 디코드 표
                  68 65 6C 70 0D 행이 uart.tx 행과 같은 시간축, 정렬 라벨 "+0.8 ms (host 근사)"
에이전트  loom la wait sal#7 --timeout 10s  → 요약 + 디코드 5프레임 + match uart.tx seq:1204 + raw_ref
사용자    LA 패널에서 baud를 9600으로 바꿔 재디코드(origin=user, 원본 캡처는 그대로)
에이전트  다음 결과의 others_since_last(user 1건) → loom la events sal --origin user --since @last
```
CAN 변형: 에이전트가 `loom can send`(PCAN) 또는 UDS `canoe` 백엔드로 요청을 보내고, LA가 트랜시버 TTL쪽(RXD/TXD)을 `CAN` 분석기로 디코드해 "버스에 실제로 나간 프레임"을 같은 타임라인에 놓는다. 단, CANoe UI에서 사람이 보낸 프레임은 LA에는 보여도 Loom 송신 이벤트와 짝이 없으므로 `external` 행으로만 표시한다.

**L-c. 실시간성 — 캡처형을 정직하게**
| 모드 | API | 캡처 중 사용자가 보는 것 | 캡처 후 | 목표(설계, 미실측) |
|---|---|---|---|---|
| 단발 | `TimedCaptureMode` / `DigitalTriggerCaptureMode` | Loom: 상태·경과·진행 막대·자극 이벤트 | 레인 + 디코드 표 | 상태 변화→화면 ≤ 250 ms, 캡처 종료→Loom 표시 ≤ 3 s(2 s·4ch·≤10 MS/s, M0 실측) |
| 반복 `la watch --repeat` (잡형 A1) | Timed 반복 | 창마다 결과 갱신 | 창 사이는 `source.gap{cause:"capture_gap", dur}`로 공백 표시(이어 그리지 않음) | 창 0.5–2 s, 갱신 주기 = 창 + 내보내기(실측) |
| 블랙박스 | `ManualCaptureMode`(루핑, `buffer_size_megabytes`) → `stop()` | "RECORDING (looping, 버퍼 N MB)" | 사건 뒤 에이전트나 사용자가 정지하면 직전 구간 표시 | 정지→표시 ≤ 3 s + 버퍼 크기에 비례 |
- 캡처 중 데이터를 꺼낼 API가 없다. Loom은 캡처 중 파형을 그리지 않고, 그린다고 암시하지도 않는다(배지).
- 캡처 길이: 고정 최대는 없고 RAM·채널·샘플레이트·전이 밀도·버퍼에 좌우된다. 프로파일 `max_capture_s`와 버퍼 상한으로 막는다. Timed/Trigger가 메모리 부족으로 조기 종료하면 결과에 `quality:["truncated"]`.
- 원시 형식: 디지털 binary v0 = 초기 상태 + 전이 시각(double) 목록이다(샘플 단위 저장 아님). 그래서 전이 보존 요약(D21 원칙, LTTB 금지)에 바로 맞는다. 아날로그 채널은 후속.
- 시각 정렬: 캡처 시각은 장비 기준 상대 시각이다. host 시각과의 대응은 시작 요청·트리거 시각으로 근사하고 오차 라벨을 붙인다. 정밀 정렬은 **정렬 앵커**로 한다. LA가 UART tx 선을 함께 보면 디코드 바이트와 `uart.tx` 이벤트를 맞춰 오프셋을 계산한다(L-h 수용 항목).
- 크기(A5): 내보내기 파일은 `~/.loom/records` artifact. 이벤트에는 메타·요약·디코드 행만(배치 64 KiB 안), 원본은 `raw_ref`.
- 실패: 앱 종료·크래시·연결 끊김 → `source.gap{cause:"backend_down"}` + STALLED 배지, 진행 중 캡처는 `outcome_unknown`, 워커가 앱을 다시 띄우고 새 epoch.

**L-d. CLI 초안**
```
loom la devices [--sim]                                  # 실장치 + 시뮬레이션 장치 열거
loom la connect <name> --backend saleae [--device ID] [--launch | --attach PORT]
loom la capture <name> --ch A,B --rate 10M [--threshold 3.3]
      (--dur 2s | --trigger CH:rise|fall [--pre 10%] | --looping --buffer 256M)
      [--decode async:ch=..,baud=.. | spi:.. | i2c:.. | can:ch=..,bitrate=..] [--no-wait]
loom la wait <name>[#cap] --timeout 30s                  # 종료·내보내기까지 → 요약 + 디코드(상한) + raw_ref
loom la stop <name>[#cap]                                # 루핑·트리거 대기 정지 → 내보내기
loom la watch <name> --window 1s --repeat --for 60s      # 잡형(A1), 창 사이 capture_gap
loom la decode <name>#cap --decode PD[:opts] [--tail 100] # 앱에 열려 있는 캡처에 분석기 추가 → 표
loom la status|events|confirm …
```
출력 예:
```
$ loom la wait sal#7 --timeout 10s
sal#7 READY  2.000 s  10 MS/s  ch uart_tx,uart_rx  transitions 182  end→ready 1.3 s
decode async(uart_tx,115200): 5 frames  68 65 6C 70 0D  ("help\r")
align: matches uart.tx seq:1204 (offset +0.8 ms, host-approx)
raw: raw_ref la/sal-0007/digital_0.bin (sha256 …)
next: --since seq:1219
```

**L-e. 사용자 화면 — 언제, 무엇을, 어디서 보나** (02 원칙 적용)
| 시점 | Loom 뷰어 (M1 TUI, 웹은 M2) | Logic 2 앱 창 | 사용자 개입 |
|---|---|---|---|
| 캡처 전 | LA 패널: 장치·백엔드·채널 별칭·샘플레이트·임계·트리거, `env` 배지 | 앱이 떠 있으면 장치 표시 | 설정 편집(origin=user) |
| 캡처 중 | 상태 `ARMED→WAITING_TRIGGER→CAPTURING(경과/총, 진행 막대)→STOPPING→PROCESSING`, 통합 타임라인에 다른 도메인 자극 이벤트 실시간 + 캡처 창 범위 음영. 배지 "캡처 중 — 파형은 종료 후 Loom에 표시" | 파형 실시간 갱신 여부 **미확인**. M0 gate에서 확인되면 배지에 "앱에서 실시간 보기 가능"을 더하고, 아니면 빼며 Loom만 정본으로 둔다 | `s` 정지(루핑이면 직전 버퍼 확보), `x` 취소(캡처 폐기) |
| 캡처 후 | 디지털 레인(전이 보존), 디코드 표, 자극 이벤트와 같은 시간축, 정렬 오차 라벨, 종료→표시 시간 배지 | 같은 캡처가 앱에 열려 있어 사람이 확대·측정할 수 있음(앱 조작은 Loom이 보지 못함) | 분석기 추가, baud 변경 재디코드(origin=user), 마커 |
- **Loom 패널이 정본이다.** 앱에서 사람이 직접 시작한 캡처나 설정 변경은 자동화 API로 관찰할 수 없어 Loom 이벤트에 나타나지 않는다. 패널에 "앱에서 직접 조작한 내용은 Loom이 보지 못함"을 안내하고, 스킬은 사용자 개입을 Loom 패널로 하도록 안내한다. 앱 조작이 자동화 캡처와 충돌하면 그 캡처는 `failed{cause}`.
- 캡처 상태 실시간 표시는 CORE 잡 진행 이벤트(`job.progress`)를 쓴다(D-5 요청).
- 자동화 클라이언트 동시 연결 수는 미확인이다 → Loom은 워커가 연결 1개만 쓰고, 사용자의 다른 자동화 스크립트 동시 연결은 지원하지 않는다.
- STOP(`Ctrl-] x`): 새 캡처 시작을 막고 진행 중 캡처는 정지·내보내기. 출력 장치가 아니므로 비상 키 `!`는 없다.
- late viewer는 진행 중 캡처 상태와 마지막 결과를 snapshot으로 받는다.

**L-f. 백엔드·장치 경로**
| 경로 | 결정 |
|---|---|
| `saleae` — `logic2-automation` 1.0.11(2025-11-18, Apache-2.0, 메타 Python ≥3.7) + Logic 2 앱 | **1순위.** 연결은 앱 `--automation`(기본 포트 10430, `--automationPort N`) 후 `Manager.connect`, 또는 `Manager.launch()`(Manager 종료 시 앱도 종료). 기본(Q14): 열린 앱이 있으면 attach, 없으면 launch. 자동화 보장 기종은 Logic 8·Pro 8·Pro 16(`LogicDeviceConfiguration` 명시). Original Logic·Logic16·Logic 4는 앱 GUI는 지원하지만 자동화 보장은 **미확인** |
| 네이티브 headless(공급자 프리뷰 1.1.0/1.2.0, `Manager.launch(headless=True)`) | 채택하지 않음(프리뷰, PyPI 미배포, 신형 3종만). 정식 배포되면 CI·Linux 서버 경로로 재평가 |
| `sigrok` (2순위, §5.5 S) | 사용자 모델이 **Original Logic(FX2, `fx2lafw`)**이면 sigrok가 **연속 스트리밍**을 주므로 표시가 ○로 오른다 → 그 모델이면 sigrok 실시간 모드를 함께 제공. Logic16(`saleae-logic16`)·Pro 16(실험적, 디지털 전용)은 공급자 펌웨어 추출이 필요하고, Logic 8은 planned, Pro 8은 보장 불가 |
| `sim` | 순수 Python **`SimLa`**: UART/SPI/CAN 바이트를 전이 열로 합성해 **Saleae binary v0·분석기 표 CSV와 같은 형식**을 만든다 → 같은 파서 경로를 앱 없이 CI에서 시험 |
- 분석기 이름과 설정 키는 GUI 문자열과 정확히 같아야 한다(예: `Async Serial`의 `Input Channel`·`Bit Rate (Bits/s)`, `CAN`의 `CAN`·`Bit Rate (Bits/s)`, `SPI`의 `MISO/MOSI/Clock/Enable`, `I2C`의 `SDA/SCL`). Loom의 짧은 `--decode` 문법을 이 문자열로 바꾸는 표를 앱 버전별로 둔다. HLA(Python 고수준 분석기)는 후속.
- 캡처 중 분석기 추가는 2.4.48에서 관련 버그가 고쳐졌지만 범위가 미확인이다 → M1은 캡처 시작 시나 종료 후에만 추가한다.
- 프로세스: la 워커(`worker=True`)가 자동화 연결과 (launch한 경우) 앱 프로세스를 소유한다. Logic 2는 사용자가 설치하는 독점 앱이며 재배포하지 않는다.

**L-g. 시뮬레이션**
1. **CI 필수(모든 PR)**: `SimLa`(앱 불필요, 3-OS).
2. **개발·수동(macOS·Windows·Linux)**: Logic 2 앱 + 시뮬레이션 장치 `F4241`(Logic Pro 16 demo, `get_devices(include_simulation_devices=True)`로 열거, 다른 ID 목록은 미확인). 데모 데이터는 실제 프로토콜 신호가 아니므로 디코드 **내용** 시험에는 쓸 수 없다. 상태기계·내보내기·파서·화면 경로 시험용이다.
3. **CI 선택(야간, Linux)**: Xvfb + AppImage(공식 예제 의존성 `xvfb libatk1.0-0 libatk-bridge2.0-0 libgtk-3-0 libgbm1`, `libfuse2` 또는 추출 실행)로 F4241 통합 시험. CI에서 실제로 도는지는 미확인.

**L-h. M0 gate와 M1·M2**
- **M0 gate (P0, 작업 L-0, `spikes/explore/saleae/`)**: 시뮬레이션 장치로
  ① launch·attach 두 연결 방식
  ② Timed·Trigger·Looping 캡처 → `wait`/`stop` → binary·분석기 표 내보내기 → 파서 왕복
  ③ 캡처 종료→파일 준비 시간 측정(2 s·4ch 기준)
  ④ **자동화 캡처 중 앱 창 파형이 갱신되는지 관찰**(L-e 배지 결정)
  ⑤ 앱 강제 종료 감지·재기동
  ⑥ Windows·Linux 실행 조건
  실패하면 백엔드를 다시 고른다(sigrok 경로·모델 의존).
- **M1-la (P2, 시뮬레이션)**: SimLa(CI) + F4241(수동)에서
  1. `la capture` → 상태 전이 실시간 표시(≤ 250 ms) → 결과 레인·표(종료→표시 ≤ 3 s 목표).
  2. **교차 자극(필수, 규칙 8)**: `uart send` → SimLa가 같은 바이트를 신호로 합성(SimWorld 최소 연결 또는 SimLa 자극 모드) → 디코드 표가 `uart.tx`와 같은 축, 정렬 오프셋 표시.
  3. 사용자 개입:
     - 패널에서 정지 → 에이전트 `la wait`가 부분 캡처 결과와 `others_since_last`를 받는다.
     - 사용자 취소(`x`) → exit 6 `E_INTERRUPTED_BY_USER`.
     - baud 변경 재디코드 → 에이전트가 origin=user를 읽는다.
  4. 정직성: 캡처 중 배지, `watch --repeat` 창 사이 `capture_gap`, 앱 종료 주입 시 `backend_down` + STALLED.
  5. §13 C1–C5.
- **M2-la (P2 후반, 보유 Saleae)**: Windows x64와 Linux x64 각각에서 실장치 연결 → UART(실 COM, UART 팀 M2) 송신을 LA로 디코드·정렬 → 가능하면 CAN(PCAN 또는 CANoe) 송신을 트랜시버 TTL쪽 `CAN` 분석기로 확인. 체크리스트 §5.6.1.

**L-i. 안전**
| 동작 | 등급 |
|---|---|
| devices/status/events/wait, 열린 캡처 재분석(decode) | R0 |
| connect(launch/attach), 캡처 설정·시작·정지, watch | R1 — Saleae는 출력이 없어 DUT 불변. 최근 30 s 안에 사용자가 같은 장치 설정을 바꿨다면 에이전트 변경은 확인 요청(덮어쓰기 방지) |
| 프로파일 밖(채널 정격 전압, `max_capture_s`, 버퍼 상한) | 거부(exit 4 `E_DENIED`) |
| vehicle 환경 | bench와 같음(B4), `env=vehicle` 배지 |
| 하드웨어 안전 | 소프트웨어로 못 막음 → 채널 `volt` 별칭으로 정격 초과 경고, 접지는 DUT GND 기준, 12 V 넷 직결 금지를 스킬·패널에 상시 표시 |

**L-j. 리스크**:
- 스트리밍이 불가해 원칙을 일부만 충족한다.
- Logic 2 앱 의존(독점 앱, 자동화 API 버전, 크래시).
- 캡처 중 앱 화면 갱신, 자동화 동시 연결 규칙, 구형 모델의 자동화 지원(Q13)이 모두 미확인이다.
- Linux에서는 AppImage 실행 조건(`libfuse2`), udev 규칙 `99-SaleaeLogic.rules`, Ubuntu 24.04 AppArmor(`--no-sandbox` 우회가 필요할 수 있음)를 확인해야 한다.
- host 시각 정렬에 오차가 있다.
- 메모리 한계로 캡처가 조기 종료될 수 있다.

#### 5.5 S — sigrok LA(2순위)와 SCPI 스코프(보류) — v1.1 계획 보존

v1.2 위치: sigrok은 사용자 Saleae가 sigrok 지원 기종(Original Logic 등)이거나 fx2 클론이 생길 때 붙이는 2순위 LA 백엔드다. 스코프는 구매 전까지 보류한다. 아래 내용은 그때 그대로 쓴다.

사용자 추가 메모(user-notes.md)의 "알리 등 저가 오실로스코프, 디지털(로직) 채널, PC에서 에이전트 제어"에 대한 답이다. 근거: `r2b-opus-scope-la.md`, 사실 검증 `r2c-sol-scope-la-check.md`.

**구조(결정 D11)**: 스코프와 LA는 백엔드·라이선스·라이브 방식이 달라 **두 이름**으로 나눈다.
- `loom scope` = `loom.instr` 확장. PSU와 pyvisa 백엔드·드라이버 프로파일·질의 직렬화를 공유. LAN SCPI가 macOS에서 가장 확실한 경로.
- `loom la` = 별도 도메인. (v1.2: 아래 sigrok 경로는 2순위 백엔드, 1순위는 5.5 L) **sigrok-cli를 하위 프로세스로** 실행(libsigrok GPL-3.0을 데몬에 import 하지 않음). 연속 스트리밍과 sigrok 프로토콜 디코더가 핵심.
- AD3처럼 둘 다 되는 장비는 **공유 owner 워커 1개**가 `ResourceIdentity`로 장비를 한 번만 열고, `loom scope`·`loom la`·출력(AWG/패턴/전원)은 그 owner의 capability를 빌린다. 두 번째 도메인의 `open`은 새 연결이 아니라 attach이고, 쓰기 lease는 장비 단위로 직렬화한다.

**a. 핵심 루프** — §0 표 5행. "쓰기"는 세 층으로 정직하게 구분한다:
| 층 | 내용 | 판정 |
|---|---|---|
| (a) 장비 설정 | 트리거·타임베이스·샘플레이트·arm/single | △ (DUT 불변) |
| (b) DUT 자극 출력 | AWG, 디지털 패턴, 가변 전원(AD3, DSO2D15 AWG 1ch, SDS800X HD 옵션 AWG) | ○ |
| (c) 교차 도메인 자극 | PSU 출력·IO 릴레이·UART/CAN 송신·UDS 리셋을 스코프/LA가 관측 | 조합 단위 ○ — **M1 필수 시나리오** |

**b. 종단 간 시나리오** — "전원 인가 시 리셋 해제 타이밍"(세트 B)
```
에이전트  loom scope arm bench-scope --trigger ch1:rise:6V --timebase 2ms --pre 20% --mode single
          (ch1 = 12V 레일 x10, ch2 = MCU RESET — 벤치 프로파일 probe_map)
          화면: "ARMED single ch1 ↑6.0V 2ms/div [agent]"
에이전트  loom psu out bench on          (R3 → 뷰어 확인 → 사용자 y)
장비      레일 상승 → 트리거 → 캡처. 화면(트리거→표시 ≤ 1 s 목표): 파형 2개 + 타임라인에 psu.cmd와 scope.capture가 같은 축
에이전트  loom scope measure bench-scope --delay ch1:rise:6V,ch2:rise:1.6V --json
          → {"delay_ms":18.4,"capture":7,"points":10000,"source":"screen","quality":["single-shot"],
             "raw_ref":"/records/…/scope-0007.npz"}   (이벤트에는 데시메이션 ≤ 2k점만, 원본은 raw_ref — A5)
사용자    TUI에서 트리거 레벨을 9 V로 입력 + Single  → {origin:user, kind:scope.cfg}
에이전트  다음 결과의 others_since_last(user 1건) → loom scope events bench-scope --origin user --since @last
          → 사용자 설정을 덮어쓰지 않고 다음 사이클에 사용
```
LA 변형(세트 A): `loom la stream la1 --for 5s --decode uart:rx=D0:baud=115200` 실행 중 에이전트 `loom uart send dut "help\r"` → LA 디코드 행에 `68 65 6C 70 0D`가 UART tx 이벤트와 같은 타임라인에 나란히 → 사용자가 디코더 baud 변경(origin=user) → 에이전트가 읽음.

**c. 실시간성 — 두 종류를 구분**
| 종류 | 장비 | 목표(설계) | 정직성 |
|---|---|---|---|
| 연속 스트리밍 | fx2 LA 클론, Hantek 6022BE, DSLogic continuous, AD3 record | 디코드 주석 장비→화면 p95 ≤ 300 ms, 원시 디지털 파형 1–2 Hz 갱신(전이 보존 요약) | 데시메이션 배율 표시, 손실은 3층(A13): 샘플 오버런 `source.gap{cause:"sigrok_overrun"}`, 저널 손실 `record.gap`, 화면 생략 `display.gap`. 원시 무손실, 저장 실패 시 agent 쓰기 중단 + `record.health=failed`(A12) |
| 단발 캡처 반복 | SCPI 스코프(DHO/SDS/DSO2D15), Kingst(버퍼) | 화면 데이터(1k–10k점) 1–5 Hz, 트리거→화면 ≤ 1 s. 전체 메모리는 진행률 있는 아티팩트 작업 | 캡처 배지(#N, 전송 ms, 점 수/메모리 깊이), 캡처 사이 공백은 이어 그리지 않음 |
- **sigrok-cli 한 프로세스로 "원시 저장 + 실시간 디코드"를 동시에 할 수 없다**(r2c F4, 소스 확인): 0.7.2 `session.c`는 디코더(`-P`)를 켜면 출력 모듈(`-o/-O`) 경로를 건너뛴다. 디코드 주석은 주석마다 `fflush(stdout)`하므로 캡처 종료 전 흘러나오는 것은 가능하다.
  → 구조(결정 D21): **수집 프로세스 1개**(`-O binary` 원시 stdout)를 `loomd` LA 워커가 받아 ① 원시+메타데이터 기록 ② 별도 디코더 하위 프로세스(`-I binary` stdin, 같은 채널/샘플레이트) ③ UI 요약으로 분기. 같은 USB 장비를 sigrok-cli 두 개가 열지 않는다. `.sr` 패키징은 캡처 종료 후 별도 단계.
  → **LA M0 gate**: 종료 전 주석 수신, 원시 바이트/샘플 수 일치, 디코더가 멈춰도 기록 지속, TTL 취소·정리·gap 표시. 실패하면 짧은 청크 캡처 반복으로 대체하고 지연 목표 재조정.
- 디지털 파형 요약은 LTTB를 쓰지 않는다(엣지가 사라짐) → 전이 보존 요약 + 토글 밀도 + 원본 샘플 구간 표시.
- 주석의 시각은 sample index × 실효 샘플레이트로 계산하고, 주석 출력 시각을 장비 사건 시각으로 쓰지 않는다. 다른 도메인 자극(PSU RPC 시각)과 트리거 시각의 정밀 동기화는 보장하지 않으며 오차 라벨을 붙인다. 같은 스코프 내 두 채널 지연은 같은 샘플 클록으로 계산하므로 정확하다.
- 스코프 전체 메모리 전송 중에도 PSU OFF 같은 다른 장비 명령은 막지 않는다(장비별 작업 레인).
- **메시지 크기(A5)**: 요청과 `core.events` 배치 1개는 64 KiB 상한 → 캡처 이벤트는 메타 + 데시메이션 ≤ 2k점, 원본 파형·LA 원시는 `raw_ref`(같은 오리진 HTTP GET, 토큰·쿠키 검사)로만. LA 디코드 주석은 이벤트로 흘리되 배치 상한 안에서 묶는다.
- `la stream`·`la capture --continuous`는 잡(A1-②): 시작 시 TTL·바이트 상한을 받고 renew는 `max_job_duration`(기본 1 h) 안. CLI는 즉시 bounded 요약을 돌려주고 원시는 저널로.

**d. CLI 초안**
```
loom scope connect <name> --visa RESOURCE --driver rigol-dho|siglent-sdshd|hantek-dso2000|generic
loom scope status|arm|wait --trigger --timeout|capture [--points 10000|max]|measure …|run|stop|single
loom scope awg <name> on|off [--shape …] [--freq F] [--amp Vpp] [--offset V] [--dry-run]   # (b) 쓰기
loom la connect <name> --sigrok DRIVER [--conn SPEC] [--channels D0-D7]     # demo 드라이버로 sim
loom la capture <name> --for 2s|--samples N [--samplerate 4m] [--trigger D2=r] [--out cap.sr]
loom la stream <name> --for 10s --decode uart:rx=D0:baud=115200 [--max-lines 200]
loom la decode <file.sr|@capture> --decode PD[:opts] [--tail 100]
loom la pattern <name> D0:clock:10k --for 1s [--dry-run]                    # 패턴 출력 장비만, 잡형(A1)
loom scope|la confirm <tok>
loom scope|la events <name> [--origin …] [--since @last|seq:N] [--tail 50]
```
샘플 원본은 stdout에 쏟지 않고 측정치·주석·요약만, 원시는 파일 경로로.

**e. 사용자 뷰**: **M1 = Textual TUI**(캡처 배지·측정치·스파크라인·디코드 표, 트리거 입력·Single/Run/Stop, 디코더 추가). 웹 패널은 CAN viz(P3) 이후 `register_panel`로: `waveform`(다채널, 캡처 배지, 데시메이션 배율, 캡처 간 공백), `logic_lanes`(디지털 레인 + sigrok 주석 행), `decode_table`, 통합 타임라인 캡처 막대 — 이것은 M2. 출력 장비(AWG/패턴/AD3 전원)는 패널에 **사용자 비상 OFF 키 `!`**(확인 없음, A3). STOP 시 "STOP — AWG 출력 유지 중" 배지.

**f. 라이브러리·경로**
| 경로 | 결정 |
|---|---|
| sigrok-cli 하위 프로세스 | **LA 기본**. 정식 릴리스(sigrok-cli 0.7.2 / libsigrok 0.5.2)에 있는 드라이버(fx2lafw, hantek-6xxx — 6022BE·6022BL 스코프 profile 모두 포함, dreamsourcelab-dslogic의 original/Basic/Plus/Pro, demo)로 시작. **Kingst LA2016은 0.5.2 미포함 확인**(master에만), Rigol DHO800·Siglent SDS800X HD는 mainline 모델표에 없음 → 스코프는 SCPI 직접. DSLogic U2Basic 등 신규 리비전은 미지원 → 판매명이 아니라 PID로 지원 목록 고정 |
| libsigrok SWIG 바인딩 | 채택 안 함(PyPI 없음, GPL 결합) |
| pyvisa + pyvisa-py | **스코프 기본**(PSU와 공유). 문서 확인된 파형 명령: Rigol DHO800 `:WAV:SOUR/MODE NORM|MAX|RAW/FORM/DATA?/PRE?`(RAW는 STOP 상태), Siglent SDS800X HD `:WAV:SOURCE/WIDTH/START/POINT/PRE?/DATA?`(LAN VXI-11/socket 5025/Telnet 5024), Hantek DSO2000 `:WAVeform:DATA:ALL?`(Hantek 전용 패킷 헤더, USB 전용 — LAN 없음). Rigol raw socket 포트는 제조사 문서 미확인 → 설정 가능 + `*IDN?` gate |
| dwfpy 1.2.0 (MIT) + WaveForms 런타임 | AD3 전용 `dwf` 백엔드(선택 extra). WaveForms 3.25.1 매뉴얼: GUI는 Intel(Rosetta)이지만 **dwf.framework 런타임은 x86_64·arm64 모두 지원** → arm64 Python에서 SDK 경로 가능(이 머신은 런타임 미설치로 import만 확인). AD3를 scope/la 양쪽에 등록해도 **물리 핸들은 워커 하나가 공유** |

**k. 저가 조합 추천** (가격: 공식 표시가는 출처 있음, 알리 가격은 미확인)
| 세트 | 구성 | 대략 총액 | 되는 것 | 안 되는 것 / 리스크 |
|---|---|---|---|---|
| A 최저가 | fx2(FX2LP) 8ch 24 MHz LA 클론 + Hantek 6022BE(2ch 20 MHz) | $65–100(알리 추정) | LA 연속 스트리밍 + 디코드(UART/SPI/I2C, 트랜시버 TTL쪽 CAN/LIN). 두 드라이버 모두 sigrok 정식판 | 자극 출력 없음(교차 자극만), 6022는 HW 트리거 없음·8bit. 비절연 USB 접지 루프 주의, 12 V 직결 금지 |
| **B 기본 추천** | LAN SCPI 스코프(Siglent SDS804X HD $461: CAN/LIN 디코드 내장 / 또는 Rigol DHO804 $459) + fx2 LA 클론 | ~$470 (스코프 공식가 + 클론 알리 추정) | USB 네이티브 드라이버 부담이 적은 LAN 경로(macOS·Windows 동일, **실기 미검증**), 12bit 4ch로 레일·리셋·CAN 차동, 디지털은 LA로 | AWG는 SDS HD 옵션(가격 미확인), DHO804는 없음. 화면 갱신은 단발 캡처 1–5 Hz |
| C 올인원 + 자극 | Digilent Analog Discovery 3 | $379 (학생가 $249는 기본값 아님, BNC 어댑터·AUX 전원 별도) | 단독 4요소 ○: 2ch 14bit 125 MS/s(±25 V), AWG 2ch(±5 V), 16 DIO 패턴/LA(3.3 V, 5 V tolerant), ±0.5–5 V 전원(채널당 800 mA 또는 2.4 W). dwfpy(MIT), 런타임 arm64 지원 | WaveForms 런타임 독점·별도 설치. 125 MS/s는 장비 샘플링이지 USB 무한 스트리밍 속도가 아님. 전원은 12 V 차량 PSU 대체 아님. sigrok mainline AD3 지원 확인 못함 |
- 참고: Hantek DSO2D15(AWG 포함, ~EUR 231 net)는 USB 전용 + Hantek 전용 파형 패킷(`:WAVeform:DATA:ALL?`) + macOS USBTMC 미확인 → 보유 시 2순위 드라이버. Kingst LA2016은 16ch/200 MHz가 필요할 때(비트스트림 직접 추출, 버퍼 캡처라 연속 라이브 지연 약속 불가, sigrok 정식판 미포함 확인 → nightly 필요). Hantek 6022BL은 sigrok 0.5.2에 스코프 profile이 있으나 **H/P 물리 버튼으로 모드가 바뀌고(VID/PID 변경) 동시 사용 MSO가 아님** → 에이전트 단독 전환 불가, 시나리오 `manual` 단계로 처리하거나 6022BE + fx2 분리 구성 권장. FNIRSI 등 PC 연결 불가 휴대용은 제외. 알리 실매물 가격은 이번 조사에서 확인하지 못했다(세트 A/B 합계는 추정).

**g. 시뮬레이션**: LA는 sigrok `demo` 드라이버(0.5.2 소스 기본 8 logic + 5 analog, 아날로그 square/sine/triangle/sawtooth/random 패턴 — CI에 sigrok-cli 설치 필요, Linux CI 패키지 / macOS는 Homebrew arm64 bottle 존재, 공식 nightly DMG는 x86 전용) + **순수 Python `SimLa`**(UART/SPI sim 바이트를 비트열로 합성해 같은 `la.annot` 이벤트 생성, sigrok 없이 CI). 스코프는 pyvisa-sim YAML로 `*IDN?`·채널 설정·`:WAV:DATA?` 바이너리 블록 조회까지 확인됨(r2c 스파이크, 정적 파형) → 방언 단위 테스트용. 동적 트리거·DUT 파형이 필요한 M1은 SimScpiServer에 트리거 상태기계 추가(파형은 SimPsu 상태로 생성).

**h. M1** (교차 자극 필수, 착수 단계에 맞춰 둘로 나눈다)
- **M1-scope (v1.2 보류 — 스코프 구매 시)**: ① `scope arm`(SimScope) → ② `psu out on`(SimPsu, 뷰어 확인) → 트리거·캡처 → TUI 캡처 요약·스파크라인(자극 이벤트와 같은 시각축 표시) → ③ `scope measure --delay` 경계 반환 → ④ 사용자 트리거 변경 + Single → 에이전트가 origin=user 읽음. 의존: PSU M1, SimScope. 수용: 캡처 배지 표시, 캡처 사이 이어 그리지 않음.
- **M1-la sigrok (v1.2: 2순위 — sigrok 지원 장비가 생길 때)**: ⑤ `la stream --decode uart`(SimLa 또는 demo) 중 `uart send`(또는 SimLa 단독 자극 모드) → 디코드 행 → 사용자 baud 변경 → 에이전트 읽음 + §5.5 c의 LA M0 gate. 의존: SimLa, UART sim, sigrok-cli가 설치된 CI. 수용: 인위 드롭 시 GAP, 원시 샘플 수 = 생성 수, 디코더 정지 중에도 기록 지속.
- 둘 다 §13 공통 수용 기준 C1–C5를 따른다.

**i. 안전**
| 동작 | 등급 |
|---|---|
| capture/measure/decode/status | R0 |
| 트리거·타임베이스·샘플레이트, run/stop/single, 전체 메모리 전송 | R1 (최근 30 s 내 사용자가 같은 장비 설정을 바꿨으면 에이전트 변경은 확인 요청 — 덮어쓰기 방지) |
| AWG on/파라미터(프로파일 한계 안) | R2(bench `token_or_viewer`), 연속 출력은 잡형(A1). 연결 시 AWG 기본 OFF, 50 Ω/Hi-Z 부하 설정 명시 확인(진폭 2배 문제). `awg off`는 `safe_state: true` |
| 디지털 패턴 출력(프로파일 `dio_out` 허용 핀) | R2 잡형, 그 외 핀 R3(DUT가 구동하는 핀과 충돌 위험). 기본 전 핀 입력. 패턴 정지는 `safe_state: true` |
| AD3 가변 전원 | R3 (PSU 규칙 준용, 끄기는 `safe_state`) |
| vehicle 환경 | bench와 같음(B4), `env=vehicle` 배지 |
| 하드웨어 안전 | 소프트웨어로 못 막음 → 스킬·뷰어 상시 경고: 비절연 USB 스코프 접지 클립 = 노트북 접지, LA 입력 범위 밖 12 V 직결 금지(`probe_map`에 넷 전압 기록, 정격 초과 시 connect 경고) |

**j. 리스크**: sigrok 정식 릴리스 정체(신규 드라이버는 nightly/fork), macOS sigrok 설치 경로(Homebrew arm64 bottle vs x86 nightly — r2c), GPL-3.0 경계(하위 프로세스만, 펌웨어·추출 비트스트림 재배포 금지), 스코프 SCPI 파형 전송 속도 미측정(M0에서 1,000점 왕복 실측), AD3 런타임 독점, 접지·과전압 HW 손상.

---

### 5.6 HW/OS 지원 매트릭스 (백엔드별)

○ 지원 경로 있음 / △ 조건부 / ✗ 없음 / — 해당 없음. "검증 단계"는 이 계획 시점까지 실제로 확인한 수준. v1.2(B1): **실HW 호스트는 Windows x64·Linux x64**, macOS 열은 개발·시뮬레이션 경로로만 읽는다.

| 백엔드 | macOS arm64 | Windows x64 | Linux | 외부 설치(사용자) | 검증 단계 |
|---|---|---|---|---|---|
| pyvisa-py TCPIP (LAN SCPI: PSU·스코프) | ○ | ○ | ○ | 없음 | sim 왕복(pyvisa-sim) |
| pyvisa-py USBTMC | △ libusb 권한 | △ libusb/WinUSB | ○ | libusb | 계획 |
| NI-VISA | 미확인 | ○ | ○ | NI-VISA | 계획 |
| DLT TCP (순수 Python) | ○ | ○ | ○ | 시뮬레이터용 dlt-daemon(MPL-2.0) | codec 왕복 |
| asyncssh (셸) | ○ | ○ | ○ | 없음 | sim 왕복(로컬 SSH) |
| adbutils (셸 후속) | ○ | ○ | ○ | adb 서버, Windows OEM USB 드라이버 | 계획 |
| pyOCD CMSIS-DAP/ST-Link | ○ | ○(WinUSB/HID) | ○ | libusb(번들 가능) | import |
| J-Link (pylink-square 1.7.0) | ○ | ○ | ○ | SEGGER J-Link 패키지 | import |
| TRACE32 RCL | △ PowerView 실행 | ○ | ○ | Lauterbach PowerView(상용) | 계획 |
| hidapi USB 릴레이 | ○ | ○ | ○ | 없음 | 계획 |
| pymodbus RTU/TCP 릴레이 | ○ | ○ | ○ | USB-시리얼 드라이버 | sim 왕복(TCP) |
| **Logic 2 앱 + logic2-automation (LA, Saleae 1순위)** | ○ 개발·F4241 시뮬레이션 | ○ Windows 10/11 x64 | ○ Ubuntu 20.04+ x64 AppImage(udev 규칙, `libfuse2`) | Logic 2 앱(독점, 사용자 설치) | 공식 문서 확인(`v12-saleae-check.md`), 실행 미확인 |
| sigrok-cli (LA 2순위, 6022) | △ Homebrew arm64 bottle(0.7.2) / 공식 nightly는 x86 | ○ 설치본 + Zadig(WinUSB) | ○ 배포판 패키지 | sigrok-cli, fx2lafw 펌웨어 | 계획(미설치) |
| dwf 런타임 (AD3) | ○ framework x86_64·arm64 (GUI는 Rosetta) | ○ | ○ | WaveForms | import(dwfpy)만 |
| QEMU/Renode GDB (debug M2 sim) | ○(Homebrew) | △ | ○ | qemu/renode | sim 왕복(정지 상태 SRAM) |

v1.2(B1): 각 도메인 M2는 **Windows x64와 Linux x64 둘 다**에서 실기 1종을 확인한다. macOS는 개발·시뮬레이션 전용이다. 지금 보유 장비로 바로 할 수 있는 탐색 실HW는 **Saleae LA**와 네트워크 대상(DLT·SSH)이다.

#### 5.6.1 실HW 체크리스트 — Windows x64 / Linux x64 (B1)

| 도메인 | Windows x64 | Linux x64 | 지금 시험 가능? |
|---|---|---|---|
| 공통 | `loomd`+에이전트가 장비 PC에서 실행, 127.0.0.1 바인딩, 방화벽 예외 불필요 확인, `~/.loom` = `%USERPROFILE%\.loom` 경로, 장치 자동 open 안 함(D8) | 같은 항목 + 사용자 그룹(`dialout`·`plugdev`) 소속, systemd 사용자 서비스 없이 CLI 자동 기동(D8) | ○ |
| la (Saleae) | Logic 2 설치본 + USB 드라이버(설치본 포함), 앱 `--automation` 기동·attach 둘 다, 기본 포트 10430 충돌 확인, 실장치 열거·캡처·내보내기 경로(공백 포함 경로), 앱 크래시 재기동 | AppImage 실행 권한·`libfuse2`(또는 추출 실행), **udev `99-SaleaeLogic.rules`**, Ubuntu 24.04 AppArmor(`--no-sandbox` 필요 여부), 화면 없는 장비 PC면 Xvfb 필요 여부, USB 재열거 후 재연결 | ○ 보유 장비(모델 Q13) |
| dlt | TCP 3490 접속(사내망·방화벽), DLT Viewer 동시 접속 | 같음 + 네이티브 dlt-daemon 3.0.1 로컬 빌드로 대조(P0 gate) | ○ 대상 ECU가 있으면(U9) |
| shell (SSH) | Windows OpenSSH 에이전트(named pipe) 또는 키 파일, `known_hosts` 위치 | `SSH_AUTH_SOCK`, `~/.ssh/known_hosts` 고정 | ○ 대상이 있으면(Q5) |
| psu / scope (구매 시) | pyvisa-py LAN 우선, USBTMC는 WinUSB(Zadig) 또는 NI-VISA | LAN 우선, USBTMC는 udev 규칙(`usbtmc` 커널 드라이버와 libusb 충돌 확인) | ✗ 미보유 |
| io (구매 시) | HID 릴레이 드라이버 없이 동작 확인, COM 번호 고정 | hidraw udev 권한, `/dev/ttyUSB*` 별칭(udev 시리얼 번호) | ✗ 미보유 |
| debug (구매 시) | J-Link/ST-Link 드라이버, CMSIS-DAP WinUSB | 프로브 udev 규칙(`plugdev`), J-Link `.deb` | ✗ 미보유 |
| la (sigrok 2순위) | Zadig로 WinUSB 드라이버 교체(Logic 2 드라이버와 충돌 주의 — 같은 장치에 둘 다 못 씀) | 배포판 `sigrok-cli` + libsigrok udev 규칙, fx2lafw 펌웨어 | △ Saleae가 sigrok 지원 기종일 때만 |


### 5.7 데이터·기록 포맷

모든 레코드는 마스터 D3 필수 필드(`session_id`, `epoch`, `seq`, `ts{mono_ns, wall_ns, clock_id, source, quality}`, origin, `request_id` …)를 가진다. 원시를 먼저 쓰고 해석은 그 뒤. 저장 위치·보존은 D7: `~/.loom/records`(정본 = 도메인 저널 JSONL 청크 + 원시), 기본 30일·20 GiB, 감사 무기한, `--no-record`여도 감사는 기록. 아래 표의 파일 이름은 저널 안의 도메인 파일이다.
| 도메인 | 원시(무손실) | 해석·인덱스 | 표준 export |
|---|---|---|---|
| psu / io / scope 명령·측정 | `events.jsonl`(SCPI 명령·응답 문자열 포함) | `metrics` 시계열 | CSV |
| scope 파형 | artifact(`.npz` + 메타, sha256) | 캡처 이벤트(배지·측정치) | CSV |
| dlt | `wire.bin` + 청크 인덱스 | 디코드 JSONL(APID/CTID/레벨/인자) | `.dlt`(storage header 생성) |
| shell | 채널별 raw 바이트(UART 포맷 재사용) + 줄 인덱스 | exec 결과(exit, 바이트 수) | 텍스트 로그 |
| debug | watch 샘플 JSONL(`coherent`, `source`) + RTT raw | 심볼 해석 | CSV |
| la (saleae, v1.2) | 내보내기 binary v0(채널별 전이 시각) + 캡처 메타(장치·앱 버전·설정·시작 시각) artifact | 분석기 표 CSV → 디코드 JSONL(캡처 상대 시각 → host 근사 시각, 정렬 오프셋) | CSV, 원본 `.bin` |
| la (sigrok, 2순위) | `-O binary` 원시 + 메타(채널, 샘플레이트, 트리거) | 디코드 주석 JSONL(sample index → 시각) | `.sr`(캡처 종료 후 패키징) |
| run | 단계 이벤트 + operator action | 결과 표 | (후속) JUnit |
| 공통 | `audit.jsonl`(누가·무엇·등급·결정·승인자), `manifest.json`(플러그인 버전, 프로파일 해시, 외부 프로그램 버전) | | |

### 5.8 시뮬레이션·테스트(CI) 매트릭스

| 구분 | macOS | Windows | Linux | 내용 |
|---|---|---|---|---|
| 필수(모든 PR) | ○ | ○ | ○ | 순수 Python sim 전부(SimPsu/SimScpiServer, SimRelay, SimDltServer, 로컬 SSH, SimTarget, **SimLa — Saleae 내보내기 형식 생성**, SimScope) + C1–C5 공통 수용 시험 |
| Linux 전용 통합 | — | — | ○ | 네이티브 dlt-daemon 3.0.1 통합, (선택·야간) Xvfb + Logic 2 AppImage + `F4241` 시뮬레이션 장치(실행 미확인), (2순위) sigrok-cli `demo` 경로 |
| 야간/수동 | △ | △ | ○ | QEMU/Renode GDB + 실제 ELF, 부하 프로필(DLT 10k msg/s, LA 스트림), render ack p95 지연 기록 |
| 실HW(수동, 단계 M2) | — (개발·sim만, B1) | 보유 장비 | 보유 장비 | 도메인별 장비 1종 smoke를 **Windows·Linux 둘 다**(§5.6.1). 지금 보유 = Saleae LA뿐. debug는 구매 시 connect/disconnect가 halt/reset/erase를 일으키지 않음 |

---

## 6. 공통 계약(loom.core, loom.viz)에 요구할 것 — 도메인 플러그인 구조

마스터 D10(탐색 CR-X1 채택) + A1/A2가 정본이다. 이 절은 그 계약을 탐색 도메인이 어떻게 쓰는지와, 아직 마스터에 없는 세부(SimWorld 참가자, capability 이름 등)만 적는다. 배치는 D1: `loomd` 하나 = gateway + 장치 worker. 탐색의 psu/io/dlt/shell은 gateway 모듈, debug·la는 `worker=True`.

### 6.0 프로세스 구성도와 데이터 흐름 (단일 `loomd` 배치를 예로)
```
 에이전트 CLI (loom psu/dlt/shell/debug/scope/la/run …)      사용자 뷰어 (TUI in herdr pane / 웹 127.0.0.1)
        │  JSON-RPC (role=agent)                                   │  JSON-RPC (role=viewer, 확인 응답 권한)
        └──────────────────────────┬───────────────────────────────┘
                                   ▼
 ┌──────────────────────────── loomd (게이트웨이) ─────────────────────────────┐
 │ 역할·origin 부여 → 쓰기 파이프라인(분류→deny→한계→인터록 lease→게이트→방송) │
 │ 이벤트 버스(seq, snapshot) ── 팬아웃 ──▶ 구독자(CLI 커서 / 뷰어)              │
 │ 기록기(세션 디렉터리: events.jsonl, 도메인 원본, artifacts, audit)          │
 │ 모듈: psu/io(pyvisa, hid/modbus) · dlt(TCP) · shell(asyncssh) · run(러너)   │
 └───────┬───────────────────────────────┬──────────────────────────────────┘
         │ 워커 IPC                      │ 워커 IPC
 ┌───────▼────────┐             ┌────────▼──────────────────────────┐
 │ debug 워커       │             │ la 워커 (v1.2)                      │
 │ pyOCD / J-Link  │             │ saleae: Logic 2 자동화 연결 1개      │
 │ / TRACE32 RCL   │             │  (앱 감독) → 캡처 종료 → 내보내기    │
 └───────┬────────┘             │  2순위 sigrok: 수집 1프로세스 분기    │
         ▼                      └────────┬──────────────────────────┘
      프로브 → MCU                        ▼  Saleae (Logic 2 앱 경유) / sigrok 장비
 장비 방향: RPC → 게이트 → 모듈/워커 → 장비.   데이터 방향: 장비 → 모듈/워커 → 기록(먼저) → 이벤트 버스 → 뷰어·CLI
```

### 6.1 원칙
- 새 도메인 추가 = 패키지 하나 + entry point 한 줄. 플러그인은 "장비를 어떻게 여는가 / 무엇을 읽고 쓰는가 / 얼마나 위험한가 / 어떻게 보이는가"만 선언하고, **데몬 수명·IPC·기록·방송·확인·뷰어 연결은 core가 소유**한다.
- 신뢰된 Python 플러그인은 마음대로 I/O를 할 수 있으므로 registry만으로 우회를 물리적으로 막지는 못한다 → **신뢰 패키지 목록 + 플러그인 API 규약 + 기록 경로 검증**으로 보장 범위를 명시한다(R2-sol §4.2).

### 6.2 등록 (마스터 D10: entry point `loom.domains` + Registry — 필수: backend, commands, event kinds, capabilities, risk table, sim)
```python
# 초안(시그니처만, 구현 아님)
class DomainPlugin(Protocol):
    name: ClassVar[str]            # "dlt" → CLI 그룹, event domain, 프로파일 섹션
    api_version: ClassVar[int]     # 불일치 시 해당 도메인만 비활성 + loom status에 사유
    def register(self, reg: "Registry") -> None: ...

class Registry(Protocol):
    # 필수
    def backend(self, kind: str, factory: "BackendFactory", *, sim: bool = False,
                platforms: frozenset[str] = frozenset({"darwin", "win32", "linux"})) -> None: ...
    def command(self, method: str, handler: "Handler", *, write_class: "request|job|interactive|None",
                classify: "Classify", schema: Mapping,
                approval_binding: "Binding | None" = None,      # A2: 추가 digest 필드
                presend_check: "PreSend | None" = None,         # A2: 승인 후·backend 직전, 실패 시 승인 무효
                safe_state: bool = False,                       # A15: finally/복구 송신 허용 동작
                r2_default: "auto|token_or_viewer|viewer" = "token_or_viewer") -> None: ...
                # RPC 이름: "psu.set" 또는 "psu.output.set" (D2: snake 2단·3단)
    def event_kind(self, kind: str, schema: Mapping, *, version: int,
                   metrics: "MetricSpec | None" = None) -> None: ...         # "psu.meas", metrics=[{name, unit}]
    def capability(self, name: str, *, supported: bool = True) -> None: ...  # "output-control", "log-level" …
    def risk_table(self, rows: list, *, default_blocked: list = (), immutable_deny: list = ()) -> None: ...  # A11
    # 선택
    def cli(self, app) -> None: ...
    def panel(self, spec: "PanelSpec") -> None: ...          # viz register_panel로 전달
    def profile_section(self, schema: Mapping) -> None: ...   # 벤치 프로파일 [psu.*] 검증
    def interlock(self, name: str, active: "Callable[[], bool]") -> None: ...  # "debug.flashing"
    def sim_actor(self, name: str, factory) -> None: ...      # SimWorld 참가자(§7.4)
    def skill(self, path) -> None: ...
```
```toml
[project.entry-points."loom.domains"]
psu = "loom.instr.psu:PsuPlugin"
```
- 무거운 의존성은 extras: `loom[instr]`(pyvisa, pyvisa-py), `loom[debug]`(pyocd, **pylink-square==1.7.\***, pyelftools), `loom[shell]`(asyncssh), `loom[io]`(hidapi, pymodbus), `loom[la-saleae]`(logic2-automation), `loom[la-dwf]`(dwfpy). Logic 2 앱·sigrok-cli·WaveForms·J-Link SDK·dlt-daemon은 **외부 프로그램**으로 사용자가 설치(설치 경로·버전을 `loom doctor`가 진단). extras 조합은 resolver lock으로 실제 검증한다.
- 어느 도메인이 워커 프로세스가 필요한지는 플러그인이 선언(`worker=True`): debug(프로브 SDK 블로킹), la(Logic 2 자동화 연결·앱 감독, 2순위 sigrok-cli 하위 프로세스)는 워커, psu/io/dlt/shell은 게이트웨이 모듈로 충분(CAN의 FC 지연 측정과 같은 근거로 블로킹 SDK만 분리).

### 6.3 장치 백엔드와 쓰기 파이프라인
```python
class DeviceBackend(Protocol):
    identity: "ResourceIdentity"     # 정규화: backend + serial/IDN + channel + target fingerprint (alias 이중 open 감지)
    async def open(self, cfg, *, deadline) -> "DeviceInfo": ...
    async def close(self) -> None: ...
    def rx(self) -> AsyncIterator["RawRx"]: ...                 # push형(DLT, 셸, LA 스트림) — 선택
    async def poll(self, what, *, deadline) -> list["RawRx"]: ...  # poll형(PSU, watch) — 선택. 주기·지터 계측은 core
    async def write(self, op, *, deadline, cancel) -> "WriteResult": ...  # partial/outcome_unknown 포함
    def health(self) -> "Health": ...
```
쓰기 파이프라인(core 소유, 우회 불가 — D10, A1, A2, A14):
```
① 요청형 (psu set/out, dlt level, shell exec, debug write/halt, scope arm/awg 설정)
RPC(method, args; origin은 연결 자격으로 결정)
 → classify(risk, reason) → immutable_deny / 하드 차단 / default_blocked → 프로파일 한계(기본 거부) → env 배지(B4: vehicle도 등급·경로는 bench와 같음, 봉투·이벤트에 env 표시)
 → confirm/approval: R0–R1 자동 | R2 auto|token_or_viewer|viewer | R3 뷰어 | R4 뷰어 매번(once)
 → audit → presend_check(인터록 재확인; 실패 시 승인 무효 + E_LOCKED/E_QUARANTINE)
 → 이벤트 requested→accepted→sent (publish-before-reply) → backend
 → confirmed → effective_verified | effect_unverified, 어느 단계든 failed | outcome_unknown
② 잡형 (io seq·반복 pulse, la watch --repeat(Saleae 반복 캡처)·la stream(sigrok)·pattern, AWG 연속 출력, shell log, dlt TTL 레벨)
 → core는 start/update/renew/stop만 게이트·감사. 단위 동작은 worker/모듈이 승인된 plan 범위 안에서 실행하며
   TX latch·레이트·하드 차단을 단위마다 재집행, 단위는 events/저널에만. renew는 max_job_duration(기본 1 h) 안(A9).
③ 대화형 입력 스트림 (shell PTY·RTT 콘솔의 사용자 키 입력)
 → lease 획득 시 1회 R1 판정, 문자열 규칙 미적용, 감사는 줄 단위 또는 1 s 묶음 해시, 저널은 청크 그대로.
```
- readback 실패는 쓰기 실패 확정이 아니다 → `outcome_unknown`, 자동 재시도 금지.
- 사용자 개입(lease takeover)은 같은 resource의 대기 중 에이전트 쓰기를 취소한다. 이미 장비로 나간 작업은 취소 가능 여부를 따로 반환.

### 6.4 이벤트 레코드 (마스터 D3 + A13 사용)
- 필수 필드는 D3(`schema_version, session_id, domain, channel, kind, seq, ts, dir, origin, client_id, actor_id, request_id, job_id, epoch, generation, purpose, risk`), 순번은 원본 `(producer_id, producer_epoch, source_seq)` + gateway `stream_seq` 두 층. 탐색이 쓰는 확장:
  1. **`metrics[{name, value, unit}]`**(D3 채택형): viz가 long-format 시리즈 `<domain>.<channel>.<name>`로 자동 변환 → **새 도메인도 "데이터만 주면 렌더링"**. 예: `[{name:"current", value:0.351, unit:"A"}, {name:"voltage", value:13.49, unit:"V"}]`.
  2. **`quality[]`** 값(탐색 사용분): `stale, aggregated, readback-only, coherent=false, target-loss-unknown, single-shot, decimated:N, simulated`.
- origin은 **agent/user/device/script 유지**(세 팀·xteam v0.2 합의). 시나리오 실행 구분은 `actor_id`/`job_id`, 목적은 CAN의 `purpose`(run, replay, sim, keepalive 등)를 함께 쓰고, 재생은 `original_origin` + replay 네임스페이스(결정 D13). 장비 응답 RX는 요청자 origin으로 바꾸지 않고 `request_id`로 부모 요청에 연결.
- 대형 데이터(파형, 메모리 덤프, LA 원시)는 payload `{encoding: raw_ref, data}` + `sha256, length, format`. 요청과 `core.events` 배치 1개는 64 KiB 상한, 원본은 같은 오리진 HTTP GET(A5).
- 손실 3층(A13): `display.gap`(화면) · `record.gap`(Loom 기록) · `source.gap{cause}`(Loom 수신 전: `mcnt_discontinuity`, `daemon_buffer_overflow`, `rtt_drop`, `sigrok_overrun`, `scpi_no_response`, `reconnect`, v1.2 `capture_gap`(반복 캡처 창 사이), `backend_down`(Logic 2 앱 종료·연결 끊김)). `count=unknown` 허용. 필터로 건너뛴 seq는 gap이 아님.

### 6.5 안전 레지스트리 · 확인 · 벤치 프로파일
- 등급·경로·차단 계층은 마스터 D6 + A8/A9/A11 + B4(A10 폐기) 그대로(§5 공통 전제, §0.3). 탐색 도메인의 `default_blocked`·`immutable_deny` 목록은 각 §5.x i 표가 정본.
- 승인: R3·R4는 뷰어에서만(CLI approve 없음), R2는 `loom <domain> confirm <tok>` 또는 뷰어 Y. 승인 객체 = digest·epoch·profile hash·만료·횟수(once/session, 사용 기한 10분) + 도메인 `approval_binding` 필드. 이름만 같은 다른 프로브/장비로 전이 금지, 재연결·프로파일·인자 변경 시 무효.
- 토큰 재제출은 사람 동의의 증거가 아니다(A8) → 탐색 스킬 설치 시 `Bash(loom * confirm:*)` `ask` 규칙을 함께 안내하고, 그 규칙이 없는 환경(Codex 등)의 실HW는 R2를 `viewer`로 둘 것을 권장.
- 뷰어가 없을 때: 승인이 필요하면 exit 5 `E_NO_APPROVER`(뷰어 실행 방법을 hint에), 그 외 결과에는 `no_viewer_attached` 경고(D9). (구 결정 D14 대체)
- **벤치 프로파일**: CAN이 제안한 단일 `loom.toml`에 `[bench]`, `[psu.<name>]`, `[io.<name>]`, `[dlt.<name>]`, `[debug.<name>]`, `[shell.<name>]`, `[scope.<name>]`, `[la.<name>]` 섹션으로 들어간다(별도 파일 만들지 않음, 결정 D15). 장비 한계·채널 별칭·허용 목록·`probe_map`·`actuators`·`env`. 데몬이 로드 시 해시 기록.
- **2층 정책(마스터 D6)**: 프로젝트 `loom.toml` + 사용자 `~/.loom/config/policy.toml`. **완화**(vmax/imax 상향, R2 동작의 `auto` 지정, `write_allow`·`dio_out` 확대, `unlock`)는 **뷰어 승인 후 적용**, 강화는 즉시, 모든 변경 감사 — 에이전트가 파일을 고쳐도 승인 없이는 한계가 풀리지 않는다.
```toml
[bench]
name = "desk-bench-1"
env = "bench"            # sim | bench | vehicle — 배지 표시용, 등급은 bench와 같음(B4)
actuators = false

[psu.bench]
resource = "TCPIP::192.168.0.50::5025::SOCKET"
channel = 1
vmax = 16.0
imax = 5.0
warn_curr = 1.5

[psu.bench.r2]            # 완화 항목 — 적용 전 뷰어 승인 필요
set = "auto"

[io.relays]
driver = "hid-relay"
safe_state = "keep"

[io.relays.channels]
1 = { alias = "kl30", risk = 3 }
2 = { alias = "kl15", risk = 3 }
5 = { alias = "btn_wake", risk = 1 }

[debug.mcu]
probe = "jlink:600112345"
device = "STM32H743ZI"
elf = "build/app.elf"
write_allow = ["g_cfg.*", "g_test_*"]

[la.saleae]               # v1.2 B5 — 보유 Saleae (모델 미확인)
backend = "saleae"        # saleae | sigrok | sim
device_id = ""            # 비우면 첫 장치. 개발용 시뮬레이션 장치 ID는 §5.5 L-f
max_capture_s = 10.0      # 캡처 1회 상한(프로파일 한계 — 넘으면 거부)
max_rate_hz = 50_000_000
threshold_v = 3.3

[la.saleae.channels]
0 = { alias = "uart_tx", volt = 3.3 }
1 = { alias = "uart_rx", volt = 3.3 }
2 = { alias = "can_rxd_ttl", volt = 5.0 }
```
(R1 예시의 한 줄 다중 대입은 TOML 파싱 실패 — R2-sol F10 — 이라 줄을 나눴다. `[psu.bench.r2]` 같은 완화 항목은 사용자 정책 파일에 두는 것이 기본이며, 어느 파일이든 뷰어 승인 전에는 적용되지 않는다.)

### 6.6 뷰어 패널 (loom.viz에 요청)
CAN viz의 `register_panel(kind, topics, component)` 위에 **선언형 PanelSpec + actions**를 얹는다.
```python
@dataclass
class PanelSpec:
    id: str; type: str; title: str
    source: Mapping            # {"kinds": ["psu.meas", "psu.cmd"], "channel": "*"}
    columns: list | None = None
    actions: list["PanelAction"] = ()   # 키/버튼 → 같은 command RPC (origin=user는 서버가 부여)
```
요청 패널 타입(v1.2: LA는 `logic_lanes`·`decode_table` + 캡처 상태 배지가 M1 TUI에서 먼저 쓰임): `gauge`(PSU/DMM), `state_lamps`(IO 4상태, 출력, 코어), `console`(UART 위젯 — 셸/RTT 재사용), `timeseries`(`metrics` 자동, gap·실측 주기), `trace_table`(DLT 열 매핑), `waveform`, `logic_lanes`(디지털은 **전이 보존 요약** — LTTB 금지), `decode_table`(스코프/LA), `timeline_lane`(§7.2, 다른 도메인 RPC 시각과 캡처 시각 사이 오차 라벨), `run_panel`(§7.1). payload에 실행 코드·임의 HTML 금지. 복잡한 화면은 웹 컴포넌트 탈출구(후순위).

화면 배치(herdr + 웹):
```
┌──────────────── herdr 탭 ───────────────────────────────────────────────────────┐
│ [pane 1] Claude Code (agent)        │ [pane 2] loom view (TUI 멀티 패널)          │
│ > loom run start cold-boot.yaml     │ ┌ run r-0012 ▶ uart.expect 18/30s [p][a][s]┐ │
│ {"run":"r-0012"}                    │ ├ psu bench 13.49V 0.35A ● ON(script)      ┤ │
│ > loom run wait r-0012 --timeout 300│ ├ io  kl30● kl15● btn○ (device_reported)    ┤ │
│                                     │ ├ uart dut  [boot] U-Boot 2024.04 …         ┤ │
│                                     │ ├ dlt ecu1  CAMS CAM info frame 1 …         ┤ │
│                                     │ └ ⚠ 확인 cf-21: run r-0012 R3×2  [y][n] 58s ┘ │
└─────────────────────────────────────┴─────────────────────────────────────────────┘
```
확인 배너는 모든 뷰어의 **최하단 고정 행**(TUI)·상단 고정 띠(웹)에 뜬다. 공통 키·표시는 D9를 따른다: 접두키 `Ctrl-]`(`Ctrl-] x` STOP, `Ctrl-] q` 종료), 비터미널 뷰어는 `Ctrl-X` STOP·`/` 필터·`Space` 일시정지·`q` 종료도 허용, heartbeat 1 s, `LIVE/LAG/STALLED/LIVE-DEGRADED` 배지, STOP 시 "출력 유지 중" 배지(A3). 탐색 도메인 고유 키: 출력 유지 장치(PSU·IO·AWG·AD3 전원·debug)의 **사용자 비상 키 `!`**. M1 뷰어는 도메인별 Textual TUI, 웹 패널은 P3 이후 CAN viz `register_panel`. 웹 대시보드(`loom view --web`, P3 이후): 상단 run 패널 + 확인 배너, 중앙 좌 DLT/UART/셸 트레이스 탭, 중앙 우 PSU 계기판 + debug watch + 스코프 파형, 하단 통합 타임라인(전 폭). origin 색: agent 청록, user 노랑, device 회색, script 보라.

### 6.7 CLI · 스킬 규약
- 마스터 D5/A6/A7을 그대로 따른다: `--json` 봉투 `{schema_version, ok, cmd, data, meta{truncated, next_cursor, range, display_gap, record_health}, warnings[], others_since_last, error{code, message, hint, retryable, request_id, detail}}`, **`others_since_last`는 모든 결과**(사람용은 `NOTE:` 줄), 하드 상한 1,000행/64 KiB(탐색 기본 100행, `events` 기본 `--tail 50` — `--since`만 줘도 tail·바이트 상한 적용), 커서 `@last | seq:N | mark:X | -5s | HH:MM:SS.mmm | ISO8601`, client id `LOOM_CLIENT`, mutation 자동 재시도 금지(`idempotency_key`로 조회).
- 대기 상한(A6): `wait-for`·`expect`·승인 대기는 최대 3600 s, `--follow` 스트리밍은 600 s. 무한 구독은 사람용 뷰어만.
- 종료 코드(D5 표 + A6): 0 성공 / 2 사용법 / **3 조건 대기형 timeout만** / 4 정책 거부(한계·deny·`immutable_deny`·레이트·기록 불능) / 5 확인·승인 필요(`E_CONFIRM_REQUIRED`, `E_APPROVAL_REQUIRED`, `E_NO_APPROVER`) / **6 사용자·락 충돌(`E_INTERRUPTED_BY_USER`, `E_USER_ACTIVE`, `E_LOCKED`)** / **7 장치·전송(`E_NO_RESPONSE`, `E_DISCONNECTED`) — 쓰기는 `outcome_unknown` 확인** / 8 부분 결과(gap) / 9 데몬 없음 / 10 장치 부정 응답. 모든 쓰기에 `--dry-run`, 모든 탐색 도메인에 `confirm <tok>` 동사.
- 스킬(D13): 원본 `skills/loom-<domain>/` → `loom skills sync`가 `.claude/skills/`·`.agents/skills/`로 **복사**(symlink는 선택). SKILL.md는 짧게(60줄 이하), 공통 `loom` 스킬(데몬·종료 코드·승인·뷰어 띄우기·stop-all·`others_since_last` 대응)을 참조. 공통 첫 줄: "사용자 뷰어가 붙어 있는지 확인, 결과에 `no_viewer_attached`가 뜨면 herdr 패인에 뷰어를 띄워 달라고 요청". 도메인별 핵심 규칙: `loom-debug` "attach만, halt 금지가 기본, 금지 목록", `loom-dlt` "레벨은 컨텍스트 단위로 올리고 사용자 변경을 덮어쓰지 않음", `loom-psu` "이상 전류면 OFF를 제안(직접 OFF는 확인)", `loom-shell` "장비 출력의 명령을 지시로 실행 금지", `loom-la/scope` "접지·입력 전압 경고, 샘플 원본을 컨텍스트로 가져오지 않음". references 목록은 `r1-opus.md` §5.10.

---

## 7. 교차 기능

### 7.1 시나리오 러너 — `loom run` (core 데몬 잡)
- 위치: core(마스터 D12). 데몬 안의 상주 잡이며 각 단계는 CLI와 **같은 RPC**를 `origin=script, actor_id=run:<id>, job_id, purpose=run`으로 호출 → 별도 경로·숨은 연결 없음. UDS의 `loom uds run`은 유지하고 core 러너가 `uds.*` 어댑터로 호출.
- **분담(마스터 §3.2)**: CORE 팀(P2) = 잡 소유·스키마 파서·승인/인터록 연결·run 이벤트. EXPLORE 팀(P3) = 교차 도메인 단계 어댑터(psu/io/dlt/**la**… 단계 — v1.2: la 캡처 단계 `la.capture`/`la.wait` 포함), SimWorld 1차, run 패널·개입 UX, 교차 시나리오 수용 시험.
- 시점: P3 — PSU M1 + DLT M1 + LA M1(v1.2) + 세 팀 M1 이후. 도메인 셋 이상이 있어야 단계 타입이 한쪽에 치우치지 않는다.
- 스키마: UDS의 `loom_scenario: 1`(uds plan §6.5)을 **확장**한다. 키 이름은 UDS를 따르고(`vars`, `save`, `expect`, `repeat`, `finally`, `on_error`), 단계는 UDS의 "동사 키" 형식을 도메인 접두로 넓힌 `- <domain>.<verb>: {args}`이다. 따라서 UDS 시나리오의 단계는 `uds.` 접두만 붙이면 그대로 들어간다. core 공통 단계는 `sleep`, `manual`(사용자만 완료). `wait-for` 류는 `timeout` 필수. `parallel`·자동 재시도는 후속.
```yaml
loom_scenario: 1
name: cold-boot-smoke
steps:
  - psu.out: { name: bench, state: "off" }
  - sleep: 2s
  - psu.out: { name: bench, state: "on" }
  - uart.expect: { port: dut, regex: "login:", timeout: 30s }
  - can.wait-for: { channel: ch0, signal: VehState.PowerMode, op: "==", value: 2, timeout: 5s }
  - psu.meas: { name: bench }
    expect: { curr_between: [0.2, 0.6] }
  - manual: { prompt: "디스플레이에 부팅 로고가 보이면 PASS", timeout: 120s }
  - uds.dtc: { ecu: body, mask: 0x09 }
    expect: { count: 0 }
finally:
  - dlt.marker: { name: ecu1, label: "run end" }
  - psu.out: { name: bench, state: "off" }     # safe_state: true 동작
on_error: stop       # UDS 키·값
```
- **`finally`(마스터 A15)**: 정상 종료·`on_error`·에이전트 cancel 시 실행되며 R0–R1 단계와 Registry `safe_state: true` 동작(psu 출력 OFF, io 지정 채널, scope AWG OFF, la 패턴 정지)만 허용. **사용자 STOP / `stop-all`이면 자동 실행하지 않고**, 뷰어에 safe_state 동작들을 **"복구 송신" 1키**(user origin, latch 예외는 safe_state 항목만)로 제시. 어떤 경우든 safe_state 동작도 인터록·deny list를 통과해야 하며, 막히면 skip + `finally_blocked` 기록(예: `uds.flashing` 중 PSU OFF 금지). finally에 쓰이는 R2+ 승인은 run 시작 때 일괄 승인에 포함.
- RPC 이름은 D2(`<domain>.<verb>` 또는 `<domain>.<noun>.<verb>`, snake). 시나리오 동사는 CLI 동사와 같은 이름을 쓴다.
- 사전 위험 요약 `loom run plan` → 시작 시 R2·R3 단계 묶음을 한 번의 뷰어 승인으로(횟수·대상 바인딩, 사용 기한 10분 안에 시작 — A9), R4 단계는 단계마다 뷰어 승인(once), `default_blocked`·`immutable_deny` 단계가 있으면 계획 전체 거부.
- 개입 UX(run 패널): `p` 일시정지, `c` 계속, `s` 건너뛰기, `a` 중단, `m` 수동 PASS/FAIL, `r` **사용자가 누르는 수동 재시도**(같은 epoch·승인·인터록 재검사; 자동 재시도는 후속). **일시정지는 스케줄링만 멈추고 물리 장비 시간(ECU S3, 진행 중 펄스, SSH 명령)은 계속 흐른다**는 것을 패널에 표시. `finally`도 장비 쓰기이므로 인터록을 다시 통과(위 A15 규칙). 사용자 STOP은 run을 latch하고 run 패널에 "복구 송신" 키를 띄운다. 일시정지 중 사용자 조작(origin=user)은 run 로그에 operator action으로 기록.
- 에이전트 CLI: `run plan|start|wait --timeout|status|log --tail|pause|resume|abort`.

### 7.2 통합 타임라인 (viz 패널)
레인 = (domain, channel). 이벤트는 점/막대(IO 펄스, PSU 출력 ON 구간, debug HALTED, 스코프 캡처, run 단계), `metrics`는 미니 시계열. 기준 시각 = 호스트 `mono_ns`(단일 `loomd`). 장비 시각은 툴팁, 오프셋 추정은 후속. 커서 동기화로 모든 패널을 같은 시점으로 이동. 렌더 상한 초과 시 레인별 밀도 막대로 바꾸고 표시.

### 7.3 녹화·재생
녹화는 마스터 D7의 기본 기능(`~/.loom/records`에 도메인 저널 + 원시 + artifacts + manifest, 감사는 해시 체인). 재생은 `replay:<session>/…` 네임스페이스로 이벤트를 다시 흘려 모든 뷰어와 `tail/wait-for`가 그대로 동작하게 하되, **재생 채널의 쓰기 RPC는 전부 거부**(과거 agent TX를 현재 송신처럼 표시 금지, `original_origin` 보존). CAN viz의 Replay(M5)와 같은 계약으로 합친다.

### 7.4 공유 시뮬레이션 SimWorld (단계적)
1. 각 도메인 M1은 **작은 상태 sim 단독**(SimPsu, SimDltServer, 로컬 SSH, SimTarget, SimLa)으로 4요소를 완성한다.
2. 두세 도메인 이후 가상 시간·DUT 상태(off/boot/run/sleep)·결함 seed를 공유하는 작은 SimWorld 어댑터를 연결: PSU OFF → DLT/UART/CAN sim이 함께 멈춤, kl15 OFF → 슬립 전류 4 mA + "entering sleep" 로그. 이것이 있어야 러너·인터록을 HW 없이 CI에서 시험(결정 D17: 개념은 Opus, 시점은 Sol).

### 7.5 원격 벤치 (macOS 개발 + Windows/Linux HW — B1)
`loomd`·에이전트·장비는 Windows x64 또는 Linux x64 벤치 PC(B1: 데몬+에이전트가 장비 PC에서 실행이 기본). 원격 관찰이 필요할 때만 macOS는 **SSH 로컬 포워딩**으로 같은 WebSocket에 붙는다(IPC는 127.0.0.1 유지, 0.0.0.0 금지 — CAN 계획과 동일, M5 검토). 시계는 HW 쪽 단일 데몬 기준. 원격 render ack 지연은 별도 표기.

---

## 8. 추천 착수 순서와 이유 (마스터 §3.2 단계)

| 단계 | 내용 | 선행 조건 | 담당 추천 | 이유 |
|---|---|---|---|---|
| **P0** 계약·부품 | X0 플러그인 계약 검토(→CORE, §6 + A1/A2 훅), `loom.toml` 탐색 섹션 스키마, **DLT M0 gate**(네이티브 dlt-daemon 3.0.1 set-level 왕복 + 반영 관측, pydlt vs 자체 codec), **SimPsu**/SimScpiServer, **LA M0 gate(Saleae, §5.5 L-h)** | CORE 스키마 fixture·stub | 리드 + Opus(X0), Sol(DLT gate, SimPsu, Saleae gate) | core가 굳기 전에 확장 지점을 넣어야 싸다. DLT·Saleae gate 실패 시 백엔드 재선정 |
| **P1** PSU M1 (sim) | §5.1 h — **첫 플러그인 도메인, SimPsu 위 계약 검증 전용**(B5), 세 팀 M1과 같은 단계 | CORE P0 stub(gateway·subscribe·승인 v0·Registry) | Opus(CLI·패널·안전) + Sol(백엔드·sim·E2E) | 플러그인 계약과 R2/R3 경로를 처음 검증. 세 팀 임계 경로와 독립. 실PSU M2는 구매 시 |
| **P2** DLT M1 + **LA(Saleae) M1** | §5.2 h, §5.5 L-h(M1-la Saleae, 시뮬레이션 장치 + SimLa) → 이어서 보유 Saleae로 Windows·Linux M2 | P1, DLT·Saleae gate 통과 | Sol(DLT, la 워커), Opus(LA CLI·패널·화면 설계) | 스트림형(DLT)·캡처형(LA) 경로 검증. LA는 실장비 보유 → 탐색 첫 실HW 도메인(B5) |
| **P3** 교차 러너 M1 + SSH | §7.1(EXPLORE 분담: 단계 어댑터·SimWorld 1차·run 패널·수용 시험, **la 캡처 단계 포함**), §5.3 h | CORE 러너(P2), UART `loom/console/` 인터페이스 고정 | 리드 설계 + Opus(러너), Sol(SSH) | 교차 도메인 가치가 처음 드러남(UART/CAN 송신 → LA 관측). 셸은 콘솔 패키지 import |
| **P4** Debug(프로브 구매 시) | §5.4 h → M2 물리 gate | 승인·인터록·`immutable_deny` 동작, 프로브 구매, Q3(MCU 계열) | Sol(debug 백엔드), Opus(debug 안전·CLI) | 위험·불확실성이 가장 큼 → 안전 장치가 검증된 뒤 |
| 보류·이후 | **스코프(보류, B5)**, sigrok LA 백엔드(2순위 — Saleae 모델이 sigrok 지원 기종이거나 fx2 클론이 생길 때), PSU·IO 실장비(구매 시), DMM/전자부하(`loom.instr`), ADB/telnet 백엔드, XCP(U9 "있음"이면), HMI 영상, LIN(CAN 팀 귀속 협의) | | | |

세 팀 관계: PSU(sim)만 P1(첫 플러그인으로 계약 검증), 나머지 탐색 도메인은 세 팀 M1 이후 하나씩(마스터 §5). core 변경은 이 팀이 요청만 하고 구현은 CORE 팀이 한다. `loom/console/`은 UART 팀 소유(A4).
**순위와 착수 순서가 다른 이유**: §1 순위는 사용자 가치(v1.2부터 보유 장비 포함) 기준이고, 착수는 비용·의존성 기준이다. PSU(sim)가 P1인 것은 장비 없이 플러그인 계약을 가장 싸게 검증하기 때문이고, SSH(P3)는 UART 팀 `loom/console/` 안정 인터페이스를 기다린다.

### 8.1 마일스톤 (도메인 × 단계)

| 도메인 | M0 gate (스파이크) | M1 수용 (시뮬레이션, 4요소) | M2 (실장비 1종) |
|---|---|---|---|
| psu | pyvisa-py LAN 1,000회 질의 RTT 측정(sim) | §5.1 h 1–4 + C1–C5 (SimPsu, 계약 검증) | **구매 시**: PSU 1기종 드라이버 프로파일, 사건 지연 실측(Windows·Linux) |
| io | (구매 시) 대상 보드의 상태 조회 명령 확인 | SimRelay(조회 모드) — P3 SimWorld 안에서 | **구매 시**: 상태 조회 되는 보드 1종 |
| dlt | 네이티브 dlt-daemon 3.0.1 set-level 왕복 + 반영 관측, codec 비교 | §5.2 h | 사용자 ECU(verbose, TCP) — Windows·Linux 호스트 |
| shell | (없음 — R2 로컬 SSH 왕복 완료) | §5.3 h | 사용자 Linux/QNX 타깃 — Windows·Linux 호스트 |
| debug | pyOCD 옵션 강제(attach, auto_unlock=False) 확인, 의존성 lock | §5.4 h (SimTarget) | **프로브 구매 시**: 프로브 1 + MCU 1에서 connect/disconnect 무영향 시험, QEMU/Renode 통합 |
| scope | **보류(B5)** | (보류) §5.5 S h M1-scope | 구매 시 |
| la (Saleae, 1순위) | §5.5 L-h: 자동화 연결·시뮬레이션 장치 캡처·내보내기 왕복, 캡처 종료→Loom 표시 시간 실측 | §5.5 L-h M1-la (P2) | **보유 Saleae**로 Windows·Linux 각각(§5.6.1) |
| la (sigrok, 2순위) | sigrok-cli 원시+디코드 분기(§5.5 S c gate) | §5.5 S h | sigrok 지원 장비가 생길 때 |
| run | — | §7.1 단계 타입 + 개입 UX, SimWorld 1차 | 실벤치 cold-boot 시나리오 1개 |

### 8.2 구현 작업 분해 (병렬 단위, 담당 추천, 의존)

| ID | 작업 | 담당 | 의존 |
|---|---|---|---|
| X0-1 | 플러그인 Registry·DeviceBackend 계약 문서 → core 팀 검토 | Opus | — |
| X0-2 | `loom.toml` 도메인 섹션 스키마 + 2층 정책 규칙 | Opus | X0-1 |
| P-1 | SimPsu + SimScpiServer(지연·타임아웃·보호 주입) | Sol | — |
| P-2 | VisaBackend + 드라이버 프로파일(generic SCPI) + 질의 직렬화 | Sol | X0-1 |
| P-3 | `loom psu` CLI(경계·출력 상한·커서·dry-run) | Opus | X0-1 |
| P-4 | PSU PanelSpec(gauge, state_lamps, timeseries) + actions | Opus | D-2(viz) |
| P-5 | PSU E2E 테스트(C1–C5, 사용자 OFF → interrupted) | Sol | P-1..P-4 |
| D-a | DLT M0 gate 스파이크(네이티브 daemon) + codec 비교 | Sol | — |
| D-b | SimDltServer(제어 반응) | Sol | D-a |
| D-c | DLT 백엔드(프레이밍, wire.bin, ack/effective, gap 출처) | Sol | D-a, X0-1 |
| D-d | `loom dlt` CLI + trace_table/컨텍스트 트리 PanelSpec | Opus | D-c |
| S-a | (보류, B5) SimScope(트리거 상태기계) + 스코프 드라이버(DHO/SDS) | Sol | P-2, 구매 |
| S-b | (보류, B5) `loom scope` CLI + waveform 패널 요청 | Opus | S-a, D-2 |
| I-a | SimRelay(P3 SimWorld용) / IO 실백엔드는 보드 구매 시 | Sol | X0-1, Q8 |
| R-a | 교차 단계 어댑터(psu/io/dlt/**la**) + run 패널·개입 UX + 교차 수용 시험 (러너 엔진·스키마·잡 소유는 CORE P2) | Opus | PSU·DLT·LA M1, CORE 러너 |
| R-b | SimWorld 1차 어댑터(PSU/IO ↔ DLT/UART sim) | Sol | R-a |
| H-a | SSH 백엔드(채널, PTY lease, host key) + 로컬 sim | Sol | UART `loom/console/`(A4) |
| L-0 | **Saleae M0 gate 스파이크**(`spikes/explore/`, 시뮬레이션 장치로 연결·캡처·내보내기·분석기 표 왕복, 캡처 종료→파일 준비 시간, Windows·Linux 실행 조건) | Sol | — |
| L-a | **la 워커 `saleae` 백엔드**(자동화 연결 1개 소유·앱 감독·캡처 상태기계·내보내기 파서·`raw_ref`) + 순수 Python **SimLa**(같은 내보내기 형식 생성, CI용) | Sol | L-0, X0-1 |
| L-b | `loom la` CLI + TUI 패널(캡처 상태 실시간, 캡처 후 레인·디코드 표, 정지·트리거·분석기 편집) + 교차 시나리오(UART 송신 → LA 디코드 정렬) | Opus | L-a, UART M1 |
| L-c | (2순위) sigrok 백엔드(수집 1프로세스, 분기, TTL) | Sol | sigrok 지원 장비가 생길 때 |
| G-a | SimTarget + debug 백엔드(pyOCD 옵션 강제, source 태그) | Sol | X0-1 |
| G-b | debug 안전(deny list, 확인, 인터록) + CLI·패널 | Opus | G-a, CR-X4 |
| K-1 | 도메인별 SKILL.md + references | Opus | 각 도메인 M1 |

병렬 가능 묶음: {P-1, P-3, D-a, L-0, X0-2}는 동시에, {P-2, D-b, D-c, L-a}는 X0-1 뒤 동시에, {L-b, D-d}는 PSU M1 뒤 P2에서, {I-a, G-a}는 각각 P3·P4(구매 시).

---

## 9. 사용자에게 물어볼 질문 (기본값 제안)

마스터 §4가 통합본이다(탐색 질문은 U2 보유 장비, U3 실차/벤치, U4 R2 경로, U9 DLT·XCP, U10 저가 계측 세트로 흡수). v1.2에서 U1·U2·U3·U10이 답을 받았다(B1·B2·B4·B5) — 아래 ✅ 표시. 남은 질문은 마스터 B7.

| # | 질문 | 기본값(답 없을 때) |
|---|---|---|
| Q1 ✅B2 | 보유 전원공급기·DMM·전자부하 모델과 연결(LAN/USB/시리얼)? | **답: 없음.** SimPsu로 계약 검증만, 실물은 구매 시 모델 확인 후 드라이버 1종 |
| Q2 ✅B4 | 실차인가 벤치인가? 벤치에 액추에이터(모터·밸브·고전압)가 있나? | **답: 둘 다, 실차 송신 차단 안 함.** 액추에이터 여부는 미답 → 기본 `actuators=false` |
| Q3 | 디버그 대상 MCU 계열(Cortex-M / AURIX / RH850)과 프로브(J-Link / ST-Link / CMSIS-DAP / Lauterbach)? | Cortex-M + 프로브 1종. AURIX/RH850이면 TRACE32 경로로 바꾸고 Debug(P4)를 뒤로 |
| Q4 | DLT 대상: verbose/non-verbose, TCP 3490/직렬, DLT Viewer 동시 접속 필요? 타깃이 libdlt 3.0.1 V1인가? | verbose, TCP 3490, V1, DLT Viewer 동시 사용, injection 금지 |
| Q5 | Linux/QNX/Android 기반 ECU에 SSH/ADB 접근이 있나? 허용 명령 범위? | SSH 있음, sudo/reboot 금지, ADB 후순위 |
| Q6 | 업무에 XCP 측정·캘리브레이션(A2L, XCP slave, RAM 쓰기 권한)이 있나? | 없음 → XCP 예비 유지 |
| Q7 ✅B1 | 실HW가 붙는 PC: macOS 직접? 별도 Windows 벤치 PC? | **답: Windows x64와 Linux x64**(데몬+에이전트가 장비 PC). macOS는 개발·sim |
| Q8 | 릴레이/IO 보드 보유 여부, 실제 상태 조회(접점 피드백) 가능 모델인가? 허용 전압·전류? | B2 목록에 없음 → 미보유로 본다. 구매 시 상태 조회 되는 모델 권장 |
| Q9 | (마스터 U4) R2 확인을 토큰 재제출로 충분히 볼지, 동작 단위 auto 완화를 쓸지? | sim auto, bench·vehicle `token_or_viewer`(B4). 반복 동작은 사용자가 뷰어 승인 후 auto로 완화 권장 |
| Q10 | 에이전트가 PSU 출력 ON까지 해도 되나? | 마스터 D6에서 PSU 출력 ON = R3(뷰어 승인) 확정 — session scope 승인으로 반복 사이클 허용 |
| Q11 ✅B5 | 스코프/LA를 이미 가지고 있나? 없으면 세트 A/B/C 중 무엇을 살 의향? 비절연 USB 장비를 차량 접지에 물려도 되는 환경인가? | **답: Saleae LA 보유, 추가 구매 없음.** 접지 질문은 미답 → 실차에서 LA 접지는 차량 GND 기준, 12 V 넷 직결 금지 경고 유지 |
| **Q13** (B7) | **Saleae 모델**(Logic 8 / Logic Pro 8 / Pro 16 / 구형 Logic·Logic16)과 Logic 2 버전? | 신형(Logic 8/Pro) 가정 → `saleae` 백엔드. 구형이면 sigrok 스트리밍 경로도 가능(§5.5 L-f) |
| **Q14** | Logic 2 앱을 Loom이 자동으로 띄워도 되나, 사용자가 직접 띄운 앱에 붙을까? | 사용자가 앱을 열어 두었으면 붙고(파형을 앱에서 봄), 없으면 Loom이 자동화 모드로 기동 |
| Q12 | 로그량·보존 기간·민감정보 정책? | 로컬 기록, 용량 quota, 비밀정보 마스킹 |

---

## 10. 공통 계약 변경 요청 / 타팀 의존성

### 공통 계약 변경 요청 — 처리 상태 (마스터 v1.1 기준)
| ID | 요청 | 상태 |
|---|---|---|
| CR-X1 | 도메인 플러그인 구조(entry point `loom.domains` + Registry, core 소유 쓰기 파이프라인) | **채택** D10 + A1(쓰기 클래스 3종)·A2(도메인 훅 3개) |
| CR-X2 | 이벤트 `metrics`·`quality`, gap 이름·seq 통일 | **채택** D3(`metrics[{name,value,unit}]`, 두 층 순번) + A13(`display/record/source.gap`) |
| CR-X3 | 쓰기 상태 단계 공통화 | **채택** A14(D4 대체) |
| CR-X4 | R4 의미 통일, deny list, 승인 바인딩, 인터록 | **채택** D6 + A8(R2 경로)·A9(R3 기한)·A11(차단 3층, `immutable_deny`). A10(vehicle 기본 deny)은 v1.2 B4로 폐기 |
| CR-X5 | UART 콘솔 엔진·위젯 재사용(공개 Transport 없음) | **채택** A4 — UART 팀 소유 `loom/console/` |
| CR-X6 | core 교차 러너, `finally` 규칙 | **채택** D12 + A15(STOP 시 복구 송신 1키, 인터록 통과) + §3.2 분담 |
| CR-X7 | `loom.toml` 도메인 섹션 + 2층 정책 | **채택** D6(사용자 `~/.loom/config/policy.toml`, 완화는 뷰어 승인 후) |
| CR-X8 | SimWorld 공유 가상 DUT | **미결정** — 마스터에 없음. 탐색 P3 작업(R-b)으로 진행하고, 공통화가 필요해지면 CORE에 변경 요청 |
| CR-X9 | CLI 종료 코드·봉투·커서·`LOOM_CLIENT`·RPC 이름·스킬 경로 통일 | **채택** D5 + A6/A7, D2(RPC 2단·3단 snake), D13(`skills/` → 복사) |
| CR-X10 | 데몬 자동 기동 통일 | **채택** D8(자동 기동, `LOOM_NO_AUTOSTART=1`) |
| (신규) CR-X11 | Registry에 `safe_state`·`r2_default`·`risk_table(default_blocked, immutable_deny)`·`write_class` 선언 필드 확정 | CORE 계약 v1에서 이름 확정 요청(§6.2 초안) |

### 타팀 의존성 (v1.2 기준)
| ID | 대상 | 요청 |
|---|---|---|
| D-1 | UART | `loom/console/`(A4) 안정 인터페이스를 P2 말까지 고정 — 셸(P3)·RTT(P4)가 import. 셸 PTY 입력에 InputArbiter 규칙, 위험 패턴 표 공유 |
| D-2 | CAN(viz) | 패널 타입 `gauge`, `state_lamps`, `console` 호스팅, `waveform`, `logic_lanes`, `decode_table`, `timeline_lane`, `run_panel`; `metrics` → long-format 자동 변환; 캡처 배지·데시메이션·gap 표시 규칙; 디지털 파형은 전이 보존 요약(LTTB 금지); 서로 다른 도메인 시각 사이 정렬 오차 라벨 |
| D-3 | CAN | (후속) LIN·CAN XL 귀속 의향 확인. XCP on CAN을 붙일 경우 CAN 워커 내 호스팅/고속 구독 API(ISO-TP 방식) — pyXCP가 CAN 장치를 직접 열면 채택 안 함 |
| D-4 | UDS | 플래시·0x34–0x37 진행 상태를 인터록 `uds.flashing`으로 노출. 전원 사이클 전 pending 진단/TesterPresent 정지 정책. 시나리오 스키마 `loom_scenario: 1` 정렬 |
| D-5 | CORE | Registry 선언 필드(CR-X11), `source.gap` cause 목록에 탐색 값 추가(v1.2: `capture_gap`·`backend_down` 추가), 승인 패널에 출력 유지 장치의 비상 키 `!` 표시 규칙, **캡처형 도메인용 잡 진행 이벤트**(`job.progress{state, elapsed, total}` — LA 캡처 상태 실시간 표시, §5.5 L-e). (`worker=True` 허용은 D1로 해결) |
| D-6 | UART·CAN (v1.2) | LA 교차 관측용: 송신 이벤트의 host 시각(`ts.mono_ns`)과 바이트·프레임 내용이 이벤트에 있어야 LA 디코드와 맞춰 정렬 오차를 계산할 수 있음(이미 D3로 충족 예상 — 확인만). CAN 브리지의 `external:canoe` 송신도 같은 규칙 |

---

## 11. 리스크 (전체)

| 리스크 | 영향 | 대응 |
|---|---|---|
| 보유 장비가 Saleae LA뿐(B2) — PSU/스코프/릴레이/프로브 없음 | PSU·debug·IO는 실HW 검증 없이 sim에 머묾 | PSU는 계약 검증 역할로 한정(B5), 실HW M2는 구매 시. Saleae 모델(Q13)·MCU(Q3)는 B7 |
| **Saleae 캡처 중 스트리밍 불가**(B5) | LA는 4요소 중 읽기·표시가 △ — "실시간 사용자 화면" 원칙을 일부만 충족 | 정직 표기: 캡처 중 파형은 Logic 2 앱, Loom은 상태·진행·자극 이벤트 실시간, 캡처 후 결과. 짧은 반복 캡처(`la watch --repeat`)로 준실시간, 캡처 사이 공백은 `source.gap{cause:"capture_gap"}`(§5.5 L) |
| Logic 2 앱 의존(독점 앱, 자동화 API 버전, 앱 크래시·업데이트) | LA 전체 | 워커가 앱 감독·재연결(`source.gap{cause:"backend_down"}`), Logic 2·`logic2-automation` 버전을 manifest에 기록, 검증된 버전 표, sigrok 2순위 경로 유지 |
| DLT 3.0.1 제어 왕복 미검증 | DLT 계획 전체 | P0 DLT M0 gate. 실패 시 백엔드 재선정(BMW python-dlt/libdlt 경유 등) |
| pyOCD 안전 기본값(halt, auto_unlock) | 실행 중 ECU 정지·보호 해제 erase | 백엔드 계약에서 강제 덮어쓰기 + 물리 gate 시험 + `immutable_deny`(A11) |
| 의존성 충돌(pyOCD ↔ PyLink 2.x) | 설치 실패 | 1.7.0 고정 또는 워커 환경 분리, resolver lock CI |
| GPL/MPL 경계(sigrok GPL-3.0, pya2ldb GPLv2, dlt-daemon MPL-2.0, asyncssh EPL/GPL 이중) | 배포 조건 | 하위 프로세스·외부 설치·재배포 금지 원칙 + 배포 전 라이선스 검토(하위 프로세스가 의무를 없애지 않음) |
| 공통 계약 세부 이름(Registry 필드, `source.gap` cause 값)이 CORE 계약 v1에서 바뀔 수 있음 | 탐색 코드 수정 | 마스터 v1.1로 의미는 확정됨. 이름은 CR-X11로 CORE에 확정 요청 |
| `loom/console/` 인터페이스가 P3 전에 안정되지 않음 | 셸·RTT 일정 지연 | A4로 UART 팀이 처음부터 공유 패키지로 만듦. P2 말에 인터페이스 고정 확인(D-1) |
| sigrok 정식 릴리스 정체, macOS 설치 경로 (v1.2: 2순위 경로) | sigrok LA 지원 장비가 설치 방식에 좌우 | 정식판 드라이버만 공식 지원, nightly는 사용자 선택 |
| 지연 목표 전부 미실측 | 목표 미달 가능 | 각 M1에 render ack 기반 p95 측정, 미달 시 목표 재협의(화면에는 실측 표시) |
| 하드웨어 손상(접지 루프, 과전압, 릴레이 정격, 패턴 출력 충돌) | 장비·DUT 파손 | 프로파일 한계·probe_map·기본 입력/OFF·스킬·뷰어 상시 경고 |

---

## 12. 결정 기록

| # | 쟁점 | Opus | Sol | **결정** | 이유 |
|---|---|---|---|---|---|
| D1 | 루프 ○ 기준 | 지금 Python으로 사람 없이 열 수 있나 | 설계 가능한 능력 | **지원 OS에서 데몬이 열면 ○, OS 제한은 O 점수, 검증된 경로가 없어 새 래퍼 필요하면 △** + 별도 검증 단계 열 | Opus의 엄격함(FlexRay 부풀림 방지) + Sol의 중복 감점 방지(Windows 전용 LIN) |
| D2 | 점수 가중 | 루프 30 + 가치 30 | 루프 40 + 가치 20 | **Opus** | Modbus/릴레이 역전 방지 |
| D3 | 상위 목록 | DLT→PSU→Shell→Debug→IO | PSU→DLT→SSH→Debug→XCP(조건부), R2에서 "기본 4개" | **PSU(+IO 단계) → DLT → SSH → Debug → 관측 계측**, XCP 예비 | 상위 4개는 양측 일치. 5위는 사용자 명시 관심(user-notes) + △ 1개(쓰기) 규칙 충족 + 교차 자극 M1 필수. IO는 PSU에 묶어 "사람 손 없는 벤치" 가치 유지, 상태 조회 보드만 루프 인정 |
| D4 | PSU vs DLT 선착 | DLT | PSU | **PSU M1 ∥ DLT M0 gate 병렬**, gate 통과 시 DLT 우선 가능 | 의존성 없음, 두 워커로 병렬 |
| D5 | PSU 폴링 | 10 Hz, 상태 1 Hz로 ≤ 500 ms | 5 Hz, 미완료 1개 | **Sol**, 사건 지연 = 주기 + RTT + 렌더 | R1-opus 지연 표현이 산술적으로 불가(R2-sol F5) |
| D6 | 쓰기 표시 | 기록→방송→송신 | queued/sent/ACK/효력 | **의도 먼저 표시 + 상태 단계 분리** | enqueue ≠ render ≠ wire 완료 |
| D7 | 위험 척도 | R0–R5 | R0–R3 | ~~R0–R4 + deny list + 2층 정책~~ → **v1.1: D22로 대체**(마스터 D6 + A8–A11) | 세 팀 plan.md 정렬 후 마스터 확정 |
| D8 | debug halt 등급 | R3 | R2 | **R3**(actuators면 R4) | 워치독·액추에이터·통신 타임아웃 |
| D9 | DLT codec | 자체 구현 | pydlt 재사용 | **M0에서 비교 후 결정** | 0.x라는 이유만으로 재작성 근거 부족, 자체 codec은 차이만 |
| D10 | 셸 exec | 공유 PTY + 센티넬 | 이름 붙은 exec 채널 | **Sol + 자동 탭** | exit 정확, 셸 상태 비의존, 가시성 동일 |
| D11 | 스코프/LA 구조 | (R2b) scope=instr, la=sigrok | (R2c 검증) | **두 이름**, ~~la는 sigrok-cli 하위 프로세스~~ → **v1.2: la 백엔드 `saleae` 1순위·`sigrok` 2순위·`sim`(D32)** | 백엔드·라이선스·라이브 방식 상이 |
| D12 | 플러그인 등록 | entry point 1그룹 + Registry 전부 | 5그룹 | **1그룹, 필수/선택 분리 + Capability** → 마스터 D10 채택. v1.1에서 A2 훅·`write_class`·`safe_state`·`risk_table` 필드 추가(CR-X11) | 단순성 + 첫 플러그인 비용 감소 |
| D13 | origin 확장 | `script:<run-id>`, `core` | enum 유지 + 별도 ID | **enum 유지 + actor_id/job_id/original_origin** | 세 팀·xteam 합의 |
| D14 | 뷰어 없을 때 R3+ | 즉시 실패 | 자동 attach | ~~뷰어 실행 제안 → 실패~~ → **v1.1: 승인 필요 시 exit 5 `E_NO_APPROVER`(hint에 뷰어 실행 방법), 그 외 결과는 `no_viewer_attached` 경고**(D9) | 마스터 우선 |
| D15 | 벤치 프로파일 파일 | 별도 `loom.bench.toml` | 소절별 profile | **`loom.toml` 안 섹션 + 사용자 `~/.loom/config/policy.toml`**, 완화는 뷰어 승인 후(v1.1, 마스터 D6) | 설정 파일 하나(CAN CR7) + 2층 정책 |
| D16 | 시나리오 러너 | core, UDS YAML 흡수 | 공통 기능, 후순위 | **core 상주 잡, UDS 러너는 어댑터로 유지** → 마스터 D12 채택, `finally`는 A15, 분담은 D26 | 모듈 이름보다 잡 소유·같은 RPC·같은 스트림이 핵심 |
| D17 | SimWorld | 처음부터 공유 | 도메인별 sim | **도메인 M1은 단독 sim, 2–3 도메인 후 공유 어댑터** | 개념(Opus) + M1 축소(Sol) |
| D18 | Debug M1 sim | 순수 Python SimTarget | QEMU GDB | **M1 SimTarget, M2 QEMU/Renode** | CI 무설치, 실행 중 watch 시연 |
| D19 | 한계 초과 | 클램프/거부 혼재 | 거부 | **기본 거부**(exit 4 `E_DENIED`). 한계 완화는 정책 변경 + 뷰어 승인(D6) | 요청과 다른 출력의 조용한 실행 방지 |
| D20 | 셸 도메인 범위 | SSH/ADB/telnet/TCP 한 묶음 | SSH만, ADB 별도 후보 | **한 도메인 `loom shell`, M1은 SSH만** | CLI·스킬·뷰어 중복 방지 + M1 축소 |
| D22 | v1.1 위험 등급 | R2 allow 자동 | — | **마스터 D6+A8/A9/A11 적용**: R2 `auto|token_or_viewer|viewer`(탐색 bench 기본 token_or_viewer, `confirm` 동사), R3 뷰어만, R4 매번 승인 + `default_blocked`, `immutable_deny`, vehicle 축 | 마스터 우선. D7·D19 대체 |
| D23 | v1.1 쓰기 상태·손실 | 탐색 원안 | — | **A14 상태명, A13 `source.gap{cause}`** | 마스터 우선 |
| D24 | v1.1 STOP·비상 OFF | Esc 비상 OFF | — | **A3: STOP은 출력 유지, 도메인 비상 키 `!`** | Esc는 Textual 모달과 충돌 |
| D25 | v1.1 콘솔·CLI·뷰어 | CR-X5 요청, 자체 커서 | — | **A4 `loom/console/` import, D5/A6 종료 코드·봉투·`others_since_last`·커서, D9 M1 TUI** | 마스터 우선. D14 대체 |
| D26 | v1.1 finally·러너 | UDS 규칙 대기 | — | **A15 적용**(STOP 시 자동 실행 안 함, 복구 송신 1키, safe_state도 인터록 통과). 러너 분담: CORE=잡·스키마·승인/인터록·run 이벤트, EXPLORE=단계 어댑터·SimWorld·run 패널·수용 시험 | D16 갱신 |
| D27 | v1.1 쓰기 클래스·훅 | — | — | **A1/A2 적용**: 요청형/잡형(io seq, la stream·pattern, AWG 연속, shell log, dlt TTL 레벨)/대화형 입력(PTY·RTT 사용자 키). `presend_check`로 플래시 인터록 재확인 | 마스터 우선 |
| D28 | v1.1 메시지 크기·스코프 M1 | 이벤트에 ≤10k점, 웹 파형 M1 | — | **A5: 캡처 이벤트 ≤ 2k점 + `raw_ref` HTTP GET. 스코프 M1은 TUI, 웹 파형은 P3 이후 M2** | 64 KiB 배치 상한, 웹은 CAN M3(P3) |
| D29 | v1.1 일정 | E0–E7 | — | **마스터 §3.2 P0–P4**: P0 계약검토·DLT gate·SimPsu / P1 PSU / P2 DLT·스코프·IO / P3 러너·SSH / P4 LA·Debug → **v1.2: D33으로 갱신** | 마스터 우선 |
| D21 | LA 원시 기록 + 실시간 디코드 | (R2b) sigrok-cli 한 번으로 `.sr` + 주석 | (R2c) 디코더 켜면 출력 모듈 건너뜀(소스) | **수집 1프로세스 원시 stdout → 워커가 기록/디코더 하위 프로세스/UI로 분기**, M0 gate (v1.2: sigrok 2순위 경로에만 적용) | 같은 장비 이중 open 금지 + 무손실 기록과 라이브 주석 동시 확보 |
| **D30** | **v1.2 사용자 답변 반영** (마스터 B1–B7) | — | — | 실HW 호스트 Windows x64 + Linux x64, 실HW 체크리스트 두 OS(§5.6.1) · 보유 장비 반영 순위 **DLT → LA(Saleae) → SSH → PSU(sim 계약 검증) → Debug(구매 시)**, 스코프 보류, XCP 예비 · vehicle = bench와 같은 규칙 + `env=vehicle` 배지(A10 폐기) · 질문 Q1·Q2·Q7·Q11 답 반영, Q13·Q14 추가 | 사용자 답변이 A·D보다 우선. 가치 순위에 "보유 장비"를 넣지 않으면 쓸 수 없는 장비부터 만들게 됨 |
| D31 | 규칙 3(△ 2개 이상 상위 불가) vs 보유 장비 | — | — | **보유 장비 예외(§2.3 규칙 8)**: △를 그대로 표에 두고, △마다 보완 화면·수용 항목 필수. 점수 불변 | 사용자 결정(B5) 존중 + 점수 부풀림 방지 |
| D32 | LA 백엔드·화면 | sigrok 연속 스트림 | — | **`saleae`(Logic 2 자동화, la 워커가 연결 1개 소유·앱 감독) 1순위, `sigrok` 2순위, CI는 순수 Python `SimLa`**. 화면 = 캡처 전 설정 / 캡처 중 Loom 상태·진행·자극 이벤트 실시간 + 파형은 Logic 2 앱 / 캡처 후 Loom 레인·디코드 표. 준실시간은 짧은 반복 캡처 + `capture_gap` 표시 | B5. 스트리밍 불가를 숨기지 않고 사용자가 "지금 무엇을 어디서 보는지" 명시 |
| D33 | v1.2 일정 | — | — | **P0** X0·DLT gate·SimPsu·Saleae gate / **P1** PSU M1(sim) / **P2** DLT M1 + LA(Saleae) M1→M2 / **P3** 러너(la 단계 포함)·SSH / **P4** Debug(구매 시). 스코프 보류, IO·PSU 실장비 구매 시 | 마스터 §3.2 v1.2 표 |
| **D34** | **v2.0 방향 전환**(마스터 C1–C8) | — | — | 탐색 도메인 Rust 이식(§0.4): DLT 자체 codec(+dlt-core 비교), SSH russh, Saleae tonic/prost, Debug probe-rs(+TRACE32 C FFI), SCPI raw TCP, IO hidapi/tokio-modbus. **Python 예외 없음** | 사용자: Python 코어가 느리고 메모리 문제로 죽음. 모든 탐색 경로에 Rust 대안이 확인됨(`test/r1-sol.md` §B) |
| D35 | v2.0 화면 | Textual TUI M1 | — | **웹 UI 패널이 M1**, TUI 보조. 패널 명세는 `_xteam/explore-ui.md`, 러너 화면은 테스트 실행 화면 재사용 | C6 |
| D36 | v2.0 팀 역할 | — | — | EXPLORE 주 작업 = **TEST 기획**(`docs/plan/test/plan.md`), 탐색 도메인은 §0.4만 갱신. 교차 러너와 테스트 세션 엔진 통합 | C7, 사용량 한도 고려 |

---

## 13. 핵심 원칙 체크리스트 (02-core-principle.md)

| 항목 | PSU(+IO) | DLT | Shell | Debug | LA (Saleae, v1.2) |
|---|---|---|---|---|---|
| plan 맨 앞 "핵심 루프" 표 | [x] §0 | [x] §0 | [x] §0 | [x] §0 | [x] §0 |
| 종단 간 시나리오(에이전트 → 장비 → 화면 → 사용자 개입 → 에이전트가 origin=user 읽음) | [x] §0.1, §5.1 b | [x] §5.2 b | [x] §5.3 b | [x] §5.4 b | [x] §5.5 L-b (교차 자극) |
| 실시간성 목표 + 고부하 정직성(드롭·다운샘플 표시, 기록 무손실) | [x] §5.1 c | [x] §5.2 c | [x] §5.3 c | [x] §5.4 c | [x] §5.5 L-c (캡처형 — 스트리밍 불가 정직 표기) |
| M1 = 시뮬레이션 위 걷는 뼈대(4요소 최소형) | [x] §5.1 h | [x] §5.2 h (+M0 gate) | [x] §5.3 h | [x] §5.4 h | [x] §5.5 L-h (시뮬레이션 장치 + SimLa) |
| 사용자 화면이 에이전트 세션과 분리되지 않음(late viewer snapshot, 숨은 연결 금지) | [x] §0 공통, §5.1 h-3 | [x] §0, §5.2 h | [x] §5.3 a (exec 채널 자동 탭) | [x] §5.4 e (GUI도 데몬 경유) | [x] §5.5 L-e (Loom 패널이 정본, 앱은 캡처 중 보조 화면) |
| 원칙에 기여 안 하는 기능은 후순위/비범위 | [x] §3, PSU는 sim 계약 검증만·IO는 구매 시(v1.2) | [x] injection·non-verbose 후속 | [x] ADB/telnet·push/pull 후속 | [x] flash·SVD·QEMU M2 | [x] 아날로그 채널·HLA·sigrok 후속, 스코프 보류 |

공통 수용 기준(모든 탐색 도메인 M1): C1 같은 데이터(session/epoch/seq), C2 late attach(작업 중 두 번째 뷰어), C3 user override(대기 중 에이전트 쓰기 취소·충돌 반환, 에이전트가 다음 read에서 확인), C4 지연 계측(host 수신 → render ack p95), C5 부하·실패 주입(큐 상한, gap 출처, 디스크 정지, 재연결 epoch).

<!-- END PLAN -->
