# UDS (DoIP + CAN-TP) 계획 — R1 초안 (uds-opus)

작성: uds-opus (Claude Opus 5.5), 2026-11-09
기준: `docs/plan/02-core-principle.md` (최우선) > `00-brief.md` > `uds/brief.md`. `_xteam/can-uds-isotp.md` v0.1, `_xteam/viz-requirements.md` `## uds` 절을 참고해 맞췄다.
무게: 아키텍처, 에이전트/사용자 UX, 스킬 설계, 안전 규칙. 라이브러리 사실은 PyPI JSON API로 확인한 것만 출처 표기, 나머지는 "추정/미확인".

---

## 0. 핵심 루프 (02-core-principle)

> 에이전트가 ECU에 직접 진단 요청을 보내 읽고 쓰고, 사용자는 **같은 트랜잭션을 같은 순간에** 라이브 트레이스로 보며 언제든 끼어든다.

| 요소 | UDS에서의 구현 | 담당 구성요소 | 첫 등장 |
|---|---|---|---|
| ① 직접 접근 | 에이전트가 `loom uds connect --doip <ip> --la 0x1001` / `--can ch0 --tx 0x7E0 --rx 0x7E8` / `--sim` 한 줄로 ECU 연결. 상주 프로세스(`loomd`)의 **UDS 세션 관리자**가 DoIP TCP 소켓과 CAN 데몬의 ISO-TP 채널을 소유. CLI는 IPC 클라이언트일 뿐 | `loom.uds.service` (loomd 안), DoIP 전송, CAN 데몬 ISO-TP | M1 (sim DoIP) |
| ② 읽기 | `read-did`, `dtc list`, `status`, `poll`, `trace --tail` — 경계 있는 명령, 사람용 디코드 출력 + `--json`, NRC를 이름·원인·다음 행동 힌트로 해석 | 트랜잭션 엔진 + 진단 DB 디코더 | M1 |
| ③ 쓰기 | `session`, `security`, `write-did`, `dtc clear`, `routine`, `io`, `reset`, `raw`, `run scenario.yaml` — 모든 경로가 데몬의 **단일 가드 관문**(위험 등급 분류 → 정책 → 확인)을 통과 | 가드(`loom.uds.guard`) + 정책 파일 + 승인 채널 | M1 (raw/write 최소), M2 (전체 가드) |
| ④ 사용자 실시간 표시 | 데몬이 트랜잭션 이벤트를 **에이전트에게 응답을 돌려주기 전에** 이벤트 버스로 발행 → `loom uds view`(TUI, herdr 패인) / viz 웹이 구독. 요청 송신 순간 "진행 중" 행, 응답 시 확정 행. origin(agent/user/script/keepalive) 표시. 사용자는 뷰어에서 직접 요청 송신·승인·제어권 회수·긴급 정지 | 이벤트 버스, `loom uds view`, viz V-UDS-1 | M1 |

설계 불변식 (이 문서 전체가 지키는 규칙):

1. **ECU와 통신하는 코드는 데몬 안에만 있다.** CLI 프로세스 안에서 udsoncan `Client`를 열어 ECU에 직접 붙는 구조는 금지(숨은 세션). 사용자가 쓰는 뷰어 송신도 같은 데몬 API를 쓴다.
2. **발행 후 응답(publish-before-reply)**: 데몬은 트랜잭션 완료 이벤트를 이벤트 버스에 먼저 넣고 나서 RPC 결과를 CLI에 돌려준다. 사용자는 에이전트보다 늦게 보지 않는다.
3. **숨김 없음**: TesterPresent, 시나리오 단계, 폴링, 사용자 수동 요청까지 전부 같은 스트림에 기록·표시된다. 접기(collapse)는 표시 방식일 뿐 데이터는 빠지지 않는다.
4. **단일 가드 관문**: 타입 명령, `raw`, 시나리오, 뷰어 수동 송신, Python API 모두 데몬의 `guard.check()`를 통과해야 전송 계층에 닿는다.

---

## 1. 범위 / 비범위

### 1.1 범위 (핵심 루프에 직접 기여)

| 영역 | 내용 | 마일스톤 |
|---|---|---|
| 전송 DoIP | UDP 13400 차량 탐색(식별 요청/공지 수신), TCP 13400 라우팅 활성화, alive check 응답, 게이트웨이 뒤 다중 논리 주소, 진단 메시지 ACK/NACK 해석 | M1(sim), M3(실HW) |
| 전송 CAN-TP | CAN 데몬 ISO-TP 채널 위(`_xteam/can-uds-isotp.md`), normal/extended/mixed, 11/29bit, functional SF, CAN FD | M3 |
| UDS 서비스 | 0x10, 0x11, 0x14, 0x19(주요 서브기능), 0x22, 0x27(seed-key 플러그인), 0x28, 0x2E, 0x2F, 0x31, 0x3E(자동 유지), 0x85, raw 임의 요청. NRC 0x78 처리, P2/P2* 타이밍 | M1(0x22/0x19/0x10/raw), M2, M4 |
| 세션 상태 | 데몬이 활성 세션·보안 레벨·TesterPresent·P2 값·제어권(lease)을 보유 | M1(최소), M2 |
| 사용자 가시성 | 라이브 트랜잭션 트레이스(TUI), 상태 바, 승인 패널, 사용자 수동 송신, 긴급 정지 | M1(최소), M2 |
| 안전 | 위험 등급표, raw 분류, 확인 토큰, 뷰어 승인, 화이트리스트, 레이트 제한, 감사 로그, dry-run, 시나리오 사전 위험 요약 | M1(분류+R0/R2 최소), M2(전체) |
| 진단 DB | Loom YAML/JSON DID·DTC·Routine 정의(이름, 스케일, 단위, 등급) | M1(최소 DID), M4 |
| 시나리오 | YAML 단계 실행, 사전 위험 요약, 단계별 트레이스 | M4 |
| DID 폴링 → 플롯 | 데몬 폴링 작업 → 이벤트 스트림 → viz 플롯(V-UDS-2) | M4 |
| 가상 ECU | UDS 서버 엔진 + DoIP 엔티티 + CAN-TP 노드, 결함 주입 | M1(DoIP), M3(CAN-TP) |
| 에이전트 스킬 | `.claude/skills/loom-uds/` + Codex 호환 | M1(최소 SKILL.md), M2 |

### 1.2 후순위 (루프는 돌지만 질을 높이는 것)

- ODX/PDX 가져오기(odxtools → Loom YAML 변환) — M5.
- Windows seed-key DLL 브리지(32bit DLL 프로세스 분리 포함) — M5.
- DoIP TLS(포트 3496) — M5. OEM 인증서·요구가 확인될 때.
- 웹 뷰어 전용 고급 화면(V-UDS-3 DTC diff, V-UDS-5 CAN 트레이스 심볼릭 묶기) — viz 팀 일정에 맞춤.
- pcapng 내보내기(DoIP 트래픽, Wireshark 확인용) — M5.
- 0x23/0x2A/0x2C/0x3D/0x86/0x87/0x29/0x84 타입 명령(raw로는 M1부터 가능, 등급 분류는 M1부터 적용).

### 1.3 비범위 (이번 계획에서 하지 않음)

- **플래시(0x34/0x35/0x36/0x37/0x38) 타입 명령과 플래시 시퀀스**: OEM마다 블록 형식·서명·지우기 루틴·체크섬이 다르고 벽돌(brick) 위험이 크다. raw로 보내는 것도 기본 R4(금지). M6 "선택"으로 플러그인 인터페이스만 남긴다(§10.2, §11).
- CDD(CANdela) 직접 파싱: 공개 파서 존재 미확인. CANdelaStudio의 ODX 내보내기 경로(추정)로 대체.
- OBD-II(SAE J1979) 전용 모드 해석, J2534 패스스루 장치, K-Line(ISO 14230).
- DoIP 서버(차량 측) 제품화 — 가상 ECU용 최소 구현만.
- 원격(네트워크 너머) 진단 — 데몬은 로컬 IPC 전용.

---

## 2. 사용 시나리오

### 2.0 종단 간 시나리오 (핵심 루프 실증) — E2E-1: VIN 읽기 → 사용자 개입

전제: 사용자가 herdr 오른쪽 패인에서 `loom uds view`를 띄워 두었다. 에이전트는 왼쪽 패인의 Claude Code.

**t=0 에이전트 연결**
```
$ loom uds connect --doip 192.168.0.10 --la 0x1001 --name bcm
connected bcm  DoIP 192.168.0.10:13400  tester 0x0E80 → ECU 0x1001
routing activation: 0x10 success (type 0x00)  rtt 8 ms
session default (assumed)  security locked  policy: bench / max auto R1
```
뷰어 상태 바가 같은 순간 바뀐다: `bcm ● connected  sess=default  sec=locked  TP off  lease: agent`.

**t=1 에이전트 읽기**
```
$ loom uds read-did F190 -t bcm
bcm  22 F190  →  62 F190  rtt 14 ms
F190  VIN  = "WVWZZZ1KZAW000001"
```
데몬 내부 순서와 사용자 화면:

| 시각(데몬 기준) | 사건 | 사용자 TUI |
|---|---|---|
| 0.0 ms | CLI RPC 수신 → 가드 R0 자동 허용 | — |
| 0.3 ms | DoIP TCP 송신, `uds.tx` 이벤트 발행 | 행 추가: `12:03:41.512 A bcm ReadDataByIdentifier F190 … ⏳` (≤ 50 ms 내 표시, 목표) |
| 14.1 ms | ECU 응답 `62 F1 90 57 56 57 …` 수신, 디코드 | — |
| 14.4 ms | `uds.txn` 완료 이벤트 발행(버스) | 같은 행이 확정: `✓ VIN="WVWZZZ1KZAW000001" 14ms` (수신 후 ≤ 100 ms p95, 목표) |
| 14.6 ms | RPC 결과 CLI 반환 | — |

```
┌ loom uds view ─ bcm ● sess=default sec=locked TP off  P2 50/5000ms  lease: agent ───┐
│ time          o  ecu  service                     summary                   res  rtt │
│ 12:03:40.990  A  bcm  RoutingActivation            tester 0x0E80             ✓    8ms │
│ 12:03:41.512  A  bcm  ReadDataByIdentifier         F190 VIN="WVWZZZ1KZAW0…"  ✓   14ms │
└──────────────────────────────────────────────────────────────── live · lag 9 ms ─────┘
```

**t=2 에이전트가 NRC를 받음**
```
$ loom uds read-did F1A0 -t bcm
bcm  22 F1A0  →  7F 22 31  rtt 11 ms
NRC 0x31 requestOutOfRange — ECU가 이 DID를 지원하지 않거나 현재 범위 밖
hint: DB에 없는 DID. `loom uds db find F1A0` 또는 `loom uds read-did --scan F1A0-F1AF` (R0)
exit 2
```
사용자 화면: `✗ 0x31 requestOutOfRange` 빨간 행.

**t=3 사용자 개입 (뷰어에서 직접 요청)**
사용자가 TUI에서 `:` 키 → 명령줄에 `22 F18C` 입력 → Enter.
```
│ 12:03:55.204  U  bcm  ReadDataByIdentifier         F18C ECUSerial="SN12345"  ✓   12ms │
```
origin `U`로 같은 스트림에 남고, 에이전트는 `loom uds trace --since last -t bcm`으로 사용자가 무엇을 했는지 그대로 본다:
```
$ loom uds trace -t bcm --since last
12:03:55.204 U ReadDataByIdentifier F18C ECUSerial="SN12345" ✓ 12ms
(1 transaction since your last call; cursor=c-000412)
```

**t=4 사용자 제어권 회수**
사용자가 TUI에서 `T`(takeover) → 상태 바 `lease: user`. 에이전트가 쓰기를 시도하면:
```
$ loom uds session extended -t bcm
LOCKED: bcm 제어권을 user가 가지고 있음 (since 12:04:02). 읽기(R0)만 가능.
hint: 사용자에게 제어권 반환을 요청하세요 (뷰어에서 R 키).
exit 6
```

이 시나리오는 M1 수용 테스트의 골격이다(§11, 가상 ECU 위에서 자동화).

### 2.1 에이전트 관점

**A1. 버전·식별 정보 수집 (R0만)**
사용자: "이 BCM 소프트웨어 버전이랑 부품번호 알려줘."
```
loom uds status -t bcm                     # 연결/세션 확인
loom uds read-did F190 F187 F189 F18C F195 -t bcm --json
```
에이전트는 JSON 결과를 표로 요약. 사용자는 같은 5개 트랜잭션을 뷰어에서 본다(다중 DID 한 요청이면 1행).

**A2. DTC 진단과 삭제 (R0 → R2)**
```
loom uds dtc list -t bcm --mask 0x09          # testFailed|confirmed
loom uds dtc snapshot B1234 -t bcm            # 0x19 04
loom uds dtc ext B1234 -t bcm                 # 0x19 06
```
에이전트가 원인 추정을 설명 → 사용자가 "지워"라고 함 →
```
$ loom uds dtc clear -t bcm
CONFIRM REQUIRED  R2  ClearDiagnosticInformation (0x14) group FFFFFF (all)
  target bcm (bench)  현재 DTC 3개: B1234 U0100 C0561
  token K7Q2 (120 s, 이 요청 1회 한정)
  → 사용자에게 위 내용을 보여주고 승인받은 뒤 같은 명령에 --confirm K7Q2
exit 3
```
에이전트는 채팅에 요약을 보이고 승인을 받은 뒤 `--confirm K7Q2`로 재실행(Claude Code 권한 프롬프트가 한 번 더 뜸, §10.4). 이후 `dtc list`로 재확인.

