# Loom 마스터 계획 (통합본 v2.2)

작성: 메인 오케스트레이터, 2026-11-10 · v1.1 = 네 리드 검토(`*/master-review.md`) 반영 · v1.2 = 사용자 답변 반영(§2.2 B1–B7) · v2.0 = Rust 코어·UI 1급·통합 테스트(§2.3 C1–C8) · **v2.1 = 팀 v2.0 계획 통합·계약 판정(§2.4 E1–E12, §3.0 단계)**. 우선순위: **F > E > C > B > A > D** (v2.2 = §2.5 F1 모두 자동). 팀 계획: `core/ ui/ uart/ can/ uds/ explore/ test/` 의 `plan.md` (v2.0).
입력: `uart/plan.md`, `can/plan.md`, `uds/plan.md`, `explore/plan.md`, `_xteam/can-uds-isotp.md`, `_xteam/viz-requirements.md`
우선 원칙: `02-core-principle.md` — **에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자는 같은 데이터를 같은 순간에 보고 개입한다.**

이 문서는 (1) 네 팀 계획 요약, (2) 팀들이 메인에 요청한 **공통 계약 결정**, (3) 구현 단계 순서와 팀 구성, (4) 사용자 질문 통합본이다. 도메인 세부는 각 팀 `plan.md`가 정본이며, 이 문서와 충돌하면 **이 문서의 §2 결정이 우선**한다.

---

## 1. 한눈에 보기

| 컴포넌트 | 에이전트 (직접 접근·읽기·쓰기) | 사용자 실시간 화면 + 개입 | M1 걷는 뼈대 (시뮬레이션) | 지연 목표 (장치→화면) |
|---|---|---|---|---|
| ① UART | `loom uart open/send/read/expect/run` | herdr 패인 `loom uart attach dut` — 터미널 모드(정제 패스스루) + 트레이스 모드, 원시 키·Ctrl-C 직접 입력 | pty 가짜 장치(`bootshell`, `flood`) | p95 < 100 ms |
| ② CAN | `loom can send/trace/last/sig get`, `loom can sig wait-for`, 주기 송신 job | `loom can monitor` TUI(M1) → 웹 Trace/Graphics/IG(M3), `:send`·STOP | virtual 버스 + 가짜 ECU SimNode | TUI p95 < 200 ms |
| ③ UDS | `loom uds connect/read-did/write-did/dtc/raw/confirm` | `loom uds view` TUI — 요청/응답 트레이스, 사용자 직접 송신·승인·긴급 정지 | 가상 DoIP ECU (CAN 일정과 무관) | p95 ≤ 100 ms / p99 ≤ 200 ms |
| ④ 추가 (탐색) | `loom psu`, `loom dlt`, `loom shell`, `loom debug`, `loom la`/`scope` | 같은 뷰어 프레임워크의 패널 (스코프 M1은 TUI, 웹 파형은 P3 이후) | 도메인별 Sim | PSU·SSH ≤ 100 ms, DLT ≤ 150 ms, debug watch ≤ 250 ms, LA 디코드 ≤ 300 ms, 스코프 트리거 ≤ 1 s (미실측 목표) |
| 공통 | `loom` 공통 스킬, `loom stop-all` | `loom.viz` — 데이터만 주면 렌더링 (`loom viz show data.csv`) | — | 플롯 ≤ 250 ms |

**탐색 팀 추천 순위**: ① 벤치 전원 PSU(+상태 조회 되는 릴레이 IO) ② DLT(사용자 기존 dlt-wrapper와 연결) ③ 타깃 셸(SSH) ④ 디버그 프로브(pyOCD/J-Link) ⑤ 관측 계측(sigrok 로직 애널라이저 + SCPI 스코프). XCP는 예비. **v1.2(B5): 보유 장비인 Saleae LA를 P2로 앞당기고, PSU는 Sim으로 플러그인 계약 검증만.** 시나리오 러너·통합 타임라인·녹화/재생은 별도 장비가 아니라 core/viz 기능으로 판정.

---

## 2. 공통 계약 결정 (메인 확정)

네 팀이 대부분 같은 방향으로 수렴했다. 아래는 그 확정과, 엇갈린 부분에 대한 메인 결정이다. 근거는 각 팀 §15와 결정 기록.

### D1. 프로세스 구성 — `loomd` 하나, 내부는 gateway + 장치 worker
- 사용자·에이전트가 보는 것은 데몬 하나 `loomd` (포트·URL·승인·감사 하나).
- 내부: **gateway**(IPC, 이벤트 허브, 정책·승인·감사, 기록 조정, UDS 세션 관리자) + **장치 owner worker 프로세스**.
- CAN은 M1부터 worker 분리 필수 (`can-hw`, `can-sim`) — CAN 팀 실측: 같은 프로세스에서 CPU 경쟁 시 ISO-TP FC p99 1.5 → 52 ms. ISO-TP 스택은 CAN worker 안.
- UDS는 gateway 안 (CAN-TP는 CAN worker와 PDU 단위 RPC, UDS 실측 왕복 p99 3.09 ms).
- UART는 worker 분리 **권장** (GIL·장애 격리) — 단, 공통 worker 계약을 따르면 UART 팀 판단.
- 탐색 도메인 debug, la는 `worker=True` 선언 허용.
- 도메인 간 API는 인프로세스/RPC 동일 시그니처.

### D2. IPC
- 외부: **JSON-RPC 2.0 over WebSocket, 127.0.0.1 단일 포트**, 같은 오리진에서 웹 정적 파일 제공. Origin/Host 검사, 메시지 64 KiB 상한, ping 20 s.
- 발견: `~/.loom/run/daemon.json` (port, pid, token, version; 권한 0600).
- 연결 첫 메시지 `hello{proto, token, client_id, want_role}` → **서버가 role 부여** (`agent` | `viewer`). viewer 자격은 대화형 TTY 뷰어 또는 웹 일회성 bootstrap→HttpOnly 쿠키. 요청 필드의 origin은 무시 — **origin은 연결 자격으로 결정**.
- 구독: `core.subscribe` = 원자적 snapshot + `after_seq` 이음매 → `core.events` 배치(≤ 40 ms / 200건 / 64 KiB). 느린 구독자는 그 구독자만 드롭 + `display.gap`. 응답·승인·상태·오류·STOP 이벤트는 **우선 제어 큐**, 드롭 금지.
- **publish-before-reply**: 장치 이벤트를 뷰어 스트림에 발행한 뒤 에이전트에게 RPC 응답. 에이전트가 결과를 보는 시점에 사용자도 이미 볼 수 있다.
- 내부 worker IPC: OS별 로컬 소켓 / named pipe.
- RPC 이름: `<domain>.<verb>` 또는 `<domain>.<noun>.<verb>` (소문자 snake). 예: `uart.send`, `can.isotp.open`, `core.subscribe`. 이벤트: `<domain>.<event>`.

### D3. 이벤트 레코드 (라이브 스트림 = 기록, 같은 스키마)
- 순번 두 층: 원본 `(producer_id, producer_epoch, source_seq)` (worker 할당, 저널 보존) + gateway 전역 `stream_epoch/stream_seq` (구독·커서·snapshot).
- 필수 필드: `schema_version, session_id, domain, channel, kind, seq, ts, dir, origin, client_id, actor_id, request_id, job_id, epoch, generation, purpose, risk`.
- `ts = {mono_ns, wall_ns, clock_id, source, quality}` — uint64는 JSON 문자열. 청크마다 wall 기록(macOS sleep 대비).
- `dir`: `rx | tx | null`(상태/제어). **origin enum 고정 4종: `agent | user | device | script`.**
- 바이트 payload `{encoding: hex|base64|raw_ref, data}` (RPC 인자는 hex 문자열).
- 수치 확장(탐색 CR-X2): 선택 필드 `metrics[{name, value, unit}]`, `quality[]` — viz가 도메인 무관하게 자동 플롯.
- 손실 이벤트 이름 통일: `display.gap`(화면 드롭), `record.gap`(기록 손실), `record.health`. `count=unknown` 허용. 도메인별 `gap`/`uds.gap` 이름은 쓰지 않음.

### D4. 쓰기 상태 단계 (탐색 CR-X3 채택)
`requested → accepted → sent → confirmed | effective_verified | failed | outcome_unknown`. 화면과 CLI 결과는 이 단계를 그대로 표시 — 거짓 성공 금지. UDS `may_have_executed`는 `outcome_unknown`과 같은 의미로 결과에 유지.

