# Blackhawk XDS560v2 디버거 UI — 구현 착수용 설계 보고서

- 작성: claude-opus-5-5 (native), session `0bddc4cb-109d-4410-9d06-a158bbcefe0f`, workspace w2F
- 기준: Loom `ec38f95`, 문서 접근일 **2026-11-14**
- API 근거: [`API-EVIDENCE.md`](API-EVIDENCE.md) / [`API-EVIDENCE.json`](API-EVIDENCE.json)
  - 별도 Opus 작업자(dbg-api-opus, w2G)가 작성
  - CCS 21.0.1 JS/Python 레퍼런스 [A1–A27]
- 이전 조사: [`../ti-debugger-survey/SURVEY.md`](../ti-debugger-survey/SURVEY.md) [S1–S29]
- 계약 초안: [`contracts/`](contracts/) — JSON Schema, 예시, manifest, Rust/TS 스케치
- **이 문서는 설계 제안입니다.**
  - Loom에 `dbg` 도메인·브리지·패널은 **아직 없습니다**.
  - CCS 설치, SDK 실행, 프로브 연결은 하지 않았습니다.
  - 코드 조각은 모두 **미실행**입니다.

**표기**

| 표기 | 의미 |
|---|---|
| **[문서]** | TI 공식 문서에 있는 API나 사실. 우리 환경에서 실행해 본 것은 아님 |
| **[제안]** | 이 보고서가 설계한 것 |
| **[미확인]** | SDK나 하드웨어를 확인해야 정해지는 것 |
| **[저장소]** | 현재 Loom 코드에 있는 계약. 파일:줄 표기 |

---

## 0. 결론

**1. 경로**
- 현재 워크스테이션은 **Darwin arm64(Apple silicon)** 입니다. 여기서 XDS560v2를 직접 구동하는 공식 경로는 없습니다 [S1][S2][S8].
- 권장 구성:

  ```
  macOS loomd ─ dbg worker ─ 127.0.0.1 (SSH -L) ─ x64 Windows/Linux 브리지 호스트
    ─ loom-ccs-bridge (CCS 21.0.1 Scripting, 헤드리스) ─ Blackhawk 드라이버 ─ XDS560v2 ─ TI 타깃
  ```
- 브리지 호스트에는 CCS 설치본이 있어야 합니다.
  - JS/Python 모듈을 외부에서 불러와도 모든 경로가 `<CCS>/ccs/scripting/...`를 가리킵니다 [A1–A3].
  - npm이나 pip 래퍼만으로 Debug Server, device support, 드라이버가 갖춰지지 않습니다.

**2. CCS UI를 임베드하지 않고 Loom 자체 UI로 만듭니다.**
- CCS(Theia) 화면을 Loom 탭에 넣는 공식 경로는 찾지 못했습니다.
  - `launchCCS()`는 IDE를 *옆에* 띄울 뿐입니다 [A4].
- 반면 CCS Scripting에는 다음 기능의 API가 문서화되어 있습니다 [A4–A19].
  - 연결과 코어 선택, run/halt/step
  - 레지스터 열거와 읽기, 메모리 읽기
  - 심볼만 로드, 심볼→주소
  - 호출 스택의 소스 줄, BP 추가와 삭제
  - 오프라인 디스어셈블리
- 이 API로 **Loom이 직접 관측 패널을 만들 수 있습니다.**

**3. 바로 시작할 수 있는 작업 (티켓 T1–T7, T4b)**
- `loom-dbg` 도메인 계약과 FakeBackend
- worker 큐와 watch
- UI 패널, CLI, 스킬 초안
- 브리지 supervisor 프로토콜 (stub executor로 검증)
- 이 단계는 **계약과 수명주기만 검증합니다. 드라이버나 프로브 호환성은 검증하지 않습니다.**

**4. 하드웨어 없이는 확정할 수 없는 것 (T8–T9)**
- Blackhawk 드라이버와 CCS 21.0.1 조합
- 실제 코어 이름
- realtime/polite 관련 설정 ID와 기본값
- 실행 중 `memory.read`의 침습 여부
- 다중 word 값의 endian
- 타임아웃 이후 타깃 상태
- PC/SP 레지스터 이름
- `openSymbolModule`의 `processorId`

**5. 소스와 BP는 MVP 다음 단계입니다.**
- PC→file:line은 halted 상태에서 `callstack.fetch`로 얻을 수 있습니다 [A13].
- 다음 기능은 문서에 API가 없습니다 [A10–A12]. Loom이나 브리지가 직접 구현해야 합니다.
  - file:line→주소 변환
  - BP 목록, enable/disable, hit 횟수
  - 경로 매핑
- 디스어셈블리는 **오브젝트 파일 기준**이고 live 메모리 기준이 아닙니다 [A11].

---

## 1. 기능 경계 매트릭스

**상태 범례**
- 상태: ✅ 문서화 · ⚠️ 문서화됐지만 제약 있음 · ❌ 문서에서 찾지 못함
- MVP: **M** = MVP · **L** = 이후 · **X** = 제외

| UI 기능 | CCS Scripting API (21.0.1) | 상태 | MVP | Loom 구현 메모 |
|---|---|---|---|---|
| 구성 로드 / 코어 목록 | `ds.configure(ccxml) → CoreList{cores,nonDebugCores}`<br>`ds.listCores()`<br>`ds.deconfigure()` [A4] | ✅ | M | configure는 코어를 연결하지 않음. 이미 구성된 상태면 예외가 남. deconfigure는 **모든 코어를 끊고 세션을 무효화**함 → 마지막 코어를 close할 때만 호출 |
| 코어 세션 열기 | `ds.openSession(pattern)`<br>`session.getName()`<br>`getPartNum()`<br>`session.close()` [A4][A5] | ✅ | M | 정규식에 처음 맞는 코어가 열림 → 설정의 `core_pattern`은 하나의 코어에만 맞아야 하고, 열린 이름을 증거로 남김. `session.close()`는 **disconnect가 아님** |
| 연결 / 해제 | `target.connect()`<br>`disconnect()`<br>`isConnected()` [A6] | ⚠️ | M | connect 시 ccxml 시작 GEL의 `OnPreTargetConnect`/`OnTargetConnect`가 실행됨. 이 GEL이 halt나 reset을 할 수 있음 [A21–A24] |
| 디버거 설정 증거 | `settings.getDetailedSettings()`<br>`get(id)`<br>`set(id,v)` [A15] | ⚠️ | M (읽기만) | realtime, polite, "debugger 접근 전 halt" 설정의 ID는 문서에 없음 → connect 전후에 전체 덤프를 증거로 남기고, HW 수용 단계에서 ID를 확정함 |
| Run / Halt | `target.run(waitForHalt=true)`<br>`halt()`<br>`isHalted()`<br>`waitForHalt()` [A6] | ✅ | M | 브리지는 반드시 `run(false)`로 호출함. run-state 이벤트 스트림이 **없음** → `isHalted()` 폴링 |
| Step | `stepInto(explicitlyAsm?)`<br>`stepOver(...)`<br>`stepOut()` [A6] | ✅ | M (asm/source 자동) | 다시 halt될 때까지 블록됨. 호출 동안 run_state는 `stepping` |
| Reset / Restart | `getResets()`<br>`reset(type?)`<br>`restart()` [A6] | ⚠️ | L | 리셋 종류는 타깃마다 다름. 실패해도 예외가 없음 [S21] → 호출 후 상태를 다시 관측해야 함. restart는 `OnRestart` GEL과 auto-run 설정의 영향을 받음 |
| 레지스터 트리 | `registers.getRegisterGroups(groupId?)`<br>`getBitfields(id)` [A9] | ✅ | M | 이름을 하드코딩하지 않고 런타임에 열거함. `RegisterInfo.location`이 있는 항목은 **MMIO** → 별도 등급 |
| 레지스터 읽기 | `registers.read(name) → bigint\|number\|string` [A9] | ✅ | M (halted) | 실행 중 읽기는 불가로 간주함(`WOULD_HALT`) |
| 레지스터 쓰기 | `registers.write(name, v)` [A9] | ✅ | L (R3) | — |
| 메모리 읽기 | `memory.read(loc, count, bitSize?) → unsigned[]`<br>`readOne` [A7] | ⚠️ | M | `count`는 **값의 개수**(바이트 수가 아님). bitSize 기본값은 타깃 word 크기. 페이지는 `"addr@PAGE"`로 지정. **실행 중 동작은 API에 명시되지 않음** [A20] |
| 메모리 맵 | `memory.memMap.get()` [A8] | ✅ | M | 범위 사전 검증용. 접근 허가 맵일 뿐이고 **부작용 분류가 아님** |
| 메모리 쓰기 / fill | `memory.write`<br>`fill` [A7] | ✅ | L (R3) | — |
| 심볼만 로드 | `symbols.load(path)`<br>`add`<br>`loadWithOffset`<br>`remove`<br>`unloadAllSymbols` [A10] | ✅ | M | 타깃에 쓰지 않음. 이미 플래시된 보드에 맞는 방식. `loadProgram`은 타깃에 씀 → 제외 |
| 심볼→주소 / 주소→심볼 | `symbols.getAddress(sym)`<br>`lookupSymbols(start,len)` [A10] | ✅ | M | watch 항목 해석에 사용 |
| 전체 심볼·소스 목록 | — | ❌ | L | 제안: Loom이 ELF/DWARF를 직접 파싱(macOS 쪽). CCS API가 아님 |
| 변수 watch (읽기 전용) | `symbols.getAddress` + `memory.read` [A7][A10] | ✅ | M | **`expressions.evaluate`를 쓰지 않음**. 타입, 폭, 부호는 사용자나 DWARF가 지정 |
| 식 평가 | `expressions.evaluate(expr)` [A14] | ⚠️ | L (R2) | GEL 함수 호출과 상태 변경이 가능함 → 읽기 전용(R0)이 **아님** |
| PC→소스 (live) | `callstack.fetch(maxLevels?) → frames[{functionName?, sourceLine?{path,line,column}, pc}]` [A13] | ✅ | L (첫 소스 단계) | halted일 때만. path는 컴파일 시점 경로로 추정 → 경로 매핑 필요 |
| 주소→소스 (오프라인) | `ds.openSymbolModule(processorId, isBigEndian)`<br>`.lookupSourceLine(addr)`<br>`lookupFunction` [A11] | ⚠️ | L | `processorId` 값이 문서에 없음 → SDK에서 확인 |
| file:line→주소 | — | ❌ | L | 제안: DWARF line table 파싱. 브리지의 `bp.add(file,line)`는 주소를 돌려주지 않음 |
| 경로 매핑 | — | ❌ | L | Loom 설정 `source_map[{from,to}]` [제안] |
| 디스어셈블리 | `symModule.disassemble(start,end) → [{address,numOpCodes,text}]` [A11] | ⚠️ | L | **오브젝트 파일 내용** 기준. 플래시와 다를 수 있음 → UI에 "ELF 기준" 배지 표시. live 메모리 디스어셈블리 ❌ |
| BP 추가 / 삭제 | `breakpoints.add(loc\|file, line?) → id`<br>`remove(id)`<br>`removeAll()`<br>`createProperties`/`setProperty`/`addWithProperties` [A12] | ✅ | L (R2) | 조건 property ID는 문서에 없음 → `getDetailedProperties`로 확인 |
| BP 목록 / enable / disable / hit 횟수 | — | ❌ | L | **브리지가 자체 테이블을 관리함**. disable은 remove 후 다시 add(id가 바뀜). hit은 halt 시 PC와 BP 주소를 비교해 **추정**(API 아님) |
| 멀티코어 동시 run/halt | `ds.simultaneous.run/halt/asmStepInto`<br>`globalBreakpoint` [A16][A17] | ✅ | L | — |
| 프로그램 로드 / 플래시 | `memory.loadProgram`<br>`loadBinaryProgram`<br>`verifyProgram` [A7] | ✅ | X (R4) | MVP에서 제외 |
| 타임아웃 | `ds.setScriptingTimeout(ms)`<br>`ScriptingTimeoutError` [A4][A19] | ⚠️ | M | **이후 모든 호출에 전역 적용됨**. 호출별 취소는 없음 → §5.4 |
| 버전 증거 | `ds.versionInfo()` [A4] | ✅ | M | hello와 세션 증거에 기록 |
| STM / trace | — | X | X | 범위 밖 [S9][S11] |