**A3. 가변 코딩 DID 쓰기 (세션 + 보안 + R2/R3)**
```
loom uds session extended -t bcm                    # R1 자동 + 알림, TP 자동 시작
loom uds security --level 1 -t bcm                  # R2 확인 → seed-key 플러그인
loom uds read-did 0x0101 -t bcm                     # 현재값 확인 (읽기 우선 규칙)
loom uds write-did 0x0101 --field region=EU -t bcm --dry-run
loom uds write-did 0x0101 --field region=EU -t bcm  # R3(DB에서 protected) → 뷰어 승인 대기
loom uds read-did 0x0101 -t bcm                     # 되읽기 검증
loom uds reset soft -t bcm                          # R2 확인
```

**A4. 시나리오 회귀 실행**
```
loom uds run scenarios/bcm-smoke.yaml --plan        # 사전 위험 요약만
loom uds run scenarios/bcm-smoke.yaml --confirm P3X9 --json
```
사용자는 뷰어 상단에 `scenario bcm-smoke 4/9 ▶ security L1`를 실시간으로 본다.

**A5. DID 폴링을 사용자 플롯으로**
```
loom uds poll F40D,F405 -t ecm --period 200ms --duration 60s --viz
```
데몬 폴링 작업이 시작되고(에이전트 CLI는 즉시 job id 반환), 사용자 브라우저/TUI에 플롯이 뜬다. 에이전트는 `loom uds poll stats <job>`로 min/max/마지막 값만 받는다(컨텍스트 보호).

**A6. 미지 차량 탐색**
```
loom uds discover --doip --iface en7 --timeout 2s
loom uds discover --can ch0 --functional --rx-range 0x7E8-0x7EF --timeout 500ms
```

### 2.2 사용자 관점

**U1. 에이전트 작업 실시간 관찰** — herdr 패인에서 `loom uds view`. 에이전트 행은 `A`, 내 행은 `U`, 시나리오 `S`, 자동 TesterPresent `K`(기본 접힘, 상태 바 카운터). 행 선택 후 Enter로 hex + 디코드 트리 + 위험 등급 + 승인 기록.

**U2. 위험 동작 승인** — 에이전트가 R3 요청을 보내면 뷰어 하단에 승인 패널이 뜬다(§5.3). 사용자가 내용 확인 후 `y`(또는 `n`). 승인자가 감사 로그에 `user@viewer`로 남는다.

**U3. 직접 개입** — `:` 명령줄에서 `22 F190`, `10 03`, `read F190`, `dtc` 같은 짧은 입력으로 요청 송신. 같은 가드가 적용되나 사용자 origin은 R2까지 즉시 실행(본인이 사람이므로 확인 생략), R3은 y/n 1회, R4 금지 동일.

**U4. 제어권 회수와 긴급 정지** — `T` 제어권 회수, `!` 긴급 정지(큐 비움, 시나리오/폴링 중단, 진행 중 승인 취소, TP 정지 → ECU는 S3 후 기본 세션 복귀). CLI로도 `loom uds abort`.

**U5. DID 플롯 보기** — viz 웹 또는 TUI 미니 스파크라인에서 폴링 값을 실시간으로 본다. 다른 창에서 CAN 트레이스와 시간 커서 동기화(V-UDS-5).

**U6. 사후 검토** — `loom uds audit --since today`로 누가 무엇을 승인했는지, `loom uds trace --file <session>.jsonl`로 재생. (사후 열람은 보조 기능. 주 기능은 라이브.)

---

## 3. 아키텍처

### 3.1 결정: UDS 세션 관리자는 상주 데몬 안의 모듈

**제안: 단일 상주 프로세스 `loomd` 안에 도메인 모듈(`core`, `uart`, `can`, `uds`)을 올린다.** UDS 모듈(`loom.uds.service`)이 DoIP 소켓을 소유하고, CAN-TP는 같은 프로세스의 CAN 모듈이 호스팅하는 ISO-TP 채널을 **인프로세스 API**로 쓴다.

근거:
- CLI는 명령마다 종료하므로 세션 상태(활성 세션, 보안 해제, TesterPresent 타이머)는 상주 주체가 들고 있어야 한다. 02 원칙상 그 주체가 사용자 뷰어가 구독하는 같은 이벤트 버스에 있어야 한다.
- CAN-TP 경계 v0.1의 PDU API를 인프로세스로 쓰면 IPC 왕복이 0이고, IPC로 분리해도 같은 인터페이스(§15.2)로 동작하게 설계해 CAN 팀의 데몬 구성 결정에 종속되지 않는다.
- 감사 로그, 승인 채널, 제어권(lease), 기록기를 도메인 간 공유(§15.1 공통 계약 변경 요청).

대안:
- (B) `loom-udsd` 독립 데몬 + CAN 데몬 IPC로 ISO-TP 사용: 장애 격리가 좋지만 이벤트 버스/승인/감사가 둘로 나뉘고 뷰어가 두 곳에 붙어야 한다. 공통 계약이 "도메인별 데몬"으로 결정되면 이 형태로 간다 — UDS 모듈은 `IsoTpPort` 인터페이스 뒤에서 바뀌지 않음.
- (C) CLI가 udsoncan Client를 직접 연다: **02 원칙 위반, 기각.**

장애 격리 보완: loomd가 도메인 모듈을 각각 asyncio 태스크 그룹으로 돌리고, 모듈 예외는 해당 모듈만 재시작. CAN 고부하로 UDS가 굶지 않도록 ISO-TP는 CAN 쪽 스레드(Notifier), UDS 트랜잭션 엔진은 이벤트 루프(저부하)로 분리(추정 — CAN 팀 구성에 맞춰 확정).

### 3.2 프로세스 구성도

```
 ┌─ Claude Code / Codex (에이전트) ─┐     ┌─ 사용자 ────────────────────────────┐
 │  loom uds read-did F190 ...      │     │ herdr 패인: loom uds view (TUI)     │
 │  (단발 CLI, 비대화형, --json)     │     │ 브라우저: loom viz (웹)             │
 └───────────────┬──────────────────┘     └──────────┬───────────────▲─────────┘
                 │ RPC (요청/응답)                   │ RPC(수동 송신,  │ 이벤트 구독
                 │                                   │  승인, takeover)│ (uds.*, can.*)
 ════════════════╪═══════════ 로컬 IPC (127.0.0.1 WebSocket JSON-RPC + 토큰) ═══╪══════
 ┌───────────────▼───────────────────────────────────────────────────┴───────┐
 │ loomd (상주 데몬, 장치 단독 소유)                                          │
 │ ┌─ core ──────────────────────────────────────────────────────────────┐   │
 │ │ RPC 라우터 · 이벤트 버스(seq 번호) · 기록기(JSONL/BLF) · 감사 로그   │   │
 │ │ 승인 서비스(approvals) · 제어권(lease) · 정책 로더(사용자 전용 파일) │   │
 │ └───────▲──────────────────────────▲──────────────────────▲───────────┘   │
 │ ┌───────┴──────── uds ─────────────┴──────────┐   ┌──────┴── can ──────┐  │
 │ │ UdsService                                   │   │ 채널(python-can)   │  │
 │ │  ├ TargetRegistry: bcm, ecm, …               │   │ ISO-TP 호스트       │  │
 │ │  │   └ TargetSession(상태·큐·lease·TP)       │   │ (can-isotp,        │  │
 │ │  ├ Guard(분류 → 정책 → 확인)  ◀ 단일 관문    │   │  FC/STmin 인프로세스)│ │
 │ │  ├ TxnEngine(P2/P2*/0x78, 재시도 금지 기본)  │   │                    │  │
 │ │  ├ Codec(udsoncan services) + DiagDB 디코더  │   │                    │  │
 │ │  ├ Poller · ScenarioRunner · SeedKeyHost     │   │                    │  │
 │ │  └ Transports: DoipTransport ─┐  IsoTpPort ──┼──▶│ isotp.open/send/rx │  │
 │ └───────────────────────────────┼──────────────┘   └─────────┬──────────┘  │
 └─────────────────────────────────┼────────────────────────────┼─────────────┘
                                   │ TCP/UDP 13400 (TLS 3496)    │ CAN/CAN FD
                           ┌───────▼────────┐             ┌──────▼──────┐
                           │ DoIP 게이트웨이 │             │ CAN ECU     │
                           │  └ ECU 0x1001…  │             │ 0x7E0/0x7E8 │
                           └────────────────┘             └─────────────┘
       (sim: loom.uds.sim 가상 DoIP 엔티티 / python-can virtual bus 위 가상 CAN-TP 노드)
```

### 3.3 세션 상태를 누가 들고 있나

`TargetSession` (데몬 메모리, 대상 ECU당 1개)이 유일한 진실 원천. CLI·뷰어는 이벤트(`uds.state`)로 사본을 받는다.

| 상태 | 값 | 갱신 규칙 |
|---|---|---|
| `link` | disconnected / connecting / connected / error | 전송 계층 이벤트 |
| `session` | default(01) / programming(02) / extended(03) / 0xNN, `since`, `confidence`(confirmed/assumed/stale) | 0x10 긍정 응답 시 confirmed. 연결 직후 assumed=default. NRC 0x7E/0x7F 수신, 예기치 않은 0x11, 링크 재연결 시 stale. `status --probe`가 DID 0xF186(activeDiagnosticSession, 지원 시)으로 확인 |
| `security` | locked / unlocked(level n), `since` | 0x27 key 긍정 응답 시 unlocked. 세션 변경·리셋·S3 만료·NRC 0x33 수신 시 locked |
| `p2`, `p2_star` | ms | 0x10 응답의 P2server_max / P2*server_max(10 ms 단위)로 갱신, 기본 50 / 5000 |
| `tp` | off / on(period, functional?), `last_ok`, `fail_count` | 비기본 세션 진입 시 자동 on(정책), 기본 세션 복귀·연결 해제·유휴 만료 시 off |
| `lease` | free / held(owner, origin, mode=shared\|exclusive, ttl) | §3.5 |
| `inflight` | 진행 중 트랜잭션 1개(또는 없음) + 큐 길이 | TxnEngine |
| `policy` | env(bench/vehicle), max_auto_grade, whitelist 항목 | 정책 파일(사용자 소유) |

**TesterPresent 유지자**는 TargetSession에 속한다. 주기 2 s(S3 5 s 대비 여유), `3E 80`(suppressPosRsp). 지난 2 s 안에 다른 요청이 나갔으면 건너뛴다. CAN-TP는 `_xteam` §5의 `isotp.periodic_start`를 쓰면 CAN 데몬 쪽에서 지터 없이 돌고, 소유자 끊김 시 자동 정지(안전 쪽 실패). DoIP는 UDS 모듈 타이머.
**유휴 만료**: 모든 클라이언트로부터 요청이 10분(정책 값) 없으면 TP를 멈춘다 → ECU가 S3 후 기본 세션으로 복귀. 아무도 보지 않는 비기본 세션이 무기한 유지되지 않게 한다. 만료 직전 뷰어에 경고.

### 3.4 데이터 흐름 (요청 1건)

```
CLI ─rpc uds.request{target, req_bytes|typed, origin=agent, client_id, confirm?}─▶ UdsService
  1. Codec: 타입 명령 → 바이트 (raw는 바이트 그대로)
  2. Guard.classify(bytes, target, db)        → grade R0..R4, 사유
  3. Guard.policy(grade, target, origin, env) → allow | confirm(R2) | approve(R3) | deny(R4)
       confirm/approve 이면 Approval 생성 → uds.approval 이벤트 발행 → CLI에 exit 3 (토큰)
  4. Lease 확인 (grade ≥ R1 이면 제어권 필요)
  5. RateLimiter
  6. TxnEngine.enqueue(target)   # 대상당 동시 1건
       └ transport.send → 발행 uds.tx {seq, txn_id, origin, req_hex, service, grade}
       └ 0x78 수신마다 발행 uds.pending {txn_id, count, elapsed}
       └ 최종 응답/타임아웃 → 디코드 → 상태 갱신 → 발행 uds.txn {…완료…}, uds.state(변경 시)
  7. 감사 로그 append (grade ≥ R1 또는 거부/승인 사건)
  8. RPC 결과 반환  ← 반드시 6의 uds.txn 발행 이후
```

### 3.5 다중 클라이언트 · 다중 ECU 직렬화와 락

**원칙**: UDS는 테스터-ECU 쌍마다 반이중(요청 하나에 응답 하나)이다. 동시성은 데몬이 흡수하고, 클라이언트는 몰라도 되게 한다.