### D5. CLI 공통 규약 (세 팀 안 통일)
- `--json` 봉투: `{schema_version, ok, cmd, data, meta{truncated, next_cursor, range, display_gap, record_health}, warnings[], others_since_last{count, origins, from_cursor}, error{code, message, hint, retryable, request_id, detail}}`.
- **`others_since_last`는 모든 도메인의 모든 결과에 붙인다** (UDS 안 채택). 사용자가 개입했다는 사실을 에이전트가 반드시 알게 하는 장치. 사람용 출력은 `NOTE:` 줄로.
- 출력 상한: 하드 1,000행 / 64 KiB 공통. 기본값은 도메인이 그 이하로 정함 (UART/CAN 100행, UDS 40행). 넘으면 잘림 + 다음 커서.
- 무한 스트림 금지: 대기형은 항상 timeout (`--follow`도 최대 600 s). 무한 구독은 사람용 뷰어만.
- 커서 문법: `--since @last | seq:N | mark:X | -5s | HH:MM:SS.mmm | ISO8601`.
- client id 환경변수: **`LOOM_CLIENT`** (없으면 부모 셸 세션+사용자로 안정 id 생성). UDS 문서의 `LOOM_CLIENT_ID`는 이 이름으로 변경.
- mutation 자동 재시도 금지, `idempotency_key`로 상태 조회.
- **종료 코드 (통일)**:

| 코드 | 의미 | 예 (`error.code`로 세분) |
|---|---|---|
| 0 | 성공 | |
| 1 | 내부 오류 | |
| 2 | 사용법·설정·DB 오류 | `E_USAGE`, `E_AMBIGUOUS_CHANNEL`, `E_DB` |
| 3 | 타임아웃 / 매치 없음 | `E_TIMEOUT` |
| 4 | 정책 거부 (하드 차단, R4, deny list, 레이트, 기록·감사 불능) | `E_DENIED`, `E_RATE_LIMIT`, `E_RECORDING_FAILED` |
| 5 | 확인·승인 필요 / 대기 / 거부 / 만료 | `E_CONFIRM_REQUIRED`(R2 토큰), `E_APPROVAL_REQUIRED`, `E_NO_APPROVER` |
| 6 | 사용자·락 충돌 (사용자 입력 중, hold, lease 없음, busy, 사용자에 의해 중단) | `E_USER_ACTIVE`, `E_LOCKED`, `E_INTERRUPTED_BY_USER`, `E_QUARANTINE` |
| 7 | 장치·전송 오류 (열리지 않음, 끊김, bus-off, 전송 timeout) — 쓰기는 `outcome_unknown` 확인 | `E_NOT_OPEN`, `E_DISCONNECTED`, `E_BUS_OFF` |
| 8 | 부분 결과 (gap 포함, 스캔 한도) | `E_CURSOR_GAP`, `E_SCAN_LIMIT` |
| 9 | 데몬 연결 불가 | `E_DAEMON` |
| 10 | 장치의 부정 응답 (UDS NRC 등 프로토콜 수준 거절) | `E_NRC` + `nrc` |

### D6. 안전 — 공통 골격 `loom.core.safety` / `loom.core.policy`
- 위험 등급 의미 (서비스·명령별 매핑 표는 각 도메인 소유):
  - **R0** 관찰 (읽기) — 자동
  - **R1** 가역·저위험 쓰기 (UART 일반 입력, sim 송신, UDS 세션 진입) — 자동 (정책으로 조정)
  - **R2** 상태 변경 (DID 쓰기, DTC 삭제, 리셋, bench 단발 송신) — **확인(confirm)**: 첫 호출이 exit 5 + 토큰을 돌려주고, `loom <domain> confirm <tok>`로 재제출하거나 사용자가 뷰어에서 `Y`. 재제출 동사를 따로 둔 이유는 프로젝트 `.claude/settings.json`의 `Bash(loom * confirm:*)` `ask` 규칙으로 호스트 확인 창을 띄울 수 있기 때문(UDS 안). 경로 정책 `r2_channel = token_or_viewer | viewer` — 기본은 sim/bench `token_or_viewer`, vehicle `viewer`. 도메인은 자기 기본값을 더 엄격하게 둘 수 있다.
  - **R3** 위험 (보안 접근, 액추에이터·IO 제어, 미등록 루틴, 주기 송신·IG·replay, 플래시 계열 콘솔 명령, PSU 출력 ON) — **뷰어 승인만** (scope once/session, 최대 10분). **CLI approve 명령 없음** (에이전트 자기 승인 차단). UART 계획의 "사용자 터미널 `approve` 명령"은 채택하지 않음.
  - **R4** 비가역·고위험 (ECU 위조 송신, 플래시·메모리 지우기, OTP) — **매번 뷰어 승인 (scope=once)**. 도메인은 R4 항목 일부를 `default_blocked`로 지정할 수 있고(UDS 플래시·미지 SID, UART 비가역 명령), 이런 항목은 사용자가 `loom.toml`에서 `unlock`(뷰어 승인 후 적용)해야 R4 승인 대상이 된다.
  - **하드 차단** (deny list: `deny_ids`, allow_ids 밖, E2E 보호 메시지, 활성 ISO-TP tx_id raw 송신 등) — 승인으로 우회 불가, agent·user 모두. `loom.toml` 변경 + 뷰어 승인으로만 바꿈.
- 환경 축 `sim | bench | vehicle` — ~~vehicle은 기본 tx deny~~ → **B4: vehicle도 송신 허용, 기본값은 bench와 같음**.
- 승인 객체: digest(plan hash)·epoch·profile hash·만료·횟수(once/session, 최대 10분)에 바인딩, 1회용.
- deny list(하드 차단)와 인터록 레지스트리 (예: `uds.flashing` 중 PSU 전원 사이클 금지).
- `--dry-run` 공통. `loom stop-all` (전 도메인 TX latch). 크래시 후 자동 재개 금지.
- 정책 파일 2층: 프로젝트 `loom.toml`(도메인 섹션 + `[bench]` 한계·별칭) + 사용자 `~/.loom/config/policy.toml`. **완화는 뷰어 승인 후 적용, 강화는 즉시**, 모든 변경 감사. 에이전트의 파일 수정으로 우회 불가(해시 기록).
- 감사 로그: 해시 체인, 끌 수 없음. **기록·감사 불능이면 새 R1+ 송신 중단**.
- 위협 모델 명시: 목표는 에이전트의 실수 방지와 사용자의 통제권이며, 같은 OS 사용자로 악의적으로 우회하는 에이전트를 막는 보안 경계는 아님.

### D7. 기록
- 모든 세션 자동 기록, 정본 = 도메인 저널(JSONL 청크 + 원시). 표준 포맷(BLF/ASC/MF4, pcapng, txt)은 export.
- `--no-record` 예외 인정(UART 요청): **데이터 페이로드만** 기록 안 함. 감사 로그(누가 무엇을 보냈는지)는 항상 기록.
- 경로: `~/.loom/{run, records, config}`. 기본 보존 30일, 전체 20 GiB quota (감사는 무기한). 비밀값은 기본 마스킹(길이·해시), 원문 보존은 opt-in.

### D8. 데몬 수명주기
- CLI는 데몬이 없으면 **자동 기동** (UART/CAN 안 채택, UDS도 따름). `LOOM_NO_AUTOSTART=1`로 끔. `loom daemon start/stop/status/logs` 공통.
- 데몬은 장치를 자동으로 열지 않는다 (`loom.toml`의 `autostart=true` 채널만).

### D9. 뷰어
- M1: 도메인별 TUI — UART `attach`(터미널 모드는 프레임워크 없이 termios/Win32 원시 입력, 트레이스 모드), CAN `monitor`·UDS `view`는 **Textual**. herdr 패인에서 실행.
- M3: **웹 SPA는 CAN 팀(viz) 소유**, UART/UDS/탐색 도메인은 패널 플러그인(`register_panel`)과 라이브 소스 플러그인(`register_source`)으로 기여.
- 공통 키: 접두키 **`Ctrl-]`** (UART 터미널 모드에서 원시 키와 충돌 방지) — `Ctrl-] x` = STOP, `Ctrl-] q` = 종료. 터미널 모드가 아닌 뷰어는 `Ctrl-X` STOP, `/` 필터, `Space` 일시정지, `q` 종료도 허용.
- 공통 표시 규칙: heartbeat 1 s, snapshot+live 늦은 합류, `LIVE / LAG / STALLED / LIVE-DEGRADED` 배지, 드롭 배너, 뷰어 0개일 때 에이전트 결과에 `no_viewer_attached` 경고.

