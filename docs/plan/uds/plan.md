# ③ UDS (DoIP + CAN-TP) — 통합 계획서

작성: uds-lead (R3 통합), 2026-11-09 · **v1.1(2026-11-10): 마스터 계획 v1.1 정합** — `00-master-plan.md` §2 D1–D14와 §2.1 A1–A16을 따르도록 수정(변경 목록 §16.1) · **v1.2(2026-11-10): 사용자 답변 반영** — 마스터 §2.2 B1–B7(실HW 호스트 Windows+Linux, `canoe` 백엔드, vehicle = bench 규칙). 변경 목록 §16.2 · **v2.0(2026-11-10): 방향 전환** — 마스터 §2.3 C1–C8·**C4.1**(Rust 코어·장치 경로, Loom UI 앱이 1차 사용자 화면, VN5640·PCAN FD 자체 스택, TC·자연어 통합 테스트, 스킬). 변경 목록 §16.3
기준 우선순위: `docs/plan/02-core-principle.md` > `00-master-plan.md` §2.3 C1–C8 > §2.2 B1–B7 > §2.1 A1–A16 > §2 D1–D14 > `00-brief.md` > `uds/brief.md`. 공통 계약(이벤트·CLI 봉투·종료 코드·승인·정책·기록·데몬)은 마스터/CORE 계약 v1이 정본이고, 이 문서는 UDS 매핑만 적는다.
v2.0 근거: `v2-sol.md`(Rust crate·DoIP·VN5640·seed-key·C2 예산, Rust 스파이크 `spikes/uds/evidence/v2-rust.json`), `v2-opus.md`(UI 패널·M1·통합 테스트·스킬·CORE 요청), `_xteam/uds-ui.md`, `_xteam/uds-core.md`.
근거 문서: `r1-opus.md`(아키텍처·UX·안전), `r1-sol.md`(라이브러리·타이밍·시뮬레이션, 스파이크 실측), `r2-opus-review.md`, `r2-sol-review.md`, `_xteam/can-uds-isotp.md` v0.2, `_xteam/viz-requirements.md` `## uds`.
표기: "측정"은 `spikes/uds/evidence/*.json`의 실측값(macOS ARM64, CPython 3.14.1, loopback), "목표"는 미측정 수용 기준, "추정"은 미확인. 상세 근거가 긴 항목은 R1 절을 가리킨다(예: Sol §7.3).

---

## 0. 핵심 루프 (02-core-principle)

> 에이전트가 `loom uds ...` 한 줄로 ECU에 직접 진단 요청을 보내 읽고 쓰고, 사용자는 **같은 트랜잭션을 같은 순간에** 라이브 화면에서 보며 언제든 직접 요청·제어권 회수·긴급 정지로 개입한다.

| 요소 | UDS에서의 구현 | 담당 구성요소 | 첫 수용 |
|---|---|---|---|
| ① 직접 접근 | `loom uds connect --doip <ip> --la 0x1001` / `--can ch0 --tx 0x7E0 --rx 0x7E8` / `--sim` / (Windows) `--backend canoe`(CANoe 진단 설정 사용, §3.7). 상주 데몬 `loomd`의 **UDS 세션 관리자**가 DoIP 소켓을 단독 소유하고 CAN-TP는 CAN 모듈의 ISO-TP 채널을 쓴다. CLI는 IPC 클라이언트일 뿐이며 CLI가 끝나도 세션은 유지된다. 사람이 별도 진단 프로그램을 열 필요 없음 | `loom-uds` crate(Rust, gateway 안), 자체 DoIP 전송, CAN worker ISO-TP(Vector VN5640·PCAN FD — CAN 팀 Rust), 선택 `canoe` 어댑터(P4) | M1 (sim DoIP), M1b (sim CAN-TP), M2 (VN5640·PCAN FD HIL) |
| ② 읽기 | `read-did`, `dtc list`, `status`, `poll`, `trace --since @last` — 모두 timeout·건수·바이트 상한이 있는 경계 있는 명령. 사람용 디코드 + `--json`, NRC 이름·의미·다음 행동 힌트 | 트랜잭션 러너 + 진단 DB 디코더 | M1 |
| ③ 쓰기 | `write-did`, `session`, `security`, `dtc clear`, `routine`, `io`, `reset`, `raw`, `run scenario.yaml` — 모든 경로(타입 명령, raw, 시나리오, 뷰어 수동 송신, Python API)가 core 쓰기 파이프라인 + UDS 훅(바이트 분류 → 정책 → 확인 → 감사 → 송신 직전 검사, 마스터 D10/A2)을 통과 | `loom.uds.guard` + 정책 파일 + 승인 서비스 | M1 (write-did, raw, R2 확인), M2 (전체) |
| ④ 사용자 실시간 표시·개입 | 데몬이 `queued → tx → pending ×N → rx/timeout` 이벤트를 **에이전트에게 RPC 결과를 돌려주기 전에** 이벤트 버스로 발행. **Loom UI 앱(웹)의 UDS 콘솔 패널**(§5.0, `_xteam/uds-ui.md` — v2.0 1차 화면) + 보조 `loom uds view`(TUI)·viz 웹이 같은 스트림을 구독. origin(agent/user/script/device) 표시. 사용자는 뷰어에서 요청 송신·위험 동작 승인·제어권 회수(`T`)·긴급 정지(`!`)·폴링 정지 | 이벤트 버스, `loom uds view`, viz V-UDS-1/2/4 | M1 |

설계 불변식 (이 문서 전체가 지킴):

1. **ECU와 통신하는 코드는 데몬 안에만 있다.** CLI 프로세스나 스크립트가 UDS·DoIP 라이브러리로 ECU에 직접 붙는 구조는 금지(숨은 세션). Python API·사용자 뷰어 송신도 같은 데몬 RPC를 쓴다.
2. **발행 후 응답(publish-before-reply)은 순서 계약이다.** 데몬은 트랜잭션 이벤트를 버스에 넣은 뒤에 RPC 결과를 돌려준다. 화면 렌더 지연은 별도 SLO(§5.4)로 측정·수용한다(렌더 완료까지 장치 처리를 막지 않는다).
3. **숨김 없음.** TesterPresent, 시나리오 단계, 폴링, 사용자 수동 요청, 가드 거부, 승인까지 전부 같은 스트림·기록에 남는다. 화면의 접기·집계는 표시 방식일 뿐 데이터는 빠지지 않으며, 집계 시 집계 건수를 표시한다.
4. **단일 쓰기 경로.** 모든 쓰기는 core 쓰기 파이프라인(policy → confirm/approval → audit → backend → events)을 지나고, UDS는 훅 3개(`classify`·`approval_binding`·`presend_check`, 마스터 A2)로 실제 전송 바이트 기준 분류와 송신 직전 재검사(물리 주소·epoch·lease·quarantine·현재값·승인 기한)를 맡는다.
5. **늦게 열어도 같은 화면.** 뷰어 attach = `snapshot(watermark seq)` + `seq > watermark` 연속 스트림. 새 ECU 연결을 만들지 않는다.
6. **origin은 데몬이 연결 자격으로 정한다**(마스터 D2 `hello` role). 요청이 스스로 신고한 origin은 쓰지 않는다. agent role 연결(CLI·Python API) = agent, core 러너 = script, viewer role 연결만 user. 화면의 "누가 했는지"가 거짓이 되지 않게 하고 사용자 완화 규칙의 사칭을 막는다.

### 0.1 v2.0 적용 규칙 (마스터 C1–C8)

v2.0은 바뀌는 절 중심 개정이다. 아래 표가 v1 본문의 해당 표현을 **대체**한다. 본문에 남은 Python·Textual·udsoncan·doipclient 표현은 v1 근거 기록이며, 구현 기준은 이 표와 v2.0 절이다.

| v1 표현 | v2.0 기준 | 절 |
|---|---|---|
| Python asyncio 데몬 모듈 `loom.uds`, udsoncan 코덱 | Rust `loom-uds` crate(gateway 안, tokio). **자체 러너 + 작은 자체 서비스 코덱**, `automotive_diag`는 SID/NRC 정의만 | §3.1, §7.0, D2·D26 |
| Python 자체 DoIP | Rust 자체 DoIP(길이 검사를 할당 전에, 0xE0 central security) — 공개 crate는 0xE0 오류·길이 상한 없음으로 비채택 | §7.0, D4 |
| TUI `loom uds view`가 M1 1차 화면 | **Loom UI 앱(웹) UDS 콘솔 패널**이 1차, TUI는 보조(M1 수용에서 빠짐) | §5.0, D28 |
| Textual pilot E2E | 브라우저 자동화(Playwright)로 실제 DOM 확인 | §9.3, §11 |
| Python seed-key 플러그인 서브프로세스 | Rust helper exe(DLL은 x86/x64 helper), 외부 실행파일 JSON 프로토콜, Python 스크립트는 대체가 없을 때만 외부 worker | §10.8, D30 |
| `canoe` 백엔드 M2c(P2) | **선택 기능 P4**(Python 어댑터 별도 프로세스), 합의(`_xteam/canoe-bridge.md`)는 보존 | §3.7, D23 |
| 실HW CAN-TP: PCAN classic → Vector M4 | **CAN-TP: VN5640(CAN FD 전용)·PCAN-USB FD**, FD는 M2 첫 실HW부터. **DoIP: PC 랜카드 + 미디어 컨버터(OS TCP/IP, C4.1)** | §8, §8.2, D27 |
| Python 3.11 CI | Rust stable(MSRV는 CORE 결정, 후보 crate 최고 요구 1.91), cargo test 3 OS | §9.3 |
| `loom.uds.seedkey` entry point 플러그인 | 폐기(C3) — 외부 helper 프로세스만 | §10.8 |

**Python이 남는 곳(C1 근거)**: ① 선택 `canoe` 어댑터(P4) — CAN 팀 브리지가 pywin32 STA·COM 이벤트를 재사용하는 선택 기능. Rust(`windows-rs`) 불가 증거는 없으므로 CAN 팀 결정에 따르고 기본 진단 경로에는 Python이 없다(D32). ② 고객이 seed-key를 Python 코드로만 줄 때의 외부 실행(대체 불가 시). ③ ODX 변환은 Rust `odx-rs` 후보가 있어 Python 필요가 입증되지 않았다 — 실 OEM PDX에서 지원 불가가 확인될 때만 odxtools 외부 변환 worker를 따로 결정한다.

---

## 1. 범위 / 비범위

### 1.1 범위 (핵심 루프에 직접 기여)

| 영역 | 내용 | 마일스톤 |
|---|---|---|
| DoIP 전송 | UDP 13400 차량 식별 요청·공지 수집(경계 있는 다중 응답), TCP 13400 라우팅 활성화, alive check 응답(상주 수신 pump), 진단 메시지 ACK/NACK 해석, **게이트웨이(엔티티)·테스터 SA당 소켓 1개 + 논리 주소 다중화** | M1 (sim), M4 (실HW) |
| CAN-TP 전송 | CAN 모듈 ISO-TP 채널 위(`_xteam/can-uds-isotp.md`). normal 11bit부터, 이후 normal-fixed/extended/mixed, 29bit, CAN FD, 기능 주소 그룹 | M1b (normal 11bit sim), M3b (전체, CAN M4 이후) |
| `canoe` 백엔드 (Windows, B3 → v2.0 C4로 선택 기능) | CANoe 진단 설정으로 요청 바이트 송신·응답 수신(CAN 팀 브리지 = Python 어댑터 별도 프로세스). 분류·승인·감사·이벤트는 Loom 파이프라인 그대로. 진단 기본 경로는 자체 스택 | **P4 선택** |
| UDS 서비스 | 0x10, 0x11, 0x14, 0x19(주요 서브기능), 0x22, 0x27(seed-key 플러그인), 0x28, 0x2E, 0x2F, 0x31, 0x3E(UDS 관리자 자동 유지), 0x85, raw 임의 요청. NRC 0x78 pending, P2/P2*, 전체 deadline | M1(0x10/0x22/0x2E/0x19 02/raw), M2, M3 |
| 세션 상태 | TargetSession이 링크·세션(신뢰도 confirmed/assumed/stale)·보안·P2 값·TP·lease·큐 보유 | M1(최소), M2 |
| 사용자 가시성 | **Loom UI 앱 UDS 콘솔 패널**(트랜잭션 표·상태 바·요청 입력·승인 카드 내용, §5.0) + 보조 TUI, 수동 송신, takeover, 긴급 정지 | M1(뷰어 자격 + R2 승인 패널 최소: 표시·포커스 시 `Y`/`N`), M2(R3 승인·다중 대기·diff 고급) |
| 안전 | R0–R4 등급표, raw 바이트 분류, 확인 토큰·뷰어 승인, 화이트리스트·env, 레이트, 감사 로그, dry-run, 시나리오 사전 위험 요약 | M1(분류 전수, R2 `auto`/토큰/뷰어 Y, R3·차단 3층 거부, env=sim 자동 화이트리스트·최소 레이트, write/raw dry-run, 기록·감사 불능 시 agent·script R1+ 쓰기 차단(A12), timeout quarantine), M2(정책 파일 전체·해시 체인·R3) |
| 진단 DB | Loom YAML/JSON DID·DTC·Routine·IO 정의(이름, 타입, 스케일, 단위, 등급) | M1(스칼라/ASCII), M3 |
| 폴링 → 플롯 | 데몬 폴링 작업 → 이벤트 → UI 팀 `loom.viz` 플롯(V-UDS-2) | M3 |
| 가상 ECU | 전송 무관 상태 모델 + DoIP 엔티티 어댑터 + CAN-TP 노드 어댑터, 결정적 결함 주입 | M1(DoIP), M1b(CAN-TP) |
| 에이전트 스킬 | `skills/loom-uds/` + `loom uds help --json`(정본) + Codex 호환, `loom-test`와 경계(§10.10) | M1(최소), M2 |
| 통합 테스트 지원 (C7) | `check`/`wait-for`/`--expect` 술어, 증거 인용 이벤트·`uds.check` 레코드, `verify_evidence` 훅, 테스트 세션 패턴 범위 사전 승인용 `uds.plan_summary` (§6.6) | M1(check·wait-for 최소), M2 |

### 1.2 후순위 (루프는 이미 돌고, 질을 높이는 것)

- 시나리오 YAML 러너와 `--plan` 위험 요약 (M3)
- ODX/PDX 가져오기: odxtools strict 모드로 Loom YAML 변환 + coverage report (M4, 선택 의존성 `loom[odx]`)
- DoIP TLS(3496): 대상 장비가 TLS를 요구하면 **그 장비의 첫 실HW 수용 전 필수 관문**, 아니면 M4 이후. 검증 실패 시 평문 대체(fallback) 금지
- Windows seed-key DLL 브리지(32bit DLL은 별도 32bit 프로세스) (M4). CANoe 측정 중이면 `canoe` 경로의 seed-key 위임이 먼저 검토됨(§3.7)
- 버스에서 관찰한 외부 진단 트래픽(CANoe UI 등 `external:canoe`)을 UDS 행으로 디코드하는 수동 모니터 — P4(구 M2c)는 CAN 트레이스 표시 + 경고만
- 웹 고급 화면(V-UDS-3 DTC diff, V-UDS-5 CAN 트레이스 심볼릭 묶기) — viz 팀 일정
- pcapng 내보내기(DoIP), 0x23/0x2A/0x2C/0x3D/0x86/0x87/0x29/0x84 타입 명령(raw로는 M1부터 가능, 분류는 M1부터 적용)

### 1.3 비범위

- **실차 범용 플래시(0x34/0x35/0x36/0x37/0x38).** OEM마다 블록 형식·서명·지우기 루틴·체크섬·복구 절차가 다르고 벽돌 위험이 크다. `maxNumberOfBlockLength`에서 0x36 헤더 2바이트 차감, 8bit 카운터 순환, DFI(압축/암호화)·ALFI 해석, 메모리 맵·erase/verify routine·서명·롤백은 모두 OEM별이다(Sol §1.2). 기본 R4(raw 포함 차단). 검증 이미지·전원·복구 절차가 확보된 **OEM 프로필 단위 선택 마일스톤(M5)** 으로만 연다. 시뮬레이터 다운로드 상태기 시험은 M3에서 허용.
- CDD(CANdela)를 등급·코덱 원천으로 직접 파싱: 공개 파서가 있긴 하나(cdd_utils — 라이선스 미확인, gocan/cdd — MIT Go) 제품 도입 근거 부족. CANdelaStudio의 ODX/CSV export(Vector 공식 페이지 명시) 경로로 대체. v1.2: CANoe 쪽 DID·루틴 **이름을 표시용으로 가져오는 것**은 P4(구 M2c) 검토 항목(§3.7, 등급은 낮추지 않음).
- OBD-II(J1979) 전용 해석, J2534 패스스루, K-Line, 원격(네트워크 너머) 진단, DoIP 서버 제품화(가상 ECU용 최소 구현만).

---

## 2. 사용 시나리오

### 2.0 종단 간 시나리오 E2E-1 (M1 수용 테스트의 골격)

> **v2.0**: 사용자 화면은 Loom UI 앱(웹)이다. 웹 기준 E2E-1(t0–t6, Playwright DOM 확인)은 §11 M1이 정본이다. 아래 타임라인·개입 흐름·종료 코드는 그대로 유효하고, "TUI"는 웹 UDS 콘솔 패널(U1–U4)로 읽는다.

전제: herdr 왼쪽 패인은 에이전트(Claude Code, `LOOM_CLIENT=claude-1`), 오른쪽 패인에서 사용자가 `loom uds view`를 띄워 두었다(뷰어 자격 연결). 가상 DoIP ECU `bench` 프로필: `F190`(VIN, 읽기 전용), `F1A0`(u8, 쓰기 가능). `connect --sim`으로 만든 대상은 **env=sim** 자동 화이트리스트(§10.6). sim의 R2 기본 경로는 `auto`(마스터 A8)이므로, E2E-1은 확인 흐름까지 보이기 위해 이 sim 대상에 `r2_path = "token_or_viewer"`를 설정해 실행한다(기본 `auto`라면 t2 쓰기는 확인 없이 바로 실행).

**t0 — 에이전트 연결 (①)**
```
$ loom uds connect --sim bench --name ecu1
connected ecu1  DoIP(sim) 127.0.0.1:13400  tester 0x0E80 → ECU 0x1001
routing activation 0x10 success (type 0x00)  1 ms
session default (assumed)  security locked  policy sim · auto ≤ R1
viewers 1 (tui)
```
뷰어 상태 바가 같은 순간 바뀐다: `ecu1 ● sim DoIP 0x1001  sess=default?  sec=🔒  TP off  lease free  SIM`.

**t1 — 에이전트 읽기 (②)**
```
$ loom uds read-did F190 -t ecu1
ecu1  22 F190 → 62 F190 …  p2 1 ms  rtt 1 ms
F190  VIN = "LOOMSIM0000000001"
```

| 데몬 시각 | 사건 | 사용자 TUI |
|---|---|---|
| 0.0 ms | RPC 수신, 가드 R0 자동 허용, `uds.queued` 발행 | — |
| 0.3 ms | DoIP 송신, `uds.tx` 발행 | 새 행 `A ecu1 ReadDataByIdentifier F190 ⏳` (목표: tx 이벤트 → 렌더 p95 ≤ 50 ms) |
| 1.2 ms | ECU 응답 수신, 디코드, `uds.rx`(origin=device, request_origin=agent) 발행 | 같은 행이 `✓ VIN="LOOMSIM…" 1ms`로 확정 (목표: 응답 수신 → 렌더 p95 ≤ 100 ms, p99 ≤ 200 ms) |
| 1.4 ms | RPC 결과 반환(발행 이후) | — |

```
┌ loom uds view ─ ecu1 ● sim DoIP 0x1001  sess=default? sec=🔒 TP off  lease free SIM ┐
│ time          o  ecu   service                 summary                     res  ms │
│ 12:03:40.990  A  ecu1  RoutingActivation       tester 0x0E80               ✓     1 │
│ 12:03:41.512  A  ecu1  ReadDataByIdentifier    F190 VIN="LOOMSIM00000…"    ✓     1 │
└──────────────────────────────────────────── live · lag 9 ms · seq 413 · rec ● ─────┘
```

**t2 — 에이전트 쓰기 + 되읽기 (③)**
```
$ loom uds write-did F1A0 --hex 01 -t ecu1
E_CONFIRM_REQUIRED  R2  WriteDataByIdentifier F1A0  00 → 01  target ecu1 (sim)
  request 2E F1A0 01   token K7Q2 (이 바이트·대상·epoch·현재값 한정, 120 s)   approval: token (또는 뷰어 Y)
  → 사용자에게 보여주고 동의를 받은 뒤: loom uds confirm K7Q2   (또는 사용자가 뷰어에서 Y)
exit 5
$ loom uds confirm K7Q2
ecu1  2E F1A0 01 → 6E F1A0  rtt 1 ms   state confirmed
$ loom uds read-did F1A0 -t ecu1
F1A0  sim_mode = 1          (직전 쓰기 → effective_verified)
```
뷰어: 첫 호출 때 하단 승인 패널(M1 최소판)에 같은 요청이 뜨고(패널 포커스 시 `Y` 승인 / `N` 거부), 확정 시 `A ecu1 WriteDataByIdentifier F1A0 00→01 ✓ R2` 행. M1 자동 수용 테스트는 토큰 경로와 뷰어 `Y` 경로를 모두 시험한다.

**t3 — 사용자 개입 (④)**
사용자가 TUI에서 `:` → `2E F1A0 02` 입력 → 입력 줄 아래에 실시간 디코드 미리보기(`WriteDataByIdentifier F1A0 sim_mode=2 · R2`) → Enter 두 번(R2 이상) → 실행. 뷰어 자격 연결이므로 origin=user, R2까지 별도 승인 없이 실행. 이 요청은 에이전트가 쥔 lease를 **암묵적으로 사용자에게 가져온다**(§3.5, 트레이스에 `lease → user` 행).
```
│ 12:04:02.204  U  ecu1  WriteDataByIdentifier   F1A0 01→02                  ✓     1 │
```
에이전트의 다음 호출 결과 마지막 `NOTE:` 줄(JSON `others_since_last`)이 사용자 개입을 알려 주고, 에이전트는 그때만 커서 조회를 한다:
```
$ loom uds read-did F1A0 -t ecu1
F1A0  sim_mode = 2
NOTE: 1 transaction by others since your last call (user) → loom uds trace --since seq:429
$ loom uds trace -t ecu1 --since seq:429
12:04:02.204 U WriteDataByIdentifier F1A0 01→02 ✓ 1ms
(1 transaction since seq:429; next_cursor seq:431)
```

**t4 — 사용자 제어권 유지와 긴급 정지**
t3에서 lease가 사용자에게 넘어갔다(명시적으로는 `T`와 같음). 상태 바 `lease: user`. 에이전트 쓰기 시도:
```
$ loom uds session extended -t ecu1
E_LOCKED: ecu1 제어권을 user가 가지고 있음 (since 12:04:02). 읽기(R0)만 가능.
hint: 사용자에게 제어권 반환(뷰어 R 키)을 요청하세요.
exit 6
```
사용자 `!`(UDS 대상 abort) → 큐 비움, 폴링 중단, 대기 승인 취소, TP 정지. (전 도메인 STOP은 `Ctrl-X`/`Ctrl-] x` = `loom stop-all`, 마스터 A3 — agent·script 쓰기와 모든 잡을 latch하고 사용자 입력은 막지 않음.) 진행 중이던 요청은 응답(또는 timeout)까지 기록되고 "이미 전송됨 — 취소 불가"로 표시. 활성 오버라이드(비기본 세션 등)가 남아 있으면 abort 화면에 "복구 송신" 키가 뜬다(§10.7, 자동 송신 아님).

**t5 — 늦은 합류**: 에이전트 CLI가 모두 끝난 뒤 사용자가 다른 패인에서 `loom uds view -t ecu1`을 새로 열면 현재 상태(세션·lease·F1A0 마지막 값)와 최근 100행을 받고 그 뒤 라이브가 이어진다. ECU 재연결 없음.

### 2.1 에이전트 관점

| # | 상황 | 명령 흐름 | 사용자가 보는 것 |
|---|---|---|---|
| A1 | "이 BCM SW 버전과 부품번호 알려줘" (R0만) | `status -t bcm` → `read-did F190 F187 F189 F18C F195 -t bcm --json` | 같은 트랜잭션(다중 DID면 1행) |
| A2 | DTC 진단·삭제 (R0 → R2) | `dtc list --mask 0x09` → `dtc snapshot B1234-00` → 원인 설명 → 사용자 "지워" → `dtc clear` exit 5 → 채팅 확인 → `confirm <tok>` → `dtc list` 재확인 | 승인 패널, clear 행, 재조회 행 |
| A3 | 가변 코딩 DID (세션 + 보안 + R3) | `session extended`(R1 자동, TP 자동 시작) → `security --level 1`(R3 — 뷰어 승인, 마스터 D6) → `read-did 0101` → `write-did 0101 --field region=EU --dry-run` → 실행(DB protected → R3, **뷰어 승인 필수**) → 되읽기 → `reset soft`(R2) → `status --probe` | R3 승인 패널(이전값→새값 diff), 단계별 행 |
| A4 | 폴링을 사용자 플롯으로 (M3) | `poll F40D,F405 -t ecm --period 200ms --duration 60s --viz` → 즉시 job id 반환 → `poll stats <job>`로 min/max/last만 | viz 플롯, TUI 폴링 그룹 1행(×N 집계) |
| A5 | 미지 차량 탐색 | `discover --doip --iface en7 --timeout 2s --max-results 100` / (M3b) `discover --can ch0 --functional --rx-range 0x7E8-0x7EF --timeout 500ms` | 탐색 요청/응답 행 |
| A6 | 0x78 오래 걸리는 루틴 | `routine start 0xFF01 -t bcm` → `7F 31 78 ×4` 동안 CLI는 TTY면 한 줄 갱신, `--json`이면 최종 1회 | 같은 행의 `pending ×4 3.9s/5.0s` 카운터가 실시간 증가 |

### 2.2 사용자 관점

| # | 상황 | 행동 |
|---|---|---|
| U1 | 에이전트 작업 실시간 관찰 | herdr 패인 `loom uds view`. origin `A` agent, `U` user, `S` script, `·` keepalive(purpose=tester_present, 기본 접힘), `D` device 응답은 같은 행에 합쳐 표시. Enter로 hex·디코드 트리·등급·승인 기록 |
| U2 | 위험 동작 승인 | 승인 패널에 포커스를 두고 대상·env·세션·요청 hex·diff·등급 사유 확인 후 `Y`/`N`(R3는 승인 코드 끝 2자). 감사 로그에 `user@viewer` |
| U3 | 직접 개입 | `:` 명령줄: `22 F190`, `read F190`, `session ext`, `dtc`. 입력 중 실시간 디코드 미리보기(서비스·DID 이름·등급). 같은 가드 적용(사용자 R2는 Enter 두 번, R3 승인 코드 입력, R4는 unlock 후 매번 뷰어 승인, 하드 차단·`immutable_deny`는 사용자도 불가) |
| U4 | 제어권 회수·긴급 정지 | `T` takeover, `R` 반환, `!` 긴급 정지, `s` 폴링 정지. CLI로도 `loom uds abort` |
| U5 | DID 플롯 (M3) | viz 웹 또는 TUI 스파크라인, CAN 트레이스와 시간 커서 동기화 |
| U6 | 사후 검토 (보조) | `loom uds audit --since today`, `loom uds trace --file <session>.jsonl` 재생 |

---

## 3. 아키텍처

### 3.1 결정: 사용자에게는 `loomd` 하나, UDS 세션 관리자는 gateway 프로세스, CAN은 CAN worker (메인 확정 필요 — §15 C1)

**v2.0(C1·C2)**: gateway·CAN worker 모두 Rust. UDS는 gateway 안 `loom-uds` crate(tokio)이며 DoIP 소켓을 소유한다. 벤더 DLL(vxlapi64·PCANBasic)은 CAN worker 안에서만 로드되고, **seed-key DLL은 별도 helper 프로세스**(x86 DLL은 `i686-pc-windows-msvc` helper)에서만 로드한다. 러너 타이머가 다른 도메인 질의·UI 직렬화에 밀리지 않도록 UDS 전용 task 우선순위 또는 전용 런타임을 CORE에 요청했다(`_xteam/uds-core.md` C-13, 목표 타이머 판정 지터 p99 ≤ 2 ms). 아래 v1 문단의 Python·asyncio 표현은 tokio로 읽는다.

- 사용자·에이전트가 보는 것은 `loomd` 하나(포트·URL·승인·감사·이벤트 버스 하나). 내부는 **gateway 프로세스 + CAN worker 프로세스**(CAN 팀 CR1 수용: 같은 프로세스에 CPU-bound Python 스레드가 있으면 FF→FC 지연 p99 1.49 → 51.9 ms로 악화 — CAN R1 실측 `spikes/can/r1-results.json`).
- **UDS 세션 관리자는 gateway 프로세스 안**에 산다. DoIP 소켓은 UDS 모듈이 소유하고, CAN-TP는 CAN worker가 호스팅하는 ISO-TP 채널을 `IsoTpPort`(PDU RPC)로 쓴다. 스파이크: 별도 프로세스 Pipe 경유 ISO-TP 왕복 p99 3.09 ms(UDS R1) — 감당 가능. CAN M2에서 PDU 완성→UDS 수신 p99 ≤ 5 ms를 측정해 확정.
- **필수 계약**은 물리 장치 단독 소유 + 도메인 간 API·이벤트 스트림 일치다. `IsoTpPort`는 **인프로세스 호출과 RPC가 같은 시그니처**(asyncio)라 메인이 배치를 어떻게 정해도 UDS 코드는 바뀌지 않는다(기본 = PDU RPC, 단위 시험 = 인프로세스 가짜 포트).
- 데몬 수명주기(마스터 D8): CLI는 데몬이 없으면 **자동 기동**(`LOOM_NO_AUTOSTART=1`로 끔), `loom daemon start|stop|status|logs`는 core 공통. 연결 불가 시 exit 9 `E_DAEMON`. 데몬은 장치를 자동으로 열지 않는다. 스파이크로 별도 프로세스 Pipe 경유 ISO-TP 왕복 p99 3.09 ms 측정(Sol §9.2).
- 격리: UDS 트랜잭션 러너는 asyncio 루프, CAN 수신/ISO-TP는 CAN 모듈 스레드, **seed-key 플러그인은 별도 서브프로세스**(시간 상한 2 s, 출력 상한, 크래시 격리 — native DLL 포함). 동기·블로킹 코드를 이벤트 루프에 직접 올리지 않는다.
- 기각: CLI가 UDS 클라이언트 라이브러리로 ECU를 직접 여는 구조(02 원칙 위반).