규칙:
1. **대상(ECU)당 인플라이트 1건.** 대상별 FIFO 큐. 우선순위: `abort` > user > agent/script > keepalive(TP는 큐가 비었을 때만, 그리고 최근 트래픽이 있으면 생략).
2. **서로 다른 대상은 병렬.** CAN: 대상마다 ISO-TP 채널이 달라 독립. DoIP: 한 게이트웨이 TCP 연결 위에 여러 논리 주소를 다중화(ISO 13400은 테스터 소스 주소당 소켓 하나 — 그래서 데몬이 엔티티당 소켓 1개를 소유하고 공유). 게이트웨이별 `max_parallel`(기본 1, 보수적) 정책.
3. **제어권(lease)**: 대상마다 하나. 
   - R0 읽기: lease 없이 누구나. 큐에서 섞여 실행.
   - R1 이상: lease 소유자만. lease가 비어 있으면 첫 요청자가 자동 획득(TTL 120 s, 활동 시 연장).
   - `exclusive` 모드: 시나리오 실행, security seed→key 쌍, (향후) 플래시 시퀀스 동안 다른 클라이언트의 R0까지 큐에서 대기시킨다(중간 끼어들기로 requestSequenceError가 나는 ECU 대비). 최대 보유 시간 제한(정책, 기본 300 s).
   - **사용자 우선**: 뷰어 `T`(takeover)는 즉시 lease를 사용자에게 넘긴다. 에이전트의 다음 쓰기 요청은 `LOCKED`(exit 6). 진행 중 exclusive 작업은 현재 단계 응답 후 중단.
   - 에이전트끼리(예: Claude + Codex 동시) 경합은 먼저 잡은 쪽 우선, 상대는 exit 6 + 소유자 정보.
4. **복합 트랜잭션**: `security`(27 odd → 27 even)는 하나의 원자 단위로 큐를 점유.
5. **기능 주소 요청**(CAN functional, DoIP 기능 논리 주소): 해당 전송(채널/게이트웨이)의 **모든 대상**에 영향을 주므로 전송 범위 lease가 필요. 진행 중 대상 트랜잭션이 끝난 뒤 실행. 락 획득 순서는 항상 전송 → 대상(교착 방지).
6. **외부 테스터 공존**: 같은 CAN ID를 다른 도구(CANoe 진단 콘솔 등)가 쓰면 응답이 섞인다. CAN 데몬이 같은 tx_id 외부 송신을 감지하면(`_xteam` §2) UDS가 대상 상태를 `stale`로 표시하고 뷰어 경고.

### 3.6 IPC 인터페이스 (UDS 도메인 RPC/이벤트)

전송 방식은 공통 결정을 따른다(제안: 127.0.0.1 WebSocket + JSON-RPC 2.0, 데몬 시작 시 생성한 토큰 파일로 인증, Origin 헤더 검사 — 브라우저 CSRF로 로컬 웹소켓에 붙는 공격 방지).

| RPC | 설명 |
|---|---|
| `uds.connect{name, transport, params}` / `uds.disconnect{target}` | 대상 생성/해제 |
| `uds.targets{}` / `uds.status{target, probe?}` | 상태 조회 |
| `uds.request{target, typed? , raw_hex?, origin, confirm?, dry_run?, timeout_ms?}` | 단일 관문 |
| `uds.poll.start{target, dids, period_ms, duration_s}` / `.stop` / `.stats` | 폴링 작업 |
| `uds.scenario.plan{file}` / `.run{file, confirm}` / `.status` | 시나리오 |
| `uds.lease.acquire/release/takeover` | 제어권 |
| `uds.approval.respond{id, decision}` | **뷰어 연결 전용**(§10.4) |
| `uds.abort{target?}` | 긴급 정지 |
| `uds.trace.query{target?, since_cursor?, tail?, filter}` | 경계 있는 조회 |
| `uds.discover{transport, …}` | 탐색 |

이벤트(구독 토픽): `uds.tx`, `uds.pending`, `uds.txn`, `uds.state`, `uds.approval`, `uds.lease`, `uds.poll.sample`, `uds.scenario.step`, `uds.alert`, `uds.heartbeat`(1 s). 모든 이벤트는 단조 증가 `seq`를 가진다(뷰어가 누락 감지, §5.5).

---

## 4. CLI / API 명세 초안

### 4.1 공통

- 형식: `loom uds <verb> [args] [-t <target>] [--json] [--timeout <dur>] [--dry-run]`.
- `-t` 생략 시 대상이 하나면 그것, 둘 이상이면 오류(묵시적 선택 금지 — 엉뚱한 ECU에 쓰기 방지).
- 숫자: `F190`, `0xF190` 모두 16진수로 해석(DID/RID/SID 맥락). 10진수는 `--dec` 또는 `#61840`.
- **출력 상한**: 사람용 기본 40줄, `--json` 기본 32 KB. 초과 시 잘라내고 `truncated: true` + 전체를 쓴 파일 경로(`--out` 자동 제안). 바이트 덤프는 기본 64바이트까지만 hex 표시(`--full`).
- **커서**: 각 CLI 호출은 클라이언트 id별 마지막 본 `seq`를 데몬이 기억 → `--since last`로 "내 마지막 호출 이후 사용자/다른 주체가 한 일"을 경계 있게 본다.
- 종료 코드:

| 코드 | 의미 |
|---|---|
| 0 | 긍정 응답 / 성공 |
| 1 | 사용법 오류, DB 오류 |
| 2 | 부정 응답(NRC) |
| 3 | 확인/승인 필요(토큰 발급됨) |
| 4 | 정책 거부(R4, 화이트리스트 밖, 레이트 제한) |
| 5 | 전송 오류 / 타임아웃 / 연결 없음 |
| 6 | 제어권 없음(LOCKED) |
| 7 | 데몬 없음 |

### 4.2 명령 체계

| 명령 | 예 | 서비스 | 기본 등급 |
|---|---|---|---|
| `discover` | `discover --doip [--iface en7] [--timeout 2s]` / `discover --can ch0 --functional --rx-range 0x7E8-0x7EF` | DoIP 0x0001/0x0004, CAN 0x3E/0x22 F190 functional | R0 |
| `connect` | `connect --doip 192.168.0.10 --la 0x1001 [--ta-tester 0x0E80] [--activation 0x00] [--tls] --name bcm` / `connect --can ch0 --tx 0x7E0 --rx 0x7E8 [--addressing normal_11] [--fd] --name ecm` / `connect --sim [profile] --name sim1` | 전송 | R0 |
| `disconnect` | `disconnect -t bcm` | — | R0 |
| `targets` / `status` | `status -t bcm [--probe]` | (probe: 0x22 F186) | R0 |
| `session` | `session default\|extended\|programming\|0x40 -t bcm` | 0x10 | 01/03: R1, 02: R3 |
| `tp` | `tp on\|off [--period 2s] [--functional]` | 0x3E | R0 |
| `security` | `security --level 1 [--plugin oem_bcm] -t bcm` / `security --seed-only` | 0x27 | R2 |
| `read-did` | `read-did F190 F18C [--raw] [--scan F100-F1FF]` | 0x22 | R0 |
| `poll` | `poll F40D,F405 --period 200ms --duration 60s [--viz] [--out f.jsonl]` / `poll stats <job>` / `poll stop <job>` | 0x22 반복 | R0 (레이트 제한) |
| `write-did` | `write-did F198 --value "TESTER01"` / `--hex 54455354` / `--field region=EU` | 0x2E | R2 (DB `protected` 시 R3) |
| `dtc list` | `dtc list [--mask 0x09] [--limit 50]` | 0x19 02 | R0 |
| `dtc count/snapshot/ext/supported` | `dtc snapshot B1234 [--record 01]` | 0x19 01/04/06/0A | R0 |
| `dtc clear` | `dtc clear [--group FFFFFF]` | 0x14 | R2 |
| `dtc-setting` | `dtc-setting off\|on` | 0x85 | R2 |
| `comm` | `comm disable-tx --type normal` | 0x28 | R2 (차량 모드 R3) |
| `routine` | `routine start 0x0203 [--data 01]` / `stop` / `result` | 0x31 | DB 등급, 미등록 R3 |
| `io` | `io 0x4F01 adjust --value 50` / `return` / `freeze` / `default` | 0x2F | R3 (차량 모드 R4) |
| `reset` | `reset hard\|soft\|key-off-on` | 0x11 01/03/02 | R2 (차량 모드 R3) |
| `raw` | `raw 22F190 [--functional] [--no-wait]` | 임의 | 바이트로 분류(§10.3) |
| `run` | `run scenario.yaml [--plan] [--confirm <tok>] [--from-step 3]` | 시나리오 | 단계 최대 등급 |
| `trace` | `trace [-t bcm] [--tail 20] [--since last\|<cursor>\|-30s] [--service 0x22] [--nrc] [--origin user]` | — | — |
| `view` | `view [-t bcm] [--compact]` (TUI, 사용자용) | — | — |
| `lock` | `lock acquire\|release [--exclusive]` | — | — |
| `abort` | `abort [-t bcm]` | — | — |
| `db` | `db load bcm.yaml -t bcm` / `db show F190` / `db find vin` / `db validate f.yaml` | — | — |
| `audit` | `audit [--since today] [--grade R2+]` | — | — |
| `policy` | `policy show` (편집·리로드는 사용자 전용, §10.6) | — | — |
| `agent-help` | 스킬 없는 에이전트용 규칙 요약 출력 (Codex 등) | — | — |
| `flash` | **비범위** (M6 선택). 존재하지 않는 명령 → 안내 메시지 | 0x34-37 | R4 |

### 4.3 출력 예시

**read-did (사람용)**
```
$ loom uds read-did F190 F40D -t ecm
ecm  22 F190 F40D  →  62 F190 … F40D …  rtt 18 ms
F190  VIN            = "WVWZZZ1KZAW000001"
F40D  VehicleSpeed   = 0 km/h     (raw 0x00)
```

**read-did --json**
```json
{
  "ok": true, "target": "ecm", "txn_id": "t-000418", "seq": 9311,
  "request": "22F190F40D", "response": "62F190575657…F40D00",
  "rtt_ms": 18.2, "pending_count": 0,
  "values": [
    {"did": "F190", "name": "VIN", "value": "WVWZZZ1KZAW000001", "unit": null, "raw": "5756575A…"},
    {"did": "F40D", "name": "VehicleSpeed", "value": 0, "unit": "km/h", "raw": "00"}
  ],
  "state": {"session": "default", "security": "locked"},
  "cursor": "c-000418"
}
```

**NRC (--json)**
```json
{"ok": false, "target": "bcm", "request": "2EF19854455354", "response": "7F2E33",
 "nrc": {"code": "0x33", "name": "securityAccessDenied",
         "meaning": "보안 접근이 해제되지 않음",
         "hint": "loom uds security --level <n> 후 재시도 (R2 확인 필요)"},
 "rtt_ms": 9.8, "exit": 2}
```

**0x78 pending 진행 (사람용, 실시간 한 줄 갱신은 TTY일 때만)**
```
$ loom uds routine start 0xFF01 -t bcm
bcm  31 01 FF01  →  7F 31 78 ×4 (pending 3.9 s, P2* 5000 ms)  →  71 01 FF01 00  rtt 4.12 s
routine FF01 checkProgrammingDependencies: started, status 0x00 (ok)
```

**dtc list**
```
$ loom uds dtc list -t bcm --mask 0x09
bcm  19 02 09  →  59 02 FF …  3 DTC  rtt 21 ms
DTC      code   status  TF TFTOC PD CD TNCSLC TFSLC TNCTOC WIR  description
B1234-00 0x9234 0x09    1  0     0  1  0      0     0      0    Door lock motor circuit open
U0100-00 0xC100 0x2F    1  1     1  1  0      1     0      0    Lost comm with ECM
C0561-71 0x4561 0x08    0  0     0  1  0      0     0      0    ESC system disabled
(3 of 3)
```

**status**
```
$ loom uds status -t bcm
bcm  DoIP 192.168.0.10 tester 0x0E80 → 0x1001   link connected 00:12:40
session  extended (confirmed 12:05:11)   security L1 unlocked (12:05:14)
TP       on 2.0 s (last ok 0.6 s ago)    P2 50 ms / P2* 5000 ms
lease    agent:claude-1 shared, ttl 104 s
policy   bench · whitelist ✓ · max auto R1 · rate 20/s
queue    0 inflight 0     viewers 1 (tui)
```

**dry-run**
```
$ loom uds write-did 0x0101 --field region=EU -t bcm --dry-run
DRY-RUN (전송 안 함)
  request   2E 0101 02 1F        (DB VariantCoding: region=EU(0x02), features=0x1F 유지)
  current   0101 = region=KR(0x05) features=0x1F   (cache 12:06:02, 재확인 권장)
  grade     R3  (DB: protected=true)
  needs     session extended ✓, security L1 ✓
  policy    viewer 승인 필요 + --risk-ack R3
  rate      write-did 2/10 per min
```

### 4.4 Python API (데몬 클라이언트, 스크립트·테스트용)

```python
from loom.client import Loom
lm = Loom.connect()                       # 데몬 토큰 파일 자동
bcm = lm.uds.target("bcm")
r = bcm.read_did(0xF190)                  # → UdsResult(values, rtt_ms, raw…)
bcm.write_did(0xF198, "TESTER01", confirm=token)   # 가드 동일
```
이 API도 데몬 RPC 래퍼다. ECU에 직접 붙는 경로는 제공하지 않는다.

---

## 5. 사용자 뷰어 UX

### 5.1 결정: TUI(`loom uds view`) 를 M1 1차 화면, viz 웹은 M4부터