### D10. 도메인 플러그인 구조 (탐색 CR-X1 채택)
- entry point 그룹 `loom.domains` + Registry (필수: backend, commands, event kinds, capabilities, risk table, sim). 세 기본 도메인도 같은 방식으로 등록.
- **쓰기 파이프라인은 core 소유**: policy → confirm/approval → audit → backend → events. 모든 도메인의 모든 쓰기가 같은 게이트를 지난다.
- UART 콘솔 엔진(버퍼·커서·expect·InputArbiter)과 콘솔 TUI 위젯은 core 재사용 컴포넌트로 분리 (탐색 CR-X5) — SSH/ADB/RTT 콘솔이 재사용. 단 외부에서 장치를 직접 여는 공개 Transport API는 만들지 않는다(데몬 우회 금지).

### D11. CAN ↔ UDS ISO-TP 경계 — 합의안 확정
- `_xteam/can-uds-isotp.md` v0.3 + CAN 의견 + UDS 답변을 **확정**.
- 남은 미결(이중 승인): **UDS 세션 관리자가 연 ISO-TP 채널은 CAN이 별도 R3 승인을 요구하지 않는다** ("UDS 정책 위임"으로 감사만). CAN은 채널 TX 정책·STOP·예약 ID·프레임 레이트만 집행, PDU 의미 판단은 UDS 가드. 에이전트가 `loom can isotp`로 직접 연 채널은 CAN 정책 적용. 승인이 필요하면 공통 승인 서비스의 **한 패널**로.
- 일정: CAN M2(K10, ISO-TP 최소판) = UDS M1b 전제, CAN M4(K14) = UDS M3b 전제.

### D12. 시나리오 러너 (탐색 CR-X6)
- 교차 도메인 러너는 core(`loom run`), 스키마 `loom_scenario: 1`(UDS 안 확장). UDS 러너는 `uds.*` 어댑터로 유지.
- `finally` 단계: 등급 R0–R1 + 도메인 Registry에 `safe_state: true`로 표시된 동작(예: PSU 출력 OFF, CAN 주기 송신 정지)만 허용. abort(사용자 STOP 포함) 시에도 실행. 그 외 단계는 abort 시 skip.

### D13. 스킬 패키징
- 원본: 저장소 `skills/` (공통 `loom` + `loom-uart`, `loom-can`, `loom-uds`, `loom-viz`, 이후 도메인). SKILL.md는 짧게, 상세는 `references/`.
- 배포: `loom skills sync`가 `.claude/skills/`와 Codex `.agents/skills/`로 **복사**(기본), symlink는 선택(Windows 권한 미확인).
- 공통 `loom` 스킬: 데몬, 출력·종료 코드, 승인·안전, 뷰어 띄우기 안내(herdr 패인), `stop-all`, `others_since_last` 대응 규칙.

### D14. 플랫폼
- Python ≥ 3.11, `uv`. 개발 macOS, 실 HW는 Windows x64 우선. CI는 3 OS (macOS/Linux/Windows), 모두 시뮬레이션 백엔드로 통과해야 함.
- HW 우선순위 (사용자 답 전 기본): PCAN-USB FD (Windows) → Vector (M4, CANoe 공존 검증 포함). macOS PCBUSB는 M5.

---

## 2.1 보완 결정 A1–A16 (v1.1, 리드 검토 반영 — D1–D14보다 우선)

| # | 대상 | 결정 | 출처 |
|---|---|---|---|
| A1 | D10 쓰기 경로 | **쓰기 클래스 3종.** ① *요청형*(단발 send, UDS 요청): 전체 파이프라인 policy → confirm/approval → audit → backend → events. ② *잡형*(주기 송신, IG, replay, ISO-TP 채널, 스트리밍 패턴): core는 job의 start/update/renew/stop만 게이트·감사하고, 단위 송신(프레임, CF/FC)은 worker가 승인된 plan 범위 안에서 직접 한다. worker는 단위마다 TX latch·예약 ID·레이트·하드 차단을 다시 집행하고, 단위는 events/저널에만 남긴다. ③ *대화형 입력 스트림*(뷰어의 사용자 키 입력): 입력권(lease)을 얻을 때 1회 정책 판정(R1, 문자열 규칙 미적용), 감사는 줄 단위 또는 1 s 묶음 해시, 저널은 청크 그대로. 입력 중재는 UART worker 안 | UART a2, CAN a1 |
| A2 | D10 도메인 훅 | 파이프라인에 도메인 훅 3개: `classify(bytes, target) → (risk, reason)`, `approval_binding(req) → 추가 digest 필드`, `presend_check(req)`(승인 후·backend 직전, 실패 시 승인 무효 + `E_QUARANTINE`/`E_LOCKED`). 대상별 인플라이트 직렬화와 seed→key 원자 실행은 도메인 backend 책임. 이벤트는 같은 `request_id` 행을 `requested → sent → 응답` 순으로 갱신한 뒤 RPC 응답 | UDS a3 |
| A3 | D6/D9 STOP | **STOP(`loom stop-all`, `Ctrl-] x`) = agent·script의 새 쓰기와 모든 잡을 latch.** 사용자 뷰어 입력은 잠그지 않는다(사용자는 계속 개입 가능). STOP은 **출력을 끄지 않는다** — PSU 출력·릴레이·AWG·halt 상태는 유지되고 뷰어에 "STOP — 출력 유지 중" 배지. 출력 유지 장치는 도메인 패널에 **사용자 비상 OFF 키**(확인 없음, 인터록은 경고만)를 둔다 | UART a3, 탐색 a5 |
| A4 | D10 콘솔 엔진 | 콘솔 엔진(버퍼·커서·expect·InputArbiter)과 콘솔 TUI 위젯은 **UART 팀 소유 공유 패키지 `loom/console/`** 로 처음부터 만든다(안정 인터페이스, SSH/ADB/RTT가 import). CORE는 쓰기 게이트·이벤트 허브만. 이관 작업 없음 | UART a4, 탐색 a7 |
| A5 | D2 배치·메시지 크기 | 배치 40 ms는 **상한**. 조용한 구간 뒤 첫 이벤트는 즉시 flush(적응형), 구독 옵션 `max_batch_ms`(터미널 뷰어 10 ms). 64 KiB 상한은 클라이언트→서버 요청과 `core.events` 배치 1개에 적용. snapshot은 `snapshot_part n/N` → `snapshot_end{stream_seq}`로 분할. viz 대용량 질의와 스코프 원본은 같은 오리진 HTTP GET(토큰·쿠키 검사) 또는 `raw_ref` | UART a5, CAN a3, 탐색 c4 |
| A6 | D5 대기 상한·종료 코드 | 600 s 상한은 스트리밍(`--follow`)에만. 단일 결과 대기(`expect`, `wait-for`)는 최대 3600 s. **종료 코드 3 = 조건 대기형 명령의 timeout만**(`expect`, `wait-for`, 승인 대기). **장치 요청 무응답(UDS P2/P2* 초과 등) = 7 `E_NO_RESPONSE`**, 쓰기는 `outcome_unknown` | UART a6, UDS a4 |
| A7 | D5 커서·출력 | 도메인 확장 커서 허용(UART `off:N`, `send:last`) — 공통 문법과 겹치지 않는 접두어만. UART 기본 출력 100행은 메인 결정(원안 200줄/16 KiB) | UART b1 |
| A8 | D6 R2 경로 | R2 경로 정책 `auto` / `token_or_viewer` / `viewer`(동작 단위). 기본: **sim = auto**, bench = 도메인 기본(UART·UDS·탐색 `token_or_viewer`, **CAN 실HW 채널 `viewer`** — 사람이 `loom.toml`에 `tx="allow"`를 둔 채널만 `token_or_viewer`), ~~vehicle = `viewer`(auto 불가)~~ (B4: vehicle = bench와 동일). bench에서 사용자가 동작 단위로 `auto` 완화 가능(완화는 뷰어 승인 후 적용). 주의: 토큰 재제출은 사람 동의의 증거가 아니다. Claude Code에서는 `Bash(loom * confirm:*)` `ask` 규칙을 함께 설치하고, 이 규칙이 없는 환경(Codex 등)에서 실HW는 `viewer`를 권장 | CAN a4, 탐색 a2 |
| A9 | D6 R3 기한 | 10분 상한 = **승인 객체 사용 기한**(그 안에 job을 시작해야 함). 시작된 job은 자기 plan(ID·신호 범위·주기)과 lease 안에서 계속 실행. renew는 `loom.toml max_job_duration`(기본 1 h) 안에서 재승인 없이 허용, 넘으면 새 승인 | CAN a2 |
| A10 | D6 vehicle | **(B4로 폐기)** vehicle의 기본 tx deny는 raw 송신·주기·IG·replay·PSU/IO/debug 쓰기에 적용. **UDS는 화이트리스트 대상에 대한 R0 진단 요청(0x22, 0x19 등)만 기본 허용**, R1+는 `unlock` 전까지 거부 | UDS a2 |
| A11 | D6 차단 계층 | 차단 3층: `default_blocked`(R4 중 기본 차단, `loom.toml unlock` + 뷰어 승인 후 R4 승인 대상) / **하드 차단** deny list(승인으로 우회 불가, 설정 변경 + 뷰어 승인으로만) / **`immutable_deny`**(도메인 선언, 설정으로도 해제 불가 — debug RDP·보안 잠금·옵션 바이트·mass erase, UDS 0x2A/0x86 같은 미구현 서비스 등. 필요하면 사용자가 벤더 도구 사용) | 탐색 a8, UDS |
| A12 | D7 기록 불능 | 기록·감사 불능 시 **agent·script의 새 R1+ 쓰기만 중단**(sim 포함). 사용자 뷰어 입력은 허용 + `record.health=failed` 배너, 그 구간 감사는 데몬 로그(stderr)에 남김 | UART a1 |
| A13 | D3 손실 이벤트 | 손실 3층: `display.gap`(화면 드롭) · `record.gap`(Loom 기록 손실) · **`source.gap`**(Loom 수신 전 손실 — 장치·전송: DLT MCNT 불연속, RTT 드롭, sigrok 오버런, SCPI 응답 누락), 필드 `cause` | 탐색 a3 |
| A14 | D4 쓰기 상태 | `requested → accepted → sent → confirmed → effective_verified` 또는 `effect_unverified`, 그리고 어느 단계에서든 `failed` / `outcome_unknown`. **`sent` = 드라이버·소켓에 넘김(wire 완료 아님).** 전송 계층 세부는 별도 필드 — ISO-TP는 `tx_state: driver_accepted / bus_confirmed`(xteam 합의값 `accepted/confirmed`에서 이름 변경, D4와 혼동 방지). UART 성공 종단은 `sent`(장치 ACK 없음), `send --expect` 매치는 별도 결과 | UDS a5, 탐색 a4, CAN c5, UART c3 |
| A15 | D12 finally | 정상 종료·on_error·에이전트 cancel → `finally` 실행(R0–R1 + `safe_state` 동작). **사용자 STOP / stop-all이면 finally를 자동 실행하지 않는다.** 대신 뷰어에 safe_state 동작을 **"복구 송신" 1키**(user origin, latch 예외는 safe_state 항목만)로 제시 — UDS `uds.recover` 패턴을 core 공통으로 승격. 어떤 경우든 safe_state 동작도 **인터록·deny list를 반드시 통과**하고, 막히면 skip + `finally_blocked` 기록(예: `uds.flashing` 중 PSU OFF 금지). finally 승인은 run 시작 때 일괄 승인에 포함 | UDS a1, 탐색 a1 |
| A16 | D11 ISO-TP 이름 | RPC는 `can.isotp.*`, 이벤트는 `can.isotp_rx_start` 형식(정확한 표기는 CORE 계약 v1). `_xteam/can-uds-isotp.md`도 함께 갱신 | CAN c4 |