### 1.1 자체 Loom UI와 CCS UI 임베드

- **자체 UI [제안]**
  - 같은 저널을 사용자와 에이전트가 함께 읽습니다.
  - STOP, ActionGate, RowStore, 증거 참조와 일관됩니다.
  - 상태 품질을 표시할 수 있습니다.
- **CCS UI 임베드 [미확인/비권장]**
  - 원격 Theia 화면을 iframe으로 넣는 경로는 문서화되어 있지 않습니다.
  - 같은 Debug Server를 두 클라이언트가 동시에 제어하는 방식도 문서에 없습니다.
    - attach 핸들 [A4]은 "동시 상호작용 가능"으로만 적혀 있습니다.
  - 사용자가 브리지 호스트에서 CCS IDE를 직접 쓰는 것은 **Loom 밖의 별도 작업**입니다. 그동안 Loom 세션은 `dbg.close`로 비워야 합니다.

### 1.2 소스 창의 실현 범위

| 항목 | 지금 설계 | 근거 |
|---|---|---|
| 심볼 파일 | `symbols.load(.out)`. 경로는 브리지 호스트 기준 | [A10] |
| 소스 파일 | 브리지는 소스를 열지 않음. macOS 쪽 `source_map`으로 compile path를 로컬 경로로 바꿔 Loom UI가 읽음. 찾지 못하면 "소스 없음, 심볼만" 표시 | [제안], 경로 API ❌ |
| 현재 PC 줄 | halted이고 `run_epoch`가 같을 때만 `callstack.fetch(1)`. running이면 마지막 halt 위치를 회색으로 표시 | [A13] |
| BP 표시 | 브리지 테이블(id, 요청 위치, 해석된 주소는 가능할 때만) | ❌ → 브리지 소유 |
| DAP / Monaco | **약속하지 않음.** 위 API로 DAP 어댑터를 만들 수는 있지만 file:line→주소, BP 상태, 변수 스코프 열거가 없어 기능이 부분적임 | — |

---

## 2. 배치, 설치, 설정

### 2.1 구성요소와 위치

| 위치 | 구성요소 | 근거 |
|---|---|---|
| macOS (Darwin arm64) | `loomd` gateway + `dbg` 도메인, `dbg` worker(프로브당 하나), UI, `loom` CLI, SSH 클라이언트 | [저장소] worker IPC는 로컬 전용 (`crates/loom-worker-host/src/listen.rs`) |
| 브리지 호스트 (x64 Win 10/11 또는 Ubuntu 22.04/24.04) | CCS 21.0.1 (Debug Server, device support, scripting), Blackhawk 드라이버, `loom-ccs-bridge` (supervisor + executor), sshd, 토큰 파일 | [S3][S26][A1–A3] |
| 브리지 ↔ 프로브 | USB 또는 LAN (BH-XDS-560v2-BP/PoE는 둘 다, BH-USB-560v2는 USB만) | [S13][S14] |
| 프로브 ↔ 타깃 | JTAG/cJTAG. SWD 전용 칩과 MSP430은 불가 | [S11] |

- Linux 호스트는 `sudo <CCS>/ccs/install_scripts/install_drivers.sh`를 실행해야 합니다 [S26].
- Blackhawk 공식 페이지는 CCS v10까지만 명시합니다 [S13]. **CCS 21.0.1과의 조합은 T9에서 확인합니다.**

### 2.2 설정 (loom.toml) [제안]

경로는 모두 **브리지 호스트 기준**입니다. 하드웨어 정보는 매개변수로만 받습니다.

```toml
[[dbg.probe]]
alias      = "bh1"
model      = "BH-XDS-560v2-BP"      # 사용자 입력: 라벨 그대로. "USB560"/"LAN560"이면 v1이라 미지원
transport  = "lan"                  # usb | lan
backend    = "ccs-remote"           # ccs-remote | fake
endpoint   = "127.0.0.1:47560"      # Mac 쪽 ssh -L 로컬 포트
token_file = "~/.config/loom/dbg/bh1.token"
ccxml      = "C:/loom-bench/bh1/target.ccxml"

[[dbg.probe.core]]
alias             = "cpu1"
core_pattern      = "C28xx_CPU1"    # openSession 정규식. listCores 결과로 확정 [미확인]
au_bits           = 16              # C28x는 16비트 addressable [A25][A26]. 다른 코어는 HW 수용 때 확정
symbols           = "C:/loom-bench/bh1/app.out"   # symbols.load: 타깃에 쓰지 않음
source_map        = [{ from = "C:/ws/app", to = "/Users/<me>/ws/app" }]
run_state_poll_ms = 250             # 제안값. 측정치는 따로 보고
stale_after_ms    = 1000

[[dbg.probe.core.region]]
name  = "ls_ram"
start = "0x8000"
end   = "0x9FFF"
class = "ram"                       # ram | flash | mmio | unknown
nonintrusive_read = false           # true는 T9에서 그 코어·모드로 확인한 뒤에만
```

- 실행 중 읽기 허용 영역은 **브리지 호스트에도** `loom-ccs-bridge.policy.json`(0600)으로 따로 둡니다.
  - worker 판정과 브리지 판정이 모두 통과해야 읽습니다 (이중 게이트).
  - 요청 필드만으로는 비침습을 주장할 수 없습니다.

SSH 터널 (사용자가 실행하거나 launchd로 관리; Loom이 SSH 키를 관리하지는 않음):

```
ssh -N -L 127.0.0.1:47560:127.0.0.1:47560 bench@bridge-win11
```

### 2.3 연결 시 부작용

- connect는 ccxml에 지정된 시작 GEL의 `OnPreTargetConnect`와 `OnTargetConnect`를 동기로 실행합니다 [A21–A24].
  - GEL이 `GEL_Reset` 같은 상태 변경을 하면 타깃이 halt됩니다.
  - 비침습 연결은 real-time mode에 대해서만 설명되어 있습니다 [A20].
- 그래서 브리지는 connect 전후에 다음을 증거로 남깁니다.
  - `isHalted()`
  - `getDetailedSettings()` 덤프
  - ccxml sha256
  - 시작 GEL 경로 (ccxml에서 파싱 [제안])
- `dbg.session.connect_side_effects`는 `"not_proven_none"`으로 기록합니다.
- **GUI 옵션 하나를 껐다고 해서 ccxml/GEL 전체가 비침습이라고 주장하지 않습니다.**
- connect 전에 running이었다가 connect 후 halted가 되면, 그 사실을 `dbg.run_state{source:"connect"}`와 경고로 표시합니다. 자동으로 다시 run하지 않습니다.

---

## 3. Loom 통합 지점

### 3.1 새 파일 트리 [제안]