- 근거: 사용자는 herdr 안에서 에이전트 옆 패인으로 본다(공통 §3-5). TUI는 의존성이 적고 M1 걷는 뼈대에 바로 들어간다. UDS 트래픽은 저속(초당 수십 건 이하)이라 TUI로 충분.
- 프레임워크: Textual(추정 — CAN 팀 TUI 선택과 통일). 대안: rich Live(가볍지만 입력 처리 약함), prompt_toolkit.
- 웹: viz V-UDS-1/2/4를 CAN 팀 렌더러에 요청(`_xteam/viz-requirements.md`). 같은 이벤트 토픽을 구독하므로 데이터 경로는 동일.

### 5.2 메인 화면

```
┌ loom uds view ───────────────────────────────────────────────────────────────────────┐
│ bcm ● DoIP 0x1001  sess=extended✓ sec=L1🔓 TP 2.0s ✓×143  P2 50/5000  lease: agent  │
│ ecm ● CAN 7E0/7E8  sess=default   sec=🔒   TP off          P2 50/5000  lease: free   │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ time          o ecu service                 summary                         res  rtt │
│ 12:05:11.020  A bcm DiagSessionControl      → extended (P2 50, P2* 5000)    ✓    9ms │
│ 12:05:11.031  · bcm TesterPresent           (keepalive 시작, 접힘)                    │
│ 12:05:13.880  A bcm SecurityAccess          L1 seed 4B → key ✓ [plugin oem_bcm] ✓ 31ms│
│ 12:05:20.402  A bcm ReadDataByIdentifier    0101 VariantCoding region=KR    ✓   12ms │
│ 12:05:31.114  U ecm ReadDataByIdentifier    F40D VehicleSpeed=0 km/h        ✓   16ms │
│ 12:05:40.950  A bcm RoutineControl          start FF01 ⏳ pending ×3  3.1s/5.0s        │
│ 12:05:44.002  A bcm WriteDataByIdentifier   0101 region=EU   ⚠R3 승인 대기            │
├─ detail (Enter) ─────────────────────────────────────────────────────────────────────┤
│ txn t-000431  A claude-1  grade R3 (DB protected)  policy: viewer approval            │
│ req  2E 01 01 02 1F                                                                   │
│ diff region: KR(0x05) → EU(0x02)   features: 0x1F (unchanged)                         │
├─ APPROVAL ───────────────────────────────────────────────────────────────────────────┤
│ ⚠ R3  agent claude-1 → bcm WriteDataByIdentifier 0101 region KR→EU   [y] 승인 [n] 거부 │
│      bench · extended · L1 unlocked · expires 98 s                                    │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ : 22 F190_                         live · lag 9 ms · seq 9402 · rec ● uds-20261109.jsonl│
│ [:]send [T]akeover [R]elease [!]abort [f]ilter [k]eepalive show [d]tc [p]oll [?]help │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

요소:
- **상태 바(대상별)**: 연결, 세션(✓ confirmed / ? assumed / ! stale), 보안, TP 카운터, P2/P2*, lease 소유자.
- **origin 열**: `A` agent(에이전트 이름은 detail), `U` user, `S` script/scenario, `·` keepalive(daemon), `!` 데몬 경고. 색: A 청록, U 노랑, S 보라, NRC 빨강.
- **결과 열**: `✓` 긍정, `✗ 0x31 requestOutOfRange` NRC 이름, `⏳` 진행 중, `⌛` 타임아웃, `⛔` 가드 거부, `⚠R3` 승인 대기.
- **rtt 열**: P2 대비 색(≤ P2 녹색, P2 초과지만 0x78로 연장 시 노랑, 타임아웃 빨강). pending 중에는 경과/한도가 실시간으로 올라감.
- **NRC 해석**: detail에 이름·의미·흔한 원인·다음 행동(예: 0x22 conditionsNotCorrect → "차속/엔진 상태/전압 조건 확인", 0x37 requiredTimeDelayNotExpired → "보안 잠금 지연 중, N초 후 재시도 — 반복 시도 금지").
- **명령줄(`:`)**: `22 F190`(raw hex), `read F190`, `session ext`, `dtc`, `clear`(→ y/n). 대상은 선택된 상태 바 행.
- **승인 패널**: R3 요청과 R2 확인 대기 항목. 사용자는 여기서 승인/거부(§10.4).

### 5.3 보조 화면

- `d` DTC 표(V-UDS-3 TUI 버전): 상태 비트 분해, clear 전후 diff.
- `p` 폴링 패널: DID별 마지막 값, 미니 스파크라인(최근 60 샘플), 샘플/드롭 카운터. 큰 플롯은 `o`로 viz 웹 열기.
- `s` 시나리오 패널: 단계 목록, 현재 단계, 각 단계 결과.
- 컴팩트 모드(`--compact`): herdr 좁은 패인용, 상태 바 1줄 + 트레이스 열 축약.

### 5.4 실시간성 목표

| 경로 | 목표 (p95, 로컬, 저부하) | 측정 방법 |
|---|---|---|
| 요청 송신 → 뷰어 "진행 중" 행 | ≤ 50 ms | `uds.tx` 이벤트 ts vs 뷰어 렌더 ts (테스트 훅) |
| ECU 응답 수신 → 뷰어 확정 행 (TUI) | **≤ 100 ms** | 응답 수신 ts(DoIP 소켓 read / isotp.rx ts_last_frame) → 렌더 |
| ECU 응답 수신 → viz 웹 표 | ≤ 200 ms | 동일, 브라우저 |
| 응답 수신 → 에이전트 CLI 결과 | ≤ 30 ms(데몬 처리) | RPC 반환 ts |
| 사용자 `!` abort → 큐 비움·전송 중단 | ≤ 100 ms (진행 중 ISO-TP 멀티프레임은 현재 PDU 완료 또는 N_* 타임아웃까지) | |
| DID 폴링 샘플 → 플롯 점 | ≤ 250 ms | |

보장 순서: 뷰어 표시가 에이전트 결과보다 늦을 수는 있어도(렌더 지연), **데몬 발행은 항상 먼저**(불변식 2).

### 5.5 고부하에서 멈추거나 거짓말하지 않는 법

원칙: **기록은 절대 버리지 않고, 화면은 집계·드롭을 명시한다.** 뷰어 렌더는 20 Hz로 묶어 갱신하고, 데몬 → 뷰어 큐가 차면 오래된 것부터 버리되 버린 수를 표시한다.

| 부하 상황 | 화면 처리 | 표시 |
|---|---|---|
| 빠른 DID 폴링(예 4 DID × 20 Hz) | 폴링 트랜잭션은 표에서 **폴링 그룹 1행**으로 집계: `poll#3 F40D,F405 ×1,204 last 42 km/h  rtt 12/15/31 ms  NRC 0` (1초마다 갱신). 개별 샘플은 플롯·기록에 전부 | 행에 `×N`과 `집계` 배지. `k`로 펼치면 최근 200개만 |
| 0x36 대량 전송(플래시 범위 들어올 경우) | 블록 단위 행 대신 **진행 행 1개**: `TransferData blk 0x5A 368/1024 KB 12.3 KB/s ETA 53 s`, ≤ 10 Hz 갱신. NRC 블록만 개별 행 | `집계` 배지 |
| 0x78 연속 | 행을 늘리지 않고 **같은 행의 카운터** `pending ×12 (P2* 5.0 s 리셋마다)` + 총 경과 | 카운터가 멈추면 마지막 pending 이후 경과를 빨강으로 |
| TesterPresent 반복 | 상태 바 `TP ✓×143 last 0.6 s`. 실패·NRC·스킵만 개별 행 | `k`로 펼침 |
| 뷰어 처리 지연 | 큐 상한(예 5,000 이벤트) 초과 시 오래된 표시용 이벤트 드롭 | 표 상단 고정 배너 `⚠ 표시 드롭 1,234건 (기록은 완전) — seq 9100–10333` |
| seq 누락(재연결 등) | 누락 구간을 표에 **gap 행**으로 삽입, 재연결 후 `trace.query`로 구간 재요청(가능하면 채움) | `── gap seq 9200–9260 (재요청 중/채움 완료) ──` |
| 데몬 정지/스트림 끊김 | `uds.heartbeat` 1 s 미수신 3회 → 화면 전체 회색 + `STALLED 3.4 s` | 거짓 "live" 표시 금지 |
| 렌더 뒤처짐 | 하단 `live · lag 9 ms` → 500 ms 초과 시 노랑, 2 s 초과 빨강 `LAG` | |
| 플롯 다운샘플 | viz가 LTTB 등 사용 시 범례에 `1:8 downsampled` | CAN 팀 viz 규칙 따름 |

---

## 6. 데이터 / 기록 포맷

### 6.1 이벤트 레코드 (공통 §3-6 확장)

라이브 이벤트와 기록 파일이 같은 스키마(JSONL 한 줄 = 한 이벤트). 트랜잭션 완료 레코드(`uds.txn`) 예:

```json
{
  "seq": 9311,
  "ts": {"mono_ns": 81234567890123, "wall": "2026-11-09T12:03:41.526Z"},
  "domain": "uds", "kind": "txn", "channel": "bcm", "dir": "txrx",
  "origin": "agent", "client": "claude-1",
  "corr_id": "t-000418",
  "payload": {
    "ecu": {"name": "bcm", "transport": "doip", "addr": "0x1001", "tester": "0x0E80",
            "doip": {"ip": "192.168.0.10", "entity": "0x1000"}},
    "sid": "0x22", "service": "ReadDataByIdentifier",
    "req_hex": "22F190", "resp_hex": "62F190575657…",
    "positive": true, "nrc": null, "nrc_name": null,
    "decoded": [{"did": "F190", "name": "VIN", "value": "WVWZZZ1KZAW000001"}],
    "t_req": {"mono_ns": 81234553700000}, "t_resp": {"mono_ns": 81234567800000},
    "rtt_ms": 14.1, "pending_count": 0, "p2_ms": 50, "p2_star_ms": 5000,
    "risk": "R0", "approval": null,
    "state_after": {"session": "default", "security": "locked"},
    "aggregate": null
  }
}
```

- `kind`: `tx`(송신 시점), `pending`, `txn`(완료), `state`, `approval`, `lease`, `poll_sample`, `scenario_step`, `alert`, `gap`.
- `dir`: 공통 계약은 rx/tx. UDS 트랜잭션은 쌍이므로 `txrx` 추가 요청(§15.1). 기록 시 `tx`, `txn` 둘 다 남겨 원시 순서도 재구성 가능.
- `corr_id`: 요청↔응답↔pending↔승인↔감사 연결. CAN-TP면 CAN 트레이스의 ISO-TP 프레임에도 같은 `corr_id`를 주석으로 붙이도록 CAN 팀에 요청(V-UDS-5).
- 타이밍: CAN-TP는 `_xteam` §3대로 `t_req = tx_done.ts_last_frame`, `t_resp = rx.ts_first_frame`. DoIP는 소켓 send 완료/recv 시각(소프트웨어, 정밀도 ~ms 추정). `rtt_ms`는 판정용, `pending_count`/개별 pending ts 별도.
- 보안: `0x27` key 바이트는 기록에 **마스킹**(`"key": "<masked 4B sha256:ab12…>"`). seed는 기록(재현에 필요, OEM 비밀 아님 — 정책으로 마스킹 선택 가능).

### 6.2 파일 배치 (제안, core가 최종 결정)

```
$LOOM_HOME/                       (기본 ./.loom/  — 프로젝트 단위)
  sessions/2026-11-09T12-00-00/
    uds.jsonl          # 위 이벤트 전부 (TP 포함, 집계 안 함)
    can.blf            # CAN 팀 (ISO-TP 프레임 포함)
    meta.json          # 대상 목록, DB 해시, 정책 해시, 버전
  audit/uds-2026-11.jsonl          # 감사 로그 (별도, 해시 체인)
  poll/<job>.jsonl                 # 폴링 long format (viz 입력)
```

- 폴링 long format(V-UDS-2): `{"ts":…, "series":"ecm.F40D.VehicleSpeed", "value":42.0, "unit":"km/h", "ok":true}`. 실패 샘플은 `value:null, nrc:"0x22"`(플롯에 빈 칸으로 — 0으로 그리지 않음).
- 내보내기: `loom uds export <session> --format csv|jsonl` (트랜잭션 표), pcapng(DoIP, 후순위).

### 6.3 감사 로그

- append-only JSONL, 레코드마다 `prev_hash`(SHA-256 체인)로 변조 흔적 탐지(방지가 아니라 탐지).
- 남기는 것: R1 이상 모든 요청(허용/거부/취소), 승인 발급/응답(승인자 origin, 채널: chat-token/viewer), 정책 로드(파일 해시), lease 이동, abort, 레이트 제한 발동, 가드 분류 결과와 사유.
- 남기지 않는 것: R0 읽기 내용(세션 기록에 있음), seed-key 비밀.

### 6.4 진단 DB (Loom YAML 스키마 초안)

