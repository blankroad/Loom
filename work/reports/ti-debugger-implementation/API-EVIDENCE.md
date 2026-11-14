# CCS Scripting API 근거 — Loom 디버거 UI용 (XDS560v2)

- 작성: claude-opus-5-5 (native), session `dd8a5132-4435-4543-89f6-2779937a8db6`, workspace w2G, agent `dbg-api-opus`
- 접근일 **2026-11-14**. 기계 판독용: `API-EVIDENCE.json` (`ready: true`, 출처 ID A1–A27)
- **표기**: ✅ 문서화됨(공식 21.0.1 레퍼런스) · ⚠️ 문서화됐지만 제약 또는 SDK 확인 필요 · ❌ 공식 문서에서 **찾지 못함**(메서드 이름을 지어내지 않음)
- **한계**: 모두 문서 근거입니다. CCS, SDK, 프로브, 타깃 어느 것도 실행하지 않았습니다. "문서화됨"이 XDS560v2·Blackhawk 드라이버·특정 칩에서 동작함을 뜻하지는 않습니다.

## 0. 버전 판정
- **CCS 21.0.1 현행 레퍼런스가 존재합니다.** JS [A2]와 Python [A3] 레퍼런스가 `users_guide/scripting/{javascript,python}/` 아래에 있고, 객체별 페이지 32개를 모두 HTTP 200으로 받았습니다. JS 페이지의 Last-Modified는 2026-08-26입니다. 이전 survey에서 "21 레퍼런스 404"로 기록한 것은 예전 TypeDoc 경로였습니다. **이 문서는 20.1/20.2 레퍼런스 대신 21.0.1 레퍼런스를 기준으로 합니다.** 같은 문서는 설치본의 `<CCS>/ccs/scripting/docs`에도 들어 있습니다 [A1].
- 실행 중인 디버거 버전은 `ds.versionInfo()`로 확인합니다(`{version, major, minor, revision, build}`) [A4]. 브리지는 시작할 때 이 값을 증거로 남겨야 합니다.
- Python도 메서드 이름은 같습니다(camelCase). 다만 객체 대신 dict를, BigInt 대신 `int`를 돌려주고, 반환 타입이 `int | str`로 표기된 곳이 많습니다 [A1][A3].
- 동기 API입니다(CCS Theia 1.3부터) [A1]. 레거시 DSS는 별개이며 호환되지 않습니다(이전 survey S19/S20).

## 1. 패널별 API 매트릭스 (JS 시그니처)