## 2.2 사용자 답변 반영 B1–B7 (v1.2, 2026-11-10 — A·D보다 우선)

사용자 답변 원문: "윈도우와 리눅스 타겟. 케노이 peak can usb시리얼 진단이야 케노이로 다 되니 이더넷이든 캔이든. 벤치와 실차 다 사용하는데 굳이 송신을 막을 이유는 없지. 살레 로직에널라이저가 있긴함"

| # | 대상 | 결정 |
|---|---|---|
| B1 | D14 플랫폼 | **실 HW 호스트 = Windows x64와 Linux x64 둘 다** (데몬+에이전트가 장비 PC에서 실행). macOS는 개발·시뮬레이션만. 실HW 체크리스트·수용 시험은 두 OS 모두. Linux: PCAN은 커널 `peak_usb` → python-can `socketcan`(`ip link set can0 type can bitrate …`은 사람이 미리 설정 또는 권한 있는 헬퍼), 시리얼은 `/dev/ttyUSB*`·`ttyACM*`(dialout 그룹, udev 시리얼 번호 별칭 권장). **Vector XL 드라이버·CANoe는 Windows 전용** — Linux에서 Vector 경로 없음 |
| B2 | 보유 장비 (U2) | Vector 하드웨어 + CANoe(모델·버전 미확인), PEAK PCAN-USB(FD 여부 미확인), USB-시리얼, **Saleae 로직 애널라이저**(모델 미확인). PSU·스코프·디버그 프로브는 없음 |
| B3 | CANoe 연동 (U5, U7) | 사용자는 **진단을 CANoe로 CAN·이더넷 모두 수행**한다. 따라서 **CANoe 브리지를 M5 대안에서 P2 정식 산출물로 승격**. Windows 전용 패키지 `loom/canoe/`(COM 자동화, 전용 스레드·STA, `py_canoe`는 참고만 하고 얇은 자체 래퍼 권장). 용도: ① **UDS 백엔드 `canoe`** — CANoe에 이미 설정된 진단 기술(CDD/ODX)·seed-key DLL·DoIP/CAN-TP 설정을 그대로 써서 요청 송신·응답 수신, 요청·응답은 Loom 이벤트로 발행(사용자는 Loom 뷰어와 CANoe 트레이스 양쪽에서 봄), 위험 분류·승인은 Loom 파이프라인을 그대로 통과(SID 바이트 기준). ② CAN — 측정 상태, 신호·시스템 변수 읽기/쓰기, 측정 시작/정지. ③ **채널 공존** — CANoe 측정 중 같은 Vector 채널을 python-can `vector`로 함께 열기(init access 없음 → 비트레이트 변경 불가, 수신·송신 가능 여부 실측). CANoe UI에서 사람이 보낸 요청은 Loom이 버스에서 관찰한 경우에만 보이며, 출처 태그 `external:canoe`로 표시. **자체 스택(DoIP·CAN-TP over PCAN/SocketCAN)은 그대로 필수** — Linux와 CANoe 없는 환경의 유일한 경로. 소유: 브리지 코어(COM 세션·측정 상태·재연결)는 **CAN 팀**, UDS `canoe` 백엔드는 **UDS 팀** |
| B4 | D6/A10 vehicle (U3) | **벤치와 실차 모두 사용, 실차 송신을 기본 차단하지 않는다.** vehicle의 위험 등급·R2 경로·기본값은 bench와 같다(A10의 vehicle tx deny 폐기, R4 예시에서 "vehicle 송신" 삭제, A8의 "vehicle = viewer, auto 불가" 폐기 → bench와 동일). 남는 것: 모든 뷰어와 CLI 결과 봉투에 환경 배지 `env=vehicle` 상시 표시, R3/R4 뷰어 승인(보안 접근·플래시 등)은 환경과 무관하게 유지, 하드 차단 목록은 사용자가 설정한 것만(기본 비어 있음, 도메인 `immutable_deny`만 고정) |
| B5 | 탐색 LA·계측 (U10) | 계측기 추가 구매 없음. **LA 백엔드 1순위 = Saleae Logic 2 자동화 API**(`logic2-automation`, gRPC, Logic 2 앱 실행 또는 headless 기동, 시뮬레이션 장치 `F4241` 제공). 제약을 계획에 정직하게 쓸 것: **캡처 중 데이터 스트리밍 불가** → 캡처(시간·트리거) 종료 후 raw/분석기 표를 내보내 Loom 타임라인·viz로 표시. 캡처 중 실시간 화면은 Logic 2 앱 자체가 담당, Loom은 캡처 상태·진행만 실시간. sigrok은 2순위(신형 Saleae Logic 8/Pro 지원 여부 확인 필요). 순위 변경: **LA(Saleae)를 P2로 앞당김**(실장비 보유). PSU는 SimPsu로 플러그인 계약 검증용 첫 도메인 역할만 유지, 실HW는 구매 시. 스코프는 보류 |
| B6 | UART | Linux 실 COM 체크리스트 추가(권한, udev 별칭, USB 재열거 시 재연결, `ttyACM` DTR 리셋 주의). Windows 콘솔 원시 입력 관문은 그대로 |
| B7 | 남은 질문 | 기본값으로 진행하되 답을 받으면 반영: CANoe 버전·Vector 하드웨어 모델(VN16xx/VN56xx, 이더넷 인터페이스 여부) · PCAN 모델(FD 여부) · Saleae 모델 · Windows에서 진단 기본 경로(기본: CANoe 측정이 켜져 있으면 `canoe` 백엔드, 아니면 자체 스택) · U6 보드/콘솔 · U9 DLT |