```yaml
loom_diag: 1
ecu:
  name: bcm
  description: Body Control Module (bench sample)
  addressing:                       # connect 기본값으로 쓸 수 있음
    doip: {logical: 0x1001, gateway: 0x1000}
    can:  {channel: ch0, tx: 0x726, rx: 0x72E, addressing: normal_11}
  timing: {p2_ms: 50, p2_star_ms: 5000, s3_ms: 5000}
  security:
    - {level: 1, plugin: oem_bcm, algo_arg: 0x1234}   # 플러그인 이름만, 비밀은 플러그인 쪽
sessions: {default: 0x01, programming: 0x02, extended: 0x03, eol: 0x40}

dids:
  - id: 0xF190
    name: VIN
    type: ascii
    length: 17
    access: {read: any}
  - id: 0xF40D
    name: VehicleSpeed
    type: uint8
    scale: 1.0
    offset: 0
    unit: km/h
    access: {read: any}
  - id: 0x0101
    name: VariantCoding
    length: 2
    type: struct
    fields:
      - {name: region, bits: "0:8", type: enum, values: {0x01: US, 0x02: EU, 0x05: KR}}
      - {name: features, bits: "8:16", type: bitfield,
         flags: {0: drl, 1: auto_lock, 2: mirror_fold, 3: welcome_light, 4: rear_wiper}}
    access: {read: any, write: {session: extended, security: 1}}
    risk: {write: R3}               # protected: 기본 R2보다 상향
    protected: true
  - id: 0x2010
    name: BatteryVoltage
    type: uint16
    endian: big
    scale: 0.001
    unit: V
    limits: {min: 0, max: 20}       # 쓰기 시 범위 검사 (쓰기 가능 DID일 때)

routines:
  - {id: 0x0203, name: SelfTestLamps, risk: R2, session: extended, params: [{name: mode, type: uint8}]}
  - {id: 0xFF00, name: EraseMemory, risk: R4}
  - {id: 0xFF01, name: CheckProgrammingDependencies, risk: R1}

io:
  - {id: 0x4F01, name: DoorLockActuator, risk: R3, type: uint8, values: {0: unlock, 1: lock}}

dtcs:
  - {code: 0x923400, display: B1234-00, text: "Door lock motor circuit open"}

types:                               # 재사용 타입 (선택)
  temp_c: {type: int16, scale: 0.1, unit: "°C"}
```

- 지원 타입: `uint8/16/32/64`, `int*`, `float32/64`, `ascii`, `utf8`, `bcd`, `bytes`(hex), `enum`, `bitfield`, `struct`(필드 비트 범위), `array`(count + item 타입). 엔디안 기본 big.
- DB 없는 DID는 hex + ASCII 추정 표시(표준 F1xx 이름표는 내장: F186, F187, F189, F18A, F18C, F190, F191, F195, F197, F19E 등).
- 디코드는 udsoncan `DidCodec` 체계로 컴파일(추정 — 맞지 않으면 자체 디코더). 
- 멀티 ECU: 파일 하나 = ECU 하나. `loom uds db load`로 대상에 바인딩, 데몬이 해시를 기록.
- **ODX/PDX**: odxtools(MIT, 11.7.1 — PyPI 확인)로 PDX를 읽어 Loom YAML로 **변환**하는 단방향 도구(M5). 런타임은 Loom YAML만 읽는다(디코더 하나 유지). 
- **CDD**: 비범위(§1.3).

### 6.5 시나리오 YAML

```yaml
loom_scenario: 1
name: bcm-coding
target: bcm                     # 정책 화이트리스트와 대조
db: diag/bcm.yaml
on_error: stop                  # stop(기본) | continue
vars: {region: EU}
steps:
  - session: extended
  - security: {level: 1}
  - read-did: [0x0101]
    save: before                # 이후 단계에서 ${before.0101.region}
  - write-did: {did: 0x0101, fields: {region: "${region}"}}
  - read-did: [0x0101]
    expect: {0x0101.region: "${region}"}   # 불일치 시 on_error
  - reset: soft
  - wait: 3s
  - status: {probe: true}
  - raw: "22F18C"               # raw 도 계획 시점에 분류
finally:                        # 성공/실패/abort 무관 실행 (R0–R1만 허용)
  - session: default
```
- 단계 이름은 CLI 동사와 동일(학습 비용 최소). 반복·분기는 `repeat: {count, until}` 정도만(튜링 완전 금지 — 위험 요약이 정적으로 가능해야 함).
- 실행 중 각 단계는 origin=`script`, `scenario_step` 이벤트로 뷰어 시나리오 패널에 표시.

---

## 7. 라이브러리 선택과 근거

PyPI JSON API(`https://pypi.org/pypi/<pkg>/json`)로 2026-11-09 확인한 값. 기능 한계·내부 구조 평가는 uds-sol 조사로 대체·보강할 것(여기서는 "추정"으로 표기).

| 패키지 | 버전(업로드일) | 라이선스 | Python | 용도 | 판단 |
|---|---|---|---|---|---|
| udsoncan | 1.26.1 (2026-06-25) | MIT | ≥3.7 | 서비스 요청 인코드/응답 파싱, NRC 표, DidCodec | **채택(코덱으로)**. 트랜잭션 엔진(큐·P2·0x78·lease·이벤트)은 자체 구현. `Client`는 데몬 안에서도 쓰지 않는 것을 기본으로 함 — 우리 관문/이벤트 통제가 필요하기 때문. 대안: 데몬 안에서 커스텀 `BaseConnection`으로 `Client` 사용(구현 빠름, 이벤트 발행 지점 통제 약함) |
| doipclient | 1.3.0 (2026-11-09) | MIT | ≥3.6 | DoIP 클라이언트(탐색, 라우팅 활성화) | **참고/초기 사용**. 한 TCP 위 다중 논리 주소·alive check·asyncio 통합 필요 → M1은 얇은 자체 asyncio DoIP 전송(헤더 8바이트 단순 프로토콜)을 제안, doipclient는 비교 테스트 상대·탐색 로직 참고. Sol 조사 결과로 확정 |
| can-isotp | 2.0.7 (2025-05-14) | MIT | ≥3.7 | ISO-TP (CAN 데몬 안) | CAN 팀 소유. UDS는 PDU API만 사용 |
| python-can | 4.6.1 (2025-08-12) | LGPL-3.0-only | ≥3.9 | CAN 백엔드 | CAN 팀 소유. LGPL — 수정 없이 동적 사용이면 배포 부담 적음(추정) |
| odxtools | 11.7.1 (2026-10-20) | MIT | ≥3.10 | PDX → Loom YAML 변환 | M5, 선택 의존성(`loom[odx]`) |
| scapy | 2.8.0 (2026-10-02) | GPL-2.0-only | ≥3.7,<4 | DoIP/UDS 계층 해석 | **채택 안 함**(GPL 전염 위험). 필요 시 개발자 개인 디버깅 도구로만 |
| Textual | 미확인 | MIT(추정) | — | TUI | CAN 팀과 통일 |

자체 구현 범위(작게 유지): DoIP 프레이밍/라우팅 활성화/alive check, 트랜잭션 엔진, 가드, 가상 ECU 서버 엔진(udsoncan은 클라이언트 전용으로 알려짐 — 추정).

---

## 8. HW / OS 지원 매트릭스

| 전송 / 장치 | macOS (개발) | Windows (실HW 유력) | Linux | 비고 |
|---|---|---|---|---|
| 가상 ECU (sim DoIP, loopback) | ✅ M1 | ✅ M1 | ✅ M1 | CI 필수 |
| 가상 ECU (sim CAN-TP, python-can virtual) | ✅ M3 | ✅ M3 | ✅ M3 | CAN 팀 virtual 백엔드 |
| DoIP 실차/벤치 (이더넷) | ✅ | ✅ | ✅ | OS 무관. DoIP 활성화선(OBD 8번 핀) 지원 ENET 케이블 또는 벤치 직결 필요. 탐색 브로드캐스트는 `--iface` 지정 권장(다중 NIC) |
| DoIP TLS 3496 | ⚠ M5 | ⚠ M5 | ⚠ M5 | Python `ssl`로 가능(추정). OEM 인증서/PKI 의존 |
| CAN-TP via Vector XL | ❌ | ✅ | ❌ | CAN 팀 매트릭스 따름 |
| CAN-TP via PCAN | ⚠ (PCBUSB/MacCAN, 미확인) | ✅ | ✅ | CAN 팀 매트릭스 따름 |
| CAN-TP via SocketCAN/vcan | ❌ | ❌ | ✅ | Linux CI 보조 |
| seed-key Python 플러그인 | ✅ | ✅ | ✅ | |
| seed-key DLL | ❌ | ✅ (M5, 32bit DLL은 별도 32bit 브리지 프로세스) | ❌ | |

---

## 9. 시뮬레이션 & 테스트 전략

### 9.1 가상 ECU (`loom.uds.sim`)

- **서버 엔진**(전송 무관): 진단 DB YAML + `sim:` 절로 구성.
  ```yaml
  sim:
    initial: {0xF190: "WVWZZZ1KZAW000001", 0xF40D: 0, 0x0101: "051F"}
    signals:  {0xF40D: {wave: sine, min: 0, max: 120, period_s: 20}}   # 폴링 플롯 데모
    sessions: {s3_ms: 5000, allow: [0x01, 0x02, 0x03]}
    security: {1: {algo: xor, const: 0x1234, max_attempts: 3, lockout_s: 10}}
    dtcs: [{code: 0x923400, status: 0x09}]
    faults:
      - {match: "31 01 FF01", respond: [{nrc: 0x78, times: 4, every_ms: 1000}, {hex: "7101FF0100"}]}
      - {match: "2E 0101", when: {session: default}, nrc: 0x7F}
      - {match: "22 F1A0", nrc: 0x31}
      - {match: "*", delay_ms: {dist: uniform, min: 5, max: 20}}
      - {match: "22 2010", drop: 0.05}           # 응답 누락 → 타임아웃 시험
  ```
  세션/보안 상태기, S3 만료, 0x78 시퀀스, NRC 주입, 지연/누락 주입, 리셋 시 상태 초기화.
- **DoIP 엔티티**(M1): UDP 13400 식별 응답/공지, TCP 라우팅 활성화(성공/거부 코드 주입), alive check, 게이트웨이 뒤 다중 논리 주소(ECU 여러 개 = YAML 여러 개).
- **CAN-TP 노드**(M3): CAN 데몬 virtual 버스 위에서 같은 ISO-TP API로 서버 측 채널을 열어 동작(추가 요구 R-TP-12, §15.2).
- 실행: `loom uds sim start profiles/bcm.yaml --doip 127.0.0.1:13400` 또는 `connect --sim bcm`(데몬 안에서 기동).

### 9.2 테스트 계층

| 계층 | 내용 | CI |
|---|---|---|
| 단위 | 가드 분류: **SID 0x00–0xFF 전수 × 서브기능** 표 기반 테스트(분류 결과 스냅샷), suppressPosRsp 비트, DID/RID DB 등급 상향, 미등록 → 보수적 등급. DB 스키마 검증·디코드. NRC 해석 표 | 3 OS |
| 엔진 | P2/P2* 타임아웃, 0x78 연장, 응답 순서 꼬임, 0x7F 처리, 세션·보안 상태 갱신 규칙, TP 스킵 로직 (가짜 전송) | 3 OS |
| 동시성 | 에이전트 2 + 사용자 1 클라이언트 동시 요청: 대상당 인플라이트 1, lease, takeover, exclusive, 기능 주소 락 순서(교착 없음) | 3 OS |
| E2E sim | §2.0 E2E-1 자동화: CLI 명령 → 가상 ECU → 헤드리스 뷰어 클라이언트가 이벤트 수신, **표시 지연 측정**(목표 §5.4), publish-before-reply 순서 검증 | 3 OS |
| 안전 E2E | R2 토큰 없이 쓰기 거부, 토큰 재사용 거부, 다른 요청에 토큰 사용 거부, R3 뷰어 승인 없이 거부, R4 항상 거부, raw로 우회 시도(`raw 2E…`, `raw 3101FF00`, `raw 34…`) 전부 동일 결과, 레이트 제한, 감사 로그 해시 체인 | 3 OS |
| 고부하 | 폴링 20 Hz × 8 DID 10분, 0x78 100회 연속, 뷰어 느린 소비자 → 드롭 배너·gap 행 표시, 기록 무손실 확인 | Linux |
| CAN-TP sim | virtual bus 위 ISO-TP: 멀티프레임 응답, FD, functional 그룹 응답, TP 에러 주입(CF 누락, FC 지연) | 3 OS (M3) |
| HIL(수동) | 실제 벤치 ECU/DoIP 게이트웨이 체크리스트 | 수동 |

CI: GitHub Actions(가정) macOS/Windows/Linux × Python 3.11/3.12(추정 — 공통 결정 따름). HW 테스트는 마커로 제외.

---

## 10. 안전 / 가드레일

### 10.1 원칙

1. 가드는 **데몬 안의 단일 관문**에서 **전송 직전 바이트**를 본다. CLI 플래그·스킬 규칙은 보조이고, 우회 경로가 없어야 한다.
2. 등급은 "되돌릴 수 있는가 × 물리적 영향 × 잠금/벽돌 위험"으로 정한다.
3. 모르면 높게(미등록 SID/RID/IO → 보수적 등급).
4. 위협 모델: **실수하는 에이전트**를 막는다(같은 OS 사용자 권한에서 악의적으로 데몬 프로토콜을 위조하는 에이전트까지 암호학적으로 막지는 못함 — 탐지·감사로 대응). 이 한계를 문서·스킬에 명시.

### 10.2 위험 등급과 확인 방식