| 패널 | 기능 | API | 판정 | 핵심 의미·제약 |
|---|---|---|---|---|
| 연결 | 구성·코어 목록 | `ds.configure(ccxml): CoreList` · `ds.listCores()` · `ds.deconfigure()` | ✅ [A4] | configure만으로는 코어에 **연결하지 않음**. 이미 구성된 상태면 예외 → 먼저 deconfigure 필요(deconfigure는 모든 코어 disconnect와 세션 무효화). 560v2 코어 이름 문자열은 실제 ccxml이 있어야 알 수 있음 |
| 연결 | 코어 세션 | `ds.openSession(pattern=""): DebugSession` · `session.getName()` · `getPartNum()` · `close()` | ✅ [A4][A5] | 정규식에 처음 일치하는 코어를 엶. `close()`는 disconnect가 **아님** |
| 연결 | 연결 | `target.connect()` · `disconnect()` · `isConnected()` | ⚠️ [A6] | connect할 때 ccxml의 startup GEL에 정의된 `OnPreTargetConnect`/`OnTargetConnect`가 동기로 실행됨 [A21–A24]. GEL이 GEL_Reset 같은 상태 변경을 하면 **타깃이 halt되어 비침습 연결이 깨짐**. 비침습 연결은 real-time mode 전제로만 설명됨 [A21] |
| 연결 | 설정 | `settings.getDetailedSettings()` · `get(id)` · `set(id,v)` | ⚠️ [A15][A1] | 문서에 나온 ID는 `AutoRunToLabelOnReset`, `AutoRunToLabelName`, `AutoRunToLabelOnRestart`뿐임. realtime·polite·"Halt the target before any debugger access"[A20]의 **ID는 문서에 없음** → 실제 호스트에서 getDetailedSettings로 덤프해 확인 |
| 연결 | 2nd 인스턴스 | `initScripting({attach})` · `ds.getInstanceHandle()` | ⚠️ [A2][A4] | 문서상 두 인스턴스가 "동시에" 상호작용할 수 있음(`"auto"`는 실험 기능). 다만 **한쪽이 블록된 동안 다른 쪽이 halt/isHalted를 처리할 수 있는지는 문서에 없음** → 제어 평면으로 쓰려면 SDK 확인 필요 |
| 실행 | run/halt/상태 | `target.run(waitForHalt=true)` · `halt()` · `waitForHalt()` · `isHalted()` | ✅ [A6] | `run()`은 기본적으로 halt될 때까지 블록 → 반드시 `run(false)`. **run-state 이벤트 스트림 없음**: isHalted 폴링 또는 GEL OnHalt 콜백뿐 |
| 실행 | step | `stepInto(asm?)` · `stepOver(asm?)` · `stepOut()` | ✅ [A6] | 다시 halt될 때까지 블록. 현재 PC에 심볼이 있으면 source step, 없으면 asm step |
| 실행 | reset/restart | `getResets(): Record<string,bool>` · `reset(type?)` · `restart()` | ✅ [A6] | reset 이름은 타깃마다 다름. restart는 OnRestart GEL을 호출하고 설정에 따라 main까지 auto-run함 |
| 실행 | 멀티코어 | `ds.simultaneous.run/halt/asmStepInto(targets[])` · `globalBreakpoint.enable/disable()` | ✅ [A16][A17] | later |
| 레지스터 | 목록 | `registers.getRegisterGroups(groupId?)` → `{registers: RegisterInfo[], groups}` · `getBitfields(id)` | ✅ [A9][A18] | `RegisterInfo{id,name,location?,readable,writable}`. `location`이 있으면 메모리 매핑 레지스터(MMIO)이므로 read 부작용 등급을 따로 둘 것. 문서 불일치: bitfield 필드가 Types 페이지에서는 `stop`, 예제에서는 `end` |
| 레지스터 | read/write | `registers.read(name): bigint\|number\|string` · `write(name, v)` · `exportAsJson/importFromJson` | ✅ [A9] | 예제의 `PC`, `R0`은 Arm계. **C28x·C6000·Cortex-R의 PC/SP 이름은 이 레퍼런스로 확인되지 않음** → 하드코딩하지 말고 런타임 목록에서 찾을 것 |
| 메모리 | read | `memory.read(loc: DSLocation, count, bitSize?)` · `readOne(loc, bitSize?)` | ⚠️ [A7][A18] | `count` = **값 개수**(바이트 수 아님). bitSize 기본값 = 타깃 word 크기, 허용값 8/16/24/32/40/48/64. 결과는 **unsigned**. endian 처리 방식은 문서에 없음. 실행 중 접근 동작은 API 문서에 없음 → 디바이스·모드 의존 [A20] |
| 메모리 | write 등 | `write(loc, values[], bitSize?)` · `fill` · `saveData/loadData/saveBinary/loadBinary` | ✅ [A7] | bitSize로 잘림, 음수는 signed로 씀. 상태를 바꾸는 action |
| 메모리 | 페이지·맵 | `memory.getPages()` · `memMap.get/add/remove/enable/reset` | ✅ [A7][A8] | 위치는 `"0x2400@DATA"`, `@pa`, `@ipa` 형식. memMap은 디버거 접근 허가 맵이며 read-sensitive 분류가 **아님** |
| 심볼 | 심볼만 로드 | `symbols.load(path)`(교체) · `add` · `loadWithOffset/addWithOffset` · `remove` · `unloadAllSymbols` | ✅ [A10] | 문서가 "symbol information만 로드"한다고 명시 → 이미 플래시된 타깃에 적합. 반대로 `memory.loadProgram`은 타깃에 쓰고 심볼도 로드함(ELF/COFF/hex) |
| 심볼 | 이름↔주소 | `symbols.getAddress(name)` · `lookupSymbols(start, length=0): string[]` | ✅ [A10] | 역조회는 주소 범위 기준 |
| 심볼 | 전체 목록 | — | ❌ | 전체 심볼·소스 파일을 나열하는 API 없음 |
| 소스 | 주소→file:line | `ds.openSymbolModule(processorId, isBigEndian=false)` → `lookupSourceLine(addr)`, `lookupFunction(addr)` | ⚠️ [A11][A4] | 타깃 없이 오프라인으로 동작. **`processorId`는 문서에서 'magic number'라고만 하고 값은 없음** → SDK 확인 필요. 세션의 `symbols`에는 이 기능이 없음 |
| 소스 | 현재 PC→소스 | `callstack.fetch(maxLevels?)` → `frames[{functionName?, sourceLine?{path,line,column}, pc: string, locals[]}]` | ✅ [A13] | halt 상태의 live 세션에서 PC→file:line을 얻는 가장 확실한 문서화 경로 |
| 소스 | file:line→주소 | — | ❌ | 직접 API 없음. `breakpoints.getProperty`는 property-set id 기준으로 문서화되어 있어 breakpoint id로 쓸 수 있다고 보장되지 않음. 제안: Loom 쪽에서 DWARF line table을 파싱 |
| 소스 | 경로 매핑 | — | ❌ | IDE의 Source Lookup은 GUI 기능 [A20]. BP 파일 인자는 **컴파일 시점의 전체 경로 또는 파일명만**(처음 일치하는 파일)을 받음 [A12] → 매핑 테이블은 Loom/브리지가 소유 |
| 디스어셈 | 범위 | `symModule.disassemble(start, end)` → `{disassembly[{address, numOpCodes, text}]}` | ⚠️ [A11] | **로드한 오브젝트 파일 내용 기준**(live 타깃 메모리 아님), 끝 주소 포함. 플래시 내용이 다르면 결과가 어긋남. processorId 필요. 세션에서 live 메모리를 디스어셈하는 API는 ❌ |
| BP | add/remove | `breakpoints.add(addr\|expr\|file, line?): number` · `remove(id)` · `removeAll()` | ✅ [A12] | 위치는 주소, `"main + 0x8"` 같은 식, file:line 모두 가능 |
| BP | 조건/HW/skip | `createProperties(0 sw\|1 hw)` → `setProperty` / `setPropertySourceLocation` / `getDetailedProperties` → `addWithProperties` | ⚠️ [A12][A1][A20] | 문서에 나온 property ID: `Hardware Configuration.Location`, `Debugger Response.Skip Count`, `Debugger Response.Action.Expression`. Condition은 GEL 식으로 존재하지만 **정확한 ID는 없음** → getDetailedProperties로 확인. property는 add 전에만 설정 가능 |
| BP | enable/disable·목록·hit | — | ❌ | 브리지가 BP 테이블을 소유해야 함. disable은 remove 후 다시 add(id가 바뀜). hit 판정은 halt 후 PC와 BP 주소를 비교해 **유도**함(API 아님) |
| Watch | 식 평가 | `expressions.evaluate(expr): void\|bigint\|number\|string` | ⚠️ [A14][A1] | **GEL 평가**이므로 GEL 함수와 등록된 JS 콜백까지 호출 가능 → **R0 read-only가 아님**. read-only watch는 `getAddress` + 타입을 아는 `memory.read`로 구현 |
| Watch | GEL 콜백 | `session.registerCallback(name, fn)` · `registerMenuCallback` | ⚠️ [A5] | 이름이 OnHalt 등이면 디버거가 호출함. 같은 이름의 GEL 함수가 이미 있으면 예외(디바이스 GEL이 미리 정의한 경우가 많음) |
| 타임아웃 | — | `initScripting({timeout})` · `ds.setScriptingTimeout(ms)`(≤0이면 끔) · `getScriptingTimeout()` · `ScriptingTimeoutError` | ⚠️ [A2][A4][A19] | **이후 모든 호출에 전역 적용**(호출별 지정 아님). 기본값은 타임아웃 없음. 타임아웃 뒤 하위 동작과 타깃 상태는 문서에 없음. 공식 예제는 타임아웃 뒤 halt를 호출하지만 **Loom은 자동 halt 금지**. 취소 API 없음 |
| 로드 | — | `memory.loadProgram(path, args?)` · `loadBinaryProgram` · `verifyProgram`(불일치 시 throw) · `verifyBinaryProgram` | ✅ [A7] | MVP 제외 |