## 2.3 v2.0 방향 전환 C1–C8 (2026-11-10, 사용자 지시 — B·A·D보다 우선)

사용자 지시 원문: "vn5640 이건 근데 pyvxl쓰면 다 됨. 어차피 라이센스는 내가 가지고 있으니까 상관없고 peak can은 can fd. 그리고 코어는 러스트로 하자.. 파이썬을 코어로 하니까 느리고 메모리 문제가 자꾸 죽음... 그리고 이게 스킬 기반이지만 여튼 UI도 있어야 하니 고려. 그리고 각 모듈을 혼합하여 사용하는 TC기반 혹은 자연어 기반 통합 테스트도 가능해야함. 그래서 스킬로 빼는것임.에이전트가 붙어있으면 TC를 위한 코딩을 안해도 되기 때문" / "어쩔수 없을때에만 파이썬 특히 케노이"

| # | 대상 | 결정 |
|---|---|---|
| C1 | 언어 (D14·D10 대체) | **코어와 장치 경로는 Rust.** `loomd`(gateway·IPC·이벤트 허브·기록/저널·정책/승인/감사·worker 감독·러너 코어)와 `loom` CLI(단일 바이너리, 에이전트가 자주 부르므로 기동 속도 중요)를 Rust로 만든다. 장치 백엔드도 기본 Rust: UART, CAN(SocketCAN·PCAN·Vector XL), ISO-TP, DoIP/UDS, DLT, SSH. **Python은 대안이 없을 때만** — 대표 예외는 **CANoe COM**. Python 어댑터는 반드시 별도 프로세스(같은 worker 프로토콜), 메모리 상한, 죽어도 데몬은 살아 있고 감독자가 재기동. Python 어댑터를 쓰려면 결정 기록에 "왜 Rust로 안 되는가" 행 필수 (예: Saleae는 공개 gRPC proto가 있으므로 Rust `tonic` 가능성을 먼저 검토) |
| C2 | 성능·메모리 (사용자 불만의 원인) | 모든 큐는 상한과 넘칠 때 정책(drop·block·spill)을 명시. 링 버퍼 고정 크기, 원본은 디스크 저널로 흘려 보냄(메모리에 누적 금지). worker별 메모리 예산과 초과 시 동작. **24시간 soak 시험**(CAN 고부하 + UART flood + UDS 폴링 동시)에서 RSS 증가 상한을 관문으로. worker 크래시가 데몬·다른 worker·저널을 죽이지 않는다. 벤더 DLL(vxlapi, PCANBasic)은 크래시 격리를 위해 worker 프로세스 안에서만 로드 |
| C3 | 계약 유지 | D2(JSON-RPC 2.0 over WS, 역할·origin), D3(이벤트 레코드), D4/A14(쓰기 상태), D5(CLI 봉투·종료 코드·커서), D6(R0–R4·승인·감사), D7(기록), A1–A16, B1·B4·B6은 **언어와 무관하므로 유지**. 스키마 정본 = Rust 타입(serde) → JSON Schema 내보내기 → UI용 TS 타입·Python 어댑터 타입 생성. D10(Python entry point 플러그인)은 폐기 → Rust 도메인 crate 등록 + 외부 어댑터 worker 프로토콜(CORE 결정) |
| C4 | Vector (B2·B3 수정, **C4.1로 확정**) | **VN5640은 CAN FD 전용으로 직접 접근**(라이선스 보유). Rust에서 `vxlapi64.dll` FFI 자체 바인딩(공개 Rust crate 없음, Windows 전용, **CAN/CAN FD API만**). pyvxl(CAN 전용)은 참고·교차 검증용. **C4.1 (사용자 확인): DoIP는 PC 랜카드 + 미디어 컨버터로 이미 사용 중 → OS TCP/IP 소켓 위 DoIP(경로 c). Vector XL 이더넷 API는 쓰지 않는다**(사용자: pyvxl은 CAN만, Vector 이더넷은 지원 안 하는 것으로 앎). 경로 (a) OS 어댑터 노출·(b) XL 원시 프레임 + `smoltcp`는 범위 밖. DoIP 실HW 체크리스트: NIC 선택·정적 IP·VLAN·방화벽(UDP 13400 발견, TCP 13400), 컨버터 설정은 사람의 사전 작업, Windows·Linux 둘 다. 소유: Vector XL 바인딩 crate = **CAN 팀**, DoIP 전송 = **UDS 팀**. **CANoe 브리지는 진단 필수 경로에서 내려 선택 기능(P4, Python 어댑터)** — 진단 기본 경로는 Loom 자체 스택(DoIP over NIC, CAN-TP over VN5640·PCAN FD). CANoe와 같은 Vector 채널 공존 실측은 유지 |
| C5 | PEAK | **PCAN-USB FD → CAN FD를 M2 첫 실HW부터.** Windows = PEAK 공식 Rust FFI(PCAN-Basic 5.1+), Linux = SocketCAN(FD) |
| C6 | UI (D9 대체) | **사용자 UI는 1급 제품.** 스킬은 에이전트 쪽 인터페이스이고, 사용자는 **Loom UI 앱**에서 모든 장치를 실시간으로 보고 개입한다. 기본안: `loomd`가 127.0.0.1로 제공하는 웹 UI(같은 WS 프로토콜) + 이후 선택적 데스크톱 셸(Tauri 등). 공통 셸(작업 공간 레이아웃·STOP·승인 대기열·env 배지·연결/gap 상태·기록 탐색/재생·테스트 실행 화면) + 도메인 패널(UART 터미널, CAN Trace/Graphics/IG/신호, UDS 콘솔, LA 레인, DLT) + `loom.viz`(데이터만 주면 렌더링). **M1 뷰어부터 웹 UI**(최소 패널), TUI는 터미널 사용자용 보조. 새 **UI 팀**이 셸·패널 프레임워크·viz를 소유하고(CAN 팀의 웹/viz 소유 이관), 도메인 팀은 패널 내용 명세와 데이터 질의를 소유 |
| C7 | 통합 테스트 (신규) | **여러 모듈을 섞는 TC 기반·자연어 기반 통합 테스트**를 지원한다. 핵심: 에이전트가 스킬로 붙어 있으므로 **TC마다 코드를 짜지 않는다** — 사람이 자연어(또는 반구조 TC 표)로 쓰고 에이전트가 `loom` 명령으로 실행. Loom이 제공할 것: TC 형식(전제·장비·환경·단계·기대 결과), 테스트 세션(`loom test …`: 시작·단계 표시·판정·종료), **증거 기반 판정**(각 단계 판정은 이벤트 seq 범위 등 증거를 인용해야 하고 Loom이 인용 증거의 존재·일치를 검증), 보고서(HTML + JUnit XML 등), 세션 시작 시 일괄 사전 승인, UI 테스트 실행 화면, 에이전트 실행 기록을 결정적 시나리오(`loom run`)로 고정해 에이전트 없이 회귀 재실행(선택). 새 **TEST 기획**은 EXPLORE 팀(w5)이 맡는다(교차 러너 설계 경험) |
| C8 | 스킬 | 스킬이 통합 테스트를 가능하게 하는 수단이다: 도메인 스킬 + 공통 `loom` 스킬 + **`loom-test` 스킬**(TC 읽기·실행·증거 인용·보고 규칙). 스킬 본문은 짧게, 상세는 `loom <domain> help --json`이 정본 |