| 등급 | 의미 | 에이전트 origin | 사용자 origin(뷰어) | script(시나리오) |
|---|---|---|---|---|
| **R0** 읽기 | 상태 변화 없음 | 자동 허용 | 자동 | 자동 |
| **R1** 가역 상태 변경 | 세션/통신 상태 등 자동 복귀 가능 | 자동 허용 + 뷰어 알림 행 + 감사 | 자동 | 자동 |
| **R2** 쓰기·삭제 | 데이터 변경, 리셋 | **확인 토큰**: 첫 호출 exit 3 + 토큰 → 채팅에서 사용자 확인 → `--confirm <tok>` (또는 뷰어에서 y) | 즉시 실행(본인) | 시나리오 계획 승인 1회로 포함 |
| **R3** 위험 | 액추에이터 구동, 보호 데이터, 프로그래밍 세션, 미등록 루틴 | **뷰어 승인 필수**(사람이 키 입력) + `--risk-ack R3`. 채팅 토큰만으로는 불가 | y/n 1회 | 단계마다 뷰어 승인(정책 `allow_r3_batch: true`면 계획 승인에 포함) |
| **R4** 금지 | 플래시·메모리 지우기·파일 전송 | 거부(exit 4) | 거부 | 거부 |

환경 보정: 정책 `env: vehicle`(실차)이면 0x11, 0x28, 0x2F, 0x31(미등록)을 한 단계 상향. `env: bench`가 기본이 아니라 **정책 파일에 명시해야** 하며, 없으면 vehicle로 간주(보수적).
R4 해제: 사용자가 정책 파일에서 대상별로 `unlock_r4: [0x34, 0x36, 0x37]`처럼 명시하면 R3로 취급(M6 플래시용). 에이전트는 정책 파일을 수정하지 않는다(§10.6).

### 10.3 서비스·서브기능별 등급표 (raw 분류에도 그대로 사용)

분류 함수 `classify(bytes, target, db) → (grade, reason)`: 첫 바이트 SID → 서브기능 서비스면 `byte1 & 0x7F`(suppressPosRsp 비트 무시) → DID/RID/IO/메모리 주소가 있으면 DB 등급으로 상향(하향 불가) → 환경 보정.

| SID | 서비스 | 서브기능/조건 | 등급 | 메모 |
|---|---|---|---|---|
| 0x10 | DiagnosticSessionControl | 01 default | R1 | 보안 잠김·TP 정지 유발 |
| | | 03 extended | R1 | |
| | | 02 programming | R3 | 부트로더 진입, 복귀 실패 위험 |
| | | 04 safetySystem, 0x40–0x5F OEM, 기타 | R3 | 미등록은 R3 |
| 0x11 | ECUReset | 01 hard, 03 soft | R2 (vehicle R3) | |
| | | 02 keyOffOn | R2 (vehicle R3) | |
| | | 04/05 rapidPowerShutdown | R3 | |
| 0x14 | ClearDiagnosticInformation | 모든 그룹 | R2 | 정비 증거 삭제 |
| 0x19 | ReadDTCInformation | 01,02,03,04,06,0A 등 모든 서브기능 | R0 | 읽기 전용 서비스. 응답 크기 상한만 적용 |
| 0x22 | ReadDataByIdentifier | | R0 | |
| 0x23 | ReadMemoryByAddress | | R1 | 메모리 덤프 — 기밀 노출 가능성으로 감사 |
| 0x24 | ReadScalingDataByIdentifier | | R0 | |
| 0x27 | SecurityAccess | 홀수(requestSeed) | R1 | |
| | | 짝수(sendKey) | R2 | 실패 누적 → 잠금. 데몬이 seed→key 원자 처리, 실패 시 자동 재시도 금지 |
| 0x28 | CommunicationControl | 00 enableRxTx | R1 | 복구 동작 |
| | | 01/02/03 disable 계열 | R2 (vehicle R3) | 차량 네트워크 영향 |
| 0x29 | Authentication | | R3 | |
| 0x2A | ReadDataByPeriodicIdentifier | | R1 | 버스 부하 |
| 0x2C | DynamicallyDefineDataIdentifier | | R1 | |
| 0x2E | WriteDataByIdentifier | DB `risk.write` 없음 | R2 | |
| | | DB `protected` / VIN(F190)·F18C 등 식별 DID | R3 | 식별 DID 내장 목록 |
| 0x2F | InputOutputControlByIdentifier | 00 returnControlToECU | R1 | 복구 동작 |
| | | 01/02/03 | R3 (vehicle R4) | 액추에이터 구동 |
| 0x31 | RoutineControl | DB 등록 RID | DB 등급 | |
| | | 미등록 RID | R3 | |
| | | FF00 eraseMemory | R4 | |
| | | FF01 checkProgrammingDependencies | R1 | |
| 0x34/0x35/0x36/0x37/0x38 | Request(Download/Upload)/Transfer/Exit/FileTransfer | | R4 | 비범위, unlock_r4로만 R3 |
| 0x3D | WriteMemoryByAddress | | R4 | |
| 0x3E | TesterPresent | 00/80 | R0 | |
| 0x84 | SecuredDataTransmission | | R3 | 내용 분류 불가 |
| 0x85 | ControlDTCSetting | 01 on | R1 | 복구 동작 |
| | | 02 off | R2 | 고장 은폐 가능 |
| 0x86 | ResponseOnEvent | | R2 | |
| 0x87 | LinkControl | | R3 | 통신 속도 변경 → 연결 상실 |
| 0xBA–0xBE | 공급자 정의 서비스 | | R3 | DB에 등급이 있으면 그 값 |
| 그 외 | ISO 예약·미정의 SID | | R4 | 미지 → 거부 |

- 길이 0인 raw, 응답 SID(0x50–0x7E, 0xC3–0xC8, 0x7F 부정 응답)로 시작하는 raw는 R4(오용 방지).
- **functional 주소**로 보내는 요청은 등급 +1(여러 ECU 동시 영향), 단 0x3E·0x22·0x19는 예외.
- 분류 결과·사유는 응답 JSON과 뷰어 detail, 감사 로그에 항상 기록(`"risk": "R3", "reason": "0x2E DID 0101 protected"`).

### 10.4 확인·승인 흐름

**R2 확인 토큰 (채팅 경로)**
1. 에이전트 첫 호출 → 데몬이 요청 바이트·대상·lease에 묶인 1회용 토큰(4~6자, 기본 120 s) 발급, exit 3 + 사람용 요약.
2. 같은 순간 뷰어 승인 패널에도 뜬다(사용자가 바로 y를 눌러도 됨 — 그 경우 에이전트는 `loom uds approval wait <tok> --timeout 120s`).
3. 에이전트는 요약을 채팅에 보이고 사용자 동의를 받은 뒤 `--confirm <tok>`.
4. 토큰은 **같은 바이트·같은 대상**에만 유효. 값 바꿔 재사용 시 거부(exit 4, 감사).
5. 추가 장치: 프로젝트 `.claude/settings.json`(저장소 안, 사용자 설정 아님)에 `--confirm`이 들어간 `loom uds` 호출을 `ask` 권한으로 두어 Claude Code의 사람 확인 대화상자를 강제(권한 규칙 패턴 문법으로 가운데 와일드카드가 되는지 **미확인** — 안 되면 동사별 `Bash(loom uds write-did:*)` 등을 `ask`로). Codex는 승인 정책(`on-request` 등)으로 대응(미확인).

**R3 뷰어 승인**
- 승인은 `uds.approval.respond` RPC로만, 그리고 **대화형 뷰어 세션 자격**으로 연결한 클라이언트만 호출 가능(뷰어는 TTY 확인 후 데몬에서 별도 자격 발급, 추정 설계). CLI에는 `approve` 명령이 **없다**.
- 뷰어가 열려 있지 않으면 R3는 실행 불가 — 에이전트에게 "사용자에게 `loom uds view`를 열어 달라고 요청하라" 힌트. 이것이 02 원칙(사용자가 보고 있는 상태에서만 위험 동작)과 맞물린다.
- 승인 패널에는 대상·환경·세션·보안·요청 hex·DB 디코드·이전값→새값 diff·등급 사유를 보여준다.

**시나리오 계획 승인**
- `run --plan`이 사전 위험 요약(§10.7)과 토큰(파일 해시 + 대상에 묶임) 발급. R2 단계는 이 1회 승인으로 포함, R3 단계는 실행 중 단계별 뷰어 승인(정책 예외 가능), R4 단계가 있으면 계획 자체 거부.

### 10.5 dry-run

- 모든 쓰기 계열 명령과 `raw`, `run`에 `--dry-run`. 출력: 인코딩 바이트, 등급과 사유, 정책 판정(무엇이 필요한지), 사전 조건(세션·보안 충족 여부), 현재값 캐시와 diff, 레이트 제한 잔여. 전송·토큰 발급 없음.
- 뷰어 명령줄에서도 `?` 접두(`? 2E 0101 021F`)로 dry-run.

### 10.6 정책 파일, 화이트리스트, 레이트 제한

`$LOOM_HOME/policy/uds.yaml` (사용자 소유):
```yaml
env: bench                       # bench | vehicle (없으면 vehicle)
max_auto_grade: R1               # 에이전트 자동 허용 상한 (R0|R1)
idle_release_s: 600
targets:
  - name: bcm
    match: {doip: {ip: 192.168.0.10, logical: 0x1001}}
    allow_max: R3                # 이 대상에서 허용되는 최고 등급
    allow_r3_batch: false
  - name: ecm
    match: {can: {channel: ch0, tx: 0x7E0}}
    allow_max: R2
unknown_targets: R0              # 화이트리스트 밖: 읽기만
rate:
  global_rps: 20                 # 대상당 요청/초
  poll_min_period_ms: 50
  poll_max_dids: 16
  security_key_failures: {max: 1, window_s: 600}    # 실패 1회 후 10분 잠금(ECU lockout 보호)
  reset: {max: 1, window_s: 30}
  clear_dtc: {max: 1, window_s: 10}
  write_did: {max: 10, window_s: 60}
  r3: {max: 5, window_s: 600}
unlock_r4: []                    # M6 플래시 전까지 비움
```
- 정책 로드·변경은 데몬 시작 또는 **뷰어에서** `policy reload`(사용자 키 입력)로만. 로드 시 파일 해시를 감사 로그에. CLI `policy show`는 읽기 전용.
- 에이전트의 정책 파일 수정 방지: 스킬 규칙 + (선택) 프로젝트 `.claude/settings.json`의 `deny`에 해당 경로 Edit/Write. 데몬은 실행 중 파일 변경을 감지하면 **자동 적용하지 않고** 뷰어에 "정책 파일 변경됨 — 리로드?" 알림.
- 레이트 제한 발동 시 exit 4 + 남은 시간. 0x27 실패 후에는 데몬이 자동 재시도하지 않는다(ECU 잠금 0x36/0x37 방지).

### 10.7 시나리오 사전 위험 요약 (`run --plan`)

```
$ loom uds run scenarios/bcm-coding.yaml --plan
scenario bcm-coding (sha256 3f9a…)  target bcm (bench, whitelist ✓, allow_max R3)
step  action                                  grade  needs
 1    session extended                        R1     -
 2    security level 1 (plugin oem_bcm)       R2     plan approval
 3    read-did 0101                           R0     -
 4    write-did 0101 region=EU                R3     viewer approval (at step)
 5    read-did 0101 expect region=EU          R0     -
 6    reset soft                              R2     plan approval
 7    wait 3s; status --probe                 R0     -
max grade R3 · confirmations: plan ×1, viewer ×1 · est. duration ~6 s
on_error: stop (안전 기본값) · abort: 뷰어 ! 또는 loom uds abort
token P3X9 (sha256-bound, 300 s)
exit 3
```
- 시나리오 YAML의 동적 바이트(`raw`, 템플릿 값)도 계획 시점에 분류. 런타임에 값이 바뀌어 등급이 오르면 해당 단계에서 정지하고 새 승인 요구.

### 10.8 기타 가드

- **긴급 정지**: 뷰어 `!`, `loom uds abort`. 큐 비움, 폴링/시나리오 중단, 대기 승인 취소, TP 정지(→ S3 후 기본 세션). 진행 중 트랜잭션 응답은 기다렸다 기록.
- **세션 복구 안내**: 비기본 세션 + 보안 해제 상태로 에이전트 lease가 끝나면 뷰어에 "bcm extended/L1 상태 유지 중 — default로 되돌릴까요?" 제안.
- **응답 크기 상한**: DTC 대량 응답 등은 기록엔 전부, CLI 출력은 상한 적용.
- **raw `--no-wait`**(응답 안 기다림)은 R0 서비스만 허용.

### 10.9 seed-key 플러그인 인터페이스

```python
# loom/uds/seedkey.py  (인터페이스 초안)
class SeedKeyContext(TypedDict):
    target: str          # "bcm"
    level: int           # 1, 3, … (requestSeed 서브기능 기준 레벨)
    ecu_info: dict       # DB의 ecu.name, algo_arg 등 (비밀 아님)

class SeedKeyPlugin(Protocol):
    name: str
    def levels(self) -> list[int]: ...
    def compute_key(self, seed: bytes, ctx: SeedKeyContext) -> bytes: ...
```
- 등록: Python entry point 그룹 `loom.uds.seedkey`, 또는 정책/DB에 경로 지정(`plugin: ./private/oem_bcm.py`, 저장소 밖 권장). 플러그인은 **데몬 프로세스 안**에서 실행(별도 서브프로세스 옵션 — 크래시 격리·DLL 비트 수 문제용).
- DLL 브리지(M5, Windows): Vector 계열 `GenerateKeyEx` 형태 DLL(시그니처는 추정, 확인 필요)을 ctypes로 감싸는 내장 플러그인 `dll`. 32bit DLL은 32bit Python 서브프로세스로 브리지.
- 데몬은 key 바이트를 기록·출력하지 않는다(마스킹). seed는 정책에 따라.
- 실패(NRC 0x35 invalidKey)는 자동 재시도 없음, 레이트 제한 적용, 0x36/0x37은 남은 지연 시간을 사용자/에이전트에 표시.
- sim용 내장 플러그인 `xor`(테스트 전용, 이름으로 명시).