### 3.2 프로세스 구성도

```
 ┌─ Claude Code / Codex (에이전트) ─┐     ┌─ 사용자 ─────────────────────────────┐
 │ loom uds read-did F190 ...       │     │ Loom UI 앱(웹) UDS 콘솔 · TUI 보조   │
 │ (단발 CLI, 비대화형, --json)      │     │ 브라우저: loom viz (웹, M3~)         │
 └───────────────┬──────────────────┘     └──────────┬───────────────▲──────────┘
                 │ RPC                               │ RPC(수동 송신,  │ 이벤트 구독
                 │                                   │ 승인, takeover) │ (uds.*, can.*)
 ════════════════╪════════ 로컬 IPC: 127.0.0.1 WebSocket JSON-RPC 2.0 + 토큰 + Origin 검사 ════
 ┌───────────────▼───────────────────────────────────────────────────┴────────┐
 │ loomd (사용자에게는 하나 — 내부: gateway 프로세스 [core+uds] / CAN worker 프로세스 [can]) │
 │ ┌ core ─────────────────────────────────────────────────────────────────┐  │
 │ │ RPC 라우터 · 이벤트 버스(전역 seq) · 기록기(bounded queue) · 감사 로그 │  │
 │ │ 승인 서비스 · lease · 정책 로더(사용자 소유 파일)                       │  │
 │ └──────▲─────────────────────────▲──────────────────────────▲─────────────┘  │
 │ ┌──────┴────────── uds ──────────┴──────────────┐   ┌───────┴── can ───────┐ │
 │ │ UdsService                                     │   │ python-can 채널       │ │
 │ │  ├ TargetRegistry → TargetSession(상태·큐·TP)  │   │ ISO-TP 호스트         │ │
 │ │  ├ Guard (분류 → 정책 → 확인)  ◀ 단일 관문     │   │ (can-isotp 2.x,       │ │
 │ │  ├ TxnRunner (P2/P2*/0x78/deadline/epoch)      │   │  FC/STmin 인프로세스) │ │
 │ │  ├ Codec(자체 Rust) + DiagDB 디코더           │   │                       │ │
 │ │  ├ Poller · ScenarioRunner · TP 스케줄러       │   │                       │ │
 │ │  ├ SeedKeyHost ──▶ [플러그인 서브프로세스]     │   │                       │ │
 │ │  └ Transports: DoipTransport ─┐  IsoTpPort ───┼──▶│ can.isotp.open/send/… │ │
 │ └────────────────────────────────┼──────────────┘   └──────────┬────────────┘ │
 └──────────────────────────────────┼─────────────────────────────┼──────────────┘
                                    │ TCP/UDP 13400 (TLS 3496)    │ CAN / CAN FD
                            ┌───────▼────────┐              ┌──────▼──────┐
                            │ DoIP 게이트웨이 │              │ CAN ECU     │
                            │  └ ECU 0x1001… │              │ 0x7E0/0x7E8 │
                            └────────────────┘              └─────────────┘
        sim: loom.uds.sim 가상 DoIP 엔티티(gateway 프로세스, 127.0.0.1, ephemeral 포트) /
             CAN worker 안 SimNode로 도는 가상 CAN-TP 노드(virtual 버스는 프로세스 로컬 → UDS가
             전송 무관 "PDU in → PDU out" 모델을 제공하고 CAN worker가 실행)
```

### 3.3 TargetSession — 유일한 진실 원천

키: `(transport, physical_device/entity, tester_SA, ecu_target, addressing[mode, SA/TA, extension])`. 별칭(`--name`)을 달리해도 같은 물리 대상이면 같은 세션이다(잠금 우회 불가).

| 상태 | 값 | 갱신 규칙 |
|---|---|---|
| `link` | disconnected / connecting / connected / **quarantine** / error, `epoch` | 재연결·회복마다 epoch 증가. 이전 epoch 응답과 quarantine 중 도착한 응답은 `late` 이벤트로 기록·표시만(§3.4) |
| `active_overrides` | 비기본 세션, IO 제어 중인 ID·값, 0x28 disable, 0x85 off | 해당 요청 긍정 응답 시 추가, 복구 요청(2F xx 00, 28 00, 85 01, 10 01) 긍정 응답·리셋·S3 만료 추정 시 제거. 상태 바 상시 표시, abort 화면의 "복구 송신" 근거 |
| `session` | 0x01/0x02/0x03/0xNN, `since`, `confidence`(confirmed/assumed/stale) | 0x50 긍정 응답 시 confirmed. 연결 직후 assumed=default. NRC 0x7E/0x7F, 예기치 않은 리셋, 재연결 시 stale. `status --probe`는 0xF186(지원 시)로 확인 |
| `security` | locked / unlocked(level), `since` | 0x67 key 긍정 시 unlocked. 세션 변경·리셋·S3 만료·NRC 0x33 시 locked |
| `p2`, `p2_star` | ms | 0x50 응답의 P2server_max/P2*server_max로 갱신. 기본값은 대상 프로필(예시 50/5000 ms — 모든 ECU의 고정값 아님). udsoncan 기본(P2 1 s, P2* 5 s)과 혼동 금지 |
| `tp` | off / on(period), `last_ok`, `fail_count` | 비기본 세션 진입 시 정책에 따라 on, 기본 세션 복귀·연결 해제·유휴 만료 시 off |
| `lease` | free / held(owner, origin, shared\|exclusive, ttl) | §3.5 |
| `queue` | 인플라이트 ≤ 1, 대기 ≤ 32 | §3.5 |
| `policy` | env, 대상 허용 최고 등급, 해시 | 정책 파일 |

**TesterPresent 스케줄러는 UDS 관리자 소유**(CAN 주기 PDU 기능 기본 미사용). 쓰기 클래스는 마스터 A1의 **잡형**: 비기본 세션 진입 요청이 승인되면 잡이 시작되고, core는 start/stop/renew만 게이트·감사하며 단위 송신(`3E 80`)은 TX latch·레이트를 단위마다 재확인한 뒤 events/저널에만 남긴다. 폴링(`poll`)도 같은 잡형(start 시 R0 판정, 단위마다 latch·레이트·quarantine 재확인). 일반 UDS 요청은 요청형. 주기 2 s(S3 5 s 예시 기준, 실제 S3는 프로필), `3E 80`. 큐에 인플라이트가 있거나 최근 2 s 안에 다른 요청이 나갔으면 건너뛴다. 데몬/모듈이 죽으면 TP가 멈추고 ECU는 S3 만료로 기본 세션 복귀(안전 쪽 실패). 자동 ECU reset은 보내지 않는다.
**유휴 만료**: 모든 클라이언트 요청이 10분(정책) 없으면 TP 정지 → ECU 기본 세션 복귀. 1분 전 뷰어 경고. CLI 종료·뷰어 detach만으로는 정지하지 않는다.

### 3.4 요청 1건의 데이터 흐름과 타이밍

```
CLI ─rpc uds.request{target, typed|raw_hex, dry_run?, deadline_ms, idempotency_key}─▶ core 쓰기 파이프라인 (마스터 D10/A1 요청형)
     (origin·client_id는 hello 연결 자격에서 core가 결정 — 불변식 6; 재제출은 uds.confirm{token})
 1. UDS Codec: 타입 명령 → 바이트 (raw는 그대로)                       → write_state requested (uds.queued 아님 — 아직 큐 전)
 2. [UDS 훅] classify(bytes, target) → (risk R0..R4, reason, block_class)   (§10.3 분류표, DB 상향·기능 주소 판정 포함, env 보정 없음 — B4)
 3. [core] policy(risk, target, origin, env, r2_path) → allow | confirm(R2) | approve(R3/R4) | deny(하드 차단·immutable_deny·default_blocked)
      [UDS 훅] approval_binding(req) → 추가 digest 필드(물리 주소·epoch·lease·현재값)
      confirm/approve → core 승인 객체 생성 → CLI exit 5 (E_CONFIRM_REQUIRED / E_APPROVAL_REQUIRED)
 4. [core] audit(요청·판정), lease·레이트·큐 상한                         → accepted
 5. UDS backend enqueue → uds.queued 발행(write_state=accepted, 큐 대기) (대상당 인플라이트 직렬화·seed→key 원자 실행은 UDS backend 책임, A2)
 6. TxnRunner(대상당 동시 1건):
      [UDS 훅] presend_check(req): 물리 주소·epoch·lease·quarantine 미해결 집합 U와 SID 겹침·현재값 변화·승인 사용 기한
               실패 시 승인 무효 + E_QUARANTINE / E_LOCKED (exit 6)
      transport.send(드라이버·소켓에 넘김) → uds.tx 발행                  → sent (A14: wire 완료 아님)
      P2 타이머:  tx_done → 첫 응답 시작(DoIP 진단 메시지 수신 / ISO-TP rx_start)
      0x78 수신마다 uds.pending{count, elapsed} 발행, P2* 재시작, 최대 횟수·총 deadline 제한
      PDU 완료 상한: rx_start → rx_complete (길이·N_Cr 기반, P2와 별도)
      완료/timeout → 디코드 → 상태 갱신 → uds.rx(긍정 → confirmed / NRC → failed) 또는 uds.timeout(쓰기 → outcome_unknown) 발행
      (되읽기 검증을 하면 effective_verified / effect_unverified — §6.1 write_state)
 7. [core] 감사 append (결과·승인 사용 기록)
 8. RPC 결과 반환(D5 봉투)  ← 반드시 6의 발행 이후 (publish-before-reply, A2)
```

**결정: 자체 트랜잭션 러너 + 코덱** (결정 기록 D2; v2.0은 Rust 자체 코덱, `automotive_diag`는 정의만 — D26). 이유: (a) P2를 "첫 응답 시작" 기준으로 끊으려면 ISO-TP `rx_start`·DoIP 진단 메시지 수신 이벤트에 직접 반응해야 하는데 `Client.wait_frame`은 완성 PDU만 다룬다(Sol §3.3). (b) 0x78 반복 제한, 외부 총 deadline, epoch 격리, abort, publish-before-reply를 한 곳에서 통제. (c) M1 전송이 자체 DoIP이므로 Client 재사용 이득이 작다. udsoncan의 `Request`/`Response`, 서비스별 `make_request`/`interpret_response`, `DidCodec`, NRC 표는 그대로 쓴다. 러너 범위는 좁게 정의한다: 단일 요청 상태기(send → P2 → pending 루프 → 완료 상한 → 총 deadline) + 응답 SID/에코(DID 등) 검증. **되돌림 조건**: U-06 단위 시험(NRC·에코·타이머 경계)이 M1 일정 안에 통과하지 못하면 Sol 안(데몬 내 `Client` + 이벤트 발행을 강제한 커스텀 `BaseConnection`)으로 전환한다 — 스파이크로 동작이 확인된 경로.

타이밍 규칙:
- 판정 시각은 데몬 타임스탬프(DoIP 소켓 수신 / ISO-TP HW 또는 host 타임스탬프, 공통 `ts.source`·`ts.quality` 표기). IPC 지연은 판정에 넣지 않고, 대기 자체는 데몬 monotonic으로 한다.
- **drain barrier (CAN-TP, CAN 팀 제안 수용)**: P2/P2* deadline이 되면 바로 timeout으로 판정하지 않는다. CAN worker에 watermark("frame seq X까지 처리했나")를 요청하거나 주기 watermark 이벤트를 받아, 그 시점 이전에 `rx_start`가 없었음을 확인한 뒤 timeout을 확정한다(이벤트가 아직 IPC 위에 있을 수 있으므로). 판정 기준은 여전히 버스 타임스탬프. 계약은 CAN 확정안 `can.isotp.barrier{isotp_id, epoch, deadline_mono_ns, timeout_ms} → {covered_ingest_seq, transport_event_seq, covered_mono_ns, complete}`; barrier가 실패·`complete=false`면 timeout을 확정하지 않고 결과를 "판정 불확실"(`timing.verdict: uncertain`, 쓰기는 `outcome_unknown`)로 보고한다.
- `can.isotp_tx_done`은 기본 **드라이버 접수 시각**(`tx_state: driver_accepted`)이고 TX 확인을 주는 backend에서만 `bus_confirmed`(마스터 A14 — D4 `accepted/confirmed`와 혼동 방지로 이름 변경). P2 시작점으로 쓰되 driver_accepted면 결과에 그 사실을 표기.
- 결과에 `p2_ms`(tx_done → 첫 응답), `rtt_ms`(tx_done → 완료), `pending_count`, `pending_total_ms`를 분리해 보고. 화면 색도 P2 판정은 `p2_ms` 기준(긴 멀티프레임 응답 때문에 RTT가 P2를 넘는 것은 정상).
- 기본 deadline: 읽기 2 s, 일반 job 30 s, routine 프로필별 최대 120 s, 연속 0x78 최대 횟수 프로필(기본 50). `--timeout` 무제한 금지. **총 deadline은 RPC 수신부터** 큐 대기·송신·ACK·응답 완료까지 포함하고, 각 단계 대기는 남은 예산으로 제한한다.
- timeout 뒤 쓰기 계열은 `may_have_executed: true`, 자동 재시도 0. 읽기만 정책상 1회 재시도 허용(0x21 busyRepeatRequest 포함).
- **timeout/abort 후 quarantine — 응답 식별자 기반 격리** (D19): UDS 응답에는 요청 id가 없으므로, timeout된 요청의 늦은 응답이 같은 식별자의 새 요청 결과로 붙을 수 있다(폴링이 정확히 이 패턴). 규칙:
  1. **미해결 집합 U** = timeout/abort됐고 늦은 완전한 최종 응답(긍정, 또는 0x78이 아닌 `7F`)이 아직 오지 않은 요청. 해제 조건: 그 요청의 **완전한 최종 응답**(긍정, 또는 0x78이 아닌 `7F <SID> <NRC>`)이 `late`로 도착 — `7F <SID> 78`(pending)·미완성 PDU는 해제하지 않고 U를 유지; DoIP TCP 재연결 + 재활성화(이전 소켓 응답 원천 차단); ECU 리셋 긍정 응답 확인(DoIP·CAN 공통); 또는 프로필의 HIL 검증 값 `late_response_max_ms` 경과(검증 프로필만; sim 프로필은 구성상 보장).
  2. **겹침 판정**: 부정 응답 `7F <SID> <NRC>`에는 DID/서브기능이 없으므로, 새 요청의 SID가 U의 어떤 요청과 같으면 겹친다(긍정 응답 에코만으로는 부족).
  3. **U가 비어 있지 않은 동안 겹치는 요청은 등급과 무관하게(R0 포함) 차단**(exit 6 `E_QUARANTINE`, 해제 조건 안내). 겹치지 않는 요청은 응답이 유일하게 식별되므로 허용하되, 정숙 구간 max(P2*, 정책 `quarantine_ms` 기본 1 s)이 지난 뒤에만 보낸다. 자동 재시도 없음.
  4. 정숙 구간 뒤 **고유 동기화 프로브**(U와 겹치지 않는 R0 요청 — 프로필 후보, 기본 `3E 00`·`22 F186` 등 ECU 지원분, 이전 프로브도 U에 포함)로 ECU 생존을 확인한다. 프로브 성공은 생존 증거일 뿐 U의 해제 증거가 아니다.
  5. 격리를 보장할 수 없는 수신(겹침 상태에서 도착한 응답)은 성공값·`confirmed` 상태로 반영하지 않고 `late`로만 기록·표시한다.
  6. DoIP는 U가 남아 겹치는 요청이 필요하면 재연결로 회복하는 것을 기본으로 한다(사용자/에이전트에 안내). CAN-TP의 해제 조건은 완전한 최종 응답 도착, ECU 리셋 확인, 검증 프로필 경과 시간이며(재연결 수단 없음), 사용자 확인만으로 격리를 선언하지 않는다. 같은 물리 ECU를 다른 SA·별칭으로 열어도 U를 우회하지 못한다. 폴링은 겹치는 tick을 skipped로 센다.
- **suppressPosRsp(SPRMIB=1)**: P2 동안 NRC(0x78 포함)만 기다리고, 무응답은 `ok_suppressed`로 기록(timeout 아님).
- **비요청 응답**: 0x2A(주기)·0x86(이벤트) 응답은 1:1 짝짓기를 깨므로, 러너가 비요청 응답 라우팅을 지원하기 전까지 두 서비스는 `immutable_deny`(exit 4 `E_UNSUPPORTED`, §10.3).
- `raw --no-wait`(R0만)도 데몬은 응답을 끝까지 수신·기록하고 다음 요청은 직렬화한다(응답이 다음 요청과 섞이지 않게).

### 3.5 다중 클라이언트 · 다중 ECU 직렬화와 lease

1. **대상당 인플라이트 1건**, FIFO 큐(상한 32). 우선순위: abort > user > agent/script > keepalive(유휴일 때만).
2. **다른 대상은 병렬.** CAN은 대상별 ISO-TP 채널. DoIP는 **엔티티·테스터 SA당 소켓 1개를 공유**하고 응답의 source LA로 대상별 분배. 같은 DoIPInterface의 다른 활성 연결에 동일 SA가 등록돼 있고 그 연결이 alive check에 응답하면 새 연결은 0x03(SA 이미 등록)으로 거부된다(AUTOSAR R24-11 SWS_DoIP_00105 p41, R4 확인 — 응답이 없으면 SWS_DoIP_00107 경로). ECU별 소켓은 서로 다른 허용 SA를 가진 경우의 예외. 게이트웨이별 `max_parallel` 기본 1.
3. **lease**(대상당 1개):
   - R0: lease 없이 누구나(큐에서 직렬).
   - R1 이상: lease 소유자만. 비어 있으면 첫 요청자가 자동 획득(TTL 120 s, 활동 시·**승인 대기 중** 연장).
   - lease 이동 시 보안 해제·active_overrides는 이어지므로, 새 소유자 결과와 승인 패널에 "보안 해제 주체·시각"을 표시하고 상태 바를 경고색으로.
   - `exclusive`: 시나리오, seed→key 쌍, (M5) 플래시 동안 다른 클라이언트의 R0까지 대기(최대 300 s).
   - **사용자 우선**: 뷰어 `T`는 다음 트랜잭션 경계에서 lease를 사용자에게 넘긴다. **사용자 origin의 R1 이상 요청은 암묵적 takeover**로 처리한다(다음 트랜잭션 경계에서 lease → user, 상태 바·트레이스에 `lease → user` 표시). 이후 에이전트 R1+ 요청은 exit 6. 진행 중 exclusive 작업은 현재 단계 응답 후 중단. 사용자는 `R`로 반환.
   - 에이전트끼리(Claude + Codex) 경합: 먼저 잡은 쪽, 상대는 exit 6 + 소유자 정보.
4. **원자 단위**: `security`(27 odd → 27 even)는 큐를 연속 점유.
5. **기능 주소 요청**: 전송(채널/게이트웨이) 범위 lease 필요, 락 순서 전송 → 대상(교착 방지).
6. **외부 송신 징후**: CAN 모듈이 예약된 진단 tx ID의 "예상 밖 프레임"을 보고하면(신원 확정은 불가 — 로컬 송신만 확정) 대상 세션을 stale로 표시하고 뷰어 경고.

### 3.6 UDS 도메인 RPC / 이벤트

| RPC | 설명 |
|---|---|
| `uds.connect{name, transport: doip\|cantp\|canoe\|auto, params, deadline_ms}` / `uds.disconnect{target}` | 대상 생성/해제 → session id, effective config, capabilities, cursor |
| `uds.targets{}` / `uds.status{target, probe?}` | 상태 |
| `uds.request{target, typed?|raw_hex?, dry_run?, deadline_ms, idempotency_key}` | 단일 관문. origin은 연결 자격으로 결정(요청 필드 없음) |
| `uds.confirm{token}` | R2 토큰 재제출(CLI `loom uds confirm <tok>`) — 호스트 `ask` 규칙을 동사 단위로 걸 수 있게 별도 동사 |
| `uds.recover{target}` | active_overrides 복구 요청 묶음(R1, Registry `safe_state: true` 동작). **user 전용**(뷰어 자격 연결의 "복구 송신" 1키 — 마스터 A15에서 core 공통 패턴으로 승격, UDS는 safe_state 동작 목록을 제공). 에이전트 CLI 명령 없음 |
| `uds.poll.start/stop/stats` | 폴링 job |
| (시나리오) | core `loom run`(마스터 D12) — UDS는 `uds.*` 단계 어댑터만 제공 (M3) |
| `uds.lease.acquire/release/takeover` | 제어권 |
| (승인 응답) | core 승인 서비스(마스터 D6) — **viewer role 연결 전용**, CLI approve 없음. UDS 승인 패널은 core 승인 API를 호출(R3 코드 끝 2자 입력은 UDS 패널 규칙) |
| `uds.abort{target?}` | 긴급 정지 |
| (구독) | core `core.subscribe`(원자적 snapshot + `after_seq`, 배치 상한 40 ms·적응형 flush, A5) — UDS는 필터만 정의 |
| `uds.trace.query{target?, since_cursor|tail, filter, max_bytes}` | 경계 있는 조회 |
| `uds.discover{transport, …, timeout, max_results}` | 탐색 |

이벤트: `uds.queued`, `uds.tx`, `uds.pending`, `uds.rx`, `uds.timeout`, `uds.late`, `uds.suppressed`, `uds.guard`(거부/확인 필요), `uds.state`, `uds.lease`, `uds.poll_sample`, `uds.alert`. 승인·run 단계·heartbeat·손실(`display.gap`/`record.gap`/`source.gap`)은 core 이벤트를 쓴다(마스터 D3·A13). 순번은 core 두 층(`source_seq` + `stream_seq`).

### 3.7 `canoe` 백엔드 (v1.2, 마스터 B3 — Windows 전용) — **v2.0: 선택 기능 P4**

**v2.0(C4)**: 진단 기본 경로는 Loom 자체 스택 over VN5640·PCAN FD다. `canoe`는 진단 필수 경로에서 내려 **P4 선택 기능**이 되고, 브리지는 Python 어댑터 별도 프로세스(같은 worker 프로토콜, 메모리 상한, 죽어도 데몬 생존)다. CANoe 측정이 켜져 있어도 `auto`의 기본은 **자체 스택**이며, `canoe`는 정책 `backend = "canoe"`로 명시할 때만 쓴다(채널 공존 실측 결과를 따름). 아래 설계와 `_xteam/canoe-bridge.md` 합의(바이트 전달, decision_id, generation, 0x78 progress, Loom이 켠 TP만 정지, 자동 전환 없음)는 그대로 보존한다.

사용자는 진단을 CANoe로 CAN·이더넷 모두 수행한다. 그래서 UDS 전송 백엔드는 셋이다: `doip`(자체), `cantp`(CAN worker ISO-TP), `canoe`(CANoe COM, Windows). **자체 스택 둘은 필수 유지** — Linux와 CANoe가 없는 환경의 유일한 경로이고 M1·M1b의 기반이다.

- **구조**: `CanoeDiagTransport`가 DoIP·CAN-TP와 같은 `Transport` 프로토콜을 구현한다. 아래는 CAN 팀의 `loom/canoe/` 브리지 v0(COM 세션·전용 STA 스레드·측정 상태·재연결)다. UDS는 COM을 직접 열지 않는다. 브리지가 UDS에 줄 API(진단 요청 송신·응답 수신·측정 상태 이벤트)는 `_xteam/canoe-bridge.md`에서 합의한다(CAN 초안, UDS `## uds 의견`).
- **바뀌지 않는 것**: TargetSession, 트랜잭션 러너의 상태 규칙(epoch·quarantine·lease·`write_state`), core 쓰기 파이프라인 + UDS 훅 3개, 등급표(§10.3), 승인, 감사, 이벤트·기록. **Loom은 항상 요청 바이트를 직접 만들고 분류한 뒤 그 바이트를 그대로 CANoe에 넘긴다**(바이트 스트림 요청). CANoe 진단 기술의 이름 기반 요청은 쓰지 않는다 — 분류 기준이 실제 전송 바이트이기 때문(불변식 4).
- **COM 경로(공개 자료 확인, 실기 미검증 — `v12-canoe-facts.md`)**: 대상 = `Application.Networks.Item(i).Devices.Item(j).Diagnostic`(CANoe 진단 설정에 있는 대상만, 자동 발견 아님). 송신 = `CreateRequestFromStream(bytes)` → `Send()`. 수신 = `Pending`·`Responses.Item(k)`의 `Stream`·`Positive`·`ResponseCode`를 브리지 스레드가 폴링(COM 이벤트 연결은 미확인). 응답 컬렉션에 0x78 중간 응답이 들어올 수 있으므로 **`ResponseCode = 0x78`은 최종 결과가 아니다**(러너 규칙 그대로) — Loom 총 deadline 만료 시 마지막 0x78을 성공으로 쓰지 않는다.
- **백엔드 선택 규칙**: `connect`에서 백엔드를 지정하지 않으면 `auto`. v1.2 안은 Windows에서 "측정 중이면 `canoe`"였으나 **v2.0에서 `auto` = 항상 자체 스택**, `canoe`는 명시 선택만. Linux·macOS는 항상 자체 스택. 정책 `[[uds.targets]] backend = "auto"|"canoe"|"doip"|"cantp"`로 고정할 수 있다. 결과·상태 바에 실제 선택(`via canoe` 등)을 항상 표시한다.
- **쓰기 상태**: `sent` = 브리지 `Send()` 반환, `OnConfirmation`은 전송 계층 `tx_state: bus_confirmed`일 뿐이고 `confirmed`는 ECU 긍정 응답(마스터 A14, `canoe-bridge.md` `## uds 의견` 정정 요청). 0x78은 브리지 `diag_progress`로 실시간 수신 → 행의 `pending ×N` 갱신. 기능 주소 요청은 v0에서 exit 4 `E_UNSUPPORTED`(`functional_on_canoe`).
- **선택은 connect 때 한 번**: 세션 중 측정이 꺼져도 자체 스택으로 자동 전환하지 않는다(보안 상태·테스터 주소가 달라지고, 같은 ECU에 테스터가 둘 생긴다). 측정 정지·CANoe 종료 → 대상 `link=down(canoe_stopped)`, 새 epoch, 진행 중 요청은 exit 7 `E_DISCONNECTED`(쓰기는 `outcome_unknown`), 사용자가 측정을 다시 켠 뒤 `connect`.
- **한 대상에 진단 경로 하나**: 같은 물리 ECU를 `canoe`와 자체 스택으로 동시에 열지 않는다(DoIP 라우팅 활성화 충돌, CAN-TP 응답 ID 중복 수신). 다른 경로로 열린 대상과 주소가 겹치면 `connect` 거부 exit 6 `E_LOCKED`(reason `target_on_other_backend`).
- **타이밍을 정직하게**: `canoe` 경로의 P2/P2*·0x78 대기는 CANoe 진단층이 하고(Loom은 별도 총 deadline만), COM 응답 객체의 시각 속성은 미확인이며 Vector도 COM 타이밍이 부정확하다고 밝힌다. 그래서 `data.timing.ts_source = "canoe"`, 시각은 브리지가 COM 이벤트·폴링에서 관측한 호스트 시각 + `ts_quality: com_event|polled`(브리지 초안 §2)이고, 이것을 wire 시각이나 P2 실측으로 표시하지 않는다(`p2_ms` 대신 `rtt_host_ms`). 0x78 중간 응답이 COM으로 보이지 않으면 행의 `pending ×N` 카운터 대신 "pending 정보 없음(canoe)"을 표시한다. 응답→뷰어 지연 목표(§5.4)는 canoe 경로에서 COM 오버헤드를 P4(구 M2c)에서 **측정해 따로 게시**한다(미측정).
- **TesterPresent**: COM `DiagStartTesterPresent()`/`DiagStopTesterPresent()`/`TesterPresentStatus`가 있다(확인). 그래서 `canoe` 경로에서도 TP는 **Loom 잡(A1)으로 소유**하되 3E 프레임을 직접 보내지 않고 CANoe TP를 켜고 끈다. STOP latch·abort·유휴 만료 → `DiagStopTesterPresent()`. 상태 바 `TP: canoe(on/off)`. TP 프레임 하나하나는 Loom 이벤트로 받지 못할 수 있어(미확인) 행 대신 상태 표시만 하고, 그 사실을 detail에 적는다. Loom 자체 3E와 CANoe 자동 TP를 동시에 돌리지 않는다.
- **seed-key**: 기본은 Loom 플러그인(서브프로세스)으로 key를 계산한다. CANoe 진단 기술에 설정된 seed-key DLL은 CAPL `DiagGenerateKeyFromSeed`가 쓰고, COM `Application.CAPL.GetFunction`으로 사용자 CAPL 함수를 부를 수 있다(확인). 그래서 "CANoe 키 계산" 경로 = **사용자 CAPL 래퍼가 key만 계산해 돌려주고, Loom이 `27 xx key` 바이트를 만들어 R3 뷰어 승인 → raw 송신**(바이트 관문 유지, key는 기록에서 마스킹). CAPL 함수 인자·배열 전달은 추정·실기 미검증(P4(구 M2c) 확인). unlock 전체를 CAPL에 맡기는 경로는 쓰지 않는다(Loom이 바이트를 못 봄).
- **진단 DB 이름**: CANoe 진단 기술(CDD/ODX)의 DID·루틴 이름을 가져와 **표시용으로만** 쓴다(등급은 낮추지 않음, §6.4 `trusted_db` 규칙 그대로). COM 이름 열거 API는 미확인. CDD는 XML이고 공개 파서(cantools)가 DIAGCLASS/DIAGINST에서 식별자·이름을 읽는 것은 확인됨 → 1순위 = **CDD 파일 읽기 전용 이름 import**(CANoe가 쓰는 변형과 일치하는지 P4(구 M2c)에서 검증), 2순위 = CANdelaStudio ODX export → odxtools(M4).
- **Vector 이더넷과 자체 DoIP**: CANoe는 DoIP에 Windows OS TCP/IP 스택(NIC)과 자체 TCP/IP 스택(VN56xx 등)을 구별해 쓴다(확인). ISO 13400/AUTOSAR 규칙상 같은 테스터 SA로 다른 TCP 연결이 활성화를 요청하면 기존 연결에 alive check를 보내고, 기존이 응답하면 새 요청은 0x03으로 거부된다. 다른 SA는 게이트웨이 연결 자리가 있으면 진행하고 자리가 없으면 0x01이다(확인). 두 테스터가 함께 활성화돼도 ECU 세션·보안 상태는 공유되므로 독립 세션이 아니다(리스크 16). 그래서 CANoe 측정 중인 Windows의 기본은 `canoe`이고, 자체 DoIP를 같은 게이트웨이에 쓰려면 다른 테스터 SA를 명시하고 사용자가 확인해야 한다.