v2.0 팀 구성: **CORE(w6, 신규)** Rust 코어·CLI·worker 프로토콜·스키마·기록·정책 · **UI(w7, 신규)** UI 앱 · UART/CAN/UDS(w2–w4) Rust 이식·HW 변경 반영 · **EXPLORE(w5) → TEST 기획 + 탐색 계획 Rust 이식**. 순서·관문은 각 팀 v2 계획 뒤 메인이 마스터 v2.1에서 다시 묶는다(§3 단계 표는 v1.2 기준 — v2.1에서 교체).

## 2.4 v2.1 메인 판정 E1–E12 (2026-11-10 21:40 — 팀 v2.0 계획의 계약 요청 처리, C 이하 모두보다 우선)

| # | 요청 출처 | 판정 |
|---|---|---|
| E1 | CORE CR-C1–C11, C13 | **전부 채택.** viewer 자격 = 웹 bootstrap(M1), 우선 큐 "드롭 금지 = 상한 초과 시 연결 종료 + 재snapshot, STOP 1칸 보존", 커서 `now`, 오류 코드 목록(CR-C4), `epoch`/`generation` 분리, 이벤트 필드(`env`·`external_actor`·`original_origin`·`test_session`·`test_step`, **payload = kind별 구조 객체**), 예약 domain 8개, 도메인 등록 = Rust `DomainModule` + 외부 worker manifest(dylib 없음), 공통 TUI 키트 삭제, `input.batch` 감사 형식, worker→gateway 적응형 배치, `auto_reconnect` + `generation+1` + `data.link` |
| E2 | CORE CR-C12 | **채택. 종료 코드 3 = 조건 판정 불충족**(대기 timeout + 즉시 판정 불일치 `E_NO_MATCH`·`E_UNEXPECTED_MATCH`·`E_COUNT_OUT_OF_RANGE`). 증거 부족 = 8 `inconclusive`, 장치 오류 7·NRC 10 유지. A6 대체 |
| E3 | CORE CR-C17, TEST | **채택. 종료 코드 11 `E_TEST_FAILED`**(`test end`·`run --test` 판정 failed), 6에 `E_SESSION_PAUSED`. 테스트 종료 코드: 0 passed / 11 failed / 8 inconclusive / 6 STOP |
| E4 | CORE CR-C18, TEST T6 | **채택. 테스트·run 묶음 승인**: 시작 ≤ 10분, 이후 세션/run 종료·`max_test_duration`(기본 1 h, 설정 ≤ 8 h)·STOP·정책/epoch 변경 중 먼저 오는 때까지 유효. 매 송신 `within_pattern` 재결속 + 원자 예약. R4는 묶음 승인 제외(A9 확장) |
| E5 | CORE CR-C14 | **채택.** 저널 정본 = 길이 + CRC32C 프레임 + codec id 헤더. M1 codec JSON, M2부터 MessagePack 허용(실측 후). JSONL은 export |
| E6 | CORE CR-C15 | **채택.** 감사 로그는 원본 quota와 별도 계정·무기한, 감사 디스크 부족 시 agent·script 쓰기 차단. 보존은 "30일 또는 20 GiB 중 먼저 도달" + **테스트 증거로 인용된 구간 자동 보호**. 고부하에서 30일 무손실은 불가함을 문서·UI에 명시 |
| E7 | CORE CR-C16 | **채택.** publish-before-reply = 허브가 seq 부여 + 모든 viewer 우선 큐 enqueue 완료 후 응답(렌더 보장 아님, 렌더 지연은 `core.metrics.report`). 미발행이면 exit 8 `E_PARTIAL{published:false}` |
| E8 | UI R-1, R-4, R-5, R-6, R-7 | **채택.** 웹 STOP = 버튼 + `Ctrl-] x`(`Ctrl-X` 미채택) · 출처 색 전 팀 통일(AGT 청록, USR 노랑, DEV 회색, SCR 보라, EXT 보라 점선) · CAN `monitor`·UDS `view` Textual TUI 폐기, UART `attach` TUI만 유지 · 쓰기 이벤트는 `request_id` + `payload.write_state` 필수 · 웹 오리진 정본 `http://localhost:<port>`, 127.0.0.1은 리디렉트 |
| E9 | UI R-2, R-3, CORE Q6 | **부분 채택.** viewer rights 3단(watch/operate/approve) 채택. **존재 확인(WebAuthn·대역 외 코드)은 R4에만 기본**, R3는 뷰어 클릭 승인(정책으로 R3에도 켤 수 있음), sim은 없음. 근거: 사용자는 승인 부담을 싫어함(B4) + 에이전트가 브라우저 자동화로 클릭을 흉내 낼 위험은 R4에서 가장 큼. WebAuthn 등록 게이트는 R4 사용 전 1회 |
| E10 | CAN 정정 | 마스터 C4의 "공개 Rust crate 없음"은 **틀림** — `vxlapi-sys` 2.1.0이 있다. 그래도 CAN 자체 XL crate 유지(shared FD `permissionMask=0`, 이벤트 보존, 시계 리셋 금지 — CAN D63) |
| E11 | CAN D71 | **CANoe도 Rust COM 스파이크를 먼저**(P4), 안 될 때만 Python 어댑터. C1의 "Python은 대안이 없을 때만"을 그대로 적용 |
| E12 | 계약 문서 | E1–E11과 C·B·A·D를 합친 **계약 v1**을 P0 첫날 `docs/contract/`로 분리(CORE `loom-proto` 태그 `core-p0-d2`와 동기) |

## 2.5 v2.2 사용자 결정 F1 (2026-11-10 22:10 — 최우선)

사용자 원문: "모두 자동. 그게 벤치니까 설정이나 위험한 부분은 테스트하는 사람이 인지하고 잘 설정해야지."

| # | 대상 | 결정 |
|---|---|---|
| F1 | D6·A8·A9·A11·B4·E4·E9 안전 기본값 | **기본 정책 = R0–R4 모두 자동 실행**(sim·bench, B4에 따라 vehicle도 같음). 확인 토큰·뷰어 승인·존재 확인(WebAuthn)은 **기본 꺼짐**. 책임 모델: 위험한 부분은 테스트하는 사람이 알고 `loom.toml`로 설정한다. **유지하는 것**: 위험 등급 분류 자체(화면·기록에 등급 표시), 모든 쓰기의 실시간 화면 표시와 감사 기록, 사용자 STOP·비상 OFF·복구 송신, env 배지, 인터록(예: 플래시 중 PSU OFF 금지 — 설정으로 해제 가능). **설정으로 켤 수 있는 것**: 동작·대상별 `confirm`/`approve`/`deny`, R4 존재 확인, 하드 차단 목록. `immutable_deny`는 **Loom이 구현하지 않은 서비스**(예: UDS 0x2A/0x86)로만 축소 — 장치 잠금·mass erase·옵션 바이트 등은 기본 허용, 막고 싶으면 사용자가 설정. 범위 영향: UI M2의 R3/R4 승인 화면·WebAuthn은 **설정으로 켰을 때만 필요한 선택 기능**으로 낮춤. 테스트 묶음 승인(E4)도 기본 불필요. Playwright 자동화 브라우저 watch 권한 제한(V5)은 승인을 켠 경우의 안전장치로 유지 |
| F2 | 범위·단계 (§3.0) | **이더넷 패킷 도메인 `eth`를 1차(P1 M1)에 포함.** 사용자 원문: "이더넷까지 1차에 포함. UI는 와이어샤크나 tshark 사용하는 것처럼 에이전트가 실시간으로 패킷을 볼 수 있어야 함." 에이전트 = tshark식 CLI(실시간 trace·필터·디코드·대기·송신), 사용자 = Wireshark식 UI 패널(목록·상세·hex·필터·스트림), DoIP·SOME/IP 디코드, UDS `request_id` ↔ DoIP 패킷 링크. 계획: `docs/plan/eth/` (EXPLORE 팀) |

---

## 3. 구현 단계와 팀 구성

### 3.0 v2.1 단계 (팀 v2.0 계획 통합 — 아래 3.2 표는 v1.2 이력)