### 10.10 스킬 설계 (안전 규칙의 에이전트 측 집행)

**`.claude/skills/loom-uds/SKILL.md` 개요 (본문 ≤ 80줄 목표)**
```markdown
---
name: loom-uds
description: ECU 진단(UDS over DoIP / CAN-TP). VIN·SW 버전·DID 읽기/쓰기, DTC 조회/삭제,
  세션·보안접근, 루틴, 리셋, 진단 시나리오 실행, DID 값 플롯. "ECU", "진단", "DTC", "DID",
  "UDS", "DoIP", "0x22", "seed key" 등이 나오면 사용.
---
# loom-uds
모든 ECU 통신은 `loom uds ...` CLI로만 한다 (데몬 경유 → 사용자가 실시간으로 봄).
udsoncan/doipclient/python-can을 직접 import 해 ECU에 붙지 말 것.

## 절차 (항상 이 순서)
1. `loom uds status` — 데몬·대상·세션·lease 확인. 대상 없으면 connect (사용자가 준 주소만).
2. 사용자에게 `loom uds view`가 열려 있는지 확인(R3는 뷰어 없으면 불가).
3. 읽기 먼저: 쓰기 전 현재값을 read-did 로 확인하고 사용자에게 보여준다.
4. 쓰기 계열은 먼저 `--dry-run`. exit 3(확인 필요)이면 요약을 사용자에게 보이고
   명시적 동의를 받은 뒤에만 `--confirm <token>`. 동의를 추정하지 말 것.
5. 쓰기 후 되읽기로 검증. 비기본 세션을 끝냈으면 `session default`.
6. 매 호출 후 `trace --since last`로 사용자 개입이 있었는지 확인.

## 금지
- R4(플래시, 0x34–0x38, 0x3D, FF00 erase) 시도, raw로 우회 시도, 정책 파일 수정
- 0x27 실패 후 재시도 반복 (ECU 잠금) — 사용자에게 보고하고 멈춤
- exit 6(LOCKED) 시 제어권 재획득 시도 — 사용자에게 요청
- 무한 폴링: poll 은 항상 --duration

## 종료 코드  0 ok · 2 NRC · 3 확인 필요 · 4 정책 거부 · 5 전송 오류 · 6 LOCKED · 7 데몬 없음
## 상세: references/
```