**사용자가 무엇을 언제 보는가 (02 원칙)**

| 장면 | Loom 뷰어(`loom uds view`, 웹) | CANoe |
|---|---|---|
| 에이전트가 `canoe` 경로로 요청 | `queued → tx → rx/timeout` 행이 publish-before-reply로 즉시(같은 `request_id`), 행·상태 바에 `via canoe` | CANoe 트레이스·진단 창에 같은 요청/응답(CANoe 표시 규칙) |
| R2/R3 승인 | Loom 승인 패널(CANoe에는 Loom 승인 UI 없음) | 승인 전에는 아무것도 송신되지 않음 |
| 사용자가 CANoe UI에서 직접 진단 요청 | Loom이 보낸 것이 아니므로 UDS 행은 없다. Loom이 같은 CAN 버스를 관찰하는 경우에만 CAN 트레이스에 `external:canoe` 프레임으로 보인다(DoIP over Vector 이더넷이면 보이지 않을 수 있음 — 화면에 그렇게 적는다). UDS는 예상 밖 응답을 보면 "다른 테스터 활동" 경고 + `session_confidence=stale` | 평소대로 |
| 측정 정지·CANoe 종료 | 상태 바 `canoe: stopped`, 대상 link down 행, 진행 중 쓰기 `⌛?`(outcome_unknown) | — |
| 사용자 개입 | Loom 뷰어 `:` 송신·`T`·`!`·STOP 모두 같은 파이프라인(경로만 `canoe`) | CANoe에서 측정을 멈추는 것도 개입이지만 Loom 승인·감사 밖 — link down으로만 반영 |

### 3.8 큐·메모리 예산과 넘침 정책 (v2.0, 마스터 C2)

원칙: 모든 큐에 개수와 바이트 상한을 두고 먼저 닿는 쪽을 적용한다. unbounded channel·Vec 누적은 금지한다. 이벤트 원본은 CORE 디스크 저널로 보내고, 화면과 trace 조회는 cursor로 읽는다. 아래 수치는 **초기 예산**(실측 전)이고 CORE 큐 타입으로 표기한다(`_xteam/uds-core.md` C-8).

| 항목 | 초기 상한 | 넘칠 때 |
|---|---|---|
| 대상·요청 큐 | 물리 ECU 64, 대상별 32·전역 512·8 MiB, 대상당 진행 중 1 | 송신 전 exit 6 `E_LOCKED(reason=queue_full)`. 큐 대기도 총 deadline에 포함. 사용자 입력·STOP용 용량은 예약. 폴링 tick은 skipped로 집계하고 밀린 tick을 재생하지 않음 |
| DoIP 수신·송신 | 연결 32, 연결당 64 KiB, UDS PDU ≤ 4095 B | 헤더 길이를 **할당 전** 검사, 초과는 연결 종료 + exit 7. 송신 버퍼가 차면 새 요청 거부. 수신 task는 느린 UI·저널 때문에 멈추지 않음(alive check 우선) |
| 이벤트 발행 | 제어 예약 256개/1 MiB + 데이터 4096개/4 MiB | CORE 저널 경로. 데이터 큐가 차면 **새 송신 수용을 멈춘다**(agent·script는 exit 4 `E_RECORDING_FAILED`, A12). 사용자 입력은 예약 경로로 계속 허용. 수신·alive 처리는 멈추지 않는다. UI 표시만 집계·gap 가능 |
| 폴링 샘플 | job 64, 고정 링 4096개 또는 1 MiB | 오래된 표시 샘플 덮어쓰기 + 집계 수 표시, 원본은 저널. 겹친 요청은 tick skip(동시성 증가로 보상하지 않음) |
| 0x78 대기 | 트랜잭션당 카운터 + 마지막 상태만, 기본 50회·hard cap 1024 | 총 deadline이나 횟수에 먼저 닿으면 exit 7(쓰기 `outcome_unknown`) + 미해결 집합 U에 등록 |
| 늦은 응답 격리 U | 대상별 SID ≤ 256/64 KiB, 전역 ≤ 4 MiB | 용량이 부족하면 새 대상·요청 거부. **LRU·TTL로 격리를 풀지 않는다**(D19 증거로만 해제) |
| trace 조회 | 페이지 32 KiB, 응답 64 KiB, 동시 8, 캐시 2 MiB | 저널 streaming. 한계는 exit 8 + 다음 cursor, 동시 초과는 busy |
| 진단 DB | 정규화 모델 8 MiB, XML 깊이 64, PDX 파일 1024개·총 32 MiB | exit 2 `E_DB`. importer는 별도 helper(64 MiB·5 s), 잘린 모델을 활성 DB로 쓰지 않음 |
| 트랜잭션 표시 링 | 대상당 10k 행 또는 256 KiB, 전역 8 MiB(먼저 닿는 쪽) | 오래된 표시 행부터 회수, 과거는 저널 조회 |

- **UDS 메모리 예산 64 MiB**(위 큐·DB·task 포함, CORE 할당 계측으로 판정). 초과 전에 새 job·대상을 거부하고 캐시를 회수한다. U와 확정 상태는 보존한다. gateway 전체 RSS soft 96 / hard 128 MiB는 CORE 검토용 제안이다.
- helper 예산: seed-key helper 64 MiB·2 s. 선택 CANoe Python worker는 128 MiB. 초과나 크래시가 나면 그 프로세스만 종료하고 세대를 폐기한다. 영향받은 요청은 `outcome_unknown` 처리하고 U에 넣는다. 재기동 후 승인·쓰기를 자동 재생하지 않는다.
- **24 h soak의 UDS 부하**(CAN FD 고부하 + UART flood와 동시):
  - 스트레스 프로필: 가상 대상 64개 × 20 Hz 폴링
  - 정상 구간: 100 tx/s
  - 반복 주입: 0x78 장기·상한, 무응답·반복 timeout으로 U 포화, SID 교대와 늦은 0x78·최종 응답, 연결 flap·세대 교체, 느린 웹 UI·trace 조회, 저널 정지·디스크 가득 참, seed helper 크래시
  - 관문 초안: 30분 warmup 뒤 전체 RSS 증가 ≤ 16 MiB/24 h(기울기 ≤ 0.5 MiB/h), UDS 할당 ≤ 64 MiB, worker 사망에도 데몬·다른 장치·저널 생존, 정상 구간 장치 이벤트→웹 렌더 p95 ≤ 100 / p99 ≤ 200 ms, 부하 구간 gap·집계·LAG 명시, 사용자 개입·STOP 보장

---

## 4. CLI / API 명세 초안

### 4.1 공통

- 형식: `loom uds <verb> [args] [-t <target>] [--json] [--timeout <dur>] [--dry-run]`.
- `-t` 생략 시 대상이 하나면 그것, 둘 이상이면 오류(묵시적 선택 금지).
- 숫자: DID/RID/SID 맥락에서 `F190`, `0xF190` 모두 16진수.
- **출력 상한**: 사람용 기본 40행(공통 하드 상한 1,000행 / 64 KiB), 바이트 덤프 64 B(`--full`로 확장). 초과 시 `meta.truncated: true` + `meta.next_cursor`.
- **client id와 커서**: CLI는 호출마다 새 프로세스이므로 client id = 환경변수 `LOOM_CLIENT`(마스터 D5; 없으면 부모 셸 세션 + 사용자 이름으로 만든 안정 id). 스킬·`agent-help`가 첫 단계로 설정을 안내. 데몬은 client id별 마지막 본 `seq`를 보관(TTL 24 h) → `--since @last`로 "내 마지막 호출 이후 다른 주체가 한 일"을 경계 있게 조회(커서 문법 `@last | seq:N | mark:X | -5s | HH:MM:SS.mmm | ISO8601`). **모든 결과**에 `others_since_last{count, origins, from_cursor}`(from_cursor = `seq:N`)를 붙이고 사람용 출력은 `NOTE:` 줄로 보인다. 커서는 호출마다 전진하므로, 알림을 받은 에이전트는 `trace --since <from_cursor>`로 그 구간을 조회한다(`--since @last`는 "직전 호출 이후"라 이미 지나친 구간을 보지 못할 수 있음). 0이 아닐 때만 `trace`를 부르면 된다.
- 정책값 `r2_path`(`auto | token_or_viewer | viewer`, 마스터 A8)는 허용 경로이고, 결과의 `data.approval.channel`(token | viewer)은 이번 요청에 실제로 열린 경로다.
- `--json`은 공통 봉투(마스터 D5): `{schema_version, ok, cmd, data, meta{env, truncated, next_cursor, range, display_gap, record_health}, warnings[], others_since_last, error{code, message, hint, retryable, request_id, detail}}`. `meta.env`(`sim|bench|vehicle`)는 모든 결과에 상시(마스터 B4 — 사람용 출력도 첫 줄에 `[VEHICLE]` 등). UDS 고유 필드는 `data` 아래: `target, backend(doip|cantp|canoe), request_id, write_state, timing{p2_ms, rtt_ms, pending_count, pending_total_ms, ts_source}, values, state, may_have_executed(= outcome_unknown), approval{approval_id, channel: token|viewer, token?, expires}`(exit 5일 때 — `approval_id`는 두 경로 공통 대기 식별자, `token`은 token 경로의 `confirm` 재제출용). NRC는 `error.detail{nrc, nrc_name, meaning}`. 뷰어가 0개면 `warnings`에 `no_viewer_attached`.
- 종료 코드(마스터 D5 + A6, UDS 매핑):

| 코드 | 의미 | UDS `error.code` 예 |
|---|---|---|
| 0 | 성공(긍정 응답) | |
| 1 | 내부 오류 | |
| 2 | 사용법·설정·DB 오류 | `E_USAGE`, `E_AMBIGUOUS_TARGET`, `E_DB` |
| 3 | **조건 대기형** 명령의 timeout만 | `approval wait`·`wait-for`·`poll --until` 만료 `E_TIMEOUT` |
| 4 | 정책 거부(사용자 하드 차단·`immutable_deny`·`default_blocked`·레이트·기록/감사 불능) | `E_DENIED`, `E_UNSUPPORTED`(0x2A/0x86), `E_RATE_LIMIT`, `E_RECORDING_FAILED` |
| 5 | 확인·승인 필요 / 승인 거부·만료 | `E_CONFIRM_REQUIRED`(R2 토큰), `E_APPROVAL_REQUIRED`, `E_NO_APPROVER`(뷰어 없음) |
| 6 | 사용자·락 충돌 | `E_LOCKED`, `E_QUARANTINE`, `E_USER_ACTIVE`, `E_INTERRUPTED_BY_USER` |
| 7 | 전송·장치 오류, **ECU 무응답(P2/P2*/완료 초과)** — 쓰기는 `outcome_unknown` | `E_NO_RESPONSE`, `E_DISCONNECTED`, `E_NOT_OPEN`, `E_ACTIVATION_DENIED` |
| 8 | 부분 결과 | `E_CURSOR_GAP`, `E_SCAN_LIMIT`(`--scan`) |
| 9 | 데몬 연결 불가 | `E_DAEMON` |
| 10 | ECU 부정 응답(NRC) — 0x78은 최종 결과가 아님 | `E_NRC` + `detail.nrc` |

### 4.2 명령 체계

| 명령 | 예 | 서비스 | 기본 등급 | M |
|---|---|---|---|---|
| `discover` | `discover --doip [--iface en7] --timeout 2s --max-results 100` / `--can ch0 --functional --rx-range 0x7E8-0x7EF` | DoIP 0x0001/0x0004, CAN 기능 주소 | R0 | M1(doip)/M3b(can) |
| `connect` | `--sim <profile>` (M1), `--doip IP --la 0x1001 [--sa 0x0E80] [--activation 0x00]` (M1 sim 게이트웨이·M4 실HW), `--can ch0 --tx 0x7E0 --rx 0x7E8 [--addressing normal_11]` (M1b), `--fd`·extended/mixed·29bit (M3b), `--tls --ca F --server-name N` (M4/요구 장비 관문); `--name` | 전송 | R0 | M1/M1b/M3b/M4 |
| `disconnect`, `targets`, `status [--probe]` | probe = 22 F186(가상 ECU 지원, 실ECU 미지원 시 `unsupported` 표시하고 assumed 유지) | — | R0 | M1 |
| `read-did` | `read-did F190 F18C [--raw]`, `--scan F100-F1FF`(M3) | 0x22 | R0 | M1 |
| `write-did` | `--value "TESTER01"` / `--hex 54455354` (M1), `--field region=EU`(struct DB, M3) | 0x2E | R2 (DB protected·식별 DID·DB 없는 쓰기 R3) | M1 |
| `confirm` | `confirm <token>` — R2 토큰 재제출(호스트 `ask` 규칙 `Bash(loom * confirm:*)` 대상) | — | — | M1 |
| `session` | `default\|extended\|programming\|0x40` | 0x10 | 01/03 R1, 02·기타 R3 | M1 |
| `raw` | `raw 22F190 [--no-wait]` (M1), `--functional`(M3b, 정책 전송 범위 필요) | 임의 | 바이트 분류(§10.3) | M1 |
| `daemon` | `loom daemon start\|stop\|status\|logs` (core 소유, CLI 자동 기동) | — | — | M1 |
| `trace` | `[--tail 20] [--since @last\|seq:N\|-30s] [--service 0x22] [--nrc] [--origin user]` | — | — | M1 |
| `view` | `view [-t ecu1] [--compact]` (사용자용 TUI) | — | — | M1 |
| `abort` | `abort [-t ecu1]` — UDS 대상 abort(전 도메인 STOP은 core `loom stop-all`) | — | — | M1 |
| `lock` | `lock acquire\|release [--exclusive]` | — | — | M1(최소)/M2 |
| `dtc list` / `count\|snapshot\|ext\|supported` | `dtc list --mask 0x09 --limit 50`, `dtc snapshot B1234-00 --record 01` | 0x19 02/01/04/06/0A | R0 | M1(list)/M2 |
| `dtc clear` | `dtc clear [--group FFFFFF]` | 0x14 | R2 | M2 |
| `tp` | `tp on\|off [--period 2s]` | 0x3E | R0 | M2 |
| `security` | `security --level 1 [--plugin oem_bcm]` | 0x27 | seed R1, key **R3**(마스터 D6) | M2 |
| `reset` | `reset hard\|soft\|key-off-on` | 0x11 | R2 | M2 |
| `routine` | `routine start 0x0203 [--data 01]` / `stop` / `result` | 0x31 | DB 등급(기본표보다 낮출 수 없음), 미등록 R3 | M3 |
| `io` | `io 0x4F01 adjust --value 50` / `freeze` / `default` / `return` | 0x2F | 01/02/03 R3, 00 return R1 | M3 |
| `comm` | `comm enable` / `comm disable-tx --type normal` | 0x28 | enable R1, disable R2 | M3 |
| `dtc-setting` | `dtc-setting on\|off` | 0x85 | on R1, off R2 | M3 |
| `poll` | `poll F40D,F405 --period 200ms --duration 60s [--viz] [--out f.jsonl]` / `poll stats\|stop <job>` | 0x22 반복 | R0(레이트) | M3 |
| `run` | core `loom run scenario.yaml [--plan]`(마스터 D12) — UDS는 `uds.*` 단계 어댑터 | 시나리오 | 단계 최대 | M3 |
| `approval wait` | `approval wait <approval_id> --timeout 120s` (뷰어 승인 대기, 만료 시 exit 3) | — | — | M1 |
| `db` | `db load bcm.yaml -t bcm` / `db show F190` / `db validate f.yaml` | — | — | M1 |
| `audit`, `policy show` | core 공통(정책 완화는 뷰어 승인 후 적용) | — | — | M2 |
| `agent-help` | 스킬이 없는 에이전트용 규칙 요약 출력 | — | — | M1 |
| `flash` | **비범위** — 존재하지 않음, 안내 메시지 | 0x34–0x37 | R4 `default_blocked` | M5(선택) |

### 4.3 출력 예시

**read-did (사람용 / `--json`)**
```
$ loom uds read-did F190 F40D -t ecm
ecm  22 F190 F40D → 62 F190 … F40D …  p2 9 ms  rtt 18 ms
F190  VIN           = "WVWZZZ1KZAW000001"
F40D  VehicleSpeed  = 0 km/h   (raw 0x00)
```
```json
{"schema_version":1,"ok":true,"cmd":"uds.read-did",
 "data":{"target":"ecm","request_id":"t-000418","request":"22F190F40D","response":"62F190575657…F40D00",
   "timing":{"p2_ms":9.1,"rtt_ms":18.2,"pending_count":0,"pending_total_ms":0,"ts_source":"host"},
   "values":[{"did":"F190","name":"VIN","value":"WVWZZZ1KZAW000001","unit":null,"raw":"5756575A…"},
             {"did":"F40D","name":"VehicleSpeed","value":0,"unit":"km/h","raw":"00"}],
   "state":{"session":"default","session_confidence":"assumed","security":"locked"},"may_have_executed":false},
 "meta":{"truncated":false,"next_cursor":"seq:9312","record_health":"ok"},"warnings":[],
 "others_since_last":{"count":0,"origins":[],"from_cursor":"seq:9311"}}
```

**NRC** (축약 예시 — §4.1 필수 필드 일부와 hex 말줄임은 예시 전용)
```json
{"ok":false,"cmd":"uds.write-did","data":{"target":"bcm","request":"2EF19854455354","response":"7F2E33","write_state":"failed"},
 "error":{"code":"E_NRC","message":"securityAccessDenied","request_id":"t-000502",
          "detail":{"nrc":"0x33","nrc_name":"securityAccessDenied","meaning":"보안 접근이 해제되지 않음"},
          "hint":"loom uds security --level <n> 후 재시도 (R3 뷰어 승인 필요)","retryable":false}}   ← exit 10
```
`7F 27 37`(requiredTimeDelayNotExpired)에는 **남은 시간 필드가 없다**. 표시는 "ECU 지연 시간 미상 — 로컬 정책상 재시도 금지 시각 hh:mm:ss"로 하고 ECU 카운트다운을 아는 것처럼 쓰지 않는다.

**0x78 pending (TTY일 때만 한 줄 갱신)**
```
$ loom uds routine start 0xFF01 -t bcm
bcm  31 01 FF01 → 7F 31 78 ×4 (pending 3.9 s, P2* 5000 ms) → 71 01 FF01 00  rtt 4.12 s
routine FF01 checkProgrammingDependencies: started, status 0x00 (ok)
```

**dtc list** — 원시 24bit DTC는 항상 유지. `19 02` 응답 자체에는 DTCFormatIdentifier가 **없다**(R4 확인: udsoncan ReadDTCInformation 소스). P/C/B/U 표기는 앞선 `19 01` 등 형식 포함 응답이나 검증된 대상 프로필에서 SAE J2012 계열임을 확보한 경우에만 쓰고, 미확인이면 raw 24bit만 표시. 아래 예시는 프로필이 J2012 형식을 선언했다고 가정.
```
$ loom uds dtc list -t bcm --mask 0x09
bcm  19 02 09 → 59 02 FF …  3 DTC  rtt 21 ms
DTC        raw       status TF TFTOC PD CD TNCSLC TFSLC TNCTOC WIR  description
B1234-00   0x923400  0x09   1  0     0  1  0      0     0      0    Door lock motor circuit open
U0100-00   0xC10000  0x2F   1  1     1  1  0      1     0      0    Lost comm with ECM
C0561-71   0x456171  0x08   0  0     0  1  0      0     0      0    ESC system disabled
(3 of 3)
```

**status**
```
$ loom uds status -t bcm
bcm  DoIP 192.168.0.10 tester 0x0E80 → 0x1001   link connected 00:12:40  epoch 1
session  extended (confirmed 12:05:11)   security L1 unlocked (12:05:14)
TP       on 2.0 s (last ok 0.6 s ago)    P2 50 ms / P2* 5000 ms (profile)
lease    agent:claude-1 shared, ttl 104 s
policy   bench · whitelist ✓ · auto ≤ R1 · rate 5/s
queue    0  inflight 0     viewers 1 (tui)   rec ● ok
```

**dry-run**
```
$ loom uds write-did 0x0101 --field region=EU -t bcm --dry-run
DRY-RUN (전송 안 함)
  request   2E 0101 02 1F      (DB VariantCoding: region=EU(0x02), features=0x1F 유지)
  current   0101 = region=KR(0x05) features=0x1F   (cache 12:06:02, 재확인 권장)
  grade     R3  (DB protected)
  needs     session extended ✓, security L1 ✓
  policy    뷰어 승인 필요
  rate      write-did 2/10 per min
```

### 4.4 Python API (데몬 클라이언트 — 스크립트·테스트용)
```python
from loom.client import Loom
lm = Loom.connect()                     # 데몬 토큰 파일 자동
bcm = lm.uds.target("bcm")
r = bcm.read_did(0xF190)                # UdsResult(values, timing, raw…)
res = bcm.write_did(0xF198, "TESTER01")   # bench token_or_viewer면 exit-5 상당(E_CONFIRM_REQUIRED): res.data.approval.token
lm.uds.confirm(res.data.approval.token)         # uds.confirm — 같은 가드·같은 정책
```
데몬 RPC 래퍼일 뿐, ECU 직결 경로는 제공하지 않는다. Python API 연결은 agent role이므로 origin=agent(마스터 D2). script origin은 core 러너(`loom run`) 전용이며 계획 승인 완화도 러너에만 적용.

---

## 5. 사용자 뷰어 UX

### 5.0 v2.0: 1차 화면 = Loom UI 앱 UDS 콘솔 (마스터 C6)

UI 팀이 셸(STOP·승인 대기열·env 배지·LIVE/LAG/STALLED·gap 배너·기록 탐색·테스트 실행 화면)과 패널 프레임워크·viz를 소유한다. UDS는 **패널 내용 명세와 데이터 질의**를 소유한다. 전체 명세는 `_xteam/uds-ui.md`(정본 동기화). 모든 패널은 `core.subscribe{domain="uds"}` 하나를 구독한다(snapshot + `after_seq`). UI 연결은 viewer role이므로 패널에서 나간 요청은 origin=user이며 에이전트와 **같은 쓰기 파이프라인**을 지난다. 행 키는 `request_id`이고, 같은 행을 `guard → queued → tx → pending×N → rx|timeout → verify`로 갱신한다.

| ID | 패널 | 내용 | 사용자 개입 | M |
|---|---|---|---|---|
| U1 | 트랜잭션 표 | 시각·origin(A/U/S/·)·대상·경로·서비스·디코드 요약·결과 글리프·p2/rtt·pending 수·`write_state`·등급, 상세 서랍(hex·디코드 트리·NRC 의미·approval_ref·ISO-TP 프레임 링크) | 필터, 행 선택, TP 접기 | M1 |
| U2 | 대상 상태 바 | link·epoch·backend·세션(confidence)·보안·TP·P2/P2*·lease·env 배지·active_overrides·큐·record.health | `T`/`R`/`!` | M1 |
| U3 | 요청 입력·미리보기 | 입력이 150 ms 멈추면 `uds.request{dry_run}` → 바이트·서비스·DID 이름·등급·사유·필요 세션/보안·현재값 diff | R0–R1 Enter, R2+ Enter 두 번, R3는 셸 승인 카드 | M1 |
| U4 | 승인 카드 내용 | `approval_binding`이 채우는 `approval.display`(대상·세션/보안·req_hex·디코드·diff·등급 사유·scope·만료·요청자·stale 경고), 토큰 원문 미표시 | 셸 규칙: 포커스 시 `Y`/`N`, R3 끝 2자, 1 s 입력 무시 | M1(R2) |
| U5 | DID 값/플롯 | 모든 `uds.rx`의 마지막 값 표 + 폴링 시계열(viz) | 지금 읽기, 플롯에 추가 | M1(값 표), M3(플롯) |
| U6 | DTC 표 | 최근 `19 xx` 결과·마스크·complete·clear 전후 diff | 다시 읽기, 삭제(R2) | M2 |
| U7 | 대상 목록·연결 | `uds.targets`·`uds.discover`·정책 등록 여부 | 연결 폼(sim/DoIP/CAN), 미등록 대상 확인 | M2 |
| U8 | 복구 송신 | active_overrides + `uds.recover` 진행 단계 | 1키(user 전용, STOP 중 safe_state만) | M2 |
| U9 | 폴링 job | `core.job.*` + `uds.poll.stats` | 시작 폼, 에이전트 job 정지 | M3 |

**지연 목표**(정상 부하, 같은 호스트): ECU 응답 → U1 확정 렌더 **p95 ≤ 100 / p99 ≤ 200 ms**, `uds.tx` → 진행 중 행 p95 ≤ 50 ms, 사용자 Enter → `uds.tx` p95 ≤ 50 ms, 폴링 샘플 → 플롯 점 p95 ≤ 250 ms, 미리보기 p95 ≤ 150 ms, 늦은 합류(snapshot 100행) ≤ 1 s. 측정은 데몬 `ts.wall_ns` 대 브라우저 `performance.mark('uds-row', request_id)`이고, 시계 차이는 CORE 시간 동기 RPC로 보정한다.

- **고부하·늦은 합류·숨김 없음**: §5.3–§5.5 규칙을 그대로 쓴다(폴링 그룹 1행 ×N, 0x78 같은 행 카운터, `aggregate`는 데몬이 만들고 UI는 임의로 합치지 않음, `display.gap` 배너, 최종 결과·승인·STOP은 우선 제어 큐).
- **에이전트 연동**: U1 상단에 "에이전트 claude-1 확인 위치 seq:N"을 표시해, 사용자가 자기 개입을 에이전트가 이미 봤는지 알 수 있게 한다(`others_since_last`).
- **키보드만으로 조작**: 모든 동작에 키를 둔다(TUI와 같은 키). 색만으로 상태를 전하지 않는다. `aria-live`는 승인·STOP·NRC만 읽는다.
- **보안**: 에이전트가 웹 viewer 자격을 얻으면 브라우저 자동화로 스스로 승인할 수 있다. 그래서 bootstrap URL은 대화형 TTY·데스크톱 셸에서만 발급하고(CORE·UI 요청), 스킬은 Loom UI에 브라우저 도구를 쓰는 것을 금지한다.
- **TUI(보조)**: U1 축약·U2·`:` 송신·`T/R/!`·STOP·승인 카드·복구 1키·gap/LAG만 남긴다(Rust TUI 키트는 CORE/UI 결정). M1 수용에서 빠지고 M2 smoke만 한다. 데이터 계약이 같으므로 별도 경로는 없다.
- 아래 §5.1–§5.5는 v1 TUI 기준 기록이다. 화면 배치는 위 패널이 대체하고, 지연·고부하·늦은 합류 규칙은 웹에도 그대로 적용한다.

### 5.1 결정
- **M1 1차 화면 = TUI `loom uds view`** (herdr 패인). UDS 트래픽은 저속이라 TUI로 충분하고 의존성이 적다. 프레임워크는 Textual 8.2.8(MIT, Python ≥3.9 — PyPI 확인) — CAN/UART 팀과 통일(§15).
- 웹은 viz V-UDS-1/2/4로 M3부터(CAN 팀 렌더러, 같은 이벤트 토픽 구독, 같은 사용자 입력 RPC).

### 5.2 메인 화면

```
┌ loom uds view ────────────────────────────────────────────────────────────────────────┐
│ bcm ● DoIP 0x1001  sess=extended✓ sec=L1🔓 TP 2.0s ✓×143  P2 50/5000  lease: agent  BENCH│
│ ecm ● CAN 7E0/7E8  sess=default?  sec=🔒   TP off          P2 50/5000  lease: free   BENCH│
├───────────────────────────────────────────────────────────────────────────────────────┤
│ time          o ecu service                 summary                        res   p2/rtt │
│ 12:05:11.020  A bcm DiagSessionControl      → extended (P2 50, P2* 5000)   ✓     9/9ms │
│ 12:05:11.031  · bcm TesterPresent           keepalive 시작 (접힘 ×143)                  │
│ 12:05:13.880  A bcm SecurityAccess          L1 seed→key [plugin oem_bcm]   ✓    31ms   │
│ 12:05:20.402  A bcm ReadDataByIdentifier    0101 VariantCoding region=KR   ✓    12ms   │
│ 12:05:31.114  U ecm ReadDataByIdentifier    F40D VehicleSpeed=0 km/h       ✓    16ms   │
│ 12:05:40.950  A bcm RoutineControl          start FF01 ⏳ pending ×3  3.1s/5.0s         │
│ 12:05:44.002  A bcm WriteDataByIdentifier   0101 region KR→EU   ⚠R3 승인 대기           │
├─ detail (Enter) ──────────────────────────────────────────────────────────────────────┤
│ txn t-000431  A claude-1  grade R3 (DB protected)  policy: viewer approval             │
│ req 2E 01 01 02 1F   diff region KR(0x05)→EU(0x02), features 0x1F (unchanged)          │
├─ APPROVAL ────────────────────────────────────────────────────────────────────────────┤
│ ⚠ R3 agent claude-1 → bcm WriteDataByIdentifier 0101 region KR→EU   승인 코드 끝 2자 [__]│
│      bench · extended · L1 unlocked(by claude-1 12:05:13) · expires 98 s   [Y]/[N]     │
├───────────────────────────────────────────────────────────────────────────────────────┤
│ : 22 F190_                        live · lag 9 ms · seq 9402 · rec ● uds-20261109.jsonl │
│   └ ReadDataByIdentifier F190 VIN · R0                                                 │
│ [:]send [T]akeover [R]elease [!]abort [^X]STOP [s]top poll [/]filter [␣]pause [k]a [d]tc [?]│
└───────────────────────────────────────────────────────────────────────────────────────┘
```