| 단계 | CORE | UI | UART | CAN | UDS | EXPLORE / TEST |
|---|---|---|---|---|---|---|
| **P0 계약·뼈대** (약 1주) | Day 1–2 `loom-proto` + compile-only trait(`core-p0-d2`), Day 3–5 testkit(FakeWorker·RegistryProbe·HookHarness)·echo·스키마→TS·`loomd --fixture-replay`(`core-p0-d5`) | 셸 골격을 `--fixture-replay`·WS stub 위에서, 패널 템플릿 v0.2 | `loom-console` crate, `serialport` native 어댑터, pty 가짜 장치 | `can-sim` worker, ISO-TP 상태기계, DBC codec | 서비스 코덱, tokio DoIP, 가상 ECU | 증거 검증기 술어 AST, `.tc.md` 파서, DLT codec·Saleae tonic 스파이크 |
| **P1 걷는 뼈대 M1 (sim)** | gateway·허브·저널(JSON codec)·정책 v0·CLI | 셸(STOP·복구 송신·Activity·R2 확인 카드) + UART 터미널 + CAN Trace + UDS 콘솔 | M1: 웹 터미널 E2E | M1: sim + 웹 Trace(paint p95 < 150 ms) | M1: 웹 E2E-1(Playwright DOM 확인) | TEST M1: TC-1 sim E2E(UI M1b `shell.test`), PSU sim |
| **P1 관문** | **사용자 시연(웹 UI)**: 세 도메인 각각 "에이전트 송신 → 장치 응답 → 사용자 화면 → 사용자 개입" + TC-1(혼합 TC)을 에이전트가 자연어로 실행·증거 판정 + 30분 연속 RSS 증가 ≤ 10 MiB |||||
| **P2 첫 실HW** | 승인 R3/R4(E9), 정책 2층, 러너·테스트 세션 완성 | 도킹, R3/R4 승인 화면, Graphics/IG, 테스트 화면 완성 | M2: 실 COM(Win·Linux), Windows 콘솔 | **M2a PCAN-USB FD(Win·Linux) + 24 h soak**, M2b VN5640 CAN FD + CANoe 공존 실측 | M2: CAN-TP over PCAN FD·VN5640, **DoIP over NIC + 미디어 컨버터(C4.1)**, 보안 접근(seed-key helper) | DLT, **LA(Saleae, tonic)** |
| **P3** | MessagePack codec(E5), 보존·증거 보호(E6) | `loom.viz`(데이터만 주면 렌더링) | M3 | M3 | M3 | SSH, 회귀 재실행(`loom run` 고정) |
| **P4** | — | Tauri(선택) | M4 | CANoe 브리지(Rust COM 먼저, E11) | `canoe` 백엔드(선택) | Debug(probe-rs, 프로브 구매 시) |

관문 공통: "로그만 있고 사용자 화면이 없음", "사용자 개입 경로 없음", "증거 없는 PASS"는 불합격. soak은 개발 중 10분 이내, 정식 24 h는 M2a 기준 머신에서.

### 3.1 핵심 판단 — `loom.core`를 별도 팀이 먼저 만든다
세 팀 모두 계획 첫 단계에 core 작업(스키마, gateway, IPC, 이벤트 허브, CLI 프레임워크, 정책·감사 v0)을 넣었다 (UART T01–T02, CAN C0–C4, UDS U-00). 각자 만들면 세 벌이 생기므로 **CORE 팀을 새로 두고** core를 맡긴다. 도메인 팀은 그동안 core에 의존하지 않는 내부 부품을 만든다.

### 3.2 단계 (v1.2 이력 — §3.0이 대체)

| 단계 | CORE 팀 | UART 팀 | CAN 팀 | UDS 팀 | EXPLORE 팀 |
|---|---|---|---|---|---|
| **P0 계약·부품** | §2 결정을 `loom.core.schema` + 계약 문서로 고정, 스키마 fixture와 **인터페이스 stub 먼저 배포** → gateway(WS JSON-RPC·발견·role·자동 기동·worker 감독), 이벤트 허브(seq·snapshot·우선 큐·gap), CLI 프레임워크(봉투·종료 코드·커서), 정책·확인·승인·감사 v0, 플러그인 Registry, 기록기 기반, 3-OS CI | T03 Transport, T04 pty 가짜 장치, T05 RingBuffer/Recorder, T06 텍스트 파이프라인, T07 escape sanitizer | S0 측정 스파이크, K1 CAN worker(can-sim, CORE stub 기반), K4 SimNode | U-01 골격, U-02 가드 분류기, U-03 진단 DB, U-04 DoIP 전송, U-05 가상 ECU, U-06 트랜잭션 러너 | X0 플러그인 계약 검토(→CORE), DLT M0 gate 스파이크(dlt-daemon 3.0.1), SimPsu |
| **P1 걷는 뼈대 (M1)** | core 안정화, 공통 TUI 키트, 지연 측정 하네스 공통화 | M1 (T08–T13) | M1 (K2, K3, T1, Q1, D0) | M1 (U-07–U-12, DoIP) | PSU M1 (첫 플러그인 도메인) |
| **P1 관문** | **사용자 시연**: herdr에서 세 도메인 각각 "에이전트 송신 → 장치 응답 → 사용자 화면 → 사용자 개입" + 수용 기준 자동 시험 green | | | | |
| **P2** | 공통 승인 패널, 정책 파일 2층, 러너 core(잡 소유·스키마·승인/인터록 연결·run 이벤트) | M2 (재연결, 시그널, 승인, Windows 콘솔) | M2 (DBC, 주기 송신, 승인, **첫 실HW PCAN — Windows·Linux SocketCAN**, ISO-TP 최소판, **CANoe 브리지 v0(B3)**) | M2 (보안접근, lease, R3) ∥ M1b (CAN-TP — CAN K10 직후, M2와 병렬) ∥ **`canoe` 백엔드(B3, 브리지 v0 직후)**. 실 DoIP 장비가 있으면 M2 HIL 스모크 | DLT M1, **LA(Saleae, B5)** |
| **P3** | — | M3 (로그 모드, 실 COM 체크리스트) | M3 (**웹 + loom.viz**, UART/UDS 패널) | M3 (routine/io, 시나리오, 폴링→viz) | 교차 러너 M1(단계 어댑터·SimWorld·run 패널·수용 시험), SSH(`loom/console` 재사용) |
| **P4** | — | M4 (웹 뷰 등) | M4 (Vector 채널 공존 실측은 **P3로 앞당김**(B3), FD, ISO-TP 완성) | M3b (CAN-TP 전체, CAN M4 후) · M4 (CAN-TP 실HW만 CAN M4 의존, DoIP 실HW는 독립) | Debug(장비 구매 시), 스코프(보류) |

임계 경로: CORE P0 → 세 도메인 M1 → CAN M2(ISO-TP 최소판 K10) → UDS M1b → CAN M3(웹·viz) → CAN M4(K14) → UDS M3b. 작업 번호는 CAN 계획 기준(UDS 문서의 K9a/K9b = K10/K14).

작업 경계: CORE = gateway·이벤트 허브(seq·snapshot·우선 큐·gap)·CLI 프레임워크·정책/승인/감사·Registry·기록기 기반. **CAN 팀 = CAN gateway 측 모듈**(조회용 링 미러의 CAN 질의, Fixed 테이블, DBC 디코더/SignalCache, viz 데이터셋). **UART 팀 = `loom/console/`** 공유 패키지(A4). Registry는 worker뿐 아니라 gateway에 올라가는 도메인 모듈도 지원.

### 3.3 팀 구성 (구현 단계)
- 기존 4개 워크스페이스(UART w2, CAN w3, UDS w4, EXPLORE w5)를 그대로 쓰고, **CORE 워크스페이스(w6)** 를 추가한다. 구성은 동일: 리드 Opus 5.5 + 워커 Opus 5.5 + 워커 Codex Sol 6.1.
- 저장소는 git 초기화 후 **팀별 git worktree + 브랜치**(`core/*`, `uart/*` …)로 충돌을 피한다. `loom/core/`는 CORE 팀만 수정, 다른 팀은 변경 요청.
- 메인(w1)은 계약 문서 소유, 관문 판정, 머지 순서 조정.
- 각 마일스톤 관문에서 "로그만 남고 사용자 화면이 없음" 또는 "사용자 개입 경로 없음"이면 불합격 (02 원칙).

---

## 4. 사용자 질문 (통합 — 답이 없으면 기본값으로 진행)

네 팀 질문 약 50개를 결정에 영향이 큰 순서로 묶었다. 상세는 각 팀 §14.