## 2. 주소 단위와 값 표현 (C28x 주의)
- **값 타입**: `Address = bigint | string | number`. JS의 number는 53비트까지만 정확하므로 TI는 BigInt나 문자열을 권장합니다 [A18][A1]. JS 정수 결과는 BigInt로 옵니다. BigInt는 `JSON.stringify`로 직렬화할 수 없으므로 **브리지는 `"0x…"` 문자열로 변환**해야 합니다.
- **C28x는 16비트 주소 단위입니다.** 주소 하나가 16비트를 가리키고, char/byte도 16비트이며, `sizeof(int)==1`입니다 [A25][A26](SPRU514는 2015년 v15.9 문서. EABI별 타입 크기는 SPRAC71 [A27]를 따로 확인해야 함). 따라서 32비트 변수 하나가 주소 2칸을 차지하고, **다음 요소 주소 = 현재 주소 + bitSize/16**입니다. 바이트 단위로 주소를 늘리면 안 됩니다. C28x에서 `bitSize=8`로 read했을 때의 동작은 **문서에 없으므로** 쓰지 않거나 SDK로 확인해야 합니다.
- Cortex-R/M/A, C6000, C29x의 주소 단위는 이번 조사에서 확인하지 않았습니다 → target capability로 `address_unit_bits`(설정값과 HW 수용 시험으로 확정)를 두고, 값 DTO에 `bit_size`, `signed`, `endian_source`를 함께 싣습니다(**제안**).