- **상태 바**: 연결, 세션(✓ confirmed / ? assumed / ! stale), 보안, TP 카운터, P2/P2*, lease 소유자, `BENCH`/`VEHICLE`/`SIM` 배지(항상 표시 — vehicle도 규칙은 bench와 같고 배지만 다르다, 마스터 B4), 경로 `via doip|cantp|canoe`, `canoe` 경로면 측정 상태(`canoe: running|stopped`)와 `TP: canoe`.
- **origin 열**: `A` agent(이름은 detail), `U` user, `S` script, `·` keepalive(purpose=tester_present). ECU 응답(origin=device)은 요청 행에 합쳐 확정. 색: A 청록, U 노랑, S 보라, NRC 빨강.
- **결과 열**: `✓`, `✗ 0x31 requestOutOfRange`, `⏳` 진행 중, `⌛` timeout, `⌛?` 실행 불확정(may_have_executed), `⛔` 가드 거부, `⚠R2/R3` 승인 대기.
- **p2/rtt 열**: P2 판정은 `p2_ms`(첫 응답 시작) 기준 색, rtt는 별도. pending 중에는 경과/한도가 실시간 증가.
- **NRC 해석**: detail에 이름·의미·흔한 원인·다음 행동(예: 0x22 → 차속/엔진/전압 조건 확인, 0x37 → "보안 지연 중, 반복 시도 금지").
- **명령줄 `:`**: `22 F190`(raw hex), `read F190`, `session ext`, `dtc`, `? 2E 0101 021F`(dry-run). 대상은 선택된 상태 바 행. 입력 중 실시간 디코드 미리보기 줄(서비스·DID 이름·등급), R2 이상은 Enter 두 번.
- **승인 키 안전 규칙**: 승인은 **패널에 포커스가 있을 때 대문자 `Y`/`N`** 으로만. R3는 승인 코드 끝 2자 입력 필수. 패널이 나타난 뒤 1 s 동안 입력 무시. 명령줄 입력 중에는 승인 키 비활성. 대기 승인이 여럿이면 목록에서 선택.
- **공통 키(마스터 D9/A3)**: `Ctrl-X`·`Ctrl-] x` = 전 도메인 STOP(`loom stop-all` — agent·script 쓰기와 잡 latch, 사용자 입력은 계속 가능, 출력 상태는 유지되며 "STOP — 출력 유지 중" 배지), `/` 필터, `Space` 일시정지, `q` 종료. UDS 전용 `!` = 선택 대상 abort.
- **보조 화면**: `d` DTC 표(상태 비트 분해), `p` 폴링 패널(마지막 값·스파크라인·샘플/스킵 카운터, `o`로 viz 웹), `n` run 패널(core 러너).
- **좁은 패인**: 80열 미만이면 자동 `--compact` — 대상이 하나면 ecu 열 생략, 시각 `HH:MM:SS.s`, `p2/rtt` 한 열, origin+결과 2글자, 승인은 전체 화면 모달. 이모지 폭 문제(Windows conhost 등)용 `--ascii`(L/U/…/T/!/X) 자동 감지.

### 5.3 늦은 합류·재접속
attach 시 core `core.subscribe{after_seq: null}` → 데몬이 원자적 snapshot(`snapshot_part n/N` → `snapshot_end{stream_seq}`, 마스터 A5 — UDS 대상 상태·최근 100행)을 주고 이어서 `seq > watermark` 이벤트를 보낸다. 재접속은 `after_seq=마지막 seq`. 이어지지 않으면 `gap` 행을 넣고 `trace.query`로 구간 재요청(채움 성공/실패 표시).

### 5.4 실시간성 목표 (정상 부하, 같은 호스트 monotonic 기준)

| 경로 | 목표 | 측정 |
|---|---|---|
| `uds.tx` 발행 → TUI "진행 중" 행 렌더 완료 | p95 ≤ 50 ms | Textual 렌더 훅 타임스탬프 |
| **ECU 응답 수신 → TUI 확정 행 렌더 완료** | **p95 ≤ 100 ms, p99 ≤ 200 ms** | 데몬 수신 ts(소켓 read / ISO-TP rx_complete) → 렌더 훅. 구간 분해: `ecu_rx → bus_emit → viewer_receive → render_complete` |
| ECU 응답 수신 → viz 웹 표 | p95 ≤ 200 ms | 브라우저 렌더 + clock offset 추정 |
| ECU 응답 수신 → 에이전트 CLI 결과 | ≤ 30 ms (데몬 처리) | RPC 반환 ts |
| 사용자 `!` → 큐 비움·새 송신 중단 | ≤ 100 ms | 진행 중 PDU는 완료 또는 N_*/P2 timeout까지(취소된 것처럼 표시 안 함) |
| 폴링 샘플 → 플롯 점 | ≤ 250 ms | viz |

"정상 부하" 정의: 대상 ≤ 20개, 합계 ≤ 100 트랜잭션/s, 뷰어 1–2개. 표본: 1,000 트랜잭션, p95/p99/max 모두 보관. 공유 CI의 지터는 넓은 임계값으로만 판정하고 성능 수용은 전용 runner에서.

### 5.5 고부하에서 멈추거나 거짓말하지 않는 법

원칙: **기록은 버리지 않고(버리게 되면 그 사실을 기록), 화면은 집계·드롭을 숫자로 명시한다.** 렌더는 20 Hz 배치. 최종 결과·실패·승인·정지·gap 이벤트는 표시 큐에서도 우선 보존하고 집계하지 않는다.

| 부하 상황 | 화면 처리 | 표시 |
|---|---|---|
| 빠른 DID 폴링 | 폴링 그룹 1행: `poll#3 F40D,F405 ×1,204 last 42 km/h p2 12/15/31 ms NRC 0 skipped 3`, 1 s 갱신. 샘플은 플롯·기록에 전부 | `×N`, `집계` 배지, `scheduled/skipped/actual_period` |
| 연속 0x78 | 같은 행 카운터 `pending ×12` + 총 경과. 카운터 정체 시 마지막 pending 이후 경과를 빨강 | — |
| TesterPresent 반복 | 상태 바 `TP ✓×143 last 0.6 s`, 실패·NRC·스킵만 개별 행 | `k`로 펼침 |
| (시뮬레이터) 0x36 대량 | 진행 행 1개 `TransferData 368/1024 KB 12.3 KB/s`, ≤ 10 Hz | 블록별 기록은 전부 |
| 느린 뷰어 | core 구독 배치(≤ 40 ms·적응형 flush / 200건 / 64 KiB, 마스터 A5). 초과 시 그 구독자만 드롭 + `display.gap{first_seq,last_seq,count,cause}`, 최종 응답·승인·STOP은 우선 제어 큐(드롭 금지) | 배너 `⚠ 표시 드롭 1,234건 (seq 9100–10333)` + 기록 상태(완전/손실 구간) |
| seq 누락(재연결) | gap 행 삽입 후 재요청 | `── gap seq 9200–9260 (채움 완료) ──` |
| 스트림 끊김 | heartbeat 3회 미수신 → 화면 회색 + `STALLED 3.4 s` | 거짓 "live" 금지 |
| 렌더 뒤처짐 | 하단 `lag` 500 ms 초과 노랑, 2 s 초과 빨강 `LIVE-DEGRADED` | "연결됨"과 "최신"을 구분 |
| 기록 장애(디스크 full, writer 큐 고갈) | **agent·script의 새 R1+ 쓰기만 중단**(exit 4 `E_RECORDING_FAILED`, sim 포함 — 마스터 A12). 사용자 뷰어 입력은 허용(그 구간 감사는 데몬 로그), 수신·FC 처리는 계속, `record.gap` 기록 | 상태 바 `rec ✗`, `record.health=failed` 배너 |
| 플롯 다운샘플 | viz LTTB 등 | 범례 `1:8 downsampled` |

"기록은 완전" 문구는 writer가 해당 seq 구간의 기록 완료를 확인한 경우에만 쓴다.

---

## 6. 데이터 / 기록 포맷

### 6.1 이벤트 레코드 (공통 §3-6 확장, 라이브 스트림과 기록이 같은 스키마)

공통 레코드는 마스터 D3: `schema_version, session_id, domain, channel, kind, seq, ts{mono_ns, wall_ns, clock_id, source, quality}, dir(rx|tx|null), origin(agent|user|device|script), client_id, actor_id, request_id, job_id, epoch, generation, purpose, risk` + 두 층 순번(`producer_id/producer_epoch/source_seq`, `stream_epoch/stream_seq`). UDS는 enum을 늘리지 않는다.
- **write_state(마스터 A14 > D4)**: 요청 접수(코덱 완료) = `requested`, core 판정 통과·큐 진입(`queued` 이벤트) = `accepted`, 송신(드라이버·소켓에 넘김) = `sent`, 긍정 응답 = `confirmed`, 되읽기 일치 = `effective_verified` / 되읽기 불일치·불가 = `effect_unverified`, NRC = `failed`, timeout·abort 후 미확정 = `outcome_unknown`(= `may_have_executed: true`).

- 송신 이벤트: `dir=tx`, `origin`=요청 주체(agent/user/script). TesterPresent는 `origin=script, purpose=tester_present, actor_id=uds-keepalive`.
- 응답 이벤트: `dir=rx`, `origin=device`, `payload.request_origin`으로 송신 주체 연결.
- 관리 이벤트(queued, guard, lease, alert, timeout, suppressed, aggregate): `dir: null`. origin은 공통 enum만 — 요청에 묶인 이벤트는 요청 주체, 장치 판정 사건(`suppressed`)은 device, 데몬 집계(`aggregate`)는 script + `purpose=aggregate`. 손실은 core 이름 `display.gap`/`record.gap`/`source.gap{cause}`(마스터 A13 — UDS의 `source.gap`: ISO-TP rx 오류·DoIP 프레이밍 손상으로 장치 응답을 잃은 구간).

**트랜잭션 라이브 이벤트 스키마 (viz 렌더러 fixture용 확정안 — CAN 팀 요청)**. 모든 이벤트는 마스터 D3 공통 레코드 전체(위 필수 필드 + 두 층 순번, uint64는 JSON 문자열, `ts{mono_ns, wall_ns, clock_id, source, quality}`) + `domain="uds"`, `channel=<target>`, `payload`를 갖고, 같은 `request_id`가 한 행을 갱신한다.

| kind | dir / origin | payload 필수 필드 | 행 상태 |
|---|---|---|---|
| `guard` | null / 요청 주체 | `write_state: requested, decision: allow\|confirm\|approve\|deny, risk, reason, block_class?, approval_ref?` | 승인 대기 / 거부(⛔) |
| `queued` | null / 요청 주체 | `write_state: accepted, ecu, transport(doip\|cantp\|canoe), sid, service, req_hex, risk, queue_pos` | 대기 |
| `tx` | tx / 요청 주체 | `req_hex, write_state: sent, tx_state: driver_accepted\|bus_confirmed(CAN-TP), ts_source` | 진행 중(⏳) |
| `pending` | rx / device | `count, elapsed_ms, p2_star_ms` | pending ×N |
| `rx` | rx / device | `request_origin, write_state: confirmed\|failed, resp_hex, positive, nrc?, nrc_name?, decoded, timing{p2_ms, rtt_ms, pending_count, pending_total_ms}, state_after, may_have_executed=false` | 확정 ✓/✗ |
| `verify` | null / 요청 주체 | `write_state: effective_verified\|effect_unverified, readback_request_id` (되읽기 했을 때) | ✓✓ / ✓? |
| `timeout` | null / 요청 주체 | `write_state: outcome_unknown(쓰기)\|failed(읽기), stage: p2\|p2_star\|complete\|deadline, verdict: timeout\|uncertain, may_have_executed, timing` | ⌛ / ⌛? |
| `late` | rx / device | `resp_hex, quarantine: true, prev_request_id?` | 늦은 응답(기록만) |
| `suppressed` | null / device | `write_state: confirmed, ok_suppressed: true` | ✓(무응답 정상) |
| `aggregate` | null / script(`purpose=aggregate`) | `group: poll\|tp\|transfer, count, last, stats, skipped` | 집계 행 |

파생 레코드(`record_type: txn`)는 위 이벤트를 한 줄로 합친 것(export·파일 렌더용).

```json
{"schema_version":1,"session_id":"s-20261109-1200","domain":"uds","channel":"bcm","kind":"rx",
 "seq":"9311","producer_id":"gw","producer_epoch":"3","source_seq":"88120","stream_epoch":"1","stream_seq":"9311",
 "ts":{"mono_ns":"81234567890123","wall_ns":"1762689821526000000","clock_id":"host-boot-A","source":"host","quality":"ok"},
 "dir":"rx","origin":"device","client_id":"claude-1","actor_id":"claude-1","request_id":"t-000418","job_id":null,
 "epoch":1,"generation":1,"purpose":null,"risk":"R0",
 "payload":{"request_origin":"agent","write_state":"confirmed",
   "ecu":{"name":"bcm","transport":"doip","addr":"0x1001","tester":"0x0E80"},
   "sid":"0x22","service":"ReadDataByIdentifier","req_hex":"22F190","resp_hex":"62F190575657…",
   "positive":true,"nrc":null,"decoded":[{"did":"F190","name":"VIN","value":"WVWZZZ1KZAW000001"}],
   "timing":{"tx_done_ns":"81234553700000","rx_start_ns":"81234562800000","rx_complete_ns":"81234567800000",
             "p2_ms":9.1,"rtt_ms":14.1,"pending_count":0},
   "approval_ref":null,"may_have_executed":false,
   "state_after":{"session":"default","security":"locked"},"db_sha256":"…"}}
```

- **파생 트랜잭션 레코드**(`record_type: txn`): 요청/pending/응답을 한 줄로 묶어 viz 표·export용. 불완전 거래는 timeout/incomplete로 원문 seq 참조.
- CAN-TP 대상이면 CAN 트레이스의 ISO-TP 프레임(FF/CF/FC)에 같은 `request_id`를 주석으로 붙이도록 CAN 팀에 요청(V-UDS-5).
- **민감 데이터**: 0x27 key는 항상 마스킹(`<masked 4B sha256:ab12…>`), seed는 정책. 승인 토큰 원문은 기록하지 않고 `approval_ref`(ID·해시·승인 주체)만. VIN·DID 값은 **제품 기본값으로** 라이브 화면·로컬 기록에서 원문, 마스킹은 export/공유 시 옵션(`--redact vin,serial`). 02 원칙의 필수 조건은 "에이전트와 뷰어가 같은 표시 정책"이며, 사용자가 마스킹 정책을 켜면 양쪽에 동일하게 적용한다.

### 6.2 파일 배치 (제안, core가 최종 결정)
```
~/.loom/records/ (마스터 D7 — 보존 30일, 전체 20 GiB quota, 감사는 무기한)
  sessions/2026-11-09T12-00-00/
    uds.jsonl        # 원시 이벤트 전부 (TP 포함, 집계 없음), rotation + quota
    (CAN 저널 참조)  # ISO-TP 프레임은 CAN 팀 저널이 정본, BLF/ASC/MF4는 export 산출물(마스터 D7)
    manifest.json    # 패키지/드라이버 버전, 대상·주소·params, DB/정책 해시, clock 정보
  audit/uds-2026-11.jsonl      # 감사 로그 (해시 체인)
  poll/<job>.jsonl             # long format (viz 입력)
```
- 폴링 long format(V-UDS-2): `{"ts":…, "series":"ecm.F40D.VehicleSpeed", "value":42.0, "unit":"km/h", "quality":"valid", "seq":…, "request_id":…}`. 실패 샘플 `value:null, quality:"nrc", nrc:"0x22"`(0으로 그리지 않음). 문자열/enum은 상태 표로.
- export: `loom uds export <session> --format csv|jsonl|parquet [--redact …]`, pcapng(DoIP, 후순위).

### 6.3 감사 로그
- append-only JSONL, 레코드마다 `prev_hash`(SHA-256 체인) — 변조 **탐지**용(방지 아님).
- 기록: R1 이상 요청(허용/거부/취소/불확정), 승인 발급·응답(승인 채널: token-resubmit / viewer), 정책 로드(해시), lease 이동, abort, 레이트 발동, 분류 결과·사유.
- 감사는 core 소유(해시 체인, 끌 수 없음). 감사 불능 시 agent·script의 새 R1+ 쓰기만 중단(마스터 A12, §5.5).

### 6.4 진단 DB (Loom YAML 스키마)
```yaml
loom_diag: 1
ecu:
  name: bcm
  addressing:
    doip: {logical: 0x1001, gateway: 0x1000}
    can:  {channel: ch0, tx: 0x726, rx: 0x72E, addressing: normal_11}
  timing: {p2_ms: 50, p2_star_ms: 5000, s3_ms: 5000}     # 프로필 값
  security: [{level: 1, plugin: oem_bcm}]                # 플러그인 이름만
sessions: {default: 0x01, programming: 0x02, extended: 0x03, eol: 0x40}
dids:
  - {id: 0xF190, name: VIN, type: ascii, length: 17, access: {read: any}}
  - {id: 0xF40D, name: VehicleSpeed, type: uint8, scale: 1.0, offset: 0, unit: km/h}
  - id: 0x0101
    name: VariantCoding
    length: 2
    type: struct
    fields:
      - {name: region, bits: "0:8", type: enum, values: {0x01: US, 0x02: EU, 0x05: KR}}
      - {name: features, bits: "8:16", type: bitfield, flags: {0: drl, 1: auto_lock}}
    access: {read: any, write: {session: extended, security: 1}}
    protected: true            # 쓰기 R3로 상향
  - {id: 0x2010, name: BatteryVoltage, type: uint16, endian: big, scale: 0.001, unit: V, limits: {min: 0, max: 20}}
routines:
  - {id: 0x0203, name: SelfTestLamps, risk: R2, session: extended}
  - {id: 0xFF00, name: EraseMemory, risk: R4}
io:
  - {id: 0x4F01, name: DoorLockActuator, risk: R3, type: uint8, values: {0: unlock, 1: lock}}
dtcs:
  - {code: 0x923400, display: B1234-00, text: "Door lock motor circuit open"}
```
- 타입: `uint8/16/32/64`, `int*`, `float32/64`, `ascii`, `utf8`, `bcd`, `bytes`, `enum`, `bitfield`, `struct`(비트 범위), `array`. endian 기본 big, signed·bit width·길이 명시. 코드 실행형 표현식 금지.
- 디코드는 검증 후 udsoncan `DidCodec`으로 컴파일. 가변 길이 DID(ReadAllRemainingData)는 다중 DID 요청에서 마지막에만 허용. DB 없는 DID는 hex + ASCII 추정, **단독 조회만**(다중 DID 응답 경계를 모르므로). 표준 F1xx 이름표(F186, F187, F189, F18A, F18C, F190, F191, F195, F197, F19E 등)는 내장.
- DB 없는 쓰기는 `--hex`만 허용하고 R3로 상향.
- **DB는 등급을 올릴 수만 있다.** 기본표(미등록 RID R3, IO 01–03 R3, DB 없는 쓰기 R3 등)보다 낮추는 DB 값은 무시하고 감사에 기록한다. 하향은 정책 파일(사용자 소유)의 `trusted_db: [sha256…]`에 해시가 등록된 DB만 허용. 에이전트가 만든·로드한 DB는 디코드 용도로만 쓰인다.
- 파일 하나 = ECU 하나. `db load`로 대상에 바인딩, 데몬이 해시 기록. 잘못된 DB는 로드 거부(송신 불가).
- ODX/PDX(M4): odxtools strict 모드로 Loom YAML 변환, 손실·미지원 항목 coverage report. 런타임 디코더는 하나(Loom YAML)만 유지.

### 6.5 시나리오 YAML (M3)
```yaml
loom_scenario: 1
name: bcm-coding
target: bcm
db: diag/bcm.yaml
on_error: stop
vars: {region: EU}
steps:                   # core 러너 단계 = <domain>.<verb> 어댑터 (정확한 문법은 CORE 계약 v1)
  - uds.session: {mode: extended}
  - uds.security: {level: 1}
  - uds.read-did: {dids: [0x0101]}
    save: before
  - uds.write-did: {did: 0x0101, fields: {region: "${region}"}}
  - uds.read-did: {dids: [0x0101]}
    expect: {0x0101.region: "${region}"}
  - uds.reset: {type: soft}
  - wait: 3s
finally:                 # 마스터 A15: 정상 종료·on_error·에이전트 cancel → 실행(R0–R1 + safe_state). 사용자 STOP/abort → 자동 실행 안 함, 뷰어 "복구 송신" 1키로 제시
  - uds.session: {mode: default}     # safe_state: true
```
- 단계 이름 = CLI 동사. 반복은 `repeat: {count, until}`만(튜링 완전 금지 — 정적 위험 요약 가능해야 함). 임의 Python 실행 금지.
- **동적 값**(`save:`로 얻은 ECU 응답 값, 변수)이 쓰기 바이트가 되면 계획 시점에 바이트가 미정이다. 그 값이 DB `limits`/`enum` 범위 안일 때만 계획 승인에 포함하고, 아니면 그 단계에서 별도 확인을 요구한다.
- 러너는 core `loom run`(마스터 D12), UDS는 `uds.*` 단계 어댑터와 safe_state 동작 목록(`10 01`, `2F xx 00`, `28 00`, `85 01`)만 제공. safe_state도 인터록·deny list를 통과해야 하며 막히면 `finally_blocked`. 동적 값 규칙은 core 러너 요구로 이관. 각 단계 origin=script, core run 이벤트로 표시.

### 6.6 통합 테스트 지원 (v2.0, 마스터 C7)

사람이 자연어나 TC 표로 쓴 테스트를 에이전트가 `loom` 명령으로 실행한다. 판정은 이벤트 증거를 인용하고 Loom이 다시 검증한다. TC 형식·세션·보고는 TEST 기획(EXPLORE 팀)과 CORE가 소유하고, UDS는 아래를 제공한다.

| 명령 | 의미 | 종료 코드 |
|---|---|---|
| `loom uds check <pred> -t T` | 지금 1회 읽고 판정 | 0 일치 / 불일치 코드는 **미정**(CORE 공통 결정 C-11 — 구현·수용의 선행 관문, 제안은 3 `E_NO_MATCH`) / 7 무응답 / 10 NRC. 확정 전 exit 3은 조건 대기 timeout에만 쓴다 |
| `loom uds wait-for <pred> -t T --timeout 30s [--every 500ms]` | 맞을 때까지 반복 읽기(폴링 레이트 안) | 0 / 3 `E_TIMEOUT` |
| `read-did F1A0 --expect 02`, `raw 22F1A1 --expect-nrc 31`, `raw <hex> --expect-resp 62F1A0??` | 요청과 기대값을 한 번에 | 위와 같음 |

- `check`·`wait-for`는 **읽기 전용(R0) 요청만** 만든다. `raw`·타입 명령의 `--expect*` 옵션은 판정만 덧붙일 뿐, 실제 바이트의 등급·승인·차단·`write_state`를 바꾸지 않는다(예: `raw 2EF1A002 --expect-resp 6E…`는 그대로 R2 쓰기).

- 술어 문법(작게, 확장은 `help --json` `checks`):
  - `did:F1A0 == 0x02`, `did:F40D >= 10`, `did:0101.region == EU`, `did:F190 ~ "^WVW"`
  - `dtc:none [mask=0x09]`, `dtc:present B1234-00 [bit=confirmed]`, `dtc:absent U0100-00`
  - `session == extended`, `security == L1`, `link == connected`
- 모든 결과에 TEST 형식의 `data.evidence[] = [{ref: "uds:<target>@seq:a..b[#ptr]", kind, request_id, predicate?, observed?, expected?, complete}]`(`_xteam/test-domains.md` `## uds 의견`). 쓰기 결과는 `uds.tx` seq(시간 기준점)와 최종 `uds.rx` seq를 둘 다 넣는다. `wait-for` 만료 시에는 창 안 읽기 seq 범위·timeout 레코드·gap 유무를 넣는다. 판정 자체는 `uds.check`(`dir:null`) 이벤트로 남긴다. 일반 JSON Pointer 술어용 고정 경로: `/payload/sid`, `/payload/positive`, `/payload/nrc`, `/payload/resp_hex`, `/payload/decoded/dids/<DID>/value`(+`/unit`), `/payload/decoded/dtc_count`, `/payload/decoded/dtcs/<i>/raw24`, `/payload/decoded/complete`, `/payload/state_after/session|security`.
- **증거로 인용 가능한 이벤트**:

| 이벤트 | 판정 필드 |
|---|---|
| `uds.rx` | `req_hex, resp_hex, positive, nrc, decoded, state_after` |
| `uds.verify` | `write_state`, `readback_request_id` |
| `uds.timeout` | `stage, verdict, may_have_executed` |
| `uds.state` | `session`, `security`, `link`, `epoch` |
| `uds.guard` | 판정 결과 |
| `uds.poll_sample` | 측정값과 품질 |
| `uds.check` | 판정 기록 |

  Loom은 도메인 `verify_evidence(pred, events)` 훅으로 술어를 다시 평가한다(Match / Mismatch / Inconclusive).
- **부재 증명**: `dtc:none`/`absent`는 다음 셋을 모두 만족하는 `19 02` 긍정 응답이 있을 때만 성립한다. 아니면 `inconclusive`.
  1. `complete:true`(잘림·gap 없음)
  2. 요청 마스크가 판정 비트를 포함
  3. 단계 마커 seq 이후의 응답
- **테스트 세션 사전 승인**: `loom test start`가 `uds.plan_summary{intents[]}`를 부른다. intents는 대상·서비스·DID/RID·값 범위이고, UDS는 단계별 등급·필요 세션/보안·차단 여부·동적 값 여부를 돌려준다. 승인은 **패턴 범위**(예: `ecu1 2E F1A0 값∈DB limits, ≤ 5회, 세션 종료까지`)로 묶는다. 부모 승인은 test plan 해시·물리 대상·허용 범위·횟수·만료에 결속한다. **매 송신은 실제 바이트와 현재 상태로 `approval_binding`을 새로 만들어 검증**한다(바이트·epoch·lease·현재값·정책/DB 해시). 범위 밖·등급 상승·상태 변화가 있으면 재승인을 받고, 횟수는 원자적으로 소비한다. R3는 `allow_r3_batch`가 없으면 단계마다 뷰어 승인, R4는 매번 once, 차단 3층은 그대로다. CORE 계약(C-7)이 확정되기 전에는 패턴 승인을 기존 승인의 대체로 구현하지 않는다.
- **재부팅 TC용 링크 자동 재연결**: 정책 `auto_reconnect`(bench 기본 on, sim on, vehicle on — B4 동일 규칙). 링크만 다시 연결하고 epoch+1을 `uds.state`로 알린다. 재연결만으로는 ECU 상태를 알 수 없으므로 세션·보안은 **stale**로 표시하고 `active_overrides`를 보존한다(확인된 ECU 리셋 응답이 있을 때만 default/locked로 확정). **미완료 요청은 `outcome_unknown`으로 남기고 재전송하지 않는다.** D19의 U 해제는 DoIP에서 구 소켓 응답을 원천 폐기하고 재활성화에 성공한 뒤에만 한다. CAN 채널 재개방·epoch 증가만으로는 U를 해제하지 않는다.
- **TC 예시 1 (UDS 단독)**: "F1A0을 2로 쓰면 되읽기 2, 소프트 리셋 뒤에도 2, 확인된 DTC 없음"
  1. `write-did F1A0 --hex 02`
  2. `reset soft` → `wait-for "link == connected"`
  3. `check "did:F1A0 == 0x02"`
  4. `check "dtc:none mask=0x08"`
  5. `loom test verdict pass --evidence uds:seq:a-b`
- **TC 예시 2 (UART + CAN + UDS)**: "콘솔에서 reboot → 로그인 프롬프트 → NM 재개 → UDS 링크 복구 후 DTC 없음"
  1. `loom uart send` / `loom uart expect "login:"`
  2. `loom can sig wait-for NM_BCM.Active == 1`
  3. `loom uds wait-for "link == connected"`
  4. `loom uds check "dtc:none mask=0x09"`
  5. 도메인별 증거 구간을 인용해 판정

  명령 이름은 각 팀 v2 기준의 가칭이다.
- 02: 사용자는 셸의 테스트 실행 화면에서 단계와 인용 증거 행을 실시간으로 본다. 사전 승인 범위는 시작 때 확인하고, STOP·복구로 개입한다. 증거 없는 PASS는 Loom이 거부한다.

---

## 7. 라이브러리 선택과 근거

### 7.0 v2.0 Rust 선택 (`v2-sol.md`, crates.io·GitHub 실제 조회 2026-11-10)

| 영역 | 결정 | 근거 |
|---|---|---|
| UDS 러너 | **자체**(P2/P2*·0x78·총 deadline·SPRMIB·epoch·D19·publish-before-reply) | 공개 Rust 클라이언트는 상태 소유 client와 결합(`automotive` 0.6.0 MIT — 참조·상호운용 시험 후보), `ecu_diagnostics`는 GPL-3.0 |
| UDS 코덱 | **작은 자체 순수 Rust 코덱**(M1: 0x10/0x22/0x2E/0x19·raw 조립, 길이·SID·DID/서브기능 에코 검증), `automotive_diag` 0.1.29(MIT/Apache)는 SID/NRC 정의만 | 조사한 후보 중 코덱만 재사용하기 적합한 범용 대체를 찾지 못함 |
| DoIP | **자체 tokio 전송**(엔티티·SA당 TCP 1개, LA 분배, 단일 수신 task, alive 우선, 발견/공지 UDP 13400, 헤더 길이를 할당 전 검사) + 선택 TLS `tokio-rustls` 0.26.6 | `doip-definitions` 3.0.14는 central security를 0x02로 정의(정답 0xE0), `doip-codec` 2.0.9는 0xE0 거부·u32::MAX 길이 헤더를 오류 없이 대기(로컬 빌드 확인), `doip-sockets`는 얇은 래퍼 → 비채택 |
| ISO-TP | UDS는 `IsoTpPort` PDU RPC만(D11). 구현은 CAN 팀 — 의견: Linux는 커널 `CAN_ISOTP`(5.10+) 검토, Windows XL·PCAN은 사용자 공간(`can-hal-isotp` 0.4.3 조건부 후보), `isotp-rs`는 전 버전 yanked | UDS에 중복 구현하지 않음 |
| seed-key | Rust helper exe + `libloading` 0.9.0(ISC) | §10.8 |
| 진단 DB | serde 1.0.229 + 제한 YAML `serde-saphyr` 2.1.1(조건부, `serde_yaml`은 deprecated), CDD 이름 읽기 `quick-xml` 0.42.0, ODX는 `odx-rs` 0.39.0(MIT, 저장소 404 — **M3: 실 PDX로 odxtools와 비교·지원 범위 검증, M4: 제한 importer 제품 수용**) | Python 잔존 필요 없음(§0.1) |
| 런타임 | tokio 1.54.1, rustls 0.23.46 | 후보 crate MSRV 최고 1.88(odx-rs) — smoltcp는 C4.1로 제외 |
| VN5640 이더넷 | **범위 밖**(C4.1) — DoIP는 PC NIC + 미디어 컨버터 위 OS TCP/IP, `smoltcp` 불필요 | 사용자 확인: 이미 이 구성으로 DoIP 사용 중 |

**Rust 스파이크(측정)**: tokio DoIP loopback(라우팅 활성화 → ACK + VIN 응답)으로 측정했다. 조건은 release 빌드, 단일 스레드, 1,000회, macOS arm64다. 결과는 **p50 0.020 / p99 0.121 / max 0.364 ms**이고, 분할·연속 코덱과 0xE0·oversize 처리도 확인했다(`spikes/uds/evidence/v2-rust.json`). Python 측정과는 조건이 달라 배수 비교는 하지 않는다. TLS·발견·완전한 러너·IPC·웹 UI·실HW는 측정하지 않았다.