```
crates/loom-dbg/                       # 새 도메인 (loom-psu와 같은 구조)
  Cargo.toml                           # features: runtime, cli
  src/lib.rs                           # DOMAIN="dbg", WORKER_KIND="dbg", KIND_* 상수
  src/model.rs                         # DTO: U64 문자열 serde, Address{au_bits}, Value, Epochs, SampleId
  src/backend.rs                       # DebugBackend trait, ControlLane, TargetCaps (contracts/sketch/backend.rs)
  src/fake.rs                          # FakeBackend: 스크립트 run-state, 메모리 맵, 지연·끊김·kill 주입
  src/ccs_remote.rs                    # bridge v1 JSON-lines 클라이언트, 시계 오프셋, 토큰
  src/worker.rs                        # 프로브당 직렬 큐, 우선순위, watch 루프, latch 처리
  src/gateway.rs                       # DomainModule::register — 명령, 위험표, 이벤트 종류, WorkerSpec, manifest, help
  src/gateway/{handlers,hooks,evidence,reducer}.rs
  src/cli.rs                           # loom dbg …
  bridge/                              # Node (CCS 동봉 Node로 실행). Rust 빌드와 분리
    supervisor.mjs                     # 소켓, 인증, ctl 레인, 큐, 데드라인, executor 수명주기
    executor.mjs                       # CCS Scripting 동기 호출 1개씩. stdin/stdout JSON-lines
    stub-executor.mjs                  # CCS 없이 프로토콜 테스트용
    test/*.test.mjs
ui/src/panels/dbg/
  dbgModel.ts                          # reducer (contracts/sketch/dto.ts)
  sessionPanel.ts  watchPanel.ts  regsPanel.ts  memoryPanel.ts  logPanel.ts
  dbg.css  fixtures.ts  dbg.test.ts
skills/loom-dbg/SKILL.md               # T5. 짧은 규칙과 명령 예시
```

### 3.2 기존 파일 수정 지점 [저장소]

| 파일 | 수정 내용 |
|---|---|
| `Cargo.toml` (workspace) | members에 `crates/loom-dbg` 추가 |
| `crates/loom-gateway/Cargo.toml:25,55` | psu와 같은 형태로 `loom-dbg` optional 의존성과 `dbg` feature 추가 |
| `crates/loom-gateway/src/domains.rs:178-184` | `#[cfg(feature="dbg")] inproc!(dbg, …)` 등록 |
| `crates/loom-gateway/src/domains.rs:236-237` | worker kind match에 `loom_dbg::WORKER_KIND => run_worker(DbgWorker)` 추가 |
| `crates/loom-cli/Cargo.toml:25,38`, `crates/loom-cli/src/lib.rs:55-56` | `CliOf::<loom_dbg::cli::DbgCli>` 추가 |
| `crates/loom-proto/src/error.rs` | **선택**: `E_WOULD_HALT` 추가 [제안 proto 변경]. 추가 전에는 `E_UNSUPPORTED` + `detail.reason="would_halt"` 사용 |
| `ui/src/panels/index.ts:7` 근처 | `import './dbg/sessionPanel'` 등 |
| `ui/src/shell/shell.css`, `ui/src/shell/dock/*` | T4b: 범위를 한정한 Bench Light 토큰 (§3.5) |

### 3.3 도메인 등록 [제안]

`gateway.rs`는 `loom-psu/src/gateway.rs:252-416` 패턴을 그대로 따릅니다.

**명령과 위험 등급** (`CommandSpec`, `RiskTable` — `crates/loom-domain-api/src/registry.rs:129,216,230`)

`default_action`은 모두 `None`입니다. F1 정책에 따라 자동으로 실행되고 사용자 정책이 우선합니다. 새로 필수 승인 흐름을 만들지 않습니다.

| method | write_class | 위험 (selector) | 비고 |
|---|---|---|---|
| `dbg.status`, `dbg.targets`, `dbg.cores` | 읽기 | R0 | 저널과 worker 스냅샷 |
| `dbg.open` | Request | **R2** | connect 시 GEL이 실행되어 halt나 reset이 일어날 수 있음 |
| `dbg.close` | Request | R1 | disconnect + session.close. 마지막 코어일 때만 deconfigure. halt/run/reset **요청은 보내지 않음**. 단 SDK/GEL disconnect 훅이 타깃에 영향을 줄 수 있으므로 타깃 상태 유지를 보장하지 않고, 이후 run_state는 `disconnected`(quality unknown)로 둠 |
| `dbg.halt` / `dbg.run` / `dbg.step` | Request | R2 | `success_terminal = confirmed` (사후 run_state 관측이 따라옴) |
| `dbg.read` (1회) | 읽기 | R0. `region=mmio\|unknown`이면 **R1** | §6.2 |
| `dbg.watch.start` / `dbg.watch.stop` | Job | 위와 같음 | `Bounds.max_wait ≤ 600 s` |
| `dbg.mem.read` (블록) | 읽기 | R0 / R1 | 아티팩트 |
| `dbg.eval` | Request | R2 | 이후. GEL 부작용 가능 |
| `dbg.bp.add` / `dbg.bp.remove` | Request | R2 | 이후 |
| `dbg.reg.write` / `dbg.mem.write` / `dbg.reset` | Request | R3 | 이후 |
| `dbg.load` / `dbg.flash` | — | R4 | 제외 (`immutable_deny`에 둘지는 사용자가 결정) |

**worker 설정**

```
WorkerSpec {
  kind: "dbg",
  instances: PerChannel { max: 4 },       // 프로브당 하나
  reopen_after_crash: Reopen::None,
  arch: Native,
  platforms: [Windows, Linux, Macos],
}
```

- macOS worker는 원격 endpoint의 **클라이언트**입니다. 프로브 드라이버를 Mac에서 돌리는 것이 아닙니다.

**등록하지 않는 것**
- `safe_state`, `emergency_off`: 디버거에는 "안전한 방향"이 하나로 정해져 있지 않습니다. halt는 제어 루프를 멈출 수 있습니다.
- 복구 동작이 필요하면 사용자가 halt나 run을 직접 고릅니다.

**capability 문자열**
- `"ccs-remote"`, `"fake"`, `"read:halted"`, `"watch:batch32"`
- `"read:running-declared"`는 T9를 통과한 뒤에만 추가합니다.

### 3.4 저널 이벤트 종류 [제안]

channel은 `target_id`입니다 (`bh1/cpu1`).

| kind | 우선(control) 경로 | payload | 용도 |
|---|---|---|---|
| `dbg.link` | ✅ | bridge instance, endpoint, rtt, offset_err, state_rev | 헤더, lost 판정 |
| `dbg.session` | ✅ | core_name, session_epoch, ccxml sha, caps, settings 덤프 ref, `connect_side_effects` | 헤더, 증거 |
| `dbg.run_state` | ✅ | `contracts/dbg-dto.schema.json#RunStatePayload` | 상태 칩, 스냅샷 무효화 |
| `dbg.action` | — (RowStore 행) | verb, write_state, effect, request_id | 동작 기록 |
| `dbg.sample` | — | `SampleBatch` (스칼라 정본 값) | watch, 레지스터, 에이전트 증거 |
| `dbg.mem_block` | — | `MemBlockRef` (artifact + sha256) | hex 뷰 |
| `dbg.gap` | — | reason, 구간, missed_polls | 끊김 표시 |
| `dbg.log` | — | 브리지 stderr, SDK 메시지 (길이 제한) | 오류 기록 |

UI는 우선 kind를 받기 위해 `ctx.onControl`을 **반드시** 사용합니다.
- `ui/src/sdk/context.ts:63-67` 주석: 우선 kind는 데이터 구독에 snapshot/backlog로만 들어옵니다.
- PSU 패널도 같은 방식입니다 (`ui/src/panels/psu/gaugePanel.ts:391`).

### 3.5 UI 패널과 manifest [제안]

파일: [`contracts/dbg-panels.manifest.json`](contracts/dbg-panels.manifest.json)

- `ActionSpec`에는 실제 필드만 씁니다 (`ui/src/sdk/manifest.ts:28-41`): `id`, `rpc`, `class`, `right`, `confirm`, `safetyDirection`, `label`.
- UI 등록 형태는 PSU와 같습니다 (`gaugePanel.ts:433-456`):

  ```ts
  registerPanel({
    …,
    propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP } },
    topics: [{
      key: 'samples',
      filter: (p) => ({ domains: ['dbg'], channels: [p.channel], kinds: ['dbg.sample', 'dbg.gap', 'dbg.run_state'] }),
      snapshot: 'required',
    }],
  }, factory)
  ```
- 동작은 모두 `confirm: 'none'`, `right: 'operate'`입니다.
  - ActionGate (`ui/src/sdk/actionGate.ts`)는 실수 방지 장치이고, 결정은 데몬 정책이 합니다.
  - `writeLocked`(STALLED)이면 `E_STALLED`로 막힙니다.
  - 변경 요청은 다시 시도하지 않습니다.
- dbg 동작에는 `safetyDirection:'reduce'`를 **붙이지 않습니다**. STOP은 셸 전역 기능입니다 (`Ctrl-] x`, `ui/src/shell/prefixKeys.ts`).
- 동작 기록 패널은 `ctx.rows({domain:'dbg'})` RowView를 사용합니다 (`ui/src/sdk/rowView.ts`).
  - `write_state`(requested→…→confirmed / failed / outcome_unknown)와 origin(USR/AGT)을 표시합니다.
- 메모리 패널은 `ctx.media('/api/v1/dbg/blocks/<id>')`로 아티팩트 바이트를 읽습니다.
  - manifest에 `media.prefixes: ['/api/v1/dbg/']`를 선언합니다 (`manifest.ts:55-57`).
  - 서버 쪽 media 라우트도 함께 추가해야 합니다 [제안].