| # | 질문 | 기본값 | 영향 |
|---|---|---|---|
| U1 ✅B1 | 실 HW가 붙는 PC는? (Windows 벤치 PC / macOS 직접) 에이전트를 그 PC에서 돌릴 수 있나? | Windows x64에서 데몬+에이전트 실행, macOS는 개발·시뮬레이션 | 원격 브리지 필요 여부, CI |
| U2 ✅B2 | 보유 장비 목록: CAN 인터페이스(PCAN/Vector 모델, FD), USB-시리얼 어댑터, DoIP 대상, PSU·릴레이, 스코프·LA, 디버그 프로브 | PCAN-USB FD + Vector 1대, FTDI/CP210x, 벤치 DoIP, 나머지 없음 | 첫 실HW 대상과 순서 |
| U3 ✅B4 | 실차에 붙여 쓰는가, 벤치만인가? | 벤치만. vehicle 환경은 송신 기본 차단 | 정책 기본값 |
| U4 | R2(쓰기·DTC 삭제·리셋·버스 송신) 확인을 에이전트 토큰 재제출로 충분히 볼지, 항상 뷰어 승인으로 할지? | sim/bench 토큰 허용, vehicle은 뷰어 | 승인 빈도 |
| U5 ✅B3 | CANoe를 켠 채 같은 채널로 Loom을 쓸 일이 많은가? CANoe 버전은? | 많다 → M4 공존 필수 | CAN M4 우선순위 |
| U6 | 주로 쓰는 보드/콘솔 (임베디드 리눅스, U-Boot, ESP32, STM32 …) | generic·linux-shell·uboot·esp32 프로필 | UART 프로필 |
| U7 | 진단 DB 원천(ODX/PDX, CDD, 없음)과 seed-key 형태(Python / Windows DLL) | Loom YAML 수작성, Python seed-key | UDS M2/M4 |
| U8 | 플래시(0x34–0x37)가 언젠가 필요한가? | 비범위, M5에서 OEM 프로필로 별도 계획 | UDS M5 |
| U9 | DLT 대상(verbose, TCP 3490, DLT Viewer 동시 사용)과 XCP 업무 여부 | verbose/TCP/동시 사용, XCP 없음 | 탐색 순서 |
| U10 ✅B5 | 저가 계측기 구매 의향: 세트 A(fx2 LA + Hantek 6022), B(LAN SCPI 스코프 + fx2 LA, 기본), C(Analog Discovery 3) | B | 탐색 E3/E6 |
| U11 | 실시간 목표(UART 100 ms, CAN 200 ms, UDS 100 ms)가 충분한가? | 그대로 | 성능 투자 |
| U12 | 기록 보존·용량, 비밀번호·VIN 원문 저장 허용 | 30일 / 20 GiB, 비밀번호 마스킹, VIN 로컬 원문 | 기록 정책 |


### 4.1 v2 사용자 질문 (CORE·UI 계획에서, 답 없으면 기본값)

| # | 질문 | 기본값 |
|---|---|---|
| V1 | 다른 PC에서 벤치 PC의 Loom UI를 자주 보나? | 가끔 → SSH 포트 포워딩 안내, 원격 인증은 M5 |
| V2 | R4 승인 때 쓸 존재 확인 수단: Touch ID / Windows Hello / 보안 키 / 없음(확인 코드) | Windows Hello·Touch ID, Linux는 확인 코드 |
| V3 | 24 h soak 기준 머신(벤치 PC CPU·RAM·디스크) | 4코어 / 16 GiB / SSD |
| V4 | 벤치 PC를 여러 사람·여러 OS 계정이 같이 쓰나? | 한 사용자 |
| V5 | 주 브라우저 | Chrome 계열 |

**V 답변 (2026-11-10 22:00, 사용자)** — 원문: "응 다른 벤치는 UI가 있음. 다만 사내라서 여기서 리웍을 못함. 그래서 그냥 러스트로 전환 + 에이전트에게 작업을 맡기는 폭을 넓히고자 이런 툴을 개발한 것임. … 24시간 시험 돌릴때 hp fury z16이거나 제온에 램 32기가짜리 워크스테이션 쓰니 리소스는 신경쓰지말고 벤치는 혼자씀 주로 쓰는 브라우저는 크롬인데 왠만하면.. 플레이라이트로 하는게 좋지 않을까?"
- 배경: 사내 벤치에는 기존 UI 도구가 있지만 회사 밖에서 고칠 수 없다. Loom의 목적 = **Rust 전환 + 에이전트에게 맡길 수 있는 작업 범위를 넓히는 것**. 승인 설계는 이 목적에 맞게 마찰을 최소화한다.
- V1 = 예(다른 PC에서 봄) → UI 원격 보기를 M2로 앞당김. 기본은 localhost 바인딩 + SSH 터널 또는 Tailscale(사용자 Mac에 설치됨) 경유, 사내망 LAN 직접 노출은 선택·인증 필수.
- V2 = 미정(사용자가 R4 의미 질문) → 기본값 유지(R4만 존재 확인).
- V3 = HP ZBook Fury 16 또는 Xeon·RAM 32 GiB 워크스테이션 → 자원 걱정 없음. soak 관문은 절대 사용량이 아니라 **증가율**(누수) 기준 그대로.
- V4 = 혼자 씀.
- V5 = Chrome. **Playwright 채택**: UI E2E 시험(각 팀 M1 수용 시험), 에이전트가 사용자와 같은 화면을 보고 스크린샷을 테스트 증거로 첨부. 단, **Playwright·자동화 브라우저 세션은 watch 권한만**(operate/approve 불가, bootstrap 쿠키 발급 안 함) — 에이전트가 브라우저로 자기 승인하는 경로 차단(E9 보강). Playwright 브라우저는 프로젝트 밖 캐시에 설치.

---

## 5. 주요 리스크

| 리스크 | 대응 |
|---|---|
| core가 늦어지면 세 도메인 M1이 모두 막힘 | P0에서 스키마 fixture·stub을 **첫 주 안에** 배포, 도메인은 stub으로 개발. UDS 계획처럼 core 미제공 항목은 같은 시그니처로 임시 구현 후 이관 |
| Python에서 ISO-TP·주기 송신 타이밍 (특히 Windows) | CAN worker 프로세스 분리(D1), M0 측정 스파이크로 숫자 확인, M2 실HW 지터 실측 게시 |
| Vector XL과 CANoe 채널 공존 | P3에서 실측(B3). 공존이 안 되면 CANoe 브리지(COM) 경로로 대체 — 브리지는 P2부터 존재 |
| CANoe COM 자동화의 안정성 (STA 스레드, 측정 재시작 시 객체 무효화, 버전별 API 차이) | 브리지 전용 스레드·재연결·버전 표 기록, CANoe 데모판으로 CI 외 수동 시험 |
| Saleae 스트리밍 불가로 LA가 실시간 원칙을 일부만 충족 | 캡처 중 화면은 Logic 2 앱, Loom은 상태 실시간 + 캡처 후 결과 표시로 정직하게 명시 |
| 고부하에서 화면이 "거짓말" (멈춤·조용한 드롭) | 모든 도메인 gap/LAG 배지 + 저널 무손실 경로 + 부하 시험을 M1 수용 기준에 포함 |
| 에이전트 자기 승인 / 안전 우회 | 뷰어 전용 승인(D6), origin은 연결 자격으로 결정, 정책 완화는 뷰어 승인, 위협 모델 명시 |
| macOS 개발 ↔ Windows·Linux HW 간극 | 3-OS CI, Windows 콘솔 원시 입력은 UART M2 관문, 실 HW 체크리스트는 각 M2/M3 |
| 범위 팽창 (탐색 도메인 5개+) | PSU만 P1(첫 플러그인으로 계약 검증), 나머지 탐색 도메인은 세 팀 M1 이후 하나씩 |

---

## 6. 다음 행동 (v2.1)

1. 사용자: 구현 시작 승인. §4.1 V1–V5는 답이 없으면 기본값.
2. 메인: git 초기화(`main` + 팀별 worktree·브랜치 `core/*` `ui/*` `uart/*` `can/*` `uds/*` `test/*`), `.gitignore`(spikes의 target·브라우저·venv 제외), `docs/contract/` 계약 v1 분리(E12).
3. CORE: P0 Day 1–2 `loom-proto` 태그 → 모든 팀이 같은 스키마로 시작.
4. 각 팀: §2.4 E 판정과 **§2.5 F1(기본 모두 자동)** 중 자기 계획과 충돌하는 부분만 P0 첫 작업으로 정리(별도 계획 라운드 없음).
5. 사용량: 18개 에이전트 동시 구현은 5시간 한도에 빨리 닿는다. P0는 CORE·UI·CAN 우선, UART·UDS·TEST는 core 스텁(`core-p0-d2`) 뒤 합류 권장.