아래 v1 표(Python 패키지)는 근거 기록이다. doip-server(Python)는 v2에서도 **외부 상호운용 시험 도구**로만 쓴다(제품 런타임 아님).

PyPI JSON·GitHub API를 2026-11-09 실제 조회(증거: `spikes/uds/evidence/pypi.json`, `github.json`). 상세 기능 조사: Sol §7.

| 패키지 | 최신(릴리스 UTC) | 라이선스 / Python | 기본 브랜치 최근 커밋 | 판단 |
|---|---|---|---|---|
| udsoncan | 1.26.1 (2026-06-25) | MIT / ≥3.7 | 2026-10-26 | **채택 — 코덱**(Request/Response, 서비스 encode/interpret, DidCodec, NRC). 클라이언트 전용(서버 프레임워크 없음). `tester_present()`는 자동 유지 아님. `request_timeout`은 0x78로 리셋되지 않음(Sol §7.2) |
| doipclient | 1.3.0 (2026-11-09, 당일 릴리스) | MIT / ≥3.6 | 2026-11-09 | **런타임 미채택, 참고·상호운용 시험용.** 측정: P2 50 ms 설정 무응답 시 2001.9 ms 대기, `use_secure=True` 기본 경로 `check_hostname requires server_hostname` 오류, 상주 수신 pump 없음, ECU별 소켓 구조(Sol §7.3, §9.2) |
| can-isotp | 2.0.7 (2025-05-14) | MIT / ≥3.7 | 2026-08-22 | CAN 팀에 채택 제안: `NotifierBasedCanStack`. 2.0.7의 send 인자는 `send_timeout`(문서 최신 예제와 다름 — pin) |
| python-can | 4.6.1 (2025-08-12) | LGPL-3.0-only / ≥3.9 | 2026-11-08 | CAN 팀 소유. 배포·수정물 의무 검토 |
| odxtools | 11.7.1 (2026-10-20) | MIT / ≥3.10 | 2026-11-08 | M4 선택 의존성. strict 모드만, OEM fixture 검증 필요 |
| Textual | 8.2.8 (2026-06-30) | MIT / ≥3.9,<4.0 | 미조사 | TUI, 타팀과 통일·pin (`spikes/uds/evidence/r4-textual.json`) |
| scapy | 2.8.0 (2026-10-02) | GPL-2.0-only / ≥3.7,<4 | 2026-11-08 | **런타임 미채택**. 외부 상호운용·결함 주입 시험 도구 후보(라이선스 의무 별도 검토) |
| doip-server | 2026.6.14.post116 (2026-06-14) | MIT / ≥3.10,<3.15 | 2026-06-14 | 독립 DoIP 시뮬레이터 — 자체 클라이언트/서버 양단 오류를 줄이는 교차 검증용(M1 최소 smoke, M2 전체) |
| py-uds | 5.0.0 (2026-09-28) | MIT / **≥3.12** | 2026-11-02 | 미채택(Python 3.11 계약 충돌, server/DoIP 미완) |
| pyvit, aioisotp | 0.2.1 (2017-08-31) / 0.1.1 (2018-11-21) | GPL-3.0-or-later / MIT | 2019 / 2020 | 미채택(archived) |
| canmatrix | 1.3.3 (2026-10-21) | BSD-2-Clause / ≥3.8 | 2026-10-28 | CDD 변환기 아님(포맷 목록에 CDD 없음) |

자체 구현 범위(작게 유지): **DoIP 클라이언트 전송**(헤더·버퍼 상한·부분 프레임 조립, 라우팅 활성화, alive check, ACK/NACK, 소켓 공유 + LA 분배, 단일 수신 pump, deadline, TLS는 검증 어댑터), 트랜잭션 러너, 가드, 가상 ECU 상태 모델 + DoIP 엔티티.

**DoIP 관문(D4)**: 같은 자체 클라이언트/서버 왕복만으로 표준 적합을 주장하지 않는다.
- **M1 관문**(U-04/U-05): 프레이밍·부분 프레임·coalescing, 활성화(성공/거부/0x11 확인 필요/OEM 4바이트 필드), ACK/NACK, 오주소, 무응답 deadline, idle alive check 결함 시험 + doip-server(독립 구현) 최소 왕복 smoke.
- **M2 관문**(U-20): doip-server 전체 상호운용 회귀 + 프로토콜 버전(0x02/0x03/0xFF) + 첫 HIL 스모크(장비가 있으면).
- 관문 미통과 시 되돌림: doipclient 1.3.0 pin + 단일 수신 pump·deadline·TLS 래퍼. 이 경우 **게이트웨이당 동시 대상 1개** 제한을 감수한다(대상 전환 시 재활성화). 되돌림 경로도 같은 관문을 통과해야 한다. 참고로 doipclient에도 `send_diagnostic_to_address`가 있어 다른 LA로 송신 자체는 가능하다 — 선택 근거는 타이머·TLS 기본 경로 결함과 LA별 수신 분배 보강 비용이다.

라우팅 활성화 타입 값(R4 확인): 공개 AUTOSAR SWS DoIP R4.1.3(p32)·R24-11(p38, SWS_DoIP_00082–84, 참고문헌에 ISO 13400-2:2019 명시)과 Wireshark·Scapy 2.8.0 디섹터는 central security에 **0xE0**를 쓴다. doipclient 1.3.0 enum의 `CentralSecurity=0xE1`과 불일치하며, 이를 ISO 2012/2019 판본 차이로 볼 근거는 찾지 못했다(ISO 정식 원문 대조는 미완료). 기본 activation은 0x00, 그 외는 OEM 프로필의 **숫자**로만 받고 doipclient enum을 표준 이름으로 재사용하지 않는다. 근거 보관: `spikes/uds/evidence/r4-facts.json`.

---

## 8. HW / OS 지원 매트릭스

범례: ◎ 계획 + CI 필수, ○ 계획(실HW 검증 전), △ 조건부, ✕ 비지원. **실HW 합격 표시가 아니다.** 현재 실측은 macOS ARM64 loopback·virtual뿐.

v1.2(마스터 B1): **실HW 호스트 = Windows x64와 Linux x64 둘 다**(데몬 + 에이전트가 장비 PC에서 실행). macOS는 개발·시뮬레이션만 — 실HW 수용 대상이 아니다. Vector XL 드라이버·CANoe는 Windows 전용.

| 전송 / 장치 | macOS (개발·sim만) | Windows x64 (실HW) | Linux x64 (실HW) | 조건·비고 |
|---|---|---|---|---|
| 가상 ECU — DoIP loopback | ◎ M1 | ◎ M1 | ◎ M1 | ephemeral 포트, 실제 13400 시험은 직렬 lane |
| 가상 ECU — CAN-TP (python-can virtual) | ◎ M1b | ◎ M1b | ◎ M1b | virtual 버스는 프로세스 로컬. 별도 ECU 프로세스 시험은 Linux vcan |
| DoIP 실 게이트웨이/ECU | ✕(개발 시험만) | ○ | ○ | OS 무관 소켓. DoIP 활성화선 지원 ENET 케이블 또는 벤치 직결. 다중 NIC면 `--iface` |
| DoIP TLS 3496 | △ | △ | △ | stdlib ssl + SNI/CA/mTLS 검증 어댑터 후. 평문 fallback 금지 |
| CAN-TP via **Vector VN5640** (XL, CAN FD 전용) | ✕ | ○ M2(FD, C4) | ✕ | CAN 팀 Rust XL FFI(`vxlapi64.dll`, worker 안에서만 로드). Linux XL 지원 없음(Vector OS 표 공란) |
| DoIP via PC NIC + 미디어 컨버터 (C4.1) | ✕(개발 시험만) | ○ M2 | ○ M2 | §8.2. VN5640 이더넷·XL 이더넷 API는 쓰지 않음 |
| CAN-TP via **PCAN-USB FD** | ✕ | ○ M2(FD, PCAN-Basic 공식 Rust FFI) | ○ M2(SocketCAN FD `peak_usb`) | C5: FD를 M2 첫 실HW부터(classic 통과로 대체 불가). Linux 인터페이스 설정 주체는 CAN 팀 결정 |
| `canoe` 백엔드 (CANoe COM) | ✕ | △ P4 선택(Python 어댑터) | ✕ | CAN 팀 브리지 v0 위. CANoe 진단 설정(CDD/ODX·seed-key·DoIP/CAN-TP) 사용. CI는 가짜 브리지, 실 CANoe는 수동 시험 |
| CAN-TP via SocketCAN/vcan | ✕ | ✕ | ◎ vcan CI / ○ 실HW | Linux CI 통합 lane(CAP_NET_ADMIN runner) |
| CAN FD, extended/mixed, 29bit | 논리 시험 | ○ | ○ | M3b, 주소 바이트 오버헤드·STmin 지터 검증 후 |
| seed-key 외부 helper(JSON 프로토콜) | ◎ | ◎ | ◎ | Rust helper 우선, 언어 독립 실행파일 |
| seed-key DLL | ✕ | ○ M2 (x86 DLL = `i686` helper, x64 DLL = x64 helper) | ✕ | Linux는 외부 helper만 |

UDS는 PCAN-ISO-TP/Vector 전용 TP DLL을 따로 열지 않고 CAN 모듈 공통 경로를 쓴다.

### 8.1 실HW 체크리스트 (v1.2 — Windows·Linux 나란히)

공통(두 OS, 대상 1개마다): 대상 프로필(주소·P2/S3·`env`·백엔드) 작성 → 뷰어를 띄운 상태에서 R0 읽기 → 허가된 R2 쓰기 + 되읽기 → 사용자 `:` 개입·`T` → `!`·STOP 후 복구 송신 1키 → 응답→뷰어 p95 측정 게시 → `env=vehicle`이면 배지가 CLI 결과·뷰어에 보이는지 확인. 결과는 OS·백엔드별로 따로 기록한다.

| 항목 | Windows x64 | Linux x64 |
|---|---|---|
| DoIP 네트워크 | 방화벽에서 UDP 13400(차량 공지·discover 응답)·TCP 13400 허용, 다중 NIC면 `--iface`/소스 IP 바인드, NIC 절전 끔. **CANoe가 Vector 이더넷 인터페이스로 같은 게이트웨이를 쓰는 중인지 확인**(쓰는 중이면 `canoe` 경로 또는 다른 테스터 SA + 사용자 확인) | 고정 IP·라우팅(`ip addr`/`ip route`), nftables/ufw에서 UDP·TCP 13400 허용, 소스 IP 바인드(`SO_BINDTODEVICE`는 권한 필요라 기본 미사용). 데몬 root 불필요 |
| CAN-TP | PCANBasic 드라이버·채널 이름, Vector XL은 CANoe 공존 실측(CAN 팀 P3) 결과를 따름 | `peak_usb` 로드, `can0` 비트레이트·up(CAN 팀 결정: 사람 사전 설정 또는 권한 헬퍼), `ip -details link show can0`로 확인, 오류 카운터·bus-off 표시 |
| `canoe` 백엔드 | CANoe 버전·Vector HW 모델 기록, 진단 설정(CDD/ODX·seed-key·TP)·측정 시작/정지, COM 등록, 측정 정지 주입 시 link down·자동 전환 없음 확인 | 해당 없음(자체 스택만) |
| seed-key | Python 플러그인 / DLL 비트수(32bit면 브리지) / `canoe` 위임(확인 시) | Python 플러그인만 |
| 시계·지연 | 타이머 해상도·전원 관리 설정 기록, COM 경로 지연은 따로 게시 | `CLOCK_MONOTONIC`, CPU 거버너 기록 |
| VN5640 (v2.0) | XL 드라이버·펌웨어 버전, CAN FD 채널(이더넷은 쓰지 않음, C4.1) | 해당 없음(XL Linux 미지원) |
| DoIP NIC + 미디어 컨버터 (C4.1) | NIC 선택·정적 IP·VLAN, 방화벽 UDP 13400(발견)·TCP 13400, 컨버터 설정(PHY master/slave·100/1000BASE-T1·속도)은 사람의 사전 작업, activation line | 같은 항목(nftables/ufw, `ip addr`·`ip link` VLAN) |
| PCAN-USB FD (v2.0) | PCAN-Basic ≥ 5.1, FD 비트레이트(nominal/data) | SocketCAN FD `ip link set can0 type can bitrate … dbitrate … fd on`(설정 주체 CAN 결정) |

### 8.2 DoIP 실HW 경로 (v2.0, 마스터 C4.1 확정)

- **확정**: 사용자는 **PC 랜카드 + 미디어 컨버터**로 DoIP를 이미 쓰고 있다(`_xteam/vn5640-eth.md` 메인 확정). PC 쪽은 OS TCP/IP이고, UDS 자체 tokio DoIP 전송을 그대로 쓴다. Windows·Linux 둘 다 같은 방식이다.
- **범위 밖**: (a) VN5640을 OS 네트워크 어댑터로 노출하는 경로, (b) Vector XL 원시 이더넷 + 사용자 공간 TCP/IP(`smoltcp`). VN5640은 CAN FD 전용이고 Vector XL 바인딩 crate(CAN 팀)는 CAN/CAN FD API만 다룬다. U-31 스파이크는 취소했다.
- **벤치 준비와 에이전트 동작 구분**: 컨버터 설정(PHY 역할·속도·100/1000BASE-T1), NIC IP·VLAN, activation line은 사람의 사전 작업이다(§8.1 체크리스트). 에이전트가 반복하는 connect·read·write와 섞지 않는다. activation line을 자동 제어할 수 없으면 "벤치에서 activation 상태 유지"를 전제로 한다.
- **M2 HIL 확인 항목(두 OS)**: VIN 읽기, 허가된 쓰기·되읽기 → 웹 UI 표시, UDP 발견·다중 ECU, link flap 후 재연결(§6.6 `auto_reconnect`), 다중 NIC에서 `--iface` 선택, CAN FD 동시 부하.
- 화면 표시: 상태 바·결과에 `backend: doip`, `iface: <NIC>`.
- 조사 기록(`v2-sol.md` §4의 (a)·(b) 분석)은 근거 기록으로 남긴다.

---

## 9. 시뮬레이션 & 테스트 전략

### 9.1 가상 ECU (`loom.uds.sim`)
- **전송 무관 상태 모델**(`VirtualEcuModel`): 세션, 보안 레벨·실패 카운트·lockout, DID 값/접근권, DTC/상태/스냅샷, routine 상태, S3 만료, (M3) 다운로드 블록 상태. 진단 DB YAML + `sim:` 절로 구성. seed-key는 시뮬레이터 전용 결정적 toy 알고리즘(`xor`, 이름으로 명시).
  ```yaml
  sim:
    initial: {0xF190: "LOOMSIM0000000001", 0xF1A0: 0, 0x0101: "051F"}
    signals: {0xF40D: {wave: sine, min: 0, max: 120, period_s: 20}}
    sessions: {s3_ms: 5000, allow: [0x01, 0x02, 0x03]}
    security: {1: {algo: xor, const: 0x1234, max_attempts: 3, lockout_s: 10}}
    faults:
      - {match: "31 01 FF01", respond: [{nrc: 0x78, times: 4, every_ms: 1000}, {hex: "7101FF0100"}]}
      - {match: "22 F1A1", nrc: 0x31}
      - {match: "22 2010", drop: 0.05}
      - {match: "*", delay_ms: {dist: uniform, min: 5, max: 20}}
      - {match: "22 F1B0", first_frame_delay_ms: 10, cf_delay_ms: 30}   # late CF (CAN-TP)
  ```
  결정적 시드 기반, 실제 스케줄러 지터 시험과 분리.
- **DoIP 엔티티 어댑터**(M1): 127.0.0.1, ephemeral TCP/UDP, VIN/EID/GID·LA, 활성화 허용/거부 코드, 소켓 한도, alive check, UDP 공지, 게이트웨이 뒤 2개 ECU, 오주소 응답.
- **CAN-TP 노드 어댑터**(M1b): python-can virtual 버스는 프로세스 로컬이므로 가상 ECU는 **CAN worker 안 SimNode**로 돈다. UDS 팀은 전송 무관 `VirtualEcuModel.handle(pdu: bytes, now_ns) -> list[(delay_ms, bytes)]`(상태는 모델 내부, I/O 없음)를 제공하고, CAN worker가 서버 측 ISO-TP 채널(R-TP-12)에 연결해 실행한다. 결함 주입 설정 변경은 RPC로.
- 실행: `loom uds sim start <profile> [--doip 127.0.0.1:0]` 또는 `connect --sim <profile>`(데몬 안 기동).

### 9.2 스파이크 실측 (R1, 근거용 — 제품 구현 아님)
| 검증 | 결과 (측정) |
|---|---|
| 별도 프로세스 Pipe ping 500회 | median 0.050 / p95 0.269 / p99 2.003 / max 10.374 ms |
| 커스텀 BaseConnection → IPC → virtual CAN/ISO-TP → ECU, 22F190 ×100 | 모두 VIN 일치, median 0.92 / p95 2.18 / p99 3.09 / max 5.97 ms, FF/FC/CF 재조립 오류 0 |
| CAN 22F191 + NRC 0x78 ×2 | 72.8 ms, 최종 성공 |
| CAN 무응답 (P2 50 ms) | 55.8 ms에 timeout |
| DoIP loopback 활성화 + 22F190 ×100 | 활성화 1.0 ms, UDS median 0.31 / p99 0.85 ms, ACK+응답 coalescing 통과 |
| DoIP ACK 뒤 무응답 (P2 50 ms, doipclient) | **2001.9 ms** — deadline 결함 |
| doipclient `use_secure=True` | `ValueError: check_hostname requires server_hostname` |
한계: harness는 상주 관리자·라이브 뷰어 없음. UI 지연·실버스·TLS 성공·Windows/Linux·Python 3.11은 미검증.

### 9.3 테스트 계층

| 계층 | 내용 | CI |
|---|---|---|
| 단위 — 가드 | **SID 0x00–0xFF 전수 × 서브기능** 분류 스냅샷, suppressPosRsp 비트, DB 상향(하향 불가), 미등록 보수 등급, 응답 SID로 시작하는 raw 거부 | 3 OS |
| 단위 — 코덱/DB | 스키마 검증, struct/bit/endian, 잘못된 길이·overflow → decode_error(0 대체 금지), DTC 24bit/표기 | 3 OS |
| 러너 | P2 직전/직후 첫 응답, 0x78 반복·최대 횟수·총 deadline, 늦은 CF(제시간 FF), 응답 SID/에코 불일치, epoch 격리, **timeout 직후 같은 SID/DID 재요청 시 늦은 응답 오매칭 없음(quarantine)**, 회복·격리 반례(원 요청 `3E 00` timeout → 늦은 `7E 00`, 프로브 재시도 중 이전 프로브 응답, `22 F190` timeout 뒤 `22 F18C`/`22 F190` 재요청 차단, 늦은 `7F 22 31`의 오귀속 없음, `22 F190` timeout → 늦은 `7F 22 78`이 U를 해제하지 않음(이후 최종 `62 F190`이 late로 처리), CAN 후보 소진·순차 처리 위반 시나리오)에서 잘못된 성공·confirmed 반영 없음, SPRMIB 무응답 = ok_suppressed, may_have_executed | 3 OS |
| 동시성 | 에이전트 2 + 사용자 1 동시 요청, 대상당 인플라이트 1, lease·takeover·exclusive, 기능 주소 락 순서(교착 없음), TP와 활성 요청 비충돌 | 3 OS |
| **E2E 핵심 루프** | **v2.0: §11 M1 E2E-1(웹) — Playwright로 실제 DOM 행·`data-request-id` 확인, 키보드만으로 사용자 입력.** (v1: CLI 명령 + 실제 TUI(Textual pilot 헤드리스) 렌더 훅) + 사용자 입력 자동화(`:` 송신, `T`, `!`) + 늦은 attach + CLI 종료 뒤 세션 유지. publish-before-reply 순서, 응답→렌더 p95/p99 측정(1,000회) | 3 OS (지연 수치는 Linux 전용 runner에서 판정) |
| 안전 E2E | `r2_path=auto`(sim)에서 R2 즉시 실행, 비-auto(`token_or_viewer`/`viewer`)에서 토큰 없이 exit 5·토큰 재사용·다른 바이트·다른 epoch 거부, R3 뷰어 승인 없이 거부, `default_blocked`는 unlock 전 거부·unlock 후 매번 뷰어 승인, 하드 차단·`immutable_deny`는 승인으로도 거부, raw 우회(`raw 2E…`, `raw 3101FF00`, `raw 34…`, `raw 2A…`) 동일 결과, **vehicle과 bench가 같은 요청에 같은 등급·같은 `r2_path` 결과**(B4)·`meta.env=vehicle` 표시, 미등록 대상의 첫 R1+는 대상 등록 확인(exit 5), 사용자 하드 차단 목록 기본 빈 목록·설정 시 승인으로도 거부, 레이트, 감사 해시 체인, 기록·감사 불능 시 agent·script R1+만 차단·사용자 입력 허용 | 3 OS |
| `canoe` 백엔드 (P4(구 M2c)) | 가짜 브리지(`FakeCanoeBridge`)로 백엔드 선택 규칙(`auto`), 측정 정지 주입 → link down·`outcome_unknown`·자동 전환 없음, 같은 대상 두 경로 거부(exit 6), 바이트 그대로 전달(분류 = 전송 바이트), 승인 전 미송신, `ts_source=canoe`·pending 정보 없음 표시. 실 CANoe(시뮬레이션 모드 또는 벤치)는 Windows 수동 시험 + COM 경로 지연 게시 | 3 OS(가짜) / Windows 수동 |
| DoIP 결함 주입 | 분할 header/payload, 연속 패킷 coalescing, inverse version 오류, 프로토콜 버전 0x02/0x03/0xFF, 초대형 length(버퍼 상한), ACK/NACK와 UDS 혼동, 활성화 거부·0x03 SA 중복·0x11 확인 필요·OEM 필드·소켓 full, 게이트웨이 뒤 다른 ECU 응답, idle/pending 중 alive check, peer RST·재연결(쓰기 재전송 금지), UDP 다중 응답 | 3 OS |
| CAN-TP sim (M1b/M3b) | 멀티프레임 4 KB, FD, 기능 주소 그룹, CF 누락·잘못된 SN·FC WAIT/overflow, N_Bs/N_Cr 만료, STmin 0/1/10 ms·F1/F9 | 3 OS / Linux vcan |
| 상호운용 (M2) | 자체 DoIP 클라이언트 ↔ doip-server(독립 구현), 자체 가상 ECU ↔ doipclient | Linux |
| TLS (요구 시) | 테스트 CA·SAN·mTLS·만료/오CA/hostname 실패·SNI, 평문 fallback 금지 | 3 OS |
| 고부하 | 폴링 5 Hz × 8 DID 10분(1 tick = 다중 DID 1요청, 레이트는 요청 단위; env=sim에서 20 Hz), 0x78 연속(기본 상한 50회에서 중단 기대 + 시험 프로필 상향 시 100회), 느린 뷰어 → 드롭 배너·gap 행, 집계 카운터 = 입력 총량, 기록 무손실 확인 | Linux |
| HIL (수동) | §8.1 체크리스트(Windows·Linux 각각), VN5640·PCAN FD 위 CAN-TP, DoIP over NIC + 미디어 컨버터(C4.1), 실ECU 응답→웹 UI 지연 | 수동(두 OS) |
| 통합 테스트 (C7) | `check`/`wait-for`/`--expect` 술어 전수, `verify_evidence` Match/Mismatch/Inconclusive, 부재 증명 규칙, 패턴 범위 승인 재확인, 자동 재연결 후 미완료 요청 재전송 없음 | 3 OS |
| soak (C2) | §3.8 UDS 부하 + CAN FD·UART 동시 24 h, RSS 기울기·UDS 할당 64 MiB·worker 크래시 격리(seed helper 크래시 주입) | Linux 전용 runner(M2 관문), Windows 주 1회 |
| helper ABI | seed-key helper: 시간 상한·잘못된 길이·크래시·x86/x64 DLL 선택, key 마스킹 | Windows |

CI 매트릭스(v2.0): Rust stable `cargo test` Linux x64·Windows x64·macOS ARM64, clippy·miri(코덱·길이 검사), 퍼징(`cargo fuzz` — DoIP 헤더·UDS 응답 파서), Linux vcan 통합(권한 runner), Playwright 웹 E2E(Linux, 지연 판정은 전용 runner). doip-server(Python)는 상호운용 시험 lane에서만. HW 시험은 마커로 제외. (v1: Python 3.11–3.14)

---

## 10. 안전 / 가드레일

### 10.1 원칙
1. 가드는 데몬 안 **단일 관문**에서 **전송 직전 바이트**를 본다. CLI 플래그·스킬 규칙은 보조, 우회 경로 없음. 사용자 입력에도 같은 관문(완화 규칙만 다름).
2. 등급 기준 = 되돌릴 수 있는가 × 물리적 영향 × 잠금/벽돌 위험. 모르면 높게.
3. **위협 모델**: 실수하는 에이전트를 막는다. 같은 OS 사용자 권한으로 데몬 프로토콜을 위조하는 악의적 에이전트까지 암호학적으로 막지는 못한다 — 탐지(감사 해시 체인)·호스트 권한(Claude Code `ask`)으로 대응. 뷰어 자격 발급 시 TTY 확인도 보안 경계가 아니다(에이전트도 pty를 할당할 수 있음). 이 한계를 스킬·문서에 명시하고, 스킬은 뷰어 패인에 키 주입(herdr/tmux send-keys 등)을 금지한다.
4. 읽기도 ECU wakeup·버스 부하·세션에 영향 → 레이트는 항상 적용. 미등록 대상은 첫 R1+ 요청 때 대상 등록 확인(§10.6 `unknown_targets`).
5. **환경과 무관한 같은 규칙(마스터 B4)**: 사용자는 벤치와 실차를 모두 쓰고 실차 송신을 기본 차단하지 않는다. vehicle의 등급·`r2_path`·기본값은 bench와 같다. 남는 차이는 `env=vehicle` 배지(뷰어·CLI 결과 상시)뿐이며, R3/R4 뷰어 승인은 환경과 무관하게 유지된다.

### 10.2 등급과 확인 방식

R0–R4 의미·승인 메커니즘은 마스터 D6/A8/A9/A11(core `loom.core.safety`), 서비스별 매핑은 UDS 소유(§10.3).

| 등급 | 의미 | 에이전트 origin | 사용자 origin(뷰어) | script(core 러너) |
|---|---|---|---|---|
| **R0** 읽기 | 상태 변화 없음 | 자동 | 자동 | 자동 |
| **R1** 가역 상태 변경 | 세션(01/03)·통신 복구 등 | 자동 + 뷰어 알림 행 + 감사 | 자동 | 자동 |
| **R2** 쓰기·삭제·리셋 | 데이터 변경 | `r2_path`(마스터 A8): **sim = `auto`**, bench·vehicle = `token_or_viewer`(첫 호출 exit 5 + 토큰 → 채팅에서 사용자 동의 → `loom uds confirm <tok>`, 또는 뷰어 `Y`, 마스터 B4). Codex 등 `ask` 규칙이 없는 환경의 실HW는 `viewer` 권장 | 미리보기 + Enter 두 번 | run 시작 때 일괄 승인에 포함(core 러너 안에서만) |
| **R3** 위험 | 보안 접근(0x27 key), 액추에이터 구동, 보호 데이터, programming 세션, 미등록 루틴 | **뷰어 승인 필수**(채팅 토큰만으로 불가). 승인 객체 사용 기한 10분(A9), UDS 기본 scope = once(요청 1건) | 승인 코드 끝 2자 입력 | 단계마다 뷰어 승인(정책 `allow_r3_batch`로 run 일괄 승인 예외) |
| **R4** 비가역·고위험 | 플래시·메모리 지우기/쓰기·파일 전송 | `default_blocked` — `loom.toml [uds] unlock` + 뷰어 승인 후에야 **매번 뷰어 승인(scope once)** 대상. 기본은 거부(exit 4 `E_DENIED`) | 동일 | 계획 자체 거부(unlock 전) |

**차단 3층(마스터 A11)**: `default_blocked`(위 R4 — 0x34–0x38, 0x3D, 0x31 FF00, 미지·예약 SID) / **하드 차단**(승인으로 우회 불가 — **사용자가 `loom.toml [uds] hard_block`에 넣은 것만, 기본 빈 목록**(마스터 B4). 목록 제거는 완화라 뷰어 승인 후 적용) / **`immutable_deny`**(설정으로도 해제 불가 — 0x2A·0x86 비요청 응답 서비스(러너 미지원), 길이 0·응답 SID(0x40–0x7F, 0xC0–0xFF 등)로 시작하는 raw). 모두 exit 4(`E_DENIED`/`E_UNSUPPORTED`). v1.1까지 하드 차단이던 "화이트리스트 밖 대상의 R1+"·"`buses` 없는 R1+ 기능 주소"는 확인 경로로 옮겼고(§10.6), "활성 ISO-TP tx_id로의 raw CAN 송신"은 사용자 하드 차단 목록이 아니라 **채널 예약 규칙**(CAN이 집행, exit 4 `E_DENIED` `detail.reason=tx_reserved` — xteam 합의, D11)으로, UDS 대상을 quiesce하면 풀린다.

- **env**: `sim | bench | vehicle`. env 미지정은 vehicle로 표시(배지가 과하게 보이는 쪽으로 틀리게). `sim`은 `connect --sim`이 띄운 가상 엔티티 주소에만 매칭되며 물리 주소와 절대 매칭되지 않는다.
- **vehicle(마스터 B4 — A10·A8의 vehicle 규칙 폐기)**: vehicle은 bench와 **같은 등급·같은 `r2_path`·같은 기본값**이다. v1.1의 "화이트리스트 대상 R0만 기본 허용"과 "vehicle이면 0x11·0x28 disable·0x2F 01/02/03·미등록 0x31 한 단계 상향(env 보정)"을 삭제했다. 실차에서의 안전은 R2 확인·R3/R4 뷰어 승인·레이트·STOP·복구 송신·사용자 하드 차단 목록으로 지킨다.
- **R4 해제**: `default_blocked` 항목은 사용자가 `loom.toml [uds]`에 대상별 `unlock = [...]`을 두고 뷰어 승인으로 적용해야 R4 승인 대상이 된다(M5 플래시 프로필용). 에이전트의 파일 수정으로는 우회 불가(core가 해시 기록·완화는 뷰어 승인 후 적용).
- **R2 토큰의 보장 범위(정직하게)**: 토큰은 "같은 요청을 재제출했다"는 확인 수단이며 데몬이 사람의 동의를 관측한 증거가 아니다. 사람 동의 보장은 (a) 스킬 규칙(동의를 추정하지 말 것), (b) 프로젝트 `.claude/settings.json`의 `Bash(loom * confirm:*)` 접두 `ask` 규칙(마스터 D6)(호스트 확인 창 — 재제출을 별도 동사로 둔 이유; 규칙 동작 검증은 M1 U-12), (c) 감사 기록에 의존한다. 그래서 `r2_path` 기본값을 env별로 둔다(마스터 A8·B4): `sim: auto`, `bench`·`vehicle`: `token_or_viewer`(사용자가 동작 단위로 `auto` 완화 가능, 뷰어 승인 후 적용). 더 엄격하게 원하면 사용자가 `viewer`로 강화(즉시 적용). 뷰어 승인은 데몬이 직접 관측한 사람 입력이다.