**테마 (사용자 지적 반영)**
- 현재 셸 기본값은 **다크**입니다 (`ui/src/shell/shell.css:3-22`, `color-scheme: dark`).
- `dockview.css:1599`의 `.dockview-theme-light`는 vendor 테마입니다. **제품 Bench Light 테마가 구현되어 있다는 근거가 아닙니다.**
- Bench Light는 요청된 디자인으로 유지합니다. 구현 방식:
  - 티켓 T4b로 workbench 루트 범위 토큰 `[data-theme="bench-light"]`을 재정의하고, dockview light 테마와 맞춥니다.
  - dbg 패널은 토큰만 사용합니다.
  - hex/console 영역은 패널 단위로 다크를 선택할 수 있습니다.
- 앱 전체를 다시 쓰지 않습니다.

---

## 4. 데이터 모델과 값 의미론

스키마: [`contracts/dbg-dto.schema.json`](contracts/dbg-dto.schema.json)

### 4.1 식별자와 카운터

| 이름 | 형식 | 규칙 |
|---|---|---|
| `probe_alias` | `[a-z][a-z0-9_-]*` | 사용자 설정 |
| `target_id` | `<probe_alias>/<core_alias>` | 패널 `channel` prop과 저널 channel. CCS 코어 이름은 `core_name`(불투명 문자열)으로 따로 저장 |
| `bridge_instance` | uuid | supervisor가 시작될 때마다 새로 생성 |
| `session_epoch` | u64 문자열 | 코어 `session.open`이 성공할 때마다 +1. **영속 high-water**를 두어 재사용하지 않음 |
| `run_epoch` | u64 문자열 | 세션 안에서 running/halted/stepping으로 전이를 관측할 때마다 +1 |
| `state_rev` | u64 문자열 | run_state, quality, link, session 중 무엇이든 바뀌면 +1. 타깃 단위로 단조 증가. UI reducer는 `≤` 이벤트를 버림 |
| `sample_id` | `<target_id>@<session>.<run>.<acq_seq>` | UI와 에이전트가 **같은 문자열**로 같은 관측을 비교함 |
| `request_id` | 기존 Loom | RowStore 행, 브리지 `req.request_id`로 상관관계 연결 |

### 4.2 주소와 값

- **주소**: `{addr:"0x8000", space?: "<page>", au_bits: 16}`
  - 주소 1 증가 = **addressable unit 1개**입니다.
  - C28x는 16비트 AU입니다 [A25][A26]. 32비트 변수 = 2 AU, 다음 요소 주소 = `+bits/16`.
  - 다른 코어는 설정이나 T9로 확정합니다.
  - `au_bits`가 없는 코어는 `mem.read`를 거부합니다 (8을 가정하지 않음).
- **브리지 호출**: `memory.read(loc, count, bitSize)` [A7]
  - count = 값 개수, 반환값은 **unsigned**입니다.
  - 브리지가 JS BigInt를 `"0x…"` 문자열로 바꿉니다 (BigInt는 JSON으로 직렬화되지 않음 [A18]).
  - 페이지는 `"0x8000@<page>"`로 전달합니다 [A7][A18].
- **값**: `{hex, bits, signed, dec?, endian?, f?}`
  - 부호 해석은 Loom이 합니다 (SDK는 unsigned로 돌려줌).
  - 다중 AU 값의 endian은 [미확인] → T9에서 알려진 상수로 확인합니다.
  - `f`(float)는 표시 전용입니다.
- **u64**: JSON Schema 패턴은 문법만 검사합니다(≤20자리). 런타임 파서가 **u64::MAX 초과를 거부**해야 합니다.
  - 예시: `contracts/examples/bridge-negative.jsonl` n7
  - TS에서는 `BigInt()`로만 파싱합니다.

### 4.3 정본 값: 스칼라 watch와 메모리 블록 (사용자 지적 반영)

**스칼라 watch 배치(≤32 항목)는 정본 저널 값입니다** (`dbg.sample`).
- UI watch/레지스터 패널과 에이전트 결과는 **같은 `dbg.sample` 레코드**를 읽습니다.
- `dbg.sample`에 없는 값은 "관측값"으로 표시하지 않습니다.
- 요약(min/max/count)만으로는 UI와 에이전트가 본 값이 같다는 것을 증명할 수 없습니다. 그래서 match, first, last 값은 항상 `sample_id` 레코드로 남깁니다.
- **일관성 한계**: `sample_id` 하나 = 한 배치 = **같은 기록 획득 구간**(`bridge_start_ns`–`bridge_end_ns`)입니다. 항목들을 하드웨어에서 원자적으로 동시에 찍은 스냅샷이 **아닙니다**. 항목은 순서대로 읽힙니다. CPU가 halted여도 DMA와 주변장치는 계속 움직일 수 있어, 같은 배치의 MMIO/DMA 버퍼 값끼리 시점이 다를 수 있습니다.

**기록 방식**

| mode | 기록 대상 | 마지막 값의 신선도 |
|---|---|---|
| `record:"all"` | 획득한 배치를 모두 기록 (항목 수 × 주기로 상한) | 각 배치가 증거 |
| `record:"change"` (기본) | `first`, 값이 바뀐 배치, `match`, `last`, 그리고 값이 같아도 `heartbeat_ms`(기본 1 s)마다 1건 | heartbeat 레코드가 증거 |

- 변화 없이 건너뛴 poll은 `WatchResult.polls`와 `recorded`의 차이로 셉니다.

**큰 메모리 블록**은 불변 아티팩트로 둡니다 (`dbg.mem_block`).
- 내용: `artifact_id` + `sha256` + `bytes` + `sample_id` + 공유 읽기 경로 `/api/v1/dbg/blocks/<id>`
- UI hex 뷰와 에이전트는 같은 digest의 바이트를 읽습니다. 미리보기(≤64 AU)는 표시 전용입니다.

**보존, 손실, 마지막 값**

| 항목 | 규칙 |
|---|---|
| 저널 보존 | Loom 기존 정책을 따름. 패널 mirror(`mirrorRows`)는 화면 캐시일 뿐이고 정본이 아님 |
| 아티팩트 보존 | 바이트 상한 LRU [제안]. 삭제되면 `artifact_missing`을 표시하고 **재구성하지 않음**. 레코드(sha256)는 남음 |
| 손실 | `dbg.gap{reason: busy\|deadline\|link\|would_halt\|item_error\|stop}`. 그래프 선을 끊고 **보간하지 않음** |
| last-known | 항목마다 가장 최근 `dbg.sample`. `current = (session_epoch, run_epoch)`가 현재와 같고 age ≤ `stale_after_ms`일 때만 `fresh` |
| 무효화 | `run_epoch`가 증가하면 이전 레지스터와 메모리 값은 **current=false**. 지우지 않고 "마지막 halt(epoch N) 기준 · M초 전"으로 표시 |
| 연결 해제 후 | 마지막 스냅샷과 이력을 계속 보여줌 (UART/UDS/PSU 이력 교훈). 상태 칩은 `lost (마지막 관측 running, 12 s 전)`. 명시적 close 뒤에는 `disconnected · 타깃 상태 미관측` |

---

## 5. 브리지 프로토콜 (JSON-lines v1) [제안]

스키마: [`contracts/bridge-v1.schema.json`](contracts/bridge-v1.schema.json)

예시 (모두 스키마로 검증함, §10.3):
- [`bridge-session.jsonl`](contracts/examples/bridge-session.jsonl) — 정상 흐름과 STOP
- [`bridge-deadline.jsonl`](contracts/examples/bridge-deadline.jsonl) — 독립된 새 세션에서 데드라인 초과
- [`bridge-negative.jsonl`](contracts/examples/bridge-negative.jsonl) — 거부되어야 하는 7건

### 5.1 왜 프로세스 2개인가

- CCS Scripting은 동기 API입니다. 호출하는 동안 Node 이벤트 루프가 블록됩니다 [A1][A2].
- 같은 프로세스 안의 readline 루프는 호출 중에 STOP, heartbeat, 취소에 **응답할 수 없습니다.**
- 그래서 두 프로세스로 나눕니다.
  - **supervisor**: CCS 모듈을 로드하지 않는 Node 프로세스
  - **executor**: CCS Scripting을 사용하는 자식 프로세스. 한 번에 op 하나만 실행

```
dbg worker (mac) ──TCP(SSH)──► supervisor (bridge host, 127.0.0.1:47560)
                                 ├─ ctl lane: ping/stop/release/cancel/status/kill_executor → 즉시 응답
                                 ├─ queue (prio action > read > poll, max 16)
                                 └─ stdin/stdout JSON-lines ─► executor (run.sh/run.bat, CCS Scripting)
                                                                 ds / sessions{core_alias→DebugSession}
```

- attach 핸들로 두 번째 CCS 인스턴스를 만드는 방식 [A4]은 쓰지 않습니다. 한쪽이 블록된 동안 다른 쪽이 처리할 수 있는지가 [미확인]이기 때문입니다.

### 5.2 메시지 종류

| t | 방향 | 핵심 필드 |
|---|---|---|
| `hello` | sup→client | bridge version/instance, ccs root/version (모르면 "unknown"), host os/arch, executor 상태, clock{mono_ns, wall_ms}, limits, caps |
| `auth` | client→sup | token (≥32자), client 정보, `takeover` (기본 false → 두 번째 클라이언트는 `BRIDGE_BUSY`) |
| `req` | client→sup | id, op, core, session_epoch, deadline_ms, prio, origin, request_id, watch_id, args |
| `ack` | sup→client | queued_at_ns, queue_pos |
| `res` | sup→client | phase, **sdk_call**, **effect**, timing{recv,start,end}, run_state_after, data, error |
| `evt` | sup→client | seq (건너뛰면 gap), kind: run_state/executor/log/gap/lost/heartbeat |
| `ctl` / `ctl_res` | 양방향 | in_flight{id, op, started_ns, cancellable:false}, cancelled[], executor 상태 |