**references/**
| 파일 | 내용 |
|---|---|
| `commands.md` | 전체 명령·인자·출력 예(§4) |
| `safety.md` | 등급표(§10.3), 확인 흐름, 레이트 제한, 위협 모델 한계 |
| `nrc.md` | NRC 코드표 + 원인 + 에이전트 다음 행동(0x78/0x21 재시도는 데몬이 처리하므로 하지 말 것 등) |
| `services.md` | 서비스별 요청/응답 바이트 구조, 서브기능, DTC 상태 비트 |
| `did-db.md` | 진단 DB YAML 스키마(§6.4), 표준 F1xx DID 목록 |
| `scenarios.md` | 시나리오 YAML 형식(§6.5), `--plan` 읽는 법, 예제 3개 |
| `seed-key.md` | 플러그인 등록, 실패 처리 규칙 |
| `troubleshooting.md` | 라우팅 활성화 거부 코드, DoIP NACK, ISO-TP 에러, 타임아웃, stale 세션 |

**Codex 호환**
- 저장소 루트 `AGENTS.md`에 "Loom 장치 작업 규칙" 절: 각 스킬 SKILL.md 경로를 가리키고 핵심 금지 규칙만 복사(중복 최소). 
- 에이전트 무관 경로: `loom uds agent-help`가 SKILL.md 핵심(절차·금지·종료 코드)을 출력 → 스킬 시스템이 없는 에이전트도 첫 호출로 규칙 습득. 원본은 하나(패키지 리소스)이고 SKILL.md는 빌드 시 생성해 불일치 방지(제안).
- Codex의 스킬 디렉터리 직접 지원 여부·경로는 **미확인** — 확인되면 심볼릭 링크 또는 생성 스크립트로 같은 파일 제공.
- 안전은 데몬이 집행하므로 에이전트 종류와 무관하게 동일 보장(스킬은 실수 줄이기용).

---

## 11. 마일스톤

| M | 이름 | 내용 | 수용 기준 |
|---|---|---|---|
| **M1** | **걷는 뼈대 (sim DoIP)** | loomd 안 UDS 모듈 최소형, 자체 asyncio DoIP 전송(라우팅 활성화·alive check), 가상 DoIP 엔티티 + 서버 엔진(세션 01/03, 0x22, 0x19 02, 0x2E, 0x10, NRC 주입), CLI `connect/status/read-did/write-did/dtc list/raw/trace/abort`, 가드 분류기 전 SID + 정책 최소(R0 자동, R1 자동, R2 토큰, R3/R4 거부), 이벤트 버스 + `loom uds view` 최소 TUI(트레이스 표, 상태 바, origin, `:` 수동 송신, `!`), JSONL 기록, SKILL.md 최소판 | (1) §2.0 E2E-1을 3 OS CI에서 자동 실행: 에이전트 CLI read-did → 헤드리스 뷰어가 같은 트랜잭션 수신, **응답→뷰어 p95 ≤ 100 ms**, publish-before-reply 순서 검증. (2) 사용자 클라이언트가 수동 요청 → 에이전트 `trace --since last`에 보임. (3) `write-did` 토큰 없이 exit 3, 토큰으로 성공, `raw 2E…`도 동일하게 exit 3. (4) `raw 34…`/`raw 3101FF00` exit 4. (5) 뷰어 미실행 상태에서도 기록 파일에 전부 남고, 나중에 뷰어 열면 즉시 라이브 스트림 + 최근 N건 표시 |
| M2 | 세션·보안·안전 완성 | security + 플러그인(Python), TP 유지자, 유휴 만료, lease(shared/exclusive/takeover), R3 뷰어 승인, 정책 파일·화이트리스트·레이트 제한, 감사 로그 해시 체인, dry-run 전 명령, NRC 해석표, 0x78/P2* 처리, status --probe | 안전 E2E 스위트(§9.2) 전부 통과. 사용자 takeover 후 에이전트 쓰기 exit 6. 0x78 ×10 시나리오에서 뷰어 한 행 카운터 갱신. TP 2 s ± 200 ms(sim) |
| M3 | CAN-TP | CAN 데몬 ISO-TP API 연동(`IsoTpPort`), 가상 CAN-TP 노드, functional 그룹, `discover --can`, CAN 트레이스 corr_id 주석 | sim에서 M1·M2 스위트를 CAN-TP 전송으로 재실행 통과. 멀티프레임(4 KB) 응답, FD. CAN 트레이스와 UDS 트레이스 동시 표시 |
| M4 | 쓰기 계열 확장·시나리오·플롯 | routine, io, reset, dtc clear/snapshot/ext, comm, dtc-setting, `run`(+`--plan`), poll 작업 + viz 연동(V-UDS-1/2/4), 고부하 표시 규칙(§5.5) | 시나리오 예제 3개 sim 통과, 계획 요약 정확. 20 Hz × 8 DID 10분 폴링: 기록 무손실, 뷰어 집계 행·드롭 배너 동작, viz 플롯 ≤ 250 ms |
| M5 | 실HW·DB 확장 | 실 DoIP 벤치/차량 검증(Windows/macOS), Vector/PCAN 위 CAN-TP 검증, ODX→YAML 변환, DLL seed-key 브리지, TLS(요구 시), pcapng 내보내기 | HIL 체크리스트 통과(최소 DoIP 1종, CAN 1종). 실ECU 응답→뷰어 p95 ≤ 100 ms |
| M6 | (선택) 플래시 | `FlashPlugin` 인터페이스, 0x34/36/37 시퀀스, 진행 표시, unlock_r4 정책 | 사용자 결정 후 별도 계획 |

M1에 넣지 않는 것: ODX, 플래시, security/seed-key, CAN-TP, 웹 viz, 시나리오. (02 원칙: 4요소 최소형이 먼저.)

---

## 12. 구현 작업 분해

담당 제안: Opus = 아키텍처·UX·안전 집행·스킬, Sol = 프로토콜·전송·타이밍·시뮬레이터·성능 측정.

| ID | 작업 | 담당 | 의존 | 병렬 그룹 | M |
|---|---|---|---|---|---|
| U-01 | 패키지 골격 `loom.uds`(service/guard/engine/codec/db/sim/cli/view), 인터페이스 스텁 | Opus | core 골격(공통) | A | M1 |
| U-02 | 가드 분류기 `classify()` + SID 전수 표 테스트 + 등급표 데이터 파일 | Opus | U-01 | A | M1 |
| U-03 | 진단 DB YAML 스키마(JSON Schema) + 로더 + 디코더(udsoncan 코덱 연결) | Sol | U-01 | A | M1 |
| U-04 | 자체 asyncio DoIP 클라이언트 전송(헤더, 라우팅 활성화, alive check, 진단 ACK/NACK, 다중 논리 주소) | Sol | U-01 | A | M1 |
| U-05 | 가상 ECU 서버 엔진(세션·NRC·지연 주입) + 가상 DoIP 엔티티 | Sol | U-03 | A | M1 |
| U-06 | TxnEngine(대상 큐, P2/P2*, 0x78, 타임아웃) + TargetSession 상태 규칙 | Sol | U-04 | B | M1 |
| U-07 | UdsService RPC/이벤트(publish-before-reply, seq, 커서, trace.query) | Opus | core IPC, U-06 | B | M1 |
| U-08 | 정책 최소 + 확인 토큰(R2) + 감사 로그 최소 | Opus | U-02, U-07 | B | M1 |
| U-09 | CLI 명령 세트 M1 + 출력 포맷(사람/JSON, 상한, 종료 코드) | Opus | U-07 | C | M1 |
| U-10 | `loom uds view` TUI 최소(표, 상태 바, `:` 송신, `!`) | Opus | U-07 | C | M1 |
| U-11 | E2E-1 자동 테스트 + 지연 측정 훅 + 헤드리스 뷰어 클라이언트 | Sol | U-09, U-05 | C | M1 |
| U-12 | SKILL.md 최소판 + `agent-help` | Opus | U-09 | C | M1 |
| U-13 | Security + seed-key 플러그인 호스트 + sim xor | Sol | U-06 | D | M2 |
| U-14 | TP 유지자·유휴 만료·stale 탐지·status --probe | Sol | U-06 | D | M2 |
| U-15 | lease(shared/exclusive/takeover) + 기능 주소 락 + 동시성 테스트 | Opus | U-07 | D | M2 |
| U-16 | R3 뷰어 승인 채널(뷰어 자격) + 승인 패널 UI | Opus | U-08, U-10 | D | M2 |
| U-17 | 정책 파일 전체(화이트리스트, env 보정, 레이트 제한), 감사 해시 체인, dry-run 전 명령 | Opus | U-08 | D | M2 |
| U-18 | NRC 해석표 + 힌트(references/nrc.md와 같은 원천) | Sol | U-02 | D | M2 |
| U-19 | `IsoTpPort` 어댑터(CAN ISO-TP API) + 가상 CAN-TP 노드 + functional 그룹 + discover --can | Sol | CAN 팀 ISO-TP API, U-06 | E | M3 |
| U-20 | 서비스 확장(routine/io/reset/clear/snapshot/comm/dtc-setting) CLI+코덱 | Sol | U-17 | F | M4 |
| U-21 | 시나리오 러너 + `--plan` 위험 요약 + 계획 승인 | Opus | U-17 | F | M4 |
| U-22 | 폴링 작업 + long format 기록 + viz 연동 + 고부하 집계/드롭 표시 | Opus(UI)/Sol(성능) | viz 팀, U-07 | F | M4 |
| U-23 | 고부하·장시간 테스트, 지연 회귀 측정 CI | Sol | U-22 | F | M4 |
| U-24 | references/ 전체 + AGENTS.md 절 + `.claude/settings.json` 권한 규칙 제안 | Opus | U-17, U-21 | F | M4 |
| U-25 | HIL 체크리스트·실HW 검증, ODX 변환, DLL 브리지, TLS | Sol | M4 | G | M5 |

병렬성: 그룹 A는 서로 독립(골격만 공유). B는 A 이후. C 안의 U-09/U-10/U-11/U-12 병렬. M3(E)는 CAN 팀 일정에 따라 M2와 병렬 가능.

---

## 13. 리스크

| # | 리스크 | 영향 | 대응 |
|---|---|---|---|
| 1 | 세션 상태 추정 오류(외부 전원 리셋, 다른 테스터 개입) → 에이전트가 잘못된 전제로 쓰기 | 쓰기 실패·혼란 | confidence(assumed/stale) 표시, NRC 0x7E/0x7F·0x33 시 stale, 쓰기 전 `--probe` 권장(스킬), 외부 tx_id 감지 |
| 2 | 0x27 반복 실패로 ECU 잠금(시간 지연/영구) | 벤치 정지 | 실패 1회 후 레이트 잠금, 자동 재시도 금지, 스킬 금지 규칙 |
| 3 | 같은 OS 사용자 권한의 에이전트가 승인 채널을 우회 | 안전 규칙 무력화 | 위협 모델 명시, 뷰어 자격·CLI approve 부재·Claude Code 권한 `ask`·감사 해시 체인으로 실수 차단+탐지. 완전 방지는 범위 밖 |
| 4 | 단일 데몬 장애 시 UART/CAN/UDS 동시 정지 | 가용성 | 모듈별 태스크 격리·재시작, TP는 CAN 데몬 periodic(소유자 끊기면 정지 = 안전 쪽) |
| 5 | DoIP 게이트웨이 구현 편차(라우팅 활성화 타입, 동시 요청 처리, alive check 엄격도) | 연결 불안정 | `max_parallel` 기본 1, 활성화 타입 옵션, HIL 조기(M5를 기다리지 말고 장비 있으면 M2부터 스모크) |
| 6 | CAN-TP API가 CAN 팀 일정에 묶임 | M3 지연 | `_xteam` 계약 조기 확정, UDS 측 가짜 IsoTpPort로 M1–M2 진행 |
| 7 | Python 타이머로 TP·P2 판정 지터 | 오판정 | 판정은 HW/소켓 타임스탬프 기준, 타임아웃 대기에 여유, CAN TP는 CAN 데몬 periodic |
| 8 | OEM seed-key·DB 비밀이 저장소/로그로 유출 | 보안 사고 | 플러그인 저장소 밖, key 마스킹, 감사에 비밀 없음, `.gitignore` 템플릿 |
| 9 | 진단 DB 부재로 쓰기 값 인코딩 실수 | 잘못된 코딩 | DB 없는 쓰기는 `--hex`만 허용 + R3 상향(제안), dry-run diff |
| 10 | 차량 모드 판단 실패(실차를 bench로 설정) | 물리적 위험 | env 미지정=vehicle, bench 설정 시 뷰어 상태 바 상시 표시, 대상별 지정 |
| 11 | 뷰어 고부하에서 "멈춘 화면이 정상처럼" 보임 | 사용자 오판 | heartbeat·STALLED·LAG·드롭 배너(§5.5) |
| 12 | udsoncan 코덱을 자체 엔진에 끼우는 비용이 예상보다 큼 | 일정 | 대안: 데몬 안 udsoncan Client + 커스텀 Connection (관문/이벤트는 Connection 래퍼에서) — Sol 스파이크로 판단 |

---

## 14. 사용자에게 물어볼 질문 (기본값 제안)

| # | 질문 | 기본값(답 없으면 이대로 진행) |
|---|---|---|
| 1 | 실제 사용할 장비: DoIP 게이트웨이/ECU 종류, CAN 인터페이스(Vector/PCAN), 실차인가 벤치인가? | 벤치 DoIP 우선, CAN은 CAN 팀 매트릭스, 정책 기본 env=vehicle(보수적) |
| 2 | 플래시(0x34–0x37)를 언젠가 필요로 하는가? | 비범위, M6 선택(R4 금지 유지) |
| 3 | seed-key 알고리즘 제공 형태: Python 코드 / Windows DLL / 없음? | Python 플러그인 인터페이스만 M2, DLL은 M5 |
| 4 | 진단 DB 원천: ODX/PDX, CDD, 엑셀, 없음? | Loom YAML 수작성 + ODX 변환(M5). CDD 비범위 |
| 5 | R2 확인을 채팅 토큰으로 충분히 볼지, 항상 뷰어 승인으로 할지? | R2 채팅 토큰(+Claude Code 권한 ask), R3 뷰어 필수 |
| 6 | 에이전트 자동 허용 상한: R0만? R1(세션 변경 포함)? | R1 |
| 7 | 뷰어 기본: TUI(herdr 패인) / 웹? | TUI 먼저, 웹은 viz 일정 |
| 8 | DoIP TLS(3496) 필요 여부(OEM 요구) | 후순위 M5 |
| 9 | 로그·감사 저장 위치(프로젝트 `./.loom/` vs 홈) | `./.loom/` |
| 10 | 같은 버스에서 CANoe 진단 콘솔을 동시에 쓸 일이 있는가? | 없다고 가정, 감지 시 경고 |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 공통 계약 변경 요청 (메인에게)

| # | 요청 | 근거 |
|---|---|---|
| C1 | **단일 데몬 `loomd` + 도메인 모듈** (§3.1). 최소한 UDS 모듈은 CAN ISO-TP 호스트와 같은 프로세스 | 세션 상태 상주, ISO-TP 인프로세스, 이벤트 버스·승인·감사 공유, 뷰어 접속점 하나 |
| C2 | 이벤트 레코드에 `seq`(데몬 전역 단조), `corr_id`, `kind`, `client` 필드 추가, `dir`에 `txrx` 허용 | 누락 감지(gap), 요청↔응답↔감사 연결, 커서 |
| C3 | `origin`에 `daemon`(또는 `keepalive`) 추가 | TesterPresent 등 데몬 자율 동작 구분 (`_xteam` §5와 표기 통일 필요: 제안 `daemon`) |
| C4 | **publish-before-reply** 를 공통 규칙으로 | 02 원칙 "같은 순간" 보장 |
| C5 | core에 공통 **승인 서비스**(토큰 R2 / 뷰어 승인 R3, 뷰어 자격) · **lease** · **감사 로그(해시 체인)** · **정책 로더** | UART 입력·CAN 송신도 같은 구조 필요, 뷰어 승인 패널 하나로 통일 |
| C6 | CLI 종료 코드 표준(§4.1) 전 도메인 공통 | 스킬 문서 단순화 |
| C7 | 데몬 heartbeat 이벤트(1 s)와 뷰어 STALLED/LAG/드롭 표시 규칙 공통화 | 02 원칙 "거짓말하지 않는 화면" |
| C8 | IPC: 127.0.0.1 WebSocket JSON-RPC + 토큰 파일 + Origin 검사 | 웹 뷰어 직결, 로컬 CSRF 방지 |

### 15.2 CAN 팀: CAN-TP(ISO-TP) API 요구 — `_xteam/can-uds-isotp.md` v0.1에 대한 UDS 측 추가 요구

v0.1(§2–§7)에 동의. 아래를 추가로 요구/확인:

| ID | 요구 | 비고 |
|---|---|---|
| R-TP-01 | open/close/list + 소유권 `ERR_BUSY` (v0.1 §2) | 동의 |
| R-TP-02 | send(PDU) + `tx_done`/`tx_error`, `rx`/`rx_error` 이벤트, 타임스탬프 first/last frame (v0.1 §3) | 동의. P2 기준점 규칙 동의 |
| R-TP-03 | functional 그룹(open_group) + 멤버별 FC 처리 (v0.1 §4) | 동의 |
| R-TP-04 | 응답자 탐색: rx_id 범위 수신 모드 (v0.1 §4 선택) → **필수로 요청** | `discover --can` |
| R-TP-05 | periodic PDU + 소유자 끊김 시 자동 정지 (v0.1 §5) | 동의, origin=`daemon` 표기 |
| R-TP-06 | **인프로세스 API와 IPC API 동일 시그니처**(asyncio) | 데몬 구성 결정과 무관하게 UDS 코드 불변 |
| R-TP-07 | send에 `corr_id` 전달 → ISO-TP 원시 프레임 이벤트에 같은 `corr_id` 주석 | V-UDS-5, 트레이스 연동 |
| R-TP-08 | 채널 `flush/abort(isotp_id)`: 진행 중 송수신 중단 | 사용자 긴급 정지 |
| R-TP-09 | listen-only/tx 금지 채널에서 open 시 `ERR_POLICY` | CAN 측 안전 규칙과 정합 |
| R-TP-10 | 외부 송신 감지: 같은 tx_id를 다른 주체(raw 송신, 외부 테스터)가 보냈을 때 이벤트 | 세션 stale 처리(§3.5-6) |
| R-TP-11 | 파라미터 변경 `isotp.set_params(isotp_id, …)` (재open 없이) | 일부 ECU가 프로그래밍 세션에서 STmin 변경 |
| R-TP-12 | **서버 측 채널**(가상 ECU용): rx/tx 방향을 뒤집어 open 가능(sim 백엔드에서) | 가상 CAN-TP 노드 |
| R-TP-13 | 결함 주입 훅(sim): CF 누락, FC 지연/WAIT 반복, 잘못된 SN | 테스트 |
| R-TP-14 | 채널 상태(bus_off 등) 이벤트 → UDS 대상 `link` 상태 반영 (v0.1 §6) | 동의 |
| R-TP-15 | 성능: PDU→UDS 전달 ≤ 10 ms, FC ≤ 수 ms (v0.1 §7) + **UDS 트랜잭션 → 뷰어 ≤ 100 ms 목표에 CAN 부하 100%에서도 지장 없을 것**(이벤트 우선순위: 진단 PDU 이벤트를 원시 프레임 이벤트보다 먼저 전달) | 02 원칙 실시간성 |

미결 질문 의견(v0.1 §8): 1) can-isotp `NotifierBasedCanStack` 찬성(단 실측 필요). 2) 패딩 기본 0xCC(흔한 OEM 관례, 추정), 대상 DB에서 지정. 3) 같은 tx_id raw 송신은 **차단 + 사용자 origin만 `--force` 허용** 제안(에이전트의 raw CAN으로 진단 우회 방지 — UDS 가드 우회 경로가 되므로). 4) periodic 데몬 제공 찬성.

### 15.3 viz 요구 (`_xteam/viz-requirements.md` `## uds`에 대한 보강 제안 — 리드가 반영 판단)

리드 초안 V-UDS-1~5에 동의. 추가:
- V-UDS-1 행 상태 **변이**: 같은 `corr_id`의 `tx`(진행 중) → `pending`(카운터) → `txn`(확정)으로 한 행이 갱신되는 모델 지원(행 추가가 아니라 갱신).
- 집계 행(`aggregate` 필드: 폴링 그룹, 0x36 진행, TP 카운터)과 펼치기.
- origin에 `daemon` 추가, 위험 등급 배지, 승인 대기 상태 표시. 승인 버튼 자체는 공통 승인 서비스(C5) UI.
- seq gap 행, 표시 드롭 배너, STALLED/LAG 표시(C7).
- 폴링 플롯: `value:null` 샘플은 끊긴 선으로(0으로 그리지 않음), NRC 샘플 마커.
- 표시 지연 목표: 웹 ≤ 200 ms (p95).

### 15.4 타팀 의존성 요약

| 의존 | 대상 | 필요 시점 |
|---|---|---|
| core 데몬·IPC·이벤트 버스·기록기 | 메인/공통 | M1 |
| 공통 승인/lease/감사 (C5) | 메인/공통 | M1(최소), M2 |
| ISO-TP API (§15.2) | CAN 팀 | M3 (가짜 포트로 M1–M2 진행) |
| viz 웹 렌더러 (V-UDS-1/2/4) | CAN 팀 | M4 |
| TUI 프레임워크 통일 | CAN/UART 팀 | M1 |

---

## 핵심 원칙 체크리스트 자가 점검 (02-core-principle.md)

| # | 항목 | 충족 | 위치 |
|---|---|---|---|
| 1 | 맨 앞 "핵심 루프" 절 — 4요소 한 표 | ✅ | §0 (표 + 설계 불변식 4개) |
| 2 | 종단 간 시나리오 1개 이상 (에이전트 송신 → ECU 응답 → 사용자 화면 ms → 사용자 개입) | ✅ | §2.0 E2E-1 (명령, 데몬 타임라인 표, TUI 목업, 사용자 `:` 송신, takeover). §2.1 A2/A3, §2.2 U2–U4 보조 |
| 3 | 실시간성 목표치 + 고부하 시 멈추거나 거짓말하지 않는 방법 | ✅ | §5.4 (응답→TUI ≤ 100 ms, 웹 ≤ 200 ms 등), §5.5 (폴링 집계, 0x36 진행 행, 0x78 카운터, TP 카운터, 드롭 배너, gap 행, STALLED/LAG), §15.2 R-TP-15 |
| 4 | M1 = 걷는 뼈대 (sim 위 4요소 최소형) | ✅ | §11 M1 (sim DoIP + CLI 읽기/쓰기 + TUI 라이브 + 사용자 송신, 수용 기준 5개), ODX·플래시·seed-key·웹은 M1 이후 |
| 5 | 사용자 화면이 에이전트 세션과 분리되지 않음 (숨은 연결 금지) | ✅ | §0 불변식 1–3, §3.1 (대안 C 기각: CLI 안 udsoncan Client), §3.3 세션 상태 데몬 보유, §4.4 Python API도 데몬 경유, §10.10 SKILL.md 금지 규칙, M1 수용 기준 (5) |
| 6 | 원칙에 기여하지 않는 기능 후순위/비범위 명시 | ✅ | §1.2 후순위, §1.3 비범위, §11 "M1에 넣지 않는 것" |

<!-- END R1 -->