### 10.3 서비스·서브기능별 등급표 (raw 분류에도 그대로)

분류 `classify(bytes, target, db) → (grade, reason)`: SID → 서브기능 서비스면 `byte1 & 0x7F` → DID/RID/IO/메모리 주소가 있으면 DB 등급으로 **상향만**(아래 표가 하한, `trusted_db`만 예외 — §6.4) → 기능 주소면 +1(0x3E·0x22·0x19 예외). env 보정은 없다(B4). **R1 이상 기능 주소 요청이 정책 전송 범위(`buses`/`gateways`) 밖이면** 미등록 ECU에 닿을 수 있으므로 등급과 별개로 확인 1회(`r2_path` 경로, reason `functional_unscoped`)를 요구한다(§10.6). CLI·스킬·뷰어 표기는 이 하나의 분류표 데이터 파일에서 생성한다.

| SID | 서비스 | 서브기능/조건 | 등급 |
|---|---|---|---|
| 0x10 | DiagnosticSessionControl | 01 default, 03 extended | R1 |
| | | 02 programming, 04 safety, OEM(0x40–0x5F), 기타 | R3 |
| 0x11 | ECUReset | 01 hard, 02 keyOffOn, 03 soft | R2 |
| | | 04/05 rapidPowerShutdown | R3 |
| 0x14 | ClearDiagnosticInformation | 전체 | R2 |
| 0x19 | ReadDTCInformation | 모든 서브기능 | R0 |
| 0x22 | ReadDataByIdentifier | | R0 |
| 0x23 | ReadMemoryByAddress | | R1 (기밀 노출 가능 — 감사) |
| 0x24 | ReadScalingDataByIdentifier | | R0 |
| 0x27 | SecurityAccess | 홀수 requestSeed | R1 |
| | | 짝수 sendKey | **R3**(마스터 D6, 실패 1회 후 레이트 잠금, 자동 재시도 금지) |
| 0x28 | CommunicationControl | 00 enableRxTx | R1 |
| | | disable 계열 | R2 |
| 0x29 | Authentication | | R3 |
| 0x2A | ReadDataByPeriodicIdentifier | | **`immutable_deny`**(비요청 주기 응답 라우팅 미구현 — 구현 시 계획 개정으로 R1) |
| 0x2C | DynamicallyDefineDataIdentifier | | R1 (02 defineByMemoryAddress는 이후 0x22로 메모리를 읽는 경로 — 동적 DID 범위 F200–F3FF 읽기를 감사 표시) |
| 0x2E | WriteDataByIdentifier | 일반 | R2 |
| | | DB protected, 식별 DID(F190·F18C 등 내장 목록), DB 없는 쓰기 | R3 |
| 0x2F | InputOutputControlByIdentifier | 00 returnControlToECU | R1 |
| | | 01/02/03 | R3 |
| 0x31 | RoutineControl | DB 등록 RID | DB 등급 |
| | | 미등록 RID | R3 |
| | | FF00 eraseMemory | R4 `default_blocked` |
| | | FF01 checkProgrammingDependencies | R1 |
| 0x34–0x38 | Download/Upload/Transfer/Exit/FileTransfer | | R4 `default_blocked`(unlock 후 매번 뷰어 승인) |
| 0x3D | WriteMemoryByAddress | | R4 `default_blocked` |
| 0x3E | TesterPresent | 00/80 | R0 |
| 0x84 | SecuredDataTransmission | | R3 |
| 0x85 | ControlDTCSetting | 01 on | R1 |
| | | 02 off | R2 |
| 0x86 | ResponseOnEvent | | **`immutable_deny`**(비요청 응답 라우팅 미구현 — 구현 시 계획 개정으로 R2) |
| 0x87 | LinkControl | | R3 |
| 0xBA–0xBE | 공급자 정의 | | R3 (DB 등급 우선) |
| 그 외 | 예약·미정의 SID | | R4 `default_blocked` |
| | 길이 0, 응답 SID로 시작하는 raw | | `immutable_deny` |

결과·사유는 응답 JSON, 뷰어 detail, 감사 로그에 항상 기록.

### 10.4 확인·승인 흐름
- **R2**: `r2_path = auto`(sim 기본)면 승인 객체 없이 바로 실행(감사만). `token_or_viewer`/`viewer`일 때만 core가 요청 바이트·대상 물리 주소·epoch·lease·**승인 화면에 보인 현재값**에 묶인 1회용 토큰(4–6자, 120 s) 발급 → exit 5 `E_CONFIRM_REQUIRED` + 사람용 요약(`data.approval.channel` 표시). `viewer` 경로는 토큰 없이 exit 5 `E_APPROVAL_REQUIRED` + `approval_id`만 반환하고 뷰어 승인만 받는다 → 같은 순간 뷰어 승인 패널에도 표시 → 에이전트는 채팅에서 사용자 동의 후 `loom uds confirm <tok>`(또는 사용자가 뷰어에서 `Y`, 에이전트는 `approval wait <approval_id>`). 값·대상·epoch가 다르면 거부(exit 5 `E_APPROVAL_REQUIRED` 재요청, 감사). 발급 뒤 다른 주체가 같은 DID를 쓰거나 세션·보안이 바뀌면 토큰 무효(diff가 거짓이 되므로).
- **stale 상태에서 쓰기**: `session_confidence`가 stale/assumed면 R2+ 요약·승인 패널에 굵게 표시하고 스킬은 `status --probe`를 먼저 하게 한다(강제 차단은 안 함 — ECU NRC가 대부분 막음).
- **R3**: 승인 응답은 core 승인 서비스의 **viewer role 연결**만 가능(마스터 D2/D6). CLI에는 approve 명령이 없다. 뷰어가 열려 있지 않으면 exit 5 `E_NO_APPROVER` → 에이전트 힌트 "사용자에게 `loom uds view`를 열어 달라고 요청". 02 원칙(사용자가 보고 있을 때만 위험 동작)과 맞물린다.
- 승인 패널 내용: 대상·env·세션·보안·요청 hex·DB 디코드·이전값→새값 diff·등급 사유·만료.
- **시나리오**: core `loom run --plan`이 사전 위험 요약 + 파일 해시·대상에 묶인 일괄 승인(finally 포함, 마스터 A15). UDS 어댑터는 단계별 `classify` 결과를 제공. R2 단계는 일괄 승인에 포함, R3 단계는 실행 중 단계별 뷰어 승인, `default_blocked`·`immutable_deny` 단계가 있으면 계획 거부. 런타임 템플릿 값으로 등급이 오르면 그 단계에서 정지하고 새 승인 요구.
- 송신 직전 재검사(`presend_check` 훅: 물리 주소·epoch·DB/정책 해시·lease·quarantine·승인 사용 기한) 불일치 시 승인 무효.

### 10.5 dry-run
모든 쓰기 계열·`raw`·`run`에 `--dry-run`: 인코딩 바이트, 등급·사유, 필요한 확인, 사전 조건(세션·보안), 현재값 캐시와 diff, 레이트 잔여. 전송·토큰 발급 없음. 뷰어 `?` 접두.

### 10.6 정책 파일·화이트리스트·레이트
정책은 core 2층 정책 파일(마스터 D6)의 UDS 섹션이다: 프로젝트 `loom.toml [uds]`(대상·한계·별칭) + 사용자 `~/.loom/config/policy.toml [uds]`(개인 강화). 완화는 뷰어 승인 후 적용, 강화는 즉시, 변경은 감사·해시 기록.
```toml
[uds]
env = "bench"                    # sim | bench | vehicle (없으면 vehicle 배지). vehicle 규칙 = bench(B4). sim 대상은 connect --sim이 자동 등록
max_auto_grade = "R1"            # 승인 없이 자동 허용하는 등급 상한 (R0|R1). R2는 별도로 r2_path가 결정(auto면 자동)
r2_path = "token_or_viewer"      # 기본: sim auto, bench·vehicle token_or_viewer (A8·B4). viewer로 강화 가능
idle_release_s = 600
quarantine_ms = 1000
trusted_db = []                  # 등급 하향을 허용할 DB sha256 (사용자만 편집)
unknown_targets = "confirm"      # 미등록 대상: R0 자동, 첫 R1+ 때 대상 등록 확인 1회(r2_path 경로, reason unregistered_target) 후 세션 동안 allow_max R3. "allow" | "r0_only"(강화)
hard_block = []                  # 사용자 하드 차단(승인으로도 불가). 기본 빈 목록(B4). 예: [{target="*", sid=0x11}]
backend = "auto"                 # auto | canoe | doip | cantp (§3.7, Linux·macOS의 auto = 자체 스택)
unlock = []                      # default_blocked 해제 항목 (뷰어 승인 후 적용)

[uds.sim]                        # connect --sim 대상 자동 화이트리스트
allow_max = "R3"
rate_exempt = true               # 지연 측정 1,000회 등

[[uds.buses]]                    # R1 이상 기능 주소 요청을 확인 없이 허용할 범위 (밖이면 확인 1회 — functional_unscoped)
can = "ch0"
functional_allow_max = "R1"

[[uds.targets]]
name = "bcm"
match = { doip = { ip = "192.168.0.10", logical = 0x1001 } }
allow_max = "R3"
allow_r3_batch = false

[[uds.targets]]
name = "ecm"
match = { can = { channel = "ch0", tx = 0x7E0 } }
allow_max = "R2"

[uds.rate]
per_target_rps = 5               # 요청 단위(폴링 1 tick = 다중 DID 1요청). bench에서 상향 가능
burst = 2
poll_default_hz = 5
poll_min_period_ms = 50
poll_max_dids = 16
security_key_failures = { max = 1, window_s = 600 }
reset = { max = 1, window_s = 30 }
clear_dtc = { max = 1, window_s = 10 }
write_did = { max = 10, window_s = 60 }
r3 = { max = 5, window_s = 600 }
```
- 정책 로드·완화 적용은 core 규칙(마스터 D6): 파일 변경 감지 시 강화는 즉시, 완화는 뷰어 승인 후. CLI `policy show`는 읽기 전용. 프로젝트 `.claude/settings.json`의 `deny`에 정책 경로 Edit/Write 추가 제안(사용자 전역 설정은 건드리지 않음).

### 10.7 기타 가드
- **긴급 정지 두 가지**: (1) 전 도메인 STOP(`Ctrl-X`/`Ctrl-] x`/`loom stop-all`, 마스터 A3) — agent·script의 새 쓰기와 모든 잡(UDS의 TesterPresent·폴링 포함)을 latch, 사용자 뷰어 입력은 계속 가능. (2) UDS 대상 abort(뷰어 `!`, `loom uds abort`) — 그 대상의 큐 비움, 폴링·run 중단, 대기 승인 취소, TP 정지. 둘 다 사용자 STOP이므로 run `finally`를 자동 실행하지 않는다(마스터 A15). 진행 중 요청은 응답/timeout까지 기록, "이미 전송됨 — 취소 불가" 표시. 쓰기였으면 결과 확정 전까지 `may_have_executed`. STOP·abort 자체는 새 요청을 보내지 않는다. 단 TesterPresent도 잡이라 멈추므로 ECU는 S3 만료 후 기본 세션으로 돌아갈 수 있다(안전 쪽). IO 제어·통신 차단 등 그 밖의 상태는 유지되며 뷰어에 "STOP — 상태 유지 중" 배지.
- **복구 송신(1키, 자동 아님)**: abort 뒤에도 IO 제어(0x2F 03)·통신 차단(0x28)·DTC 설정 off(0x85)·비기본 세션은 기본 세션 복귀까지(최대 S3, 프로필에 따라 더 길 수 있음) 유지된다. 그래서 STOP/abort 화면과 상태 바에 `active_overrides`를 보여 주고 **"복구 송신" 단일 키**(마스터 A15 core 공통 패턴, latch 예외는 safe_state 항목만; `uds.recover`: `2F <id> 00`, `28 00`, `85 01`, `10 01` — 모두 R1, origin=user, 뷰어 전용)를 제공한다. 순서: 사용자 lease 확보 → 진행 중 PDU 종료 대기 → 해당 복구 요청이 quarantine 미해결 집합과 겹치지 않음 확인(§3.4, 겹치면 DoIP 재연결 등 해제 후) → 복구 요청을 순서대로 송신. safe_state 동작도 인터록·deny list를 통과해야 하며 막히면 skip + `finally_blocked`. 각 단계의 대기·거부 사유를 화면에 표시하며 "즉시 복구"를 보장하지 않는다(회복 실패 시 S3 만료·수동 조치 안내).
- **세션 복구 안내**: 비기본 세션 + 보안 해제 상태로 lease가 끝나면 뷰어에 "default로 되돌릴까요?" 제안(자동 송신 안 함).
- **재시도**: 상태 변경 요청 자동 재시도 0. 0x78은 대기 연장이지 재송신 아님. 0x21은 읽기만 정책상 1회.
- **raw `--no-wait`**: R0만, 데몬은 응답 drain.
- **raw CAN 충돌**: 활성 ISO-TP 채널의 tx ID로의 CAN 송신은 CAN 모듈이 기본 차단 — raw 송신뿐 아니라 **로그 재생(replay)·주기 송신·IG** 경로도 같은 차단 또는 해당 ID 필터(xteam 요청). 강제하려면 UDS 대상을 먼저 disconnect(quiesce)·lease 해제(사용자 `--force` 예외 없음 — D11). 의도적 진단 ID 주입 강건성 시험은 "UDS disconnect 후" 절차로 한다.
- **seed-key**: 플러그인 서브프로세스(시간 상한 2 s), key 마스킹, 실패(0x35) 자동 재시도 없음, 0x36/0x37 후 로컬 재시도 금지 시각 표시.

### 10.8 seed-key (v2.0 — CANoe 없이, 외부 helper)

- **원칙**: 알고리즘 실행은 항상 데몬 밖 **별도 helper 프로세스**에서 한다(C2 크래시 격리). helper는 key만 계산하고, 승인된 0x27 송신·seed→key 원자 구간·epoch는 러너가 통제한다. key는 이벤트·감사에서 마스킹한다. 실패 시 0 키 대체·자동 재송신은 금지다. 시간 상한 2 s, seed/key ≤ 4 KiB, 메시지 ≤ 64 KiB, RSS 64 MiB.
- **우선순위**:
  1. **외부 실행파일 + stdin/stdout JSON 프로토콜**(버전 필드, 언어 독립 — Rust 권장)
  2. **기존 Windows DLL**: Rust helper가 `libloading`으로 로드한다. 시그니처는 `GenerateKeyEx(seed, seed_len, level, variant, key, capacity, &actual_len) → VKeyGenResultEx`, `GenerateKeyExOpt`(options 추가)이며 근거는 Vector SN-IND-1-040이다. **x86 DLL은 64비트 프로세스에서 로드할 수 없으므로 `i686-pc-windows-msvc` helper**, x64 DLL은 x64 helper를 쓴다. 호출 규약·enum 크기·export는 OEM 헤더/PE로 확인하고, C++ 이름 장식·예외가 있으면 helper 안에 작은 C ABI shim을 둔다.
  3. 고객이 Python 구현만 주고 대체가 없을 때 명시적 외부 Python 실행(같은 JSON 프로토콜)
  4. (P4) `canoe` 어댑터의 CAPL 키 계산(§3.7)
- 폐기: in-process 플러그인·entry point(`loom.uds.seedkey`, C3). Windows DLL을 Linux에서 로드하는 경로는 두지 않는다.
- 시뮬레이터용 내장 `xor`(테스트 전용)은 helper 프로토콜 시험에도 쓴다.
- 미확인: 실제 OEM DLL의 ABI·의존 DLL·MSVC 런타임(U-14 helper ABI 시험).

### 10.9 스킬 설계

**원본 `skills/loom-uds/SKILL.md` (본문 ≤ 60줄 목표)** — `loom skills sync`가 `.claude/skills/`와 Codex `.agents/skills/`로 복사(마스터 D13). 데몬·출력 봉투·종료 코드 공통 의미·승인·`others_since_last`·STOP·뷰어 띄우기 규칙은 **공통 `loom` 스킬**이 담당하고, 아래는 UDS 고유 절차·분기만.
```markdown
---
name: loom-uds
description: ECU 진단(UDS over DoIP / CAN-TP). VIN·SW 버전·DID 읽기/쓰기, DTC 조회/삭제,
  세션·보안접근, 루틴, 리셋, 진단 시나리오, DID 플롯. "ECU", "진단", "DTC", "DID", "UDS",
  "DoIP", "0x22", "seed key" 등이 나오면 사용.
---
# loom-uds
공통 규칙은 `loom` 스킬을 먼저 따른다. 모든 ECU 통신은 `loom uds ...` CLI로만 (데몬 경유 → 사용자가 실시간으로 봄).
udsoncan/doipclient/python-can을 직접 import 해 ECU에 붙지 말 것.
## 절차
0. (`loom` 스킬) `LOOM_CLIENT` 설정 — 다른 에이전트 값 사용 금지.
1. `loom uds status` — 데몬·대상·세션·lease·viewers 확인. 대상 없으면 connect(사용자가 준 주소만).
2. viewers 0이면 사용자에게 옆 패인에 `loom uds view`를 열어 달라고 요청(R3는 뷰어 없으면 불가).
3. 읽기 먼저: 쓰기 전 현재값을 read-did로 확인해 보여준다. stale/assumed면 status --probe(미지원이면 그 사실을 사용자에게 알림).
4. 쓰기는 첫 호출이 곧 미리보기다(exit 5, sim은 확인 없이 실행될 수 있음). 값을 조립·검토할 때만 --dry-run.
5. 결과의 others_since_last.count > 0 이면 `trace --since <from_cursor>`로 사용자 개입을 확인하고 반영.
6. 쓰기 후 되읽기로 검증. 비기본 세션이 끝나면 session default.
## UDS 고유 분기 (공통 종료 코드 의미는 `loom` 스킬)
5 E_CONFIRM_REQUIRED → 요약을 보여주고 명시적 동의 후 `loom uds confirm <data.approval.token>`
  E_APPROVAL_REQUIRED(data.approval.channel=viewer) → 사용자에게 뷰어 승인 요청 후 `loom uds approval wait <data.approval.approval_id>`
  E_NO_APPROVER → 사용자에게 `loom uds view`를 열어 달라고 요청
6 E_LOCKED → 사용자에게 제어권 반환 요청 / E_QUARANTINE → 안내된 해제 조건을 보고, 같은 요청 반복 금지
7 E_NO_RESPONSE → data.write_state=outcome_unknown이면 재전송 금지·보고, 읽기만 1회 / data.timing.verdict=uncertain도 동일
10 E_NRC → error.detail.nrc_name·error.hint를 사용자에게 보고, 같은 요청 반복 금지 (0x78은 결과가 아님)
## 금지
- `default_blocked`·`immutable_deny` 시도(플래시 0x34–0x38, 0x3D, FF00 erase, 0x2A/0x86), raw 우회, 정책·DB 등급 하향 시도, 정책 파일 수정
- 0x27 실패 후 재시도, LOCKED 시 제어권 재획득 시도, --duration 없는 poll
- 뷰어 패인에 send-keys 등으로 키 입력(승인·takeover 대리 입력과 같음)
- 동의를 추정해 confirm 실행
## 상세: references/
```
references/(UDS 고유만): `commands.md`(§4), `safety.md`(등급표·확인 흐름·위협 모델 한계), `nrc.md`(NRC표·원인·다음 행동), `services.md`(바이트 구조·DTC 상태 비트), `did-db.md`(§6.4·표준 F1xx), `scenarios.md`(§6.5·`--plan`), `seed-key.md`, `troubleshooting.md`(활성화 거부 코드, NACK, ISO-TP 오류, stale 세션).

**Codex 호환**: `loom skills sync`가 같은 원본을 `.agents/skills/`로 복사(마스터 D13). `ask` 규칙 `Bash(loom * confirm:*)`은 Claude Code 전용이므로 Codex 환경의 실HW는 `r2_path = viewer` 권장(마스터 A8). `loom uds agent-help`는 스킬 없는 에이전트용 요약 출력. 안전 집행은 데몬이 하므로 에이전트 종류와 무관.

### 10.10 스킬 개정 (v2.0, 마스터 C8)

정본은 `loom uds help --json`이다(clap 정의 + Registry 위험표에서 생성 — 명령·인자·등급·승인 경로·`mutates`·`waits`·종료 코드·`evidence_kinds`·예시·`checks` 문법). 스킬 본문은 짧게 쓴다. 공통 의미(봉투·종료 코드·`others_since_last`·데몬·STOP)는 `loom` 스킬, TC 절차는 `loom-test` 스킬이 맡는다.

```markdown
---
name: loom-uds
description: ECU 진단(UDS over DoIP/CAN-TP) — DID 읽기/쓰기, DTC, 세션, 리셋, 루틴, 검증(check/wait-for).
  "ECU", "진단", "DTC", "DID", "UDS", "DoIP", "seed key"에 사용.
---
# loom-uds  (공통 규칙은 `loom` 스킬, 테스트 절차는 `loom-test` 스킬)
정본: `loom uds help --json`. 모르면 먼저 조회.
모든 ECU 통신은 `loom uds …`로만. UDS 라이브러리를 직접 쓰거나 Loom UI를 브라우저 도구로 조작하지 말 것.
## 절차
1. `loom uds status` — 대상·세션(confidence)·lease·viewers. viewers 0이면 사용자에게 Loom UI를 열어 달라고 요청.
2. 대상이 없으면 사용자가 준 주소로만 `connect`.
3. 읽기 → 쓰기(exit 5면 동의 후 `confirm`) → 되읽기 또는 `check`.
4. others_since_last.count > 0 이면 `trace --since <from_cursor>`.
5. 끝나면 비기본 세션을 `session default`로.
## UDS 분기
5 E_CONFIRM_REQUIRED → 동의 후 confirm / E_APPROVAL_REQUIRED·E_NO_APPROVER → UI 승인 요청 후 `approval wait`
6 E_LOCKED → 제어권 반환 요청 · E_QUARANTINE → 해제 조건 보고
7 E_NO_RESPONSE → write_state=outcome_unknown이면 재전송 금지
10 E_NRC → nrc_name·hint 보고, 같은 요청 반복 금지
## 금지
default_blocked·immutable_deny 시도, raw 우회, 정책·DB 등급 하향, 0x27 재시도, 동의 추정, 뷰어 키 대리 입력, Loom UI 브라우저 자동화
## TC 단계로 쓸 때
판정 명령: `check <pred>` · `wait-for <pred> --timeout` · `read-did … --expect`.
증거 kind: uds.rx · uds.verify · uds.state · uds.check. REF는 결과 `data.evidence[].ref`를 복사만 한다(지어내지 말 것).
쓰기는 테스트 세션의 사전 승인 범위 안에서만. 범위 밖이면 멈추고 사용자에게 묻는다.
재부팅 뒤에는 `wait-for "link == connected"` 다음에 읽는다(미완료 요청은 재전송되지 않음).
```
- 경계: `loom-uds`는 UDS 동작을 안전하게 수행하는 법과 술어 문법(`checks`)을 맡는다. `loom-test`는 TC 해석·의도 목록과 일괄 승인·단계 마커·증거 인용·판정·보고를 맡는다. UDS 스킬에 TC 절차를 넣지 않는다.
- v1 §10.9의 원본 위치(`skills/loom-uds/` + `loom skills sync`)와 `ask` 규칙 `Bash(loom * confirm:*)`는 유지한다.

---

## 11. 마일스톤 (수용 기준)

선행: **CORE 팀 P0 산출물**(마스터 §3 — 스키마 fixture·인터페이스 stub, gateway·이벤트 허브·CLI 프레임워크·정책/확인/승인/감사 v0·Registry·기록기). UDS는 stub으로 개발하고, core가 M1까지 못 주는 항목만 같은 시그니처로 임시 구현 후 이관(변경은 CORE에 요청). UDS M1은 CAN 팀 일정에 의존하지 않는다. 마스터 단계(v1.2 기준 — v2.1에서 메인이 다시 묶음): P0 = U-01–U-06, P1 = M1(U-07–U-12), P2 = M2 ∥ M1b, P3 = M3, P4 = M3b·M4·`canoe`(선택). **v2.0: 모든 단계는 Rust crate + 웹 UI 기준**이며 M1 1차 화면은 Loom UI 앱이다(TUI는 M2 smoke). 실HW 수용은 Windows x64·Linux x64 각각(§8.1, 마스터 B1).

| M | 이름 | 내용 | 수용 기준 |
|---|---|---|---|
| **M1** | **걷는 뼈대 (sim DoIP, Rust + 웹 UI)** | Rust `loom-uds` crate(gateway 안): 자체 DoIP 전송, 트랜잭션 러너(P2/0x78/deadline/epoch/quarantine), 자체 코덱, UDS 훅 3개 + core 정책 v0 위 최소 정책(env=sim 자동 등록, R0/R1 자동, R2 `auto`/토큰/뷰어 Y, R3·차단 3층 거부, DB 하향 무시, 최소 레이트), sim ECU(세션 01/03, 0x22, 0x2E, 0x19 02, 0x10, NRC·지연 주입). CLI `connect --sim/status/read-did/write-did/confirm/session/dtc list/raw/trace/abort/approval wait/help --json` + `check`·`wait-for` 최소형(§6.6). **웹 패널 U1·U2·U3·U4(R2)·U5 값 표**(UI 팀 셸 위, 명세 `_xteam/uds-ui.md`). 기록·감사, 기록 불능 시 agent·script R1+ 차단(A12), `write_state` 표시, 스킬 최소판 + `ask` 규칙 검증 | (1) **E2E-1(웹)**을 3 OS CI에서 자동 실행(Playwright, 실제 DOM). t0 `connect --sim` → 상태 바·`data-env=sim` → t1 `read-did F190` → U1 행 `data-request-id`가 CLI JSON `request_id`와 같음 → t2 `write-did F1A0`(sim `auto`, `token_or_viewer` 토큰, 뷰어 `Y` 세 경로) + 되읽기 `effective_verified`, 승인 카드에 토큰 원문 없음 → t3 사용자가 **키보드만으로** U3에 `write F1A0 02` → lease가 user로 넘어감, 에이전트 다음 결과 `others_since_last.count=1` → t4 에이전트 `session extended`는 거부 행 + exit 6 `E_LOCKED`, `R` 반환 후 성공 → t5 `!` 대상 abort와 셸 STOP(STOP 중 에이전트 쓰기 거부·사용자 U3 송신 허용, `finally` 자동 실행 없음) → t6 CLI 종료 뒤 새로고침 → snapshot 100행 + 라이브 ≤ 1 s, ECU 재연결 없음. (2) 응답→U1 렌더 p95 ≤ 100 / p99 ≤ 200 ms(1,000회, Linux 전용 runner, Chromium 헤드리스 렌더 포함), tx→행 p95 ≤ 50 ms, 사용자 입력→`uds.tx` p95 ≤ 50 ms. (3) 30분 연속 E2E 반복에서 데몬 RSS 증가 ≤ 10 MiB(24 h soak은 M2 공통 관문). (4) v1.2 안전 수용 유지: token_or_viewer에서 토큰 없이 exit 5, `raw 2E…` 동일, `raw 34…`/`raw 3101FF00`·`raw 2A…` exit 4, 요청 필드 origin 무시. (5) DoIP 무응답 시 exit 7 `E_NO_RESPONSE`(쓰기 `outcome_unknown`), 늦은 응답 오매칭 없음. (6) 기록 불능 주입 시 agent 쓰기 exit 4 + `record.health` 배너, 사용자 송신 허용. (7) DoIP M1 관문(§7). (8) `check`/`wait-for` 술어와 `data.evidence`. **헤드리스 구독자 수신이나 WS 메시지 확인만으로는 불합격** — DOM 행 텍스트와 `data-request-id`를 확인해야 한다 |
| **M1b** | **걷는 뼈대 (sim CAN-TP)** | `IsoTpPort` 어댑터(CAN 모듈 ISO-TP 최소판: normal 11bit, virtual), 가상 CAN-TP 노드, `rx_start` 기반 P2, `connect --can` | 선행: CAN 팀 ISO-TP 최소판(CAN M2 **K10** + xteam §9·§11 M1b 항목·`can.isotp.barrier`). **M2와 병렬**(M2에 의존하지 않음). M1 E2E-1 스위트를 CAN-TP 전송으로 재실행 통과 + 멀티프레임(VIN) 응답 + late CF에서 P2 오판 없음. CAN 트레이스에 ISO-TP 프레임과 UDS 행이 같은 request_id로 같은 타임라인에 보임 |
| M2 | 세션·보안·안전 완성 | security + 플러그인 서브프로세스, TP 스케줄러·유휴 만료·stale 규칙 전체, lease 전체(shared/exclusive, 승인 대기 중 연장, 보안 승계 경고), **R3 뷰어 승인(코드 끝 2자) + 다중 대기**, `loom.toml [uds]` 정책 전체(대상 등록·`unknown_targets`·env 배지·레이트·`r2_path`·buses·trusted_db·unlock·`hard_block`(기본 빈 목록)·`backend` — core 2층 정책 위), 감사 해시 체인, dry-run 전 명령, NRC 해석표, dtc clear/snapshot/ext, reset, 뷰어 복구 송신(active_overrides, core 공통 패턴 A15), 0x78 장시간, DoIP M2 관문(doip-server 전체 상호운용), **v2.0: seed-key helper(x86/x64 DLL·외부 실행파일), 웹 패널 U6·U7·U8, TUI smoke, DoIP over PC NIC + 미디어 컨버터 HIL(Windows·Linux, C4.1), CAN K10 이후 VN5640·PCAN FD 위 CAN-TP HIL(C5), 24 h soak UDS 부하(§3.8)** | 안전 E2E 스위트 전부 통과(토큰 무효화·기능 주소 범위·DB 하향 포함). 0x78 ×10에서 한 행 카운터 갱신. TP 2 s ± 200 ms(sim)·인플라이트 중 TP 억제. STOP/abort 후 복구 송신 1키로 세션 default 복귀(인터록 통과). 0x27 key는 뷰어 승인 없이 거부. DoIP 결함 주입 목록 통과. **v2.0: helper ABI·크래시 시험 통과, NIC + 미디어 컨버터 경로에서 VIN 읽기·허가된 쓰기·되읽기가 웹 UI에 보임(두 OS), FD 위 CAN-TP 멀티프레임, 24 h soak 관문(§3.8)** |
| (P4) | `canoe` 백엔드 (Windows, 선택 — v2.0 C4) | v1.2 P4(구 M2c) 내용 그대로(§3.7, `_xteam/canoe-bridge.md`). Python 어댑터 별도 프로세스 위 프록시 | v1.2 P4(구 M2c) 수용 기준 그대로. 진단 기본 경로가 아니므로 다른 마일스톤의 선행이 아니다 |
| M3 | 서비스 확장·데이터·반복 작업 | routine, io, comm, dtc-setting, core `loom run`용 `uds.*` 단계 어댑터 + safe_state 목록, poll job + viz(V-UDS-1/2/4), 고부하 표시 규칙, `write-did --field`(struct), `read-did --scan`, 시뮬레이터 다운로드 상태기 | 시나리오 예제 3개 sim 통과·계획 요약 정확. 폴링 10분: 기록 무손실, 집계 행·드롭 배너, 집계 카운터 = 입력 총량, 플롯 ≤ 250 ms |
| M3b | CAN-TP 전체 | normal 29bit·normal-fixed, extended/mixed, CAN FD, 기능 주소 그룹(정책 `buses`), `discover --can`, `raw --functional` | 선행: CAN M4(**K14**). CAN-TP 4 KB·FD·기능 그룹·주소 모드별 SF 길이 통과 |
| M4 | 실HW·DB 확장 | 실 DoIP 벤치(**CAN 일정과 독립** — 장비가 있으면 M2 HIL 스모크부터; TLS 요구 시 검증 어댑터가 선행 관문), CAN-TP 전체 기능의 실HW(주소 모드·FD 기능 그룹 — M3b·CAN M4 의존), ODX→YAML 변환(`odx-rs` 제한 채택), pcapng | §8.1 HIL 체크리스트를 **Windows·Linux 각각**(DoIP 1종 + CAN 1종, `canoe`는 Windows만): 장비 읽기·허가된 벤치 쓰기·라이브 개입, 실ECU 응답→뷰어 p95 ≤ 100 ms. TLS 대상이면 인증 성공·실패 폐쇄 |
| M5 | (선택) OEM 플래시 프로필 | `FlashPlugin`, 0x34/36/37 시퀀스, 진행 표시, `default_blocked` unlock + 매번 뷰어 승인, 인터록 `uds.flashing`(PSU 전원 사이클 금지 등) | 사용자 결정 후 별도 계획. 검증 이미지·전원·복구 절차를 실제 벤치에서 수용한 프로필만 |