**스키마로 강제하는 규칙** (ROOT 검토 반영)
- session.* 이외의 op는 `core`와 `session_epoch`가 **필수**입니다.
- `prio:"action"`이면 `origin`과 `request_id`가 필수입니다.
- `sdk_call`과 `effect`는 **서로 다른 축**입니다.
  - `sdk_call ∈ {not_started, started_no_result, completed}`
  - `effect ∈ {none, applied, unknown}`은 타깃 **변경** 효과만 뜻합니다.
- phase에 따른 강제:
  - `rejected`/`cancelled_queued` → `sdk_call=not_started`, `effect=none`
  - `deadline`/`executor_died` → `sdk_call=started_no_result`
  - `done` → `sdk_call=completed`이고 effect는 `unknown`이 될 수 없음
- **`effect=none`을 보고 "SDK를 호출하지 않았다"고 추론하지 않습니다.** 성공한 읽기는 `completed` + `none`입니다.

### 5.3 op → CCS API 매핑 (executor)

| op | 호출 (문서 [A#]) | 비고 |
|---|---|---|
| `session.configure` | `ds.configure(ccxml)` · `ds.versionInfo()` | 이미 구성된 상태면 `deconfigure()`를 **자동으로 호출하지 않음** → 오류 반환 |
| `session.cores` | `ds.listCores()` | — |
| `session.open` | `ds.openSession(core_pattern)` · `getName()` · `settings.getDetailedSettings()` | 열린 이름이 설정의 코어와 다르면 `NO_SUCH_CORE` |
| `session.close` | `target.disconnect()` → `session.close()`<br>마지막 코어면 `ds.deconfigure()` | run/halt 요청 없음. disconnect 훅의 영향은 미확인 → 해제 후 상태는 unknown, 다시 연결해야 관측 가능 |
| `target.connect` | `isConnected()` → `connect()` → `isHalted()` | 전후 상태와 설정 덤프를 증거로 남김 |
| `target.state` | `isHalted()` (+ `isConnected()`) | 폴링. 실행 중에 이 호출 자체가 침습적인지는 [미확인] (T9) |
| `target.halt` / `target.run` | `halt()` / `run(false)` | 끝난 뒤 `isHalted()`를 다시 관측 |
| `target.step` | `stepInto/stepOver/stepOut(explicitlyAsm?)` | 블록됨 |
| `reg.read` | `registers.getRegisterGroups()` (캐시) · `registers.read(name)` | halted 상태에서만 |
| `read.batch` / `mem.read` | `symbols.getAddress(sym)` (캐시) · `memory.read(loc,count,bitSize)` | 실행 중이고 **브리지 호스트 정책 파일**에 등록된 영역이 아니면 `WOULD_HALT`. 메모리 호출을 하지 않음. 요청 필드로는 허용 범위를 넓힐 수 없음 |
| `sym.lookup` | `symbols.getAddress` · `lookupSymbols` | — |
| `expr.eval` | `expressions.evaluate` | 이후. R2 |
| `bp.add` / `bp.remove` / `bp.list` | `breakpoints.add/remove`, **list는 브리지 테이블** | 이후 |
| `src.addr_to_line` | `callstack.fetch` (live) / `symModule.lookupSourceLine` (offline, processorId 확인 필요) | 이후 |
| `src.line_to_addr` | — (❌) → Loom DWARF | 이후 |
| `disasm` | `symModule.disassemble(start,end)` | 이후. ELF 기준 |

### 5.4 데드라인, 타임아웃, 취소, STOP

- `setScriptingTimeout`은 **이후 모든 호출에 전역 적용**됩니다 [A4][A19].
  - executor는 op마다 그 op의 `deadline_ms`로 다시 설정한 뒤 호출합니다 (호출 2번).
  - 이렇게 호출별 타임아웃을 **흉내 낼 뿐**이고, 진행 중인 호출을 취소하는 기능은 아닙니다.
- **`ScriptingTimeoutError`**
  - `res{phase:"deadline", sdk_call:"started_no_result", effect: 변경 op면 "unknown", 읽기 op면 "none"}`
  - 이후 하위 동작과 타깃 상태는 문서에 없습니다 [A19].
  - 그래서 run_state quality는 `unknown`으로 둡니다. 다음 `target.state` 관측이 성공해야 회복합니다.
  - TI 예제처럼 타임아웃 뒤 halt하는 동작은 **하지 않습니다.**
- **executor가 응답하지 않을 때**
  - deadline + grace(제안 3 s)를 넘기면 supervisor가 executor 프로세스 그룹을 종료합니다.
  - 이후: `evt lost{reason:"executor_killed"}`, 해당 `session_epoch`은 **죽은 것으로 처리**, executor를 idle로 재시작합니다.
  - 자동 재연결, 자동 configure/open은 하지 않습니다 → `NOT_OPEN` (예시 bridge-deadline.jsonl d2).
- **STOP** (`WorkerHost::latch`, 200 ms 안에 ack — `crates/loom-gateway/src/worker_host.rs:18-19`)
  - worker가 하는 일:
    - 모든 watch 중지
    - `ctl stop`: 큐에서 대기 중인 요청을 `cancelled_queued`/`effect none`으로 처리
    - 대기 중이던 쓰기 행: `write_state=failed (E_STOPPED)`
  - **진행 중인 호출은 끝까지 실행됩니다.** 큐 취소 ≠ 물리 동작 되돌림.
    - 예: STOP 직전에 보낸 `run`은 그대로 적용됨 (bridge-session.jsonl a2)
  - supervisor는 latched 상태가 되어 agent/script/test가 보낸 이후 요청을 `STOP_LATCHED`로 거부합니다 (a3).
    - Loom worker가 먼저 막으므로 이것은 다중 방어입니다.
    - 사용자 요청(읽기 등)은 허용됩니다 (r9).
  - STOP과 정리 단계에서 **halt, run, reset, flash, disconnect를 하지 않습니다.**
- **재시도 금지**
  - 쓰기나 동작이 `effect=unknown`이면 `write_state=outcome_unknown`으로 두고 다시 보내지 않습니다.
  - 읽기 poll은 다음 주기가 새 요청입니다 (재시도가 아님).

### 5.5 큐, 우선순위, 역압

- **프로브 하나 = supervisor 하나 = executor 하나 = 직렬 큐 하나**입니다. JTAG 체인을 공유하기 때문입니다.
  - 멀티코어는 executor 안의 코어별 `DebugSession`으로 처리합니다 (affinity: `core` 필드).
- 우선순위: `action > read > poll`
  - poll은 watch마다 동시에 1개만 둡니다. 다음 tick에도 앞 poll이 끝나지 않았으면 `missed`로 세고, 쌓아 두지 않습니다.
- 큐 상한은 16개(제안)입니다. 넘치면 `QUEUE_FULL → E_WORKER_BUSY`.
- `evt.seq`가 건너뛰면 supervisor 링 버퍼가 넘친 것입니다. worker가 `dbg.gap{reason:"link"}`을 기록합니다.

### 5.6 신뢰 경계

- supervisor는 브리지 호스트의 **127.0.0.1에만 bind**합니다.
- 경계는 두 겹입니다.
  1. SSH (호스트 간 인증과 암호화)
  2. 토큰 (같은 호스트의 다른 로컬 사용자를 막음. 토큰 파일 0600)
- 연결은 한 번에 클라이언트 하나만 허용합니다. `takeover`는 사용자가 명시적으로 명령할 때만 씁니다.
- 브리지는 임의 JS나 GEL 텍스트를 받지 않습니다. op 목록에 있는 것만 실행합니다. `expr.eval`은 이후 단계이고 R2입니다.

### 5.7 시계

- `hello.clock`과 `ctl ping`을 왕복해 오프셋을 추정합니다 (중간점 방식; 오차 = RTT/2).
- 기록 필드:
  - 브리지 단조 시각: `bridge_start_ns`, `bridge_end_ns`
  - Mac 단조 시각: `mac_send_mono_ns`, `mac_recv_mono_ns`
  - `offset_ns ± offset_err_ns`
- **두 호스트의 단조 시계 값을 직접 빼지 않습니다.** 오차보다 작은 차이는 의미가 없습니다.

---

## 6. 동작과 읽기 의미론

### 6.1 정책

- 기존 Loom 기본값을 따릅니다. F1: 모든 등급 `Auto`, `PolicyAction` — `crates/loom-proto/src/policy.rs:17-27`.
- 사용자가 `Confirm`/`Approve`/`Deny`로 바꾸면 그 설정이 적용됩니다.
- UI의 `confirm`은 `none`입니다. 새 필수 승인 흐름은 없습니다.
- 등급은 기록과 정책의 키로 쓰입니다 (§3.3).

### 6.2 읽기가 "읽기 전용"이 아닌 경우

| 경우 | 처리 |
|---|---|
| 타깃 running + 영역 미선언 | `WOULD_HALT` (SDK 호출 없음) → `E_UNSUPPORTED{reason:"would_halt"}` 또는 제안 `E_WOULD_HALT`. **자동 halt→read→resume을 하지 않음** |
| "debugger 접근 전 halt" 설정이 켜져 있을 가능성 | 기본값은 off [S23]. 설정 ID를 모름 [A15] → 덤프에서 확인되기 전에는 running read를 허용하지 않음 |
| MMIO (`RegisterInfo.location` 있음, 또는 region class `mmio`/`unknown`) | 읽기만 해도 상태 비트가 지워질 수 있음 → R1 selector로 분류. watch 항목에 `region_class`를 명시해야 하고, 기본 watch 허용 목록에서 제외 |
| `expressions.evaluate` | GEL 함수 호출과 쓰기가 가능함 [A14] → `dbg.eval`(R2)로 분리. watch 항목은 `symbol`/`address`/`register`만 받음 |
| halted 상태의 안정성 | executor가 소유자 하나뿐이므로 halted→running 전이는 **우리 run/step 요청으로만** 일어남. running→halted(BP hit)는 읽기에 안전한 방향. 다른 디버거 인스턴스가 붙어 있으면 이 가정이 깨짐 → `dbg.open`은 다른 클라이언트가 이미 연결된 상태를 거부 (LAN 프로브는 `dtc_conf … ipClient` 사전 점검 [S18]) |

### 6.3 오류 코드

- 기존 `ErrorCode`(`crates/loom-proto/src/error.rs:91+`)로 매핑합니다.
- 새 코드는 `E_WOULD_HALT` 하나만 **제안**합니다. 나머지 매핑: `dbg-dto.schema.json#ProposedErrorMapping`

| 브리지 오류 | Loom ErrorCode |
|---|---|
| `EPOCH_MISMATCH` | `E_CONNECTION_CHANGED` |
| `LINK_LOST` | `E_DISCONNECTED` |
| `DEADLINE` | `E_TIMEOUT` |
| `CANCELLED_BY_STOP` / `STOP_LATCHED` | `E_STOPPED` |
| `QUEUE_FULL` | `E_WORKER_BUSY` |
| `BRIDGE_BUSY` | `E_BUSY` |
| `SDK_ERROR` | `E_NO_RESPONSE` |

---

## 7. UI 설계 (Bench Light 워크벤치) [제안. 실제 프로브 화면이 아님]

### 7.1 레이아웃

| 영역 | 내용 |
|---|---|
| 상단 세션 바 (`dbg.session`, channel_bar 슬롯) | backend `ccs-remote` · probe `bh1 BH-XDS-560v2 LAN` · host `bridge-win11 (127.0.0.1:47560)` · CCS `21.0.1` 또는 `unknown` · core `cpu1 = <core_name>` · 상태 칩 `RUNNING · 0.4 s 전 · epoch 7/3` · RTT · 진행 중 동작. **SIM/BENCH 배지만 단독으로 표시하지 않음** |
| 왼쪽 | 타깃 트리 (probe → core, 각 상태 칩), 저장된 구성 (ccxml/symbols/source_map) |
| 가운데 탭 | Watch (MVP 기본), Memory (hex; 다크 옵션), Registers. 이후 단계: Source, Disassembly, Breakpoints |
| 오른쪽/아래 | 동작·오류 기록 (RowStore), 증거 refs, gap |
| 아래 입력줄 | 주소/심볼 입력 초안, 히스토리 |

- dockview로 분할, 크기 조절, 최대화, 탭, 레이아웃 저장을 합니다.
  - 기존 `LocalLayoutStore`(`ui/src/shell/dock/layoutStore.ts`)와 propsSchema 검증을 그대로 사용합니다.
- 저장되는 구성은 **설정 참조**뿐입니다. 레이아웃을 복원해도 장치 동작은 실행되지 않습니다.

### 7.2 운용 흐름

1. **connect**: 구성 선택 → `dbg.open` (R2; GEL 경고 문구 표시) → 진행 중 표시 → `dbg.session` + 연결 전후 run_state
2. **select target**: 코어 칩 클릭 → 모든 패널의 `channel` prop 변경
3. **observe**: watch 항목 추가 → `dbg.watch.start` → `dbg.sample` 스트림
4. **action**: Halt/Run/Step 버튼 또는 단축키 → RowStore 행이 requested→confirmed로 진행되고, 이어서 `dbg.run_state`
5. **result**: 값, 증거 ref, 기록

### 7.3 상태별 표시

| 상태 | 헤더 칩 | 데이터 패널 | 동작 버튼 |
|---|---|---|---|
| empty (구성 없음) | `구성 없음` | 안내만. 가짜 값 없음 | open만 |
| configured, not open | `미연결` | 이전 세션 이력 (회색, epoch 표시) | open |
| connecting (in flight) | `연결 중 · n s` | 이전 이력 | 비활성 (STOP은 셸) |
| running (fresh) | `RUNNING · age` | 레지스터 = 마지막 halt 기준 (회색). watch는 선언된 영역만 값, 나머지는 `would_halt` gap | Halt |
| halted (fresh) | `HALTED · age` | 레지스터, 메모리 현재값 | Run / Step |
| stepping | `STEP 중` | 값 동결 | 비활성 |
| stale (age > stale_after) | `RUNNING? · 3.2 s 전 (오래됨)` | 값에 stale 표시 | 동작은 가능하지만 결과는 새 관측으로 판정 |
| lost | `연결 끊김 · 마지막 running · 12 s 전` | 마지막 스냅샷과 이력 유지 | open (자동 재연결 없음) |
| error | `오류: <code>` | 유지 | open/close |
| STOP latched | 셸 STOP 배너 | 유지 | agent/script 쓰기 잠김. 사용자 입력은 허용 (`ctx.latch`) |
| outcome_unknown | 해당 행 `결과 불명` | quality=unknown | 재시도 버튼 **없음**. 대신 "상태 다시 관측" (R0) |

- 초기값은 `unknown`입니다. `OFF`, `halted`, `fresh`를 기본값으로 두지 않습니다.
- 재연결해도 이전 watch, BP, halt를 **자동으로 복원하지 않습니다.**

### 7.4 키보드, 포커스, 초안, 히스토리

- run/halt/step 단축키는 **dbg 툴바나 소스 패널에 포커스가 있을 때만** 동작합니다.
  - 입력 필드에 포커스가 있으면 키는 입력으로 갑니다.
  - 전역 단일 키 바인딩은 두지 않습니다 (CCS 키 배열을 복제한다고 주장하지 않음).
- 셸 prefix 키 `Ctrl-] x`(STOP), `Ctrl-] m`(마커)는 그대로 둡니다.
- 주소/심볼 입력은 초안으로 유지됩니다.
  - 검증 오류(AU 정렬, 범위 밖)는 즉시 표시합니다.
  - 히스토리는 ↑/↓로 탐색합니다 (탭 로컬, 장치 동작 없음).

### 7.5 viewer, 사용자, 에이전트

- **watch 권한(viewer)**: 모든 패널을 볼 수 있습니다. 읽기 요청(R0)은 `ctx.query`로 가능합니다. 동작 버튼은 `E_RIGHTS`로 막힙니다.
- **operate**: 동작과 watch를 시작할 수 있습니다.
- **에이전트**: CLI/RPC로 같은 명령을 씁니다. origin=agent로 RowStore에 AGT로 표시됩니다.
  - 사용자는 같은 저널에서 에이전트의 watch와 sample을 실시간으로 봅니다.
  - 에이전트가 화면을 따로 만들지 않습니다.

---

## 8. 실시간 watch/sample과 Luna 경로

### 8.1 worker가 획득과 판정을 맡음

- 호출 한 번이 다음을 모두 처리합니다.
  - 항목 해석 (심볼은 1회 캐시)
  - 주기마다 `read.batch` 1회 (항목 ≤32)
  - 조건 판정 (`eq/ne/lt/…/changed/bit_*`, `hold_ms`)
  - 기록
- `dbg.watch.start{items, predicate, every_ms, timeout_ms, record, from}`
- **모델은 값마다 호출하지 않습니다.**
- 요청 주기와 **측정 주기**(p50/p95/max)를 따로 보고합니다.
  - 20 ms 같은 값은 목표로 제안할 수 있을 뿐, **지원되는 사실이 아닙니다.**
- 실제 주기를 정하는 요소:
  - 왕복 시간 = SSH RTT + supervisor + executor + JTAG
  - `target.state` 폴링이 같은 직렬 큐를 사용함
- 결과(`WatchResult`)에 담기는 것:
  - outcome: `matched | timeout_no_match | stopped | lost | epoch_changed | refused`
  - match `sample_id`와 값
  - 항목별 마지막 값과 quality
  - polls, recorded, gaps
  - observed_window: **부재 주장은 이 구간에만 적용**
  - 증거 ref ≤8개

### 8.2 요청 경계와 컨텍스트

- 에이전트 작업이 디스패치되는 순간 `dispatch_cursor = hub seq high-water`와 `t_dispatch`를 **모델이 탐색을 시작하기 전에** 캡처합니다.
  - 그러면 `--from seq:N`으로 모델이 준비하는 동안 생긴 이벤트(run_state 변화, 사용자 watch의 sample)도 포함할 수 있습니다.
  - 단, **watch를 arm하기 전의 메모리 값은 관측된 적이 없습니다.** 사용자 watch가 이미 기록 중이었다면 그 레코드를 쓰고, 아니면 "t_arm 이전 미관측"으로 보고합니다.
- **미리 적재하는 컨텍스트 (dbg 전용, 작게)**
  - 대상 목록
  - `dbg status` 한 줄 (state, quality, epoch, age, caps: au_bits, nonintrusive)
  - 스킬 핵심 규칙 5–7줄
  - 명령 예시 3개
- **근거 (기존 측정, `work/reports/luna-skill-validation/index.html`, UART)**

  | 경로 | 측정값 |
  |---|---|
  | 파일 읽기 콜드 | 10.49 s (W1), 8.27 s (V1) |
  | 웜 | 3.44 s (W2) |
  | 컨텍스트 주입 콜드 | 3.89 s (X1, n=1, 하네스만) |
  | 대상 완료→보고 | 1.07–1.99 s |

  - 컨텍스트 주입은 **하네스에서만** 했고, 제품 디스패처는 없습니다.
  - n이 작아 SLO가 아닙니다. dbg 수치가 아니며, 다른 모듈은 Luna로 검증되지 않았습니다.

### 8.3 계측 지점 [제안]

| 지점 | 시계 | 정의 |
|---|---|---|
| T0 | mac wall/mono | 요청 디스패치. C0 = dispatch cursor |
| T1 | mac | 컨텍스트 조립 완료 |
| T2 | mac | 첫 도구 호출 |
| T3 | mac | gateway가 `dbg.watch.start`를 수락 (request_id) |
| T4 | mac mono | worker가 브리지 req를 보냄 (`mac_send_mono_ns`) |
| T5/T6/T7 | **bridge mono** | recv / SDK 시작 / SDK 끝 |
| T8 | mac mono | worker가 응답을 받음 |
| T9 | mac | 저널 emit (seq) |
| T10 | — | match sample = 그 배치의 T6–T7 (bridge) |
| T11 | mac | 결과를 에이전트에 반환 |
| T12 | mac | 최종 보고 |

**보고할 값 (모두 따로 기록하고, 전체 지연을 판정에서 빼지 않음)**
- 획득 지연: `T7−T6` (bridge), `T8−T4` (mac 왕복)
- 저널 지연: `T9−T8`
- LLM 지연: `(T2−T0) + (T12−T11)`
- E2E: `T12−T0`
- 이벤트→보고: `T12 − T8(match)`
- `T6−T4`처럼 두 호스트에 걸친 구간은 offset±err과 함께만 보고합니다.

---

## 9. 구현 티켓

### 9.1 의존 관계

```
T1 ──► T2 ──► T3
 │      ├───► T4 ◄── T4b
 │      └───► T5
 └──► T6 ──► T7 (T2 필요) ──► T8 ──► T9 ──► T10 (소스/BP/디스어셈블리)
                                           └─► T11 (멀티코어, reset/write/load)
```

### 9.2 티켓 목록

| # | 내용 | 소유 / 파일 | 완료 기준 (확인 가능해야 함) |
|---|---|---|---|
| **T1** | `loom-dbg` 모델 + backend trait + FakeBackend | 도메인 담당<br>`crates/loom-dbg/{Cargo.toml,src/lib.rs,model.rs,backend.rs,fake.rs}`, workspace members | `cargo test -p loom-dbg` 통과:<br>① u64 문자열 왕복, u64::MAX+1 거부<br>② AU 주소 산술 (16비트: 32비트 값 = 2 AU)<br>③ running + 미선언 영역 → WouldHalt, Fake 호출 횟수 0<br>④ epoch/state_rev 단조 증가<br>⑤ `contracts/examples` JSON 역직렬화 |
| **T2** | worker + gateway 등록 (Fake) | 도메인 담당<br>`worker.rs`, `gateway.rs`, `gateway/*`<br>`loom-gateway/Cargo.toml`, `domains.rs:178,236` | testkit 통합 테스트:<br>① STOP → Fake에 halt/run/reset/close 호출 0, 대기 쓰기 = failed(E_STOPPED), in-flight는 결과 유지<br>② watch matched/timeout/gap/lost/epoch_changed<br>③ `effect=unknown` → outcome_unknown, 재전송 0<br>④ 큐 상한 → E_WORKER_BUSY<br>⑤ Reopen::None: crash 후 자동 open 0 |
| **T3** | CLI `loom dbg status/open/close/read/watch/halt/run/step` + help | 도메인 담당<br>`cli.rs`, `loom-cli/Cargo.toml`, `src/lib.rs:55` | 키가 고정된 한 줄 JSON. 배열 상한. `--json` 스냅샷 테스트. exit code 매핑 표 테스트 |
| **T4b** | 범위 한정 Bench Light 토큰 | UI 담당<br>`shell.css` 토큰, workbench 루트 `data-theme`, dockview light 연결 | 기존 다크 테스트 그대로 통과. bench-light에서 dbg 패널 대비 검사 (vitest + Playwright 스크린샷 1장). 앱 전체 변경 없음 |
| **T4** | UI 패널 5개 (Fake fixtures) | UI 담당<br>`ui/src/panels/dbg/*`, `panels/index.ts` | vitest:<br>① 초기 `unknown`<br>② onControl run_state → 칩 변경, state_rev 역순 무시<br>③ run_epoch 증가 → 레지스터 current=false (값 유지)<br>④ lost에서 이력 유지<br>⑤ viewer는 E_RIGHTS<br>⑥ writeLocked면 버튼 비활성<br>⑦ sample_id 표시 = 저널 레코드<br>Playwright: desktop/mobile 스크린샷 |
| **T5** | 스킬 초안 + Luna 하네스 시나리오 (Fake) | 스킬 담당<br>`skills/loom-dbg/SKILL.md` | §10.1 시나리오에서 정답률. T0–T12를 기록하되 SLO를 주장하지 않음 |
| **T6** | bridge supervisor + stub executor | 브리지 담당<br>`crates/loom-dbg/bridge/*` | `node --test` (CCS 없이):<br>① ctl ping이 executor 블록(stub sleep) 중에도 ≤ 100 ms 안에 응답 (측정값 기록)<br>② stop → queued 취소, in-flight 유지<br>③ deadline+grace → executor kill, lost evt<br>④ 두 번째 클라이언트 BRIDGE_BUSY<br>⑤ 토큰 불일치 AUTH<br>⑥ 스키마 검증 실패 → BAD_ARGS<br>⑦ 0.0.0.0 bind 금지 |
| **T7** | `CcsRemote` backend | 도메인 담당<br>`ccs_remote.rs` | T6 stub과 Rust 통합 테스트: 시계 오프셋 ± err, seq 건너뜀 → gap, 재연결 시 새 epoch |
| **T8** | 브리지 호스트 bring-up (사용자가 CCS와 드라이버 설치) | 사용자 + 브리지 담당<br>`executor.mjs` 실제 호출 | 프로브 없이:<br>`versionInfo` 기록, configure 오류 경로, setScriptingTimeout 동작, `getDetailedSettings` 덤프 (가능한 범위)<br>"HW 미검증"으로 표기 |
| **T9** | 하드웨어 수용 | 사용자 보드 | §10.2 행렬. 통과 후에만 "검증됨"으로 표기 |
| **T10** | 소스, BP, 디스어셈블리 | 도메인 + UI | `callstack.fetch`, `symbols.load`, 브리지 BP 테이블, `symModule.disassemble` (processorId 확인 후), DWARF line 파싱 |
| **T11** | 멀티코어, reset, write, load | — | 사용자 정책 하에서 write_state 기록 |

---

## 10. 테스트 계획

### 10.1 FakeBackend 시나리오 (T2/T4/T5)

FakeBackend는 **계약, 수명주기, UI, Luna**만 검증합니다. 드라이버나 프로브 호환성은 검증하지 않습니다. TI 공식 시뮬레이터는 가정하지 않습니다 [S29].

| 시나리오 | 주입 | 기대 (UI와 에이전트가 **같은 sample_id, 값, 시각**) |
|---|---|---|
| 정상 match | g_state 0→3 (t=2 s) | matched, sample_id 같음, `T12−T8` 기록 |
| stale | run_state 폴링 정지 | quality stale. 에이전트가 "현재 running"이라고 단정하지 않음 |
| 잘못된 코어 | channel=bh1/cpu2 미오픈 | E_NOT_OPEN. 다른 코어의 값을 쓰지 않음 |
| epoch 불일치 | 요청 중 재open | E_CONNECTION_CHANGED, outcome=epoch_changed |
| 끊김 | 링크 drop | lost, last-known 유지, 자동 재연결 0 |
| 타임아웃 | halt가 응답 없음 | outcome_unknown, 재시도 0, 자동 halt 0 |
| STOP + in-flight | run 진행 중 STOP | 큐 취소, run 결과 반영, 이후 agent 쓰기 E_STOPPED |
| 정밀도 | u64 0xFFFF_FFFF_FFFF_FFFF, i16 −1 | 문자열 왕복 정확, dec = "−1" |
| AU | C28x 32비트 배열 | addr+2 간격 |
| false positive | 조건 경계값 (`gt 3`에 3) | not matched |
| 유한 부재 | 10 s 동안 조건 없음 | timeout_no_match + observed_window. "절대 없음"이라고 말하지 않음 |
| would_halt | running + 미선언 영역 | refused/gap, Fake halt 호출 0 |

### 10.2 하드웨어 수용 행렬 (T9)

| 축 | 값 (사용자 입력) |
|---|---|
| Blackhawk 변형 | BH-XDS-560v2-BP/PoE (USB/LAN) / BH-USB-560v2 |
| 펌웨어 / 드라이버 | Bh560v2Config에 보이는 값 [S15], CCS 동봉 드라이버 버전 |
| CCS / OS | 21.0.1 / Win11 x64 또는 Ubuntu 24.04 x64 |
| TI 칩 / 코어 | 예: F2837x C28x CPU1 (미정) |
| 연결 전후 | connect 전후 isHalted, 설정 덤프, GEL 경로 |
| 읽기 침습성 | halted read 정확도 (알려진 상수), running read에서 PC나 타이머 진행이 멈추는지, `isHalted` 폴링 영향 |
| 값 | 다중 AU endian, bitSize 8 on C28x |
| 실패 | 케이블 분리 → lost, 타임아웃 후 상태, STOP 중 in-flight |
| 지연 | 획득 p50/p95 (n ≥ 100), E2E (n은 별도) |

### 10.3 이 보고서에서 실행한 검증

다시 실행: `python3 -I work/reports/ti-debugger-implementation/contracts/validate.py`
- 보고서 전용 검증 도구입니다. 제품 테스트가 아닙니다.
- 필요: `jsonschema` ≥ 4.18 (여기서는 4.26.0)

검증 내용:
- 스키마 2개의 Draft 2020-12 메타스키마 검사
- `bridge-session.jsonl`, `bridge-deadline.jsonl`: 모든 줄이 스키마에 유효하고 u64 범위 안에 있음
- `bridge-negative.jsonl`: 7건이 모두 예상대로 거부됨
  - 6건은 스키마가 거부
  - n7은 문법상 유효하지만 런타임 u64 범위 검사가 거부
- `dbg-journal.json`의 run_state/sample/mem_block payload와 WatchResult가 DTO 스키마에 유효

결과: `TOTAL_FAILS 0`

---

## 11. 브리지 참조 흐름 (미실행)

API 이름은 CCS 21.0.1 JS 레퍼런스 [A2–A19] 기준입니다. **어떤 호스트에서도 실행하지 않았습니다.** 인자 형식은 `contracts/examples/bridge-session.jsonl`과 같습니다.

```js
// executor.mjs — UNEXECUTED. One op at a time; stdout JSON-lines only.
// POLICY = bridge-host file (e.g. loom-ccs-bridge.policy.json, 0600), loaded at start.
//   It lists per core_alias the regions whose running read was accepted in T9.
//   Default: empty → every running read is WOULD_HALT. No request field can widen it.
const ds = initScripting({ suppressMessages: true });          // [A2]; import form confirmed in T8
const sessions = new Map();                                     // core_alias → DebugSession
const big = (v) => BigInt(v);                  // SDK returns bigint | number | string [A7][A9][A10]; BigInt("0x1F")/("31") ok
const hex = (v) => '0x' + big(v).toString(16).toUpperCase();
const loc = (a, page) => page ? `${hex(a)}@${page}` : big(a);  // DSLocation 'num@page' [A18]; hex-in-string form → T8 check
const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function regionAllowsRunningRead(coreAlias, addr, countAu) {     // helper gate (pseudocode)
  const regions = POLICY.cores[coreAlias]?.nonintrusive_regions ?? [];
  return regions.some((r) => big(r.start) <= addr && addr + big(countAu) - 1n <= big(r.end));
}

function exec(req) {
  ds.setScriptingTimeout(req.deadline_ms);                      // global for following calls [A4]
  const s = req.core ? sessions.get(req.core) : null;
  switch (req.op) {
    case 'session.configure': return { ver: ds.versionInfo(), cores: ds.configure(req.args.ccxml).cores };
    case 'session.open': {                                      // args.core_name = exact string from session.cores
      const ses = ds.openSession(new RegExp('^' + escapeRe(req.args.core_name) + '$'));   // [A4] RegExp|string
      if (ses.getName() !== req.args.core_name) { ses.close(); return { reject: 'NO_SUCH_CORE' }; }
      sessions.set(req.core, ses);
      return { core_name: ses.getName(), settings: ses.settings.getDetailedSettings() };
    }
    case 'target.connect': {
      const before = s.target.isConnected();
      s.target.connect();                                       // runs OnTargetConnect GEL [A21]
      return { before, halted_after: s.target.isHalted() };
    }
    case 'target.state': return { halted: s.target.isHalted() };
    case 'target.halt':  s.target.halt();      return { halted_after: s.target.isHalted() };
    case 'target.run':   s.target.run(false);  return { halted_after: s.target.isHalted() };  // never run()
    case 'read.batch': {                                        // args.require = "halted_or_declared_nonintrusive"
      const halted = s.target.isHalted();
      const items = req.args.items.map((it) => {
        const a = it.symbol ? s.symbols.getAddress(it.symbol) : big(it.addr);       // [A10] may be null
        return { it, a: a == null ? null : big(a), au: Math.max(1, it.bits / AU_BITS[req.core]) };
      });
      if (!halted && !items.every((x) => x.a != null && regionAllowsRunningRead(req.core, x.a, x.au)))
        return { reject: 'WOULD_HALT' };                        // no memory call made
      return { run_state_at_read: halted ? 'halted' : 'running',
               values: items.map(({ it, a }) => a == null ? { key: it.key, err: 'NO_SUCH_SYMBOL' }
                 : { key: it.key, hex: hex(s.memory.read(loc(a, it.page), 1, it.bits)[0]), bits: it.bits, signed: it.signed }) };
    }                                                           // count=values; values unsigned [A7]
  }
}
```

**이 코드에서 아직 확인되지 않은 것** (T8에서 확정)
- `initScripting`을 import하는 정확한 구문 (run.sh 환경 vs 외부 require [A1–A3])
- `'0x…@PAGE'` 문자열 형식 (hex인지 decimal인지)
- `openSession`에 앵커 정규식을 넘기는 방식과 실제 코어 이름에 들어 있는 특수문자
- `AU_BITS`: 코어 설정에서 가져옴. C28x는 16 [A25][A26], 그 외 코어는 T9
- running 상태의 `isHalted()`와 읽기 사이에는 경합이 있음 (BP hit로 halt되는 방향은 읽기에 안전)
- worker도 같은 판정을 미리 해서 브리지 정책과 **이중 게이트**를 이룸. 클라이언트 플래그 하나로는 비침습을 주장할 수 없음

---

## 12. 지금 할 수 있는 것과 할 수 없는 것

| 지금 구현 가능 (환경 불필요) | CCS 호스트 필요 (T8) | 하드웨어 필요 (T9) |
|---|---|---|
| T1–T7, T4b<br>스키마, Fake, UI, CLI, 스킬, supervisor, 프로토콜 테스트 | import 구문, versionInfo, 설정 덤프 형식, 타임아웃 이후 executor 상태, processorId 후보 | 코어 이름, 연결 부작용, 실행 중 읽기, endian, 레지스터 이름, 획득 지연, Blackhawk + CCS 21 호환성 |

## 13. 사용자 입력 (보고서를 막지 않음. 매개변수로 처리)

1. 정확한 Blackhawk 모델 라벨 (USB 전용인지, USB+LAN인지)
2. TI 칩/보드 P/N과 코어
3. 브리지 호스트 OS와 위치
4. 기존 `.ccxml`, `.out` 파일과 시작 GEL 내용
5. 실행 중 읽기를 허용할 영역 (T9 이후)
6. halt/run/step/BP에 대한 사용자 정책 (기본은 F1 Auto)

---

## 출처

**API (접근일 2026-11-14, CCS 21.0.1)**
- 전체 목록: [`API-EVIDENCE.json`](API-EVIDENCE.json) `sources`
- 핵심:
  - [A1] CCS 21.0.1 Scripting 핸드북 — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-scripting.html
  - [A2] JS API Reference — https://software-dl.ti.com/ccs/esd/documents/users_guide/scripting/javascript/api.html
  - [A3] Python API Reference — https://software-dl.ti.com/ccs/esd/documents/users_guide/scripting/python/api.html
  - [A4] DebuggerScripting — …/scripting/javascript/DebuggerObjects/DebuggerRootObject.html
  - [A6] Target — …/TaskObjects/Target.html
  - [A7] Memory — …/TaskObjects/Memory/MemoryRootObject.html
  - [A9] Registers — …/TaskObjects/Registers.html
  - [A10] Symbols — …/TaskObjects/Symbols.html
  - [A11] SymbolModule — …/SymbolModuleObjects/SymbolRootObject.html · 버전 고정 URL(ROOT 확인): https://software-dl.ti.com/ccs/esd/documents/users_guide_ccs_21.0.1/scripting/javascript/SymbolModuleObjects/SymbolRootObject.html
  - [A12] Breakpoints — …/TaskObjects/Breakpoints.html
  - [A13] Callstack — …/TaskObjects/Callstack.html
  - [A14] Expressions — …/TaskObjects/Expressions.html
  - [A15] Settings — …/TaskObjects/Settings.html
  - [A18] Types — …/common.html
  - [A19] Exceptions — …/exceptions.html
  - [A20] CCS Debug guide — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-main.html
  - [A21] GEL OnTargetConnect — https://software-dl.ti.com/ccs/esd/documents/users_guide/gel/OnTargetConnect.html
  - [A25] C28x byte accesses — https://software-dl.ti.com/ccs/esd/documents/c2000_byte-accesses-with-the-c28x-cpu.html
  - [A26] SPRU514I C28x C/C++ — http://downloads.ti.com/docs/esd/SPRU514I/Content/SPRU514I_HTML/tms320c28x_c_c_language_implementation.html
  - 접두사 `…` = `https://software-dl.ti.com/ccs/esd/documents/users_guide/scripting/javascript/`

**제품, 호스트, 드라이버 (이전 조사 재사용)**
- [S1] [S2] [S3] [S8] [S9] [S11] [S13] [S14] [S15] [S18] [S21] [S23] [S26] [S29] — [`../ti-debugger-survey/SURVEY.md`](../ti-debugger-survey/SURVEY.md) 출처 목록

**저장소**
- 기준 `ec38f95`
- 본문에 파일:줄로 표기
- 로컬 호스트: `uname -sm` = `Darwin arm64`

**Luna 지연**
- `work/reports/luna-skill-validation/index.html` (UART, n=1 케이스. SLO 아님)