## 3. 실행 중 접근과 부작용 (문서 근거)
- Debug guide [A20]:
  - 공식 real-time mode를 지원하는 것은 **C64x, C674x, C66x, F28x 전부**(ICEMaker)입니다.
  - Cortex-M/R은 full real-time mode를 지원하지 않지만, **DAP를 통한 비침습 메모리 접근**은 지원하며 이 접근은 항상 켜져 있습니다.
  - real-time mode를 명시적으로 켜지 않으면 메모리 접근은 사실상 **rude** 모드로 동작합니다(DBGM 무시).
  - "Halt the target before any debugger access"는 halt→access→resume 순서로 동작하며 기본값은 off입니다.
  - 레지스터 Continuous Refresh는 full Real-time Mode를 지원하는 디바이스에서만 동작합니다.
- **MMIO read-to-clear 같은 부작용 분류 API는 없습니다**(❌). memMap은 접근 허가만 다룹니다. → "안전한 read"를 일괄 약속하지 말고, 레지스터 중 `location`이 있는 MMIO 영역과 사용자가 지정한 영역을 별도 등급으로 둡니다.

## 4. 동기 호출과 취소: 구현에 미치는 영향
- JS 호출은 Node 이벤트 루프를 블록합니다(`sleep`도 마찬가지) [A1][A2]. **같은 프로세스 안의 readline 루프는 호출이 진행되는 동안 STOP이나 heartbeat에 응답할 수 없습니다.**
- 문서로 확인된 탈출 수단은 두 가지뿐입니다.
  1. 전역 `setScriptingTimeout`에 의한 `ScriptingTimeoutError`
  2. 프로세스 종료
- 제안: 제어 평면(STOP·heartbeat·큐 취소)을 별도 프로세스나 스레드에 두고, 진행 중인 호출은 `in_flight/uncancellable`로 표시합니다. 데드라인을 넘기면 프로세스 그룹을 종료하고 상태를 `unknown`으로 기록합니다(자동 halt/reset 없음).
- `attach` 핸들을 이용한 2차 인스턴스는 후보이지만 **동시 처리 가능 여부를 SDK로 확인**해야 합니다.

## 5. 런타임 의존성 [A1][A2][A3]
- 실행 방법:
  - `<CCS>/ccs/scripting/run.sh|run.bat <script.js>`
  - Python: `<CCS>/ccs/scripting/python/run.sh` (동봉 Python 3.14)
  - 외부 Node: `require(ccsPath + '/ccs/scripting/node_modules/scripting')`
  - 외부 Python ≥3.10: `pip install -e <CCS>/ccs/scripting/python/site-packages/scripting`. `-e` 없이 설치하면 `ccsRoot`를 지정해야 하고 CCS를 업데이트할 때마다 다시 설치해야 함
- **모든 경로가 CCS 설치본을 가리킵니다.** 모듈을 외부에서 import하더라도 CCS 설치(Debug Server, device support)와 Blackhawk 드라이버(이전 survey S1/S2/S26)가 필요합니다. 최소 standalone 번들은 **문서화되지 않았고 검증하지도 않았습니다**. IDE 없이 헤드리스로 실행할 수 있습니다(`interactive.mjs`). `launchCCS()`로 IDE를 옆에 띄우는 기능은 선택 사항입니다.
- XDS560v2와의 관계: API 자체는 프로브에 무관하고, 프로브는 ccxml로 선택합니다. macOS에서 560v2를 지원하지 않는다는 점과 Blackhawk 드라이버 번들 현황은 이전 survey의 결론을 그대로 따릅니다.