M1에 넣지 않는 것(v2.0): TUI, R3 승인, security/seed-key, CAN-TP(M1b), 폴링 플롯·job(U5 플롯·U9), DTC 패널(U6), 연결 대화상자(U7, CLI로 대체), 복구 송신 패널(U8, M2), `canoe`(P4), ODX, TLS, 플래시 (02 원칙: 4요소 최소형 먼저).

---

## 12. 구현 단계 작업 분해

담당: Opus = 아키텍처·UX·안전 집행·스킬, Sol = 프로토콜·전송·타이밍·시뮬레이터·측정. "그룹" 안의 작업은 병렬 가능.

| ID | 작업 | 담당 | 의존 | 그룹 | M |
|---|---|---|---|---|---|
| U-00 | **CORE 팀으로 이관**(마스터 §3.1). UDS는 core 계약 v1 검토와 변경 요청만: 쓰기 파이프라인 훅 3개(A2), 승인 digest 추가 필드, safe_state 등록, `E_QUARANTINE`/`E_NO_RESPONSE`/`E_UNSUPPORTED` 코드, viewer role 승인 API | Opus | CORE P0 | 0 | P0 |
| U-01 | `loom-uds` Rust crate 골격(Registry 도메인 등록: backend·commands·event kinds·capabilities·risk table·sim — C3), service/runner/codec/db/sim/cli 모듈, `IsoTpPort`·`Transport` trait. CORE stub crate 기반 | Opus | CORE stub | A | P0 |
| U-02 | UDS 분류기(`classify` 훅) + 분류표 데이터 파일(R0–R4·차단 3층, env 보정 없음(B4), CLI·스킬·뷰어 표기 원천) + SID 전수 테스트 + DB 하향 무시·기능 주소 범위 테스트 | Opus | U-01 | A | P0 |
| U-03 | 진단 DB 스키마(serde + JSON Schema) + 제한 YAML 로더(크기·깊이·alias 상한) + DID 코덱 컴파일 + 표준 F1xx | Sol | U-01 | A | P0 |
| U-04 | 자체 tokio DoIP 클라이언트 전송(할당 전 길이 검사, 활성화(0xE0 포함), alive 우선 수신 task, ACK/NACK, 소켓 공유·LA 분배, deadline, 발견 UDP) + DoIP M1 관문 시험 + 퍼징 | Sol | U-01 | A | P0 |
| U-05 | 가상 ECU 상태 모델(`handle()` 순수 함수형) + DoIP 엔티티 어댑터 + 결함 주입 + `connect --sim` 데몬 내 기동 + doip-server 최소 smoke | Sol | U-03 | A | P0 |
| U-06 | Rust 트랜잭션 러너 + 자체 서비스 코덱(P2 첫 응답/0x78/완료 상한/총 deadline/epoch/quarantine + 응답 식별자 기반 격리·고유 생존 프로브(반례 시험 포함)/에코 검증/SPRMIB) + TargetSession 상태 규칙 + `status --probe` + `write_state` 매핑. **D2 관문 = 러너 시험 목록 통과, 판정 시점 = 그룹 B 종료** — 실패 시 범위 축소·일정 조정(Python 회귀 없음) | Sol | U-04 | A→B | P0–M1 |
| U-07 | UDS RPC·이벤트(core 이벤트 허브 위: publish-before-reply 순서, 행 갱신 이벤트, `others_since_last`, trace 질의) | Opus | CORE 허브, U-06 | B | M1 |
| U-08 | UDS 훅 `approval_binding`·`presend_check` + core 정책 v0 위 UDS 최소 정책(env=sim 자동 화이트리스트·`r2_path`·최소 레이트) + `confirm` 동사 + lease 최소(takeover) + write/raw dry-run. 승인·감사·기록 불능 차단은 core 사용 | Opus | U-02, U-07, CORE 정책 v0 | B | M1 |
| U-09 | CLI M1 명령 + 출력(core 봉투·종료 코드 0–10 매핑·approval.channel) | Opus | U-07 | C | M1 |
| U-10 | **웹 패널 명세 확정·UI 팀과 통합**(U1–U5, `_xteam/uds-ui.md`): `approval.display` JSON Schema, snapshot provider(UDS 절), dry_run 미리보기 RPC, 렌더 확정 mark. 패널 구현은 UI 팀, UDS는 데이터·계약·시험 fixture | Opus | U-07, U-08, UI 셸 | C | M1 |
| U-11 | **E2E-1(웹) 자동 테스트**(Playwright 실제 DOM + 키보드 입력 + core 공통 지연 하네스 + 늦은 attach + 30분 RSS + 안전 수용 (4)(6)) + 3 OS CI | Sol | U-05, U-08, U-09, U-10 | C | M1 |
| U-12 | `skills/loom-uds/SKILL.md` 최소판(공통 `loom` 스킬 위) + `agent-help` + `Bash(loom * confirm:*)` `ask` 규칙 동작 검증 | Opus | U-09 | C | M1 |
| U-13 | `IsoTpPort` 어댑터(`can.isotp.*` RPC, `can.isotp_rx_start` 등) + `can.isotp.barrier` 기반 timeout 판정 + CAN worker SimNode용 가상 CAN-TP 노드 연결 + `connect --can` | Sol | CAN K10, U-06 | D | M1b (P2, M2와 병렬) |
| U-14 | Security + **seed-key helper**(외부 실행파일 JSON 프로토콜, `libloading` DLL helper x64/`i686`, 2 s·64 MiB, 크래시·ABI 시험) + sim xor(0x27 key R3 경로 포함) | Sol | U-06 | E | M2 |
| U-15 | TesterPresent 잡과 UDS 공통 잡 기반(폴링 서비스 자체는 M3 U-23이 이 기반 위에 구현) — **잡형**(A1) + 유휴 만료·stale 규칙 + `active_overrides` 추적 + safe_state 등록·뷰어 복구 송신(core A15 패턴) | Sol | U-06, CORE 잡 API | E | M2 |
| U-16 | lease 전체(shared/exclusive, 승인 대기 연장, 보안 승계 경고) + 기능 주소 락 + 동시성 테스트 | Opus | U-08 | E | M2 |
| U-29 | **통합 테스트 지원**(C7): `check`/`wait-for`/`--expect` 술어 파서·평가, `data.evidence`, `uds.check` 이벤트, `verify_evidence` 훅, `uds.plan_summary`, `auto_reconnect` | Opus(계약)/Sol(구현) | U-07, CORE 테스트 세션 | C(최소)→E | M1(최소)–M2 |
| U-30 | **C2 예산·soak**: §3.8 상한을 CORE 큐 타입으로 구현, UDS 할당 계측, 24 h soak UDS 부하 생성기(64 가상 대상 × 20 Hz, 주입 목록) | Sol | U-06, CORE soak 하네스 | E | M2 |
| ~~U-31~~ | 취소(C4.1): VN5640 XL 이더넷 + `smoltcp` 경로는 범위 밖 | — | — | — | — |
| U-32 | 스킬 v2(§10.10) + `loom uds help --json` 생성 + `loom-test` 경계 | Opus | U-09, U-29 | C→F | M1–M3 |
| U-17 | R3 UDS 승인 패널 규칙(코드 끝 2자, 다중 대기 목록 — core 공통 승인 패널 P2와 통합) + 토큰 무효화 규칙(`approval_binding`) | Opus | U-08, U-10, CORE 승인 패널 | E | M2 |
| U-18 | `loom.toml [uds]` 정책 스키마 전체(buses·gateways·trusted_db·`r2_path`·`unknown_targets`·`hard_block`·`backend`·unlock) + dry-run 전 명령. 해시 체인·완화 승인은 core | Opus | U-08, CORE 정책 2층 | E | M2 |
| U-19 | NRC 해석표(references/nrc.md와 같은 원천) + DTC 서비스 확장(clear/snapshot/ext) + reset | Sol | U-02 | E | M2 |
| U-20 | DoIP M2 관문: doip-server 전체 상호운용 + 결함 주입 스위트 + 프로토콜 버전 + (장비 있으면) HIL 스모크 | Sol | U-04, U-05 | E | M2 |
| U-21 | 서비스 확장(routine/io/comm/dtc-setting) CLI+코덱, `write-did --field`, `read-did --scan` | Sol | U-18 | F | M3 |
| U-22 | core `loom run`용 `uds.*` 단계 어댑터 + safe_state 목록 + 단계별 classify 제공. 동적 값 규칙은 core 러너 요구로 전달 | Opus | U-18, CORE 러너 | F | M3 |
| U-23 | 폴링 잡 + long format + viz 소스 데이터(패널 내용 명세 U5·U9) + 고부하 집계/드롭. 렌더러·패널 프레임워크는 UI 팀 | Opus(명세)/Sol(성능) | UI viz, U-07 | F | M3 |
| U-24 | CAN-TP 전체(주소 모드·FD·기능 그룹·discover --can) | Sol | CAN K14, U-13 | G | M3b |
| U-25 | `skills/loom-uds/references/` 전체 | Opus | U-18, U-22 | F | M3 |
| U-26 | §8.1 HIL 체크리스트·실HW를 Windows·Linux 각각: **DoIP over PC NIC + 미디어 컨버터(M2, C4.1)**, **VN5640·PCAN FD 위 CAN-TP(CAN K10 이후 M2)**, TLS 검증 어댑터, ODX 변환(M4) | Sol | U-20, CAN K10 | H | M2–M4 |
| U-27 | (P4 선택) `canoe` 백엔드 프록시 + 가짜 브리지 시험 — v1.2 내용 보존 | Sol | CAN 브리지(P4) | P4 | P4 |
| U-28 | (P4 선택) `canoe` 경로 UX·CAPL 키 계산·이름 가져오기 | Opus | U-27 | P4 | P4 |

병렬성: 0(U-00, CORE 조율) → A(U-01 후 U-02~05 동시, P0) → B → C(U-09/10/12 병렬, U-11이 합류). D(M1b)는 CAN K10 직후 E(M2)와 병렬, G(M3b)는 CAN K14 후, P4(`canoe`)는 선택.

---

## 13. 리스크

| # | 리스크 | 영향 | 대응 |
|---|---|---|---|
| 1 | 핵심 루프가 DB/UI 다듬기에 밀림 | 원칙 위반 | M1 수용 기준에 실제 웹 UI 렌더(Playwright DOM)·사용자 키보드 입력·read/write/개입을 묶음. ODX/웹/시나리오/플래시는 M3 이후 |
| 2 | 자체 DoIP 전송·러너의 표준 적합 오류(자기 클라이언트↔자기 서버만 맞음) | 실HW에서 실패 | 결함 주입 목록 + doip-server 상호운용 + 조기 HIL 스모크(장비 있으면 M2부터). 관문 미통과 시 doipclient + 좁은 어댑터 / 데몬 내 udsoncan Client로 되돌림(D2, D4) |
| 3 | P2와 PDU 완료 혼동 | 긴 응답 오판 | `rx_start` 계약(xteam §9), p2_ms/rtt_ms 분리, late CF 시험 |
| 4 | CAN-TP API가 CAN 팀 일정에 묶임 | M1b 지연 | M1을 DoIP로 독립 완성, xteam 조기 합의, 가짜 IsoTpPort로 러너 시험 |
| 5 | 세션 상태 추정 오류(외부 리셋·다른 테스터) | 잘못된 전제로 쓰기 | confidence 표시, stale 규칙, 쓰기 전 `--probe` 권장, 예상 밖 프레임 경고 |
| 6 | 0x27 반복 실패로 ECU 잠금 | 벤치 정지 | 실패 1회 후 10분 레이트 잠금, 자동 재시도 금지, 스킬 금지 |
| 7 | 같은 OS 사용자 권한 에이전트의 승인 우회 | 안전 무력화 | 위협 모델 명시, 뷰어 자격·CLI approve 부재·호스트 `ask`·해시 체인 탐지, `r2_path = viewer` 옵션(사용자 강화) |
| 8 | 단일 데몬 장애 시 전 도메인 정지 | 가용성 | 모듈 태스크 격리·재시작, 플러그인 서브프로세스, TP 정지 = S3 복귀(안전 쪽) |
| 9 | DoIP 게이트웨이 편차(활성화 타입·동시 처리·alive 엄격도·SA 정책) | 연결 불안정 | 소켓 공유 기본, `max_parallel=1`, 활성화 타입 숫자 옵션, HIL 조기 |
| 10 | 타이머·OS 지터 (v1 Python Pipe max 10.4 ms 측정, v2 Rust 재측정 필요) | TP·P2 오판 | 판정은 데몬 타임스탬프, 대기 여유, 분포·max 기록, 전용 runner |
| 11 | OEM 비밀(seed-key·DB) 유출 | 보안 사고 | 플러그인 저장소 밖, key 마스킹, 감사에 비밀 없음, `.gitignore` 템플릿 |
| 12 | 실차 송신 기본 허용(B4)과 env 표기 오류 | 실차에서 R2 동작이 bench와 같은 확인만으로 실행 | 규칙은 env와 무관하므로 오설정이 안전 규칙을 바꾸지 않음. env 미지정 = vehicle 배지, CLI 결과·뷰어 상시 표시, R3/R4 뷰어 승인 유지, 사용자 `hard_block`·`r2_path = viewer` 강화 가능, STOP·복구 송신 |
| 13 | 뷰어가 멈춘 화면을 정상처럼 표시 | 사용자 오판 | heartbeat·STALLED·LAG·드롭 배너·gap 행, 기록 상태 표시 |
| 14 | 라이선스(python-can LGPL, scapy GPL, CDD 후보 미확인) | 배포 문제 | SBOM·notice, GPL 런타임 미채택, 무허가 복사 금지 |
| 15 | doipclient 당일 릴리스(1.3.0) 등 의존성 변동 | 회귀 | 버전 pin, 시험용으로만 사용 |
| 16 | CANoe가 Vector 이더넷 인터페이스로 DoIP를 쓰는 중에 자체 DoIP(PC NIC)가 같은 게이트웨이에 접속 | 같은 SA면 활성화 0x03 거부(기존이 alive 응답 시), 다른 SA는 자리가 없으면 0x01, 함께 활성화돼도 ECU 세션·보안 상태 공유로 서로의 전제를 깨뜨림, 두 인터페이스가 같은 망이면 IP 충돌 가능 | CANoe 측정 중 Windows 기본 = `canoe`, 한 대상 한 경로(exit 6), 자체 DoIP는 다른 SA 명시 + 사용자 확인, §8.1 체크리스트, P4(구 M2c) HIL에서 실측(미확인) |
| 17 | CANoe COM 자동화 불안정(STA 스레드, 측정 재시작 시 객체 무효화, 버전별 API 차이) | `canoe` 경로 끊김 | 브리지 코어는 CAN 팀 소유(재연결·버전 표), UDS는 link down + 명시적 재connect, 자동 전환 없음 |
| 18 | `canoe` 경로의 타이밍·0x78 가시성 저하, CANoe UI 직접 요청은 Loom에 안 보일 수 있음 | 사용자 화면이 덜 정확 | `ts_source=canoe`·pending 정보 없음·"CANoe 직접 요청은 보이지 않을 수 있음"을 화면에 명시, 지연 별도 게시, 예상 밖 응답 시 stale |
| 19 | DoIP 실HW(NIC + 미디어 컨버터) 설정 편차(PHY 역할·속도·VLAN·방화벽·activation line) | 연결 실패·불안정 | §8.1 체크리스트(사람 사전 작업), M2 HIL 두 OS, link flap 시험, 결과 표시에 NIC·연결 상태 |
| 20 | Rust 재작성 일정(러너·DoIP·코덱 자체 구현) | M1 지연 | Python 스파이크·v1 시험 목록을 Rust 시험으로 옮겨 관문 유지, 공개 crate는 정의·참조만, 범위 축소로 대응(Python 회귀 없음) |
| 21 | seed-key DLL ABI·비트수·MSVC 런타임 차이 | 보안 접근 불가·helper 크래시 | 외부 helper·x86 helper, ABI 시험, 실패 시 0 키 대체 금지 |
| 22 | 웹 viewer 자격을 에이전트가 얻어 자기 승인 | 안전 무력화 | bootstrap은 TTY·데스크톱 셸에서만(CORE·UI), 스킬이 브라우저 자동화 금지, 감사 |

---

## 14. 사용자에게 물어볼 질문 (답이 없으면 기본값으로 진행)

v1.1: 마스터 §4 통합 질문 U1–U12가 정본이다(Q1·Q2 → U1/U2/U3, Q3 → U8, Q4·Q5 → U7, Q6 → U4, Q9 → U12, Q10 → U5). 아래는 UDS 세부와 기본값.

| # | 질문 | 기본값 | 바뀌는 결정 |
|---|---|---|---|
| Q1 | 첫 실장비: DoIP 게이트웨이/ECU 종류, CAN 인터페이스(Vector/PCAN), 실차 vs 벤치, OS? | v1.2 답(B1·B2·B4): 벤치·실차 모두, Windows x64·Linux x64, Vector + CANoe, PCAN-USB. 남은 기본값: 벤치 DoIP 우선, PCAN은 두 OS, CANoe 측정 중이면 `canoe` | M4 HIL lane·전송 우선순위 |
| Q2 | 첫 ECU의 주소(LA 또는 CAN ID)·P2/S3·쓰기 허용 DID 목록이 있는가? | 시뮬레이터 프로필만. 실장비는 프로필 없으면 미등록 대상(R0 자동, 첫 R1+ 때 등록 확인 — bench·vehicle 같음) | 정책·DB |
| Q3 | 플래시(0x34–0x37)가 언젠가 필요한가? | 비범위, M5 OEM 프로필 선택 | M5 착수 여부 |
| Q4 | seed-key 제공 형태: 실행파일 / Windows DLL(32/64비트) / Python 코드 / 없음? | v2.0: 외부 helper 프로토콜, DLL은 x86/x64 helper(M2), Python은 대체 없을 때만 | U-14 |
| Q5 | 진단 DB 원천: ODX/PDX, CDD, 엑셀, 없음? | Loom YAML 수작성 + ODX 변환(M4), CDD는 CANdelaStudio ODX export 경유. v1.2: CANoe 쪽 이름 표시용 가져오기 검토(P4(구 M2c)) | M4 importer, U-28 |
| Q6 | R2(쓰기·DTC 삭제·리셋) 확인을 채팅 토큰으로 충분히 볼지, 항상 뷰어 승인으로 할지? | sim `auto`, bench·vehicle `token_or_viewer`(마스터 A8·B4, R3는 항상 뷰어) | 정책 기본값 |
| Q7 | 에이전트 자동 허용 상한: R0만? R1(extended 세션 진입 포함)? | R1 | 확인 빈도 |
| Q8 | DoIP TLS(3496)·OEM 인증 활성화 요구가 있는가? | 없음 가정, 요구 시 해당 장비 첫 수용 전 필수 관문 | TLS 시점 |
| Q9 | VIN 등 식별 정보를 로그에 원문 저장해도 되는가(보존 기간)? | 로컬 원문, export 시 `--redact` 옵션, 보존은 공통 quota | 기록 정책 |
| Q10 | 같은 버스에서 CANoe(진단 콘솔 포함)를 동시에 쓸 일이 있는가? | v1.2 답(B3): 진단을 CANoe로 CAN·이더넷 모두 수행 → `canoe` 백엔드 P4(구 M2c), 채널 공존 실측은 CAN P3. 같은 대상 두 경로 거부, 예상 밖 응답 시 경고·stale | 공존 설계 |
| Q11 | (B7) CANoe 버전·Vector HW 모델(VN16xx/VN56xx, 이더넷 인터페이스 여부)·PCAN 모델(FD)·Windows 진단 기본 경로 | 기본: CANoe 측정 중이면 `canoe`, 아니면 자체 스택. 모델은 §8.1 체크리스트에 기록 | P4(구 M2c) 범위, 리스크 16 |
| Q13 | (v2.0) 미디어 컨버터 모델·대상 ECU PHY(100/1000BASE-T1)·벤치 activation line 제어 방법 | 100BASE-T1, activation은 벤치에서 유지 | §8.1·§8.2, U-26 |
| Q14 | (v2.0) 사용 중인 pyvxl 판(사용자 확인: CAN 전용) | CAN 팀 교차 검증 참고만 | CAN XL crate |
| Q12 | CANoe에 설정된 seed-key DLL을 Loom이 그대로 써야 하는가(위임), 아니면 Python 플러그인/DLL 직접 로드로 충분한가? | Loom 플러그인 기본, CANoe 위임은 COM 지원 확인 시 | U-14, U-28 |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 메인(공통 계약)에 요청 — v1.1 결과

모두 마스터 v1.1에서 결정됐다: C1 → D1, C2 → D3/A13/A14, C3 → D2/A2, C4 → D2/D6/D10/A2(`LOOM_CLIENT`로 이름 변경), C5 → D6/A8–A11(0x27 R3 등), C6 → D5/A6(코드 0–10), C7 → D9/A13, C8 → D2/A5, C9 → D7/A12(agent·script R1+만 중단), C10 → D14, C11 → §3.3 관문. UDS의 결정 수용 내용은 §16.1.

| # | 요청(원안) | 근거 |
|---|---|---|
| C1 | **사용자에게는 `loomd` 하나, 내부는 gateway(core+uds) + CAN worker** — CAN 팀 CR1에 동의. 필수 계약은 장치 단독 소유 + 도메인 간 API(인프로세스/RPC 동일 시그니처)·이벤트 스트림 일치. 네이티브 플러그인은 서브프로세스. 데몬 수명주기 `loom daemon start/stop/status`와 exit 7 처리 공통화 | 세션 상주, ISO-TP 인프로세스, 이벤트 버스·승인·감사·뷰어 접속점 하나. CAN Opus 초안도 `loomd` 전제. uds-sol은 CAN worker 분리 + 공통 broker 선호(결정 기록 D3) — 동일 시그니처로 둘 다 수용 |
| C2 | 이벤트 공통 필드 추가: `schema_version, seq(데몬 전역 단조), kind, request_id, job_id, epoch, actor_id, purpose`, `ts.clock_id/source`. **origin·dir enum은 그대로** | gap 감지, 요청↔응답↔감사 연결, 커서, keepalive 구분 |
| C3 | **publish-before-reply** 순서 계약을 전 도메인 공통으로 + 렌더 지연은 별도 SLO | 02 원칙 "같은 순간" |
| C4 | core 공통 **승인 서비스**(토큰 R2 / 뷰어 승인 R3, CLI approve 없음, 재제출은 `confirm` 동사) · **뷰어 자격과 연결 자격 기반 origin 결정**(요청 필드 origin 금지) · **client id**(`LOOM_CLIENT_ID`) · **lease** · **감사 로그(해시 체인)** · **정책 로더** | UART 입력·CAN 송신도 같은 구조. 뷰어 승인 패널 하나로 통일. origin 사칭 방지 |
| C5 | 위험 등급 R0–R4 의미를 전 도메인 공통으로(서비스별 표는 도메인 소유) | 스킬·뷰어 표기 통일 |
| C6 | CLI 종료 코드 표준(§4.1) 전 도메인 공통 | 스킬 단순화 |
| C7 | heartbeat(1 s)·snapshot/watermark attach·gap·STALLED/LAG/LIVE-DEGRADED·표시 드롭 배너 규칙 공통 | "거짓말하지 않는 화면" |
| C8 | IPC: 127.0.0.1 WebSocket JSON-RPC 2.0 + 토큰 파일 + Origin 검사 + 요청 크기 상한. 에이전트 명령은 경계 있음, 사용자 뷰어 구독은 장기 연결 허용(구분) | 웹 뷰어 직결, 로컬 CSRF 방지 |
| C9 | 기록기: bounded queue, rotation/quota, 기록 불능 시 새 장치 송신 중단 정책 공통 | 감사 불능 상태에서 위험 동작 방지 |
| C10 | Python 하한 3.11 유지 | py-uds(≥3.12) 때문에 올리지 않음 |
| C11 | M1 공통 수용 정의: "시뮬레이터 왕복 성공"이 아니라 **agent connect/read/write + 실제 사용자 화면 렌더 + 사용자 개입 + 같은 stream 증거** | 02 원칙 |

### 15.2 CAN 팀 — `_xteam/can-uds-isotp.md`

**v1.1**: 마스터 D11로 합의안 확정(이중 승인 미결 해소 — UDS가 연 ISO-TP 채널은 "UDS 정책 위임", CAN은 채널 TX 정책·STOP·예약 ID·프레임 레이트만 집행). 이름은 A16(`can.isotp.*` RPC, `can.isotp_rx_start` 이벤트), `tx_state: driver_accepted|bus_confirmed`(A14). CAN 회신의 `can.isotp.barrier` 계약과 두 층 순번(`isotp.open`의 epoch와 별개)을 §3.4에 반영. 작업 번호 K9a/K9b = CAN **K10/K14**.

**합의 내용(현행, 이름은 A16)**: CAN worker 별도 프로세스(CR1) + PDU RPC, 패딩 0xCC 기본·`profile`, `blocksize_rx` 8, `tx_state: driver_accepted|bus_confirmed`·`ts_quality`, drain barrier(`can.isotp.barrier`), `can.isotp.cancel`·`can.isotp.recv`, unknown responder 관측만(자동 FC 금지), `lease_ms` heartbeat, 가상 CAN-TP ECU는 CAN worker SimNode에서 UDS의 `VirtualEcuModel.handle()` 실행. 이중 승인은 D11로 해소(UDS가 연 채널 = UDS 정책 위임). 아래 v0.2/v0.3 문단의 `isotp.*` 표기는 역사 기록이며 현행 이름은 `can.isotp.*`.
v0.2 핵심: PDU 단위 경계, `rx_start`/`rx_complete` 분리, `epoch`·`req_id`·`deadline_ms`, `ts_source`, TesterPresent는 UDS 소유(CAN 주기 PDU는 선택·idle 게이팅 조건부), 소유권 키에 주소 모드·SA/TA/extension, 활성 ISO-TP tx ID raw 송신 기본 차단, 기능 주소 SF 길이(normal 7/62, ext·mixed 6/61), max_pdu 4095, origin enum 유지, 일정(UDS M1은 DoIP, M1b는 CAN M2 ISO-TP 최소판 위).
추가 요구(R3 반영, xteam v0.3로 기입): `request_id` 전달 → ISO-TP 원시 프레임 주석, `flush/abort(isotp_id)`, 서버 측 채널(sim 가상 ECU용), 결함 주입 훅(CF 누락·FC 지연/WAIT·잘못된 SN), 응답자 범위 탐색(M3b 필수), 채널 상태 이벤트, `set_params`, 진단 PDU 이벤트를 원시 프레임 이벤트보다 우선 전달. R4 추가: 활성 ISO-TP tx ID 차단을 raw 송신뿐 아니라 **replay·주기 송신·IG**에도 적용.

### 15.3 viz (v2.0: UI 팀 소유로 이관 — C6) — `_xteam/viz-requirements.md` `## uds`, 패널 명세는 `_xteam/uds-ui.md`
V-UDS-0(핵심 원칙: 점진 행 갱신, p95 ≤ 100/200 ms, gap/집계 명시, 늦은 합류, 사용자 입력 경로) + V-UDS-1 트랜잭션 표 > V-UDS-2 DID 플롯 > V-UDS-4 상태 패널 > V-UDS-3 DTC 표 > V-UDS-5 CAN 트레이스 연동. 추가: 결과 열 `may_have_executed`, 승인 토큰 원문 비노출(approval_ref만), 원시 24bit DTC 항상 보존.

### 15.3a CAN 팀 — `_xteam/canoe-bridge.md` (v1.2, B3)
CAN 팀이 브리지 v0 API 초안을 쓰고 UDS는 `## uds 의견`으로 요구를 적는다. UDS 요구: (1) 진단 요청을 **바이트 스트림**으로 송신하고 응답 바이트(긍정/NRC, 0x78 중간 응답이 보이면 그것도)를 비동기로 받는 호출 — 요청마다 Loom `request_id` 전달, (2) 측정 상태 이벤트(running/stopped/CANoe 종료)와 재연결 시 세대 번호(UDS epoch에 대응), (3) 응답 시각이 있으면 CANoe 시각, 없으면 COM 반환 시각과 품질 표시, (4) CANoe 진단 설정에서 대상(ECU 이름·주소·전송 종류)·TP 설정 조회, (5) 사용자 CAPL 함수 호출(`CAPL.GetFunction` — key 계산용), TesterPresent 시작·정지·상태(`DiagStart/StopTesterPresent`, `TesterPresentStatus`), 가능하면 DID·루틴 이름 열거, (6) CI용 가짜 브리지(같은 API). STOP latch는 core 파이프라인에서 송신 전에 걸리므로 브리지는 추가 정책이 없다.

### 15.3b v2.0 팀 간 문서
- `_xteam/uds-ui.md`(v0.1, UDS 작성): 패널 명세 U1–U9, 지연 목표, M1 Playwright 수용, UI 팀 질문 4개.
- `_xteam/uds-core.md`(v0.1, UDS 작성): C-1 훅 trait, C-2 승인 digest·invalidate, C-3 잡 API, C-4 safe_state, C-5 이벤트·snapshot provider·dry_run·시간 동기, C-6 테스트 증거, C-7 패턴 범위 승인, C-8 프로세스·메모리, C-9 역할·bootstrap, C-10 기록 상태, C-11 `check` 불일치 exit 3, C-12 자동 재연결 정책, C-13 러너 타이머 런타임.
- `_xteam/vn5640-eth.md`: CAN 초안이 없어 uds-lead가 파일을 만들고 `## uds 의견`을 적었고, 메인이 경로 (c) PC NIC + 미디어 컨버터로 확정(C4.1). (a)·(b)·`smoltcp`는 범위 밖.
- `_xteam/canoe-bridge.md`: 합의 보존, P4 선택.
- `_xteam/can-uds-isotp.md`: D11·A14·A16 그대로(ISO-TP 구현 언어만 Rust).

### 15.4 타팀 의존성 요약

| 의존 | 대상 | 필요 시점 |
|---|---|---|
| core gateway·IPC·이벤트 허브·CLI 프레임워크·기록기·정책/승인/감사 v0·Registry | **CORE 팀** P0 | M1 |
| 쓰기 파이프라인 훅 3개(A2)·잡 API(A1)·safe_state 복구 송신(A15) | CORE 팀 | M1(훅), M2(잡·복구) |
| 공통 승인 패널·정책 2층·러너 core | CORE 팀 P2 | M2–M3 |
| **UI 셸·패널 프레임워크·viz·Playwright 하네스**(v2.0) | UI 팀 | M1 |
| CORE stub crate·스키마(serde → JSON Schema → TS)·테스트 세션·soak 하네스·지연 하네스(브라우저 시간 동기) | CORE 팀 | P0–M2 |
| Vector XL FFI crate(CAN FD + 이더넷 프레임)·PCAN-Basic FD FFI·SocketCAN FD·Rust ISO-TP | CAN 팀 | M1b–M2 |
| ISO-TP 최소판(K10: normal 11bit, virtual + v0.2 필드 + barrier) | CAN 팀 | M1b (CAN M2) |
| ISO-TP 전체(K14) | CAN 팀 | M3b (CAN M4) |
| viz 렌더러·패널 프레임워크(V-UDS-1/2/4) | **UI 팀**(v2.0, CAN은 프레임·신호 데이터·XL/ISO-TP) | M1(표)–M3(플롯) |
| CANoe 브리지(Python 어댑터, 선택) + 진단 요청 API | CAN 팀 P4 | P4 |
| Vector 채널 공존 실측 결과 | CAN 팀 P3 | M4(Vector CAN-TP) |

---

## 16. 결정 기록

| ID | 쟁점 | 결정 | 이유 | 기각/보류안 |
|---|---|---|---|---|
| D1 | M1 전송 범위 | **DoIP sim 단독 M1**, CAN-TP는 M1b(CAN M2 ISO-TP 최소판 직후) | DoIP만으로 4요소 전부 증명 가능. CAN 두 초안 모두 ISO-TP를 M2/M4에 둠 → 동시 M1이면 걷는 뼈대 지연. R2에서 Sol도 양보 | 두 전송 동시 M1 (Sol R1) |
| D2 | udsoncan 사용 방식 | **자체 트랜잭션 러너 + udsoncan 코덱** (v2.0: Rust 자체 러너 + 자체 코덱 — D26) | P2를 첫 응답 시작(`rx_start`)으로 끊어야 하고(Client는 완성 PDU 기준), 0x78 제한·총 deadline·epoch·quarantine·abort·발행 순서를 한 곳에서 통제. M1 전송이 자체 DoIP라 Client 재사용 이득 작음. 러너 범위는 단일 요청 상태기로 좁게 | 데몬 내 `Client` + 이벤트 강제 커스텀 `BaseConnection`(Sol) — **되돌림 대안**. 관문 = U-06 러너 시험 목록(quarantine 오매칭 포함) 통과, 판정 시점 = 그룹 B 종료. 되돌림 경로도 같은 시험 통과 필수(스파이크 성공은 첫 프레임 타이밍·다중 LA·UI까지 검증한 것이 아님) |
| D3 | 프로세스 구성 | 사용자에게 `loomd` 하나, 내부는 gateway(core+UDS 세션 관리자) + CAN worker, `IsoTpPort` = PDU RPC(인프로세스와 동일 시그니처), 플러그인 서브프로세스 | CAN 팀 실측(같은 프로세스 CPU 부하 시 FC 지연 p99 51.9 ms)으로 CAN worker 분리(CAN CR1) 수용. UDS 스파이크로 PDU RPC 왕복 p99 3.09 ms 확인. drain barrier로 IPC 지연 중 timeout 오판 방지 | 단일 프로세스 인프로세스 ISO-TP(UDS Opus R1) — CAN 실측으로 기각 |
| D4 | DoIP 전송 | **자체 asyncio 전송**(v2.0: 자체 tokio 전송, 공개 crate 비채택 — D26)(엔티티·SA당 소켓 1개 + LA 분배, 단일 수신 pump), doipclient는 참고·상호운용 시험용 | doipclient 기본 경로의 타이머(2 s 고정 대기)·TLS(hostname 누락) 결함과 LA별 수신 분배·pump 보강 비용. 관문: M1(결함 시험 + doip-server smoke), M2(전체 상호운용 + HIL 스모크) — §7 | doipclient pin + pump·deadline·TLS 래퍼(Sol). 되돌리면 게이트웨이당 동시 대상 1개 제한 감수 |
| D5 | 위험 등급 | **R0–R4 + 서브기능 + DB 상향**(v1.2: env 보정 삭제 — B4), v1.1: 의미는 마스터 D6, 0x27 key R3, R4 = `default_blocked`(unlock 후 매번 뷰어 승인), 차단 3층(A11, 0x2A/0x86 `immutable_deny`) | 물리 영향 기준, 미구현·비가역 항목을 승인으로도 열지 못하게 | R0–R3 4단계(Sol) / v1.0의 "R4 = 항상 금지" |
| D6 | R1(세션 01/03) | 자동 + 뷰어 알림 + 감사, programming R3 | 가역(S3 복귀), 확인 피로 방지. Q7로 사용자 확인 | 최초 모드 변경 확인(Sol) |
| D7 | 승인 채널 | core 승인 서비스 사용(마스터 D6). R2 = `r2_path`(sim `auto` / bench·vehicle `token_or_viewer`, A8·B4), 토큰은 바이트·대상·epoch·현재값 결합(`approval_binding`), 재제출 `loom uds confirm`, R3 뷰어 전용(코드 끝 2자, 사용 기한 10분 A9), CLI approve 없음, origin은 hello role | 토큰은 사람 동의 증거가 아님을 명시, `ask` 규칙 없는 환경(Codex) 실HW는 viewer 권장 | preview → grant(발급 주체 미정) / v1.0 sim token_or_viewer |
| D8 | TesterPresent | **UDS 관리자 스케줄러**(유휴 시만, 인플라이트 억제), CAN 주기 PDU 기본 미사용 | 무조건 주기 송신은 활성 요청과 충돌. 관리자 사망 → TP 정지 → S3 복귀 | CAN `periodic_start`(Opus R1, 철회) |
| D9 | origin/dir | enum 유지, `purpose/actor_id/kind` 필드, 응답 origin=device + request_origin, 트랜잭션은 파생 레코드 | 공통 계약 변경 최소화 | `daemon` origin·`txrx` dir 추가(Opus R1, 철회) |
| D10 | VIN 마스킹 | 제품 기본값: 라이브·로컬 기록 원문, export 시 `--redact`. 마스킹을 켜면 에이전트·뷰어 동일 적용 | 02의 필수 조건은 "같은 표시 정책". 원문 보존은 진단 작업 편의를 위한 기본값(Q9로 확인). key는 항상 마스킹 | 기본 마스킹(Sol R1) |
| D11 | raw CAN 충돌 | 활성 ISO-TP tx ID로의 송신을 raw·replay·주기·IG 모두 기본 차단, 강제는 UDS 대상 quiesce 후 | 사용자 강제 송신도 CF/FC·응답을 깨뜨림. 강건성 주입 시험은 "UDS disconnect 후" 절차가 되돌림 경로 | 사용자 `--force` 예외(Opus R1) |
| D12 | 레이트 기본 | 대상당 5 req/s, burst 2, 폴링 5 Hz, bench 정책으로 상향 | 실ECU 보수성 | 20 req/s(Opus R1) |
| D13 | TLS 시점 | 요구 장비면 그 장비의 첫 실HW 수용 전 필수 관문, 아니면 M4 이후. 평문 fallback 금지 | 요구 여부가 장비에 따라 다름(Q8) | M2 일괄(Sol) / M5 일괄(Opus) |
| D14 | 플래시 | 실차 범용 비범위·R4, 시뮬레이터 상태기 M3, OEM 프로필 M5 선택 | 벽돌 위험, OEM별 형식 | — |
| D15 | 종료 코드 | 마스터 D5/A6 공통 0–10: 5 확인·승인, 6 LOCKED·QUARANTINE, 7 전송·ECU 무응답(`E_NO_RESPONSE`), 3 조건 대기 timeout만, 9 데몬, 10 NRC + `data.may_have_executed`(= outcome_unknown) | 전 도메인 통일, 스킬 분기 공통화 | v1.0 UDS 안(0/1/2 NRC/3 확인/4/5 전송/6/7 데몬) |
| D16 | STOP/abort와 finally | 마스터 A15: 정상 종료·on_error·에이전트 cancel → finally 실행(R0–R1 + safe_state). **사용자 STOP(`Ctrl-X`/stop-all)·UDS 대상 abort(`!`)** → 자동 실행 안 함, 뷰어 "복구 송신" 1키(user origin, latch 예외는 safe_state만, 인터록 통과 필수) | 긴급 정지 뒤 자동 송신은 사용자 의도와 충돌. IO 구동·통신 차단은 사람 1키로 복구 | v1.0: abort 시 finally 항상 skip / finally 항상 실행 |
| D17 | CDD | native 비범위 유지(등급·코덱 원천으로), 공개 파서 존재 사실은 정정 기재. v1.2: CANoe 쪽 이름은 표시용 가져오기만 검토(M2c) | cdd_utils 라이선스 미확인, gocan은 Go | — |
| D18 | origin 결정 | `hello` role로 core가 결정(요청 필드 무시, 마스터 D2). Python API·CLI = agent, core 러너 = script, viewer role = user | R4 반대 점검: 자기 신고 origin으로 사용자 완화(R2 즉시) 우회 가능 | 요청 필드 origin |
| D19 | timeout 후 늦은 응답 | **응답 식별자 기반 격리**: 미해결 요청 집합 U와 SID가 겹치는 요청은 R0 포함 차단, 겹치지 않는 요청만 정숙 구간 후 허용. U 해제 = 완전한 최종 응답(0x78 제외)의 late 도착 / DoIP 재연결 / 리셋 확인 / 검증 프로필의 `late_response_max_ms`. 프로브는 생존 확인일 뿐 해제 증거 아님 | UDS에 요청 id가 없어 같은 SID/DID 재요청에 이전 응답이 붙을 수 있음(폴링 패턴) | epoch를 재연결에만 증가 |
| D20 | DB와 등급 | DB는 상향만, 하향은 정책 `trusted_db` 해시만 | 에이전트가 DB를 작성·로드해 등급을 낮추는 우회 차단 | DB 등급 그대로 적용 |
| D21 | M1 승인 UX | M1에 viewer role + R2 승인 패널 최소(core 승인 API, 포커스 시 `Y`/`N`), R3·공통 승인 패널 통합은 M2(CORE P2) | E2E 사용자 개입이 origin=user 판별을 요구, 02 원칙상 화면 개입 필요 | M1은 채팅 토큰만(Sol R4 제안) |
| D22 | 기능 주소 정책 | v1.2: R1 이상 기능 주소 요청이 `[[uds.buses]]`/`gateways` 범위 밖이면 **확인 1회**(`functional_unscoped`, `r2_path` 경로) + 기능 주소 +1 등급. 0x2A/0x86은 `immutable_deny` | 미등록 ECU에 닿는 경로를 사람이 보게 하되, 하드 차단 기본 빈 목록(B4)과 맞춤 | v1.1 하드 차단(B4로 폐기) / +1 등급만 |
| D23 | `canoe` 백엔드 (B3; **v2.0 C4로 P4 선택 기능**, `auto` = 자체 스택) | Windows 전용 세 번째 전송. CAN 팀 브리지 위 `CanoeDiagTransport`, Loom이 바이트를 만들고 분류한 뒤 그대로 전달, 기본 경로 = 측정 중이면 `canoe` 아니면 자체 스택(Linux 항상 자체), 선택은 connect 때 한 번(자동 전환 없음), 한 대상 한 경로, 타이밍 출처 표시. M2c(P2) | 사용자가 진단을 CANoe로 수행 — CANoe의 진단 기술·seed-key·DoIP/CAN-TP 설정을 재사용하면서 승인·감사·실시간 화면은 Loom이 유지 | 이름 기반 CANoe 요청(분류 바이트 불명) / 측정 정지 시 자체 스택 자동 전환(테스터 중복) / 자체 스택 폐기(Linux 경로 없음) |
| D24 | vehicle 규칙 (B4) | vehicle = bench(등급·`r2_path`·기본값), env 보정·vehicle R0 기본·vehicle viewer 삭제, `env` 배지 상시(`meta.env`), 하드 차단 = 사용자 `hard_block`만(기본 빈 목록), 미등록 대상은 첫 R1+ 때 등록 확인 | 사용자 답: 벤치·실차 모두 쓰며 송신을 막을 이유 없음. R3/R4 뷰어 승인은 유지 | A10 vehicle tx deny·R0만, A8 vehicle viewer |
| D25 | 실HW 호스트 (B1) | Windows x64·Linux x64 둘 다 실HW 수용, macOS는 개발·sim. 체크리스트 §8.1 두 OS 나란히 | 사용자 답: 윈도우와 리눅스 타겟 | Windows 단일 실HW |
| D26 | 구현 언어·라이브러리 (C1) | Rust `loom-uds` crate: 자체 러너·자체 코덱·자체 tokio DoIP, `automotive_diag`는 정의만, TLS `tokio-rustls`, DB serde + 제한 YAML·`quick-xml`·`odx-rs`(M3 제한). Python 의존(udsoncan·doipclient·python-can·Textual) 제거 | 사용자: Python 코어가 느리고 메모리 문제로 죽음. 공개 DoIP crate는 0xE0 오류·길이 상한 없음, UDS crate는 상태 결합·GPL. Rust 스파이크 DoIP loopback p99 0.12 ms | 공개 crate 채택 / Python 유지 |
| D27 | 실HW 기본 경로 (C4·C4.1·C5) | 진단 기본 = 자체 스택. **CAN-TP over VN5640(CAN FD 전용)·PCAN-USB FD**, FD는 M2 첫 실HW부터. **DoIP over PC NIC + 미디어 컨버터(OS TCP/IP)** — VN5640 이더넷 경로 (a)·(b)·`smoltcp`는 범위 밖(§8.2) | 사용자가 이 구성으로 DoIP를 이미 사용 중이고, pyvxl·XL 이더넷은 쓰지 않음. 기존 DoIP 전송을 그대로 씀 | CANoe 경로 기본(v1.2 B3) / VN5640 XL 이더넷 + `smoltcp`(v2.0 초안) |
| D28 | 사용자 화면 (C6) | **Loom UI 앱 UDS 콘솔 패널**이 1차(UI 팀 셸 위, UDS는 내용·데이터 소유), TUI는 보조·M2 smoke, M1 수용 = Playwright 실제 DOM | 사용자: UI가 있어야 함. 웹은 같은 WS 프로토콜 | TUI 1차(v1) |
| D29 | 통합 테스트 지원 (C7) | `check`/`wait-for`/`--expect` 술어, `data.evidence`·`uds.check`, `verify_evidence` 훅, 부재 증명 규칙, 패턴 범위 사전 승인(`uds.plan_summary`), `auto_reconnect`(재전송 없음, 세션·보안 stale, U 해제는 D19 조건만) | 사용자: TC마다 코딩하지 않고 에이전트가 실행. 증거 없는 PASS 거부 | TC별 스크립트 |
| D30 | seed-key (C1·C2) | 데몬 밖 helper만: 외부 실행파일 JSON > DLL helper(x86=`i686` helper) > Python 외부 실행(대체 없을 때) > (P4) CAPL. in-process 플러그인 폐기 | x64 프로세스는 x86 DLL 로드 불가, 크래시 격리 | entry point 플러그인(v1) / CANoe 위임 우선(v1.2) |
| D31 | 큐·메모리 (C2) | §3.8 상한·넘침 정책, UDS 64 MiB, U는 eviction으로 해제하지 않음, 24 h soak UDS 부하 | 사용자: 메모리 문제로 죽음. 격리 안전과 메모리 상한 동시 만족 | 무상한 큐 |
| D32 | Python 잔존 (C1 근거 행) | ① `canoe` 어댑터(P4, CAN 팀 결정 — pywin32 STA·COM 이벤트 재사용, Rust `windows-rs` 불가 증거 없음 → 기술 부채로 기록) ② 고객 Python seed-key(대체 없을 때 외부 실행) ③ ODX 변환은 Python 불필요(실 PDX에서 `odx-rs` 불가 입증 시에만 odxtools 외부 worker) | C1 "어쩔 수 없을 때만" | — |
| v2.0 | **방향 전환**(마스터 §2.3 C1–C8) | C1 → D26·D32·§0.1·§7.0, C2 → D31·§3.8, C3 → 계약 유지·entry point 폐기, C4·C5 → D27·§8·§8.2, C6 → D28·§5.0, C7 → D29·§6.6, C8 → §10.10, M1 → §11(Rust + 웹) | 변경 목록 §16.3 | — |
| v1.2 | **사용자 답변 반영**(마스터 §2.2 B1–B7) | B1 → D25·§8·§8.1, B2 → Q1·Q11, B3 → D23·§3.7·M2c·U-27/28·§15.3a·리스크 16–18, B4 → D24·D5·D7·D22·§10, B5·B6 → UDS 영향 없음, B7 → Q11·Q12 | 변경 목록 §16.2 | — |

---

## 핵심 원칙 체크리스트 (02-core-principle.md) — R4 독립 점검 반영 후 최종

R4 결과: uds-sol `r4-check.md` = **PASS with fixes**(6항목 모두 PASS, P1 6건), uds-opus `r4-opus-check.md` = P1 12건(항목 2·4·5 조건부). 두 보고서의 P1 18건과 P2 대부분을 R4 반영본에 적용했고, 재확인(`r4b-recheck.md`)에서 나온 B1–B8(사용자 R1+ 요청의 암묵 takeover, M1 probe·approval wait·SPRMIB, `from_cursor`, 동기화 프로브 회복 관문, recover 순서·user 전용, `uds.confirm` 단일화, M3b 표기, Y/N·Q6 표기)도 반영했다. 이후 R4c–R4f에서 늦은 응답 격리 규칙(응답 식별자 기반 격리, 0x78은 U를 해제하지 않음)을 보완해 uds-sol 독립 점검 **FINAL PASS**(`r4c-final.md` R4f)를 받았다. 02 체크리스트 6개 항목 PASS(origin 연결 자격, env=sim, client id/`others_since_last`, DB 하향 금지, 기능 주소 범위, quarantine, D4 관문 일정, active_overrides 복구, 승인 키 안전, `confirm` 동사·`ask` 검증 M1, U-00 core 의존, 0xE0 확인, DTC 형식, 등급 표기 통일, M1 최소 fail-stop·dry-run, M3b 분리).

| # | 항목 | 충족 | 위치 |
|---|---|---|---|
| 1 | 맨 앞 "핵심 루프" 절 — 4요소 한 표 | ✅ | §0 표 + 불변식 5개 |
| 2 | 종단 간 시나리오 (에이전트 송신 → 장비 응답 → 사용자 화면 몇 ms → 사용자 개입) | ✅ | §2.0 E2E-1: connect → 22 F190 → 데몬 타임라인 표(렌더 목표 50/100 ms) → TUI 목업 → write+되읽기 → 사용자 `:` 송신(암묵 takeover) → 에이전트 `others_since_last`·`trace --since <from_cursor>` → LOCKED → abort → 늦은 합류 |
| 3 | 실시간성 목표 + 고부하 시 멈추거나 거짓말하지 않는 방법 | ✅ | §5.4(응답→TUI p95 ≤ 100 / p99 ≤ 200 ms, 웹 ≤ 200 ms, 측정 방법·표본), §5.5(폴링 집계, 0x78 카운터, TP 카운터, 표시 드롭·gap·STALLED·LAG·기록 장애 표시) |
| 4 | M1 = 걷는 뼈대 (시뮬레이션 위 4요소 최소형) | ✅ | §11 M1(sim DoIP, 실제 TUI 렌더 + 사용자 입력 자동화 수용, 헤드리스 수신만으로 불합격), M1b CAN-TP |
| 5 | 사용자 화면이 에이전트 세션과 분리되지 않음 | ✅ | §0 불변식 1·5, §3.1(CLI Client 기각), §3.3 TargetSession, §4.4 Python API도 데몬 경유, §5.3 snapshot/watermark attach, §10.9 스킬 금지 규칙, M1 수용 (3) |
| 6 | 원칙에 기여하지 않는 기능은 후순위/비범위 | ✅ | §1.2 후순위, §1.3 비범위, §11 "M1에 넣지 않는 것" |

v2.0 점검: 6항목을 웹 UI 기준으로 다시 확인했다. 1 §0 표(1차 화면 = Loom UI), 2 §11 M1 E2E-1(웹, t0–t6, Playwright DOM), 3 §5.0 지연 목표 + §5.3–§5.5·§3.8(고부하 집계·gap·우선 제어 큐·메모리 상한), 4 §11 M1(sim DoIP + 웹 최소 패널, 헤드리스 불합격), 5 §0 불변식 1·5·§5.0(패널 = 같은 구독·같은 쓰기 파이프라인), 6 §0.1·§11 "M1에 넣지 않는 것"·`canoe` P4.

v1.2 점검: 새 `canoe` 경로도 같은 6항목을 지킨다 — 요청은 Loom 파이프라인·이벤트를 그대로 지나고(항목 5), 사용자가 무엇을 언제 보는지는 §3.7 표, 타이밍·0x78·외부 요청 가시성의 한계는 화면에 명시(항목 3), M2c 수용은 "Loom 뷰어 행 없이 CANoe 화면에만 보이면 불합격"(항목 2·4). vehicle = bench(B4)는 화면·개입 경로를 바꾸지 않는다.

## 16.1 v1.1 마스터 정합 변경 (완료)

| 마스터 | UDS 변경 | 위치 |
|---|---|---|
| D1 | gateway(core+UDS 세션 관리자) + CAN worker, 동일 시그니처 `IsoTpPort` | §3.1 |
| D2/A2 | origin = hello role(Python API = agent), core 쓰기 파이프라인 + UDS 훅 3개(`classify`·`approval_binding`·`presend_check`), publish-before-reply | §0·§3.4·§3.6·§4.4 |
| A1 | UDS 요청 = 요청형, TesterPresent·폴링 = 잡형(단위마다 latch·레이트 재확인) | §3.3 |
| D3/A13/A14 | 이벤트 공통 필드·두 층 seq·`write_state`(effective_verified/effect_unverified 포함), `dir:null` 관리 이벤트, gap 3층(`source.gap` 포함), ISO-TP `tx_state: driver_accepted/bus_confirmed` | §3.4·§6.1 |
| D5/A6/A7 | 종료 코드 0–10(확인 5, NRC 10, ECU 무응답 7, 3 = 조건 대기만), JSON 봉투, `LOOM_CLIENT`, 커서 `@last`/`seq:N`, `NOTE:` 줄, 40행 기본 | §2.0·§4·§10.9 |
| D6/A8–A11 | 0x27 key R3, `r2_path`(sim auto), vehicle R0만 기본 허용(v1.2 B4로 폐기 — §16.2), 차단 3층(`default_blocked`/하드/`immutable_deny`), R3 사용 기한 10분, 정책 `loom.toml [uds]` + 사용자 `policy.toml` | §10.2·§10.3·§10.6 |
| D7/A12 | 기록 `~/.loom/records`(30일/20 GiB), 기록·감사 불능 시 agent·script R1+만 중단, 사용자 입력 허용 | §5.5·§6.2·§6.3 |
| D8 | CLI 데몬 자동 기동, `LOOM_NO_AUTOSTART`, exit 9 | §3.1·§4.2 |
| D9/A3/A5 | STOP `Ctrl-X`/`Ctrl-] x`(agent·script·잡 latch, 사용자 입력 유지, 상태 유지 배지), `/`·Space·q, `!` = 대상 abort, 구독 배치·적응형 flush, `no_viewer_attached` | §2.0·§5.2·§5.5·§10.7 |
| D10 | Registry 도메인 등록(U-01) | §12 |
| D11/A16 | 이중 승인 해소(UDS 정책 위임), `can.isotp.*`·`can.isotp_rx_start`, `can.isotp.barrier`, K10/K14 | §3.4·§15.2·§11·§12 |
| D12/A15 | core `loom run` + `uds.*` 어댑터, finally 규칙, 복구 송신 1키 core 공통, safe_state 목록 | §6.5·§10.7·D16 |
| D13 | 스킬 원본 `skills/loom-uds/` + `loom skills sync`, 공통 규칙은 `loom` 스킬, `Bash(loom * confirm:*)` | §10.9·U-12 |
| §3 | U-00 → CORE, CORE stub 위 P0, M1b ∥ M2, DoIP 실HW는 CAN과 독립 | §11·§12 |

## 16.2 v1.2 사용자 답변 반영 변경 (마스터 §2.2 B1–B7)

| 마스터 | UDS 변경 | 위치 |
|---|---|---|
| B1 | 실HW 호스트 Windows x64·Linux x64(macOS 개발·sim만), HW 매트릭스 열 교체, PCAN 두 OS, 실HW 체크리스트 두 OS 나란히, M4·U-26 두 OS 수용 | §8·§8.1·§11·§12 |
| B2·B7 | 보유 장비(Vector+CANoe, PCAN-USB, Saleae) 반영, 남은 질문 Q11(모델·버전·기본 경로)·Q12(seed-key 위임) | §14 |
| B3 | `canoe` 백엔드(Windows): 바이트 전달·분류·승인·감사·이벤트 그대로, 기본 경로 규칙, 자동 전환 없음, 한 대상 한 경로, 타이밍 출처, TP·seed-key·DB 이름 처리, 사용자가 보는 것 표, Vector 이더넷 ↔ 자체 DoIP 충돌 리스크, M2c·U-27/28, `_xteam/canoe-bridge.md` 요구 | §0·§1·§3.6·§3.7·§4.1·§5.2·§6.1·§9.3·§11·§12·§13·§15.3a |
| B4 | vehicle = bench(env 보정·vehicle R0 기본·vehicle viewer 삭제), `meta.env` 상시, 하드 차단 = 사용자 `hard_block`(기본 빈 목록), 미등록 대상·범위 밖 기능 주소는 차단 대신 확인 1회, 활성 ISO-TP tx ID raw CAN은 사용자 목록과 별개인 채널 예약 규칙(CAN 집행, exit 4 `tx_reserved`) | §4.1·§4.2·§9.3·§10.1–§10.3·§10.6·D5·D7·D22·D24 |
| B5·B6 | UDS 영향 없음(EXPLORE·UART 범위) | — |

## 16.3 v2.0 방향 전환 변경 (마스터 §2.3 C1–C8)

| 마스터 | UDS 변경 | 위치 |
|---|---|---|
| C1 | Rust `loom-uds` crate, 자체 러너·코덱·tokio DoIP, Python 의존 제거, Python 잔존 근거(D32) | §0.1·§3.1·§7.0·D26·D32 |
| C2 | 큐 상한·넘침 정책 표, UDS 64 MiB, helper 예산, 24 h soak UDS 부하와 관문 초안, 타이머 런타임 요청 | §3.8·§9.3·D31·`_xteam/uds-core.md` C-8·C-13 |
| C3 | 계약(D2–D7·A·B4) 유지, entry point 플러그인 폐기, 스키마 = Rust 타입 | §0.1·§10.8 |
| C4·C4.1·C5 | CAN-TP는 VN5640(CAN FD 전용)·PCAN FD, DoIP는 PC NIC + 미디어 컨버터(C4.1 확정, (a)·(b) 범위 밖·U-31 취소), `canoe` P4 선택(`auto` = 자체 스택) | §3.7·§8·§8.1·§8.2·D23·D27 |
| C6 | UDS 콘솔 패널 U1–U9 명세·지연 목표, TUI 보조, viewer bootstrap 보안 | §5.0·`_xteam/uds-ui.md`·D28 |
| C7 | check·wait-for·expect 술어, 증거 이벤트·`verify_evidence`, 부재 증명, 패턴 범위 사전 승인, 자동 재연결 | §6.6·U-29·D29 |
| C8 | 스킬 골격·`help --json` 정본·`loom-test` 경계 | §10.10·U-32 |
| M1 | Rust + 웹 E2E-1(Playwright DOM, 키보드 입력, 30분 RSS) | §11·U-10·U-11 |