## 6. 최소 브리지 흐름 — **미실행 의사코드, SDK 없이 작성**
```js
// UNEXECUTED sketch. API names from CCS 21.0.1 JS reference [A2–A19]; not run on any host.
const ds = initScripting({ suppressMessages: true, timeout: 5000 });
const ver = ds.versionInfo();                       // evidence
const { cores } = ds.configure(CCXML);              // no core connect yet
const s = ds.openSession(CORE_RE);
const settings = s.settings.getDetailedSettings();  // record realtime/auto-run ids+values BEFORE connect
s.target.connect();                                 // runs OnTargetConnect GEL (may halt!)
const halted = s.target.isHalted();
s.symbols.load(ELF);                                // symbols only, no target write
const addr = s.symbols.getAddress("g_state");       // bigint|null
const [v] = s.memory.read(addr, 1, 32);             // count=values; C28x: 32-bit = 2 address units
emit({ addr: "0x" + addr.toString(16), value: "0x" + v.toString(16), bit_size: 32, halted });
if (halted) { const { frames } = s.callstack.fetch(1); /* frames[0].sourceLine, pc */ }
```

## 7. 문서 불일치
- `launchCCS`의 기본값이 API 레퍼런스에서는 `"waitUntilLaunch"`, 핸드북에서는 `"detached"`로 서로 다릅니다 [A4][A1].
- 핸드북은 `initScripting({inProcess:true})`를 예로 들지만, API 옵션 표에는 `inProcess`가 없습니다 [A1][A2].
- RegisterBitfield 필드 이름이 `stop`(Types 페이지)과 `end`(예제)로 다릅니다 [A9][A18].

## 8. 공식 문서에서 찾지 못한 것 / SDK·HW로 확인할 것
- **찾지 못함 ❌**:
  - 전체 심볼·소스 파일 목록
  - file:line→주소 직접 변환
  - 경로 매핑 API
  - live 디스어셈블리
  - BP enable/disable·목록·hit 상태와 횟수
  - run-state 이벤트 스트림
  - 호출별 타임아웃과 취소
  - realtime 관련 설정 ID
  - `processorId` 값
  - MMIO read 부작용 분류
- **확인할 것**:
  - 560v2 코어 이름 문자열(`listCores`)
  - getDetailedSettings 덤프
  - 디바이스·모드별 실행 중 `memory.read` 동작
  - C28x의 기본 word 크기, `bitSize=8` 동작, 다중 word 값의 endian
  - attach 인스턴스의 동시성
  - ScriptingTimeoutError 이후 상태
  - `processorId`
  - BP Condition property ID
  - 선택한 코어의 PC/SP 이름

## 출처 (접근일 2026-11-14, 모두 TI 공식. A27은 검색 결과만 확인)
- A1 https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-scripting.html
- A2 https://software-dl.ti.com/ccs/esd/documents/users_guide/scripting/javascript/api.html
- A3 https://software-dl.ti.com/ccs/esd/documents/users_guide/scripting/python/api.html
- A4 …/scripting/javascript/DebuggerObjects/DebuggerRootObject.html · A5 …/TaskObjects/TaskRootObject.html · A6 …/TaskObjects/Target.html · A7 …/TaskObjects/Memory/MemoryRootObject.html · A8 …/TaskObjects/Memory/MemMap.html · A9 …/TaskObjects/Registers.html · A10 …/TaskObjects/Symbols.html · A11 …/SymbolModuleObjects/SymbolRootObject.html · A12 …/TaskObjects/Breakpoints.html · A13 …/TaskObjects/Callstack.html · A14 …/TaskObjects/Expressions.html · A15 …/TaskObjects/Settings.html · A16 …/DebuggerObjects/Simultaneous.html · A17 …/TaskObjects/GlobalBreakpoint.html · A18 …/common.html · A19 …/exceptions.html (접두사 `https://software-dl.ti.com/ccs/esd/documents/users_guide/scripting/javascript/`, 전체 URL은 JSON에 있음. Python판은 `…/scripting/python/` 아래 같은 경로)
- A20 https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-main.html
- A21 …/users_guide/gel/OnTargetConnect.html · A22 …/gel/OnPreTargetConnect.html · A23 …/gel/files.html · A24 …/gel/targetconfig.html
- A25 https://software-dl.ti.com/ccs/esd/documents/c2000_byte-accesses-with-the-c28x-cpu.html
- A26 http://downloads.ti.com/docs/esd/SPRU514I/Content/SPRU514I_HTML/tms320c28x_c_c_language_implementation.html
- A27 https://www.ti.com/lit/pdf/sprac71 (미열람)
- 재사용: `work/reports/ti-debugger-survey/SURVEY.md` S1–S29 (다시 가져오지 않음)
