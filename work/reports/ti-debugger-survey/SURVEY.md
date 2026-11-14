# Blackhawk XDS560v2 × TI 디버거 — Loom 연동 타당성 조사

- 작성: claude-opus-5-5 (native), session `b69cea51-12e2-4404-b913-e21c602b935f`, workspace w2B
- 기준: Loom HEAD `d6fc6f3`, 웹 출처 접근일 **2026-11-14**
- 범위 (사용자 정정 반영): **Blackhawk XDS560v2 계열만**. XDS200/XDS110 등은 구분에 필요한 만큼만 언급. 구현·장치 스캔·설치·하드웨어 검증은 **하지 않음**.
- 표기: **[V]** = Blackhawk(벤더) 고유 문서 근거, **[T]** = TI 일반 XDS560v2/CCS 문서 근거(Blackhawk 드라이버 증거로 자동 간주하지 않음), **[I]** = 추론(문서화된 지원 아님), **[F]** = TI E2E 포럼 등 2차 출처.

---

## 0. 결론 (먼저)

| 조합 | 판정 | 핵심 근거 |
|---|---|---|
| **macOS 호스트(Intel/Apple silicon) + CCS 20.x/21.x + Blackhawk XDS560v2 (USB 또는 LAN)** | ❌ **미지원** | [T] CCS 20.4.0/20.4.1 릴리스 노트: macOS 버전은 XDS560v2를 지원하지 않는다고 명시 [S1][S2] · [T] CCS v12 macOS 페이지는 Blackhawk XDS560v2(LAN/USB)를 **미지원 목록**에 둠 [S8] · [V] Blackhawk 제품 페이지 호스트 OS 목록에 macOS 없음 (Windows/Linux만) [S13] · CCS 21.0.x 릴리스 노트는 이 항목에 대해 언급 없음 → 반대 근거가 없으므로 미지원으로 간주 [S3][S4] |
| **Windows 10/11 x64 또는 Ubuntu 22.04/24.04(/26.04) x64 + CCS 20.x/21.x + Blackhawk XDS560v2** | ✅ **가능 (조건부)** | [T] CCS 21.0.x 호스트 OS = Win10/11 64-bit, Ubuntu 22.04/24.04/26.04 [S3] · [T] CCS 20.4.x 구성요소 목록에 `BlackHawk Emulator Drivers v9.0.0.002` 번들 [S1][S2] · [T] `dtc_conf` 어댑터 `bh560v2u`/`bh560v2e` (Blackhawk USB/Ethernet) [S18] · [V] Blackhawk: Windows/Linux 드라이버 [S13][S14][S16]. **조건**: 대상 TI 칩이 XDS560v2 지원군(JTAG/cJTAG)일 것. SWD 전용 칩, MSP430은 불가 [T][S11]. Blackhawk 페이지가 명시한 CCS 버전은 v4.2–v10뿐 [V][S13]이라, **CCS 21 + Blackhawk 조합은 실기 게이트로 확인해야 함** |
| **macOS Loom ↔ (SSH 터널) ↔ x64 Windows/Linux 브리지 호스트(CCS Scripting) ↔ XDS560v2** | ✅ **권장 경로 [I]** | 각 구성요소는 문서화되어 있음: 헤드리스 CCS Scripting `run.sh`/`run.bat`, IDE 의존성 없는 `interactive.mjs` [S19]. 하지만 이 조합 전체는 TI/Blackhawk가 문서화한 구성이 **아님** |
| macOS + VM(Parallels/VMware/UTM) + USB 패스스루 | ⚠️ **비공식, 비권장** | [T] VM은 공식 지원 구성이 아니며, VM에서는 trace를 쓸 수 없음 [S24]. Apple silicon VM 게스트는 Arm OS인데 CCS는 x64 Windows/Linux만 명시함 [S3] → **공식 지원 미확인, 비권장 [I]** (불가능하다는 증거는 아님). Intel Mac + x64 게스트도 공식 지원 아님. **네이티브 x64 호스트 권장** |
| TI 시뮬레이터로 MVP 검증 | ⚠️ **공식 지원 확인 못함 → MVP 검증 수단으로 삼지 않음** | CCS 20.x/21.x 릴리스 노트에 시뮬레이터 언급이 없음(존재하지 않는다고 단정하지는 않음) [S1–S4][S27]. [F] TI 포럼: CCSv6부터 시뮬레이터를 공식 지원하지 않는다는 TI 직원 답변 (검색 스니펫, 원문 403) [S29] → MVP는 **fake backend**로 진행(실기 검증 아님) |

**한 줄 권고**: 현재 macOS 워크스테이션에서 XDS560v2를 직접 구동할 공식 경로는 없습니다. **x64 Windows 11 또는 Ubuntu 24.04 브리지 호스트에 CCS 21.0.x를 설치하고, Loom 소유의 얇은 CCS Scripting 브리지 스크립트를 SSH 터널 너머로 macOS loomd의 `dbg` worker가 호출하는 구조**를 권장합니다. LAN 모델(BH-XDS-560v2)이면 브리지 호스트 위치에 USB 제약이 없어 더 유리합니다(드라이버와 디버그 서버는 여전히 x64 Win/Linux 호스트에서 실행).

---

## 1. Blackhawk XDS560v2 제품 구분 (모델 확정 필요)

| 모델 / P/N | 호스트 인터페이스 | 근거 | 비고 |
|---|---|---|---|
| **BH-XDS-560v2-BP** (SKU 12161), 제품명 "Blackhawk XDS560v2 STM Emulator (TMDSEMU560v2STM-UE)" | USB 2.0 HS + 10/100 Ethernet(LAN), USB 버스 전원 | [V] 제품 페이지 [S13] | 내부에 임베디드 Linux가 돌고, 부팅에 최대 60초 걸리며 safe mode가 있음 [V][S15]. LAN 기본값은 DHCP [V][S15][T][S18] |
| **BH-XDS-560v2-PoE** (SKU 12162/12162EU) | 위 + PoE | [V] [S13] | 페이지에서 PoE 항목 P/N 표기가 BP와 충돌함(벤더 페이지 자체의 불일치) |
| **BH-USB-560v2** "USB560v2" | **USB만** | [V] 데이터시트 USB560v2-DS-01 (2012) [S14], Bh560v2Config UG [S15] | 부팅 20–30초 [V][S15] |
| (구분용) Blackhawk XDS560 **v1** — USB560, LAN560, USB560m/bp | — | [T] Blackhawk XDS560v1 프로브는 CCS v20 미지원 [S7], v1 obsolete [S10] | **범위 밖. 모델명이 "USB560"이면 v1일 가능성이 있으니 주의** |
| (구분용) XDS560v2 PRO TRACE | — | [T] 핀 trace는 PRO TRACE 별도 필요 [S9][S11] | MVP 범위 밖 |

**공통 하드웨어(Blackhawk 고유) [V][S13][S14]**: IEEE 1149.1 JTAG + 1149.7(cJTAG), TCLK 최대 50 MHz, 타깃 전압 1.2–4.1 V 자동 감지, 네이티브 MIPI-60 케이블과 TI14 / cTI20 / ARM20 핀 변환기(BP 키트에는 TI60-Trace 추가), STM 1–4핀, 128 MB 버퍼. **SWD는 Blackhawk 문서에 언급 없음.** [T] TI 일반 XDS560v2 페이지에는 SWD를 지원하지 않는다고 명시됨 [S11].

**Blackhawk 도구 [V]**: Bh560v2Config (Java GUI: 탐색, IP 설정, 펌웨어, 스캔 테스트). 문서상 모든 Blackhawk XDS560v2 모델에서 USB·Ethernet으로 동작하며, 플랫폼은 Windows와 Linux [S14]. 공개 SDK/API는 Blackhawk 제품·다운로드 페이지 어디에도 없음 [S13][S16]. 다운로드 센터에는 CCS용 Windows·Linux 디바이스 드라이버만 있음 [S16].
**TI 측 CLI [T]**: `dtc_conf get bh560v2u 0`, `dtc_conf get bh560v2e <ip> ipClient`(LAN 프로브를 다른 사용자가 점유 중인지 확인) [S18]. 이 명령은 사전 점검 수단으로 유용함.

> ⚠️ 출처 범위 주의: TI `TMDSEMU560V2STM-U/-UE` 페이지의 지원 디바이스 목록과 SWD 미지원은 **TI 레퍼런스 XDS560v2 일반 사양 [T]** 입니다. Blackhawk 제품은 TI XDS560v2 system trace 레퍼런스 디자인 기반이라고 밝히므로 [V][S13] 같을 가능성이 높지만, Blackhawk 드라이버의 지원 범위를 직접 증명하지는 않습니다.

---

## 2. TI 타깃 패밀리 (칩 미정 → 조건부)

| 패밀리 | TI 일반 XDS560v2 [T][S11][S12] | Blackhawk 목록 [V][S13][S14] | Loom 관점 메모 |
|---|---|---|---|
| C2000 / C28x | 지원 | C28xxx 계열 명시 | 실시간 모드(polite/rude)로 **비정지 메모리 접근 가능** [T][S23]. 가장 유리함 |
| C29x (F29H85x) | — | — | [F] TI 엔지니어: C29x에는 실시간 모드가 없다고 답변 [S28] → 실행 중 읽기를 보장할 수 없음 |
| Arm Cortex-A/R/M (Sitara, Hercules, TM4C, Jacinto) | 지원 (JTAG/cJTAG) | Cortex A/R/M, ARM9/11, Stellaris, Jacinto, Sitara | [T] CCS 21 신기능: XDS560v2/XDS200에서 Cortex-R 실시간 메모리 접근 [S5]. 단 EXT_EP-12431: 일부 Cortex-R에서 동작하지 않고 device XML 수정이 필요함 [S3] |
| C6000 (C674x, C66x KeyStone) | 지원 (C671x/C672x/C674x/C66x) | C6000, C66x, DaVinci, OMAP | 멀티코어 세션. 실행 중 접근 가능 여부는 디바이스마다 다름 |
| C62x/C64x 구형, C54x | **미지원** | (C5000 일반 표기) | 벤더 목록과 TI 목록이 불일치함 → TI 기준을 따름 |
| MSP430 | **미지원** | — | XDS560v2로는 불가 |
| SWD 전용 디바이스 | **미지원** | — | — |
| AWR mmWave | AWR12xx **미지원** | "AWRx" 지원 표기 | 불일치. 칩이 정해지면 확인 |

---

## 3. CCS / API 현황

- **현재 버전**: CCStudio IDE **21.0.0** (TI 다운로드 페이지 기준 출시일 2026-06-15), **21.0.1** 서비스 릴리스. Theia 1.70.2 기반 [S5][S4]. 20.x 라인 마지막은 20.5.0 [S27].
- **호스트 OS (21.0.x) [T]**: Windows 10/11 64-bit, Ubuntu 26.04/24.04/22.04 64-bit, macOS 14/15/26 [S3][S4]. Ubuntu 20.04와 macOS 13 지원 중단, Windows 32-bit 미지원, XDS510 계열 프로브는 CCS 21에서 미지원 [S5].
- **macOS 아키텍처 [T]**: 21.0.0 mac 설치본은 `CCS_21.0.0.00014_mac_x86.dmg` [S5]. Apple Mx에서는 Rosetta로 실행 가능 [S6]. 20.1.0 노트: 백엔드 구성요소가 아직 x86이라 Rosetta가 필요 [S1]. Intel Mac과 Apple silicon 모두 XDS560v2는 미지원 [S1][S2].
- **스크립팅 [T][S19][S20]**
  - **CCS Scripting** (권장): JavaScript(기본, CCS 동봉 Node) 또는 Python(CCS 21.0.1 동봉 3.14, 외부 인터프리터는 3.10+). `<CCS>/ccs/scripting/run.sh|run.bat <script>`로 실행. `interactive.mjs`는 IDE에 의존하지 않음 → **헤드리스 가능**. Theia 1.3부터 동기 API.
  - **DSS (Legacy)**: 21.0.1에서도 계속 제공 [S19]. 20.1.1 문서에는 deprecated 경고가 있었음(검색 스니펫, 원문 미확인). 마이그레이션 가이드: 기존 DSS 스크립트는 새 CCS Scripting과 호환되지 않음 → 재작성 필요 [S20]. **신규 연동은 CCS Scripting만 대상으로 함.**
  - **API (20.1/20.2 레퍼런스) [S21][S22]**: `ds.configure(ccxml)`, `ds.listCores()`, `ds.openSession(name|regex)`(멀티코어 = 다중 세션), `ds.setScriptingTimeout(ms)`(기본값 무제한, `ScriptingTimeoutError`), `target.connect/disconnect/isConnected/halt/isHalted/run(waitForHalt?)/waitForHalt/reset(type?)/getResets/stepInto/stepOver/stepOut`, `memory.read/readOne/write/fill/loadProgram/loadBinaryProgram/verifyProgram/saveData`, `breakpoints.add/remove/…`, `registers.read`, `expressions.evaluate`, `symbols`, `flash`(+`performOperation`), `settings.get/set`.
  - ⚠️ API 함정: `run()`은 **기본값이 halt될 때까지 블록**함 → 반드시 `run(false)` 사용. `reset()`은 리셋이 실패해도 예외를 던지지 않음 → 사후 상태 재확인 필요. `loadProgram`은 기본적으로 main까지 실행된 뒤 halt 상태로 반환됨. `loadBinaryProgram`은 로드 전에 타깃을 halt함. 진행 중인 호출을 취소하는 API는 문서에 없음 → **타임아웃 또는 프로세스 종료만 가능**.
- **GDB 서버나 범용 프로브 SDK**: Blackhawk/TI 문서 어디에서도 XDS560v2용 GDB 서버나 공개 SDK를 **찾지 못함**. 존재한다고 가정하지 않음.
- **라이선스 [T][S25]**: CCS는 무료(royalty-free)이고 바이너리 재배포가 가능하지만 TI 디바이스와 함께 쓰는 경우로 한정됨. 리버스 엔지니어링은 금지됨. 서드파티 구성요소는 각 manifest를 따름. **Loom은 CCS나 Blackhawk 바이너리를 재배포하지 않고, 사용자가 설치한 CCS를 호출만 함.** Blackhawk LAN 프로토콜(장치 파라미터 `addrIo :55755`/`addrConf :55756` [S15])을 직접 구현하는 방식은 금지.
- **Linux 설치 [T][S26]**: 일반 사용자로 설치한 뒤 `sudo <CCS>/ccs/install_scripts/install_drivers.sh` 실행(문서가 생략하지 말라고 강조). 무인 설치는 `--mode unattended`.

---

## 4. 기능 매트릭스 (XDS560v2 + CCS Scripting)

| 기능 | 문서 근거 | 비침습? | MVP |
|---|---|---|---|
| `.ccxml` 연결, 코어 선택 | `configure`, `listCores`, `openSession(regex)` [S19] | 연결 시 타깃을 halt하거나 리셋하는 연결 옵션이 있음 [S23] → **반드시 끈 상태로 운용** | ✅ |
| 멀티코어/멀티세션 | 다중 `openSession` [S19], sync group [S23] | — | 단일 코어 먼저 |
| halt/run/step | Target API [S21] | ❌ 상태 변경 (action) | ✅ (명시 action) |
| 브레이크포인트 | `breakpoints.*` [S19][S20] | 설정은 무해할 수 있지만 hit 시 정지 | Phase 2 |
| 레지스터 | `registers.read` [S19] | 레지스터 연속 갱신은 완전한 실시간 모드를 지원하는 디바이스에서만 [S23] | halted 상태에서만 |
| 메모리 read/write | `memory.read/write` [S22] | **디바이스·모드 의존.** 메모리 연속 갱신은 비침습 접근을 지원하는 디바이스에서만 동작. "디버거 접근 전 halt" 옵션은 기본값 off [S23]. C28x 실시간 모드: halt 없이 접근 가능 [S23]. Cortex-R + XDS560v2는 CCS 21에서 개선됐으나 이슈 있음 [S3][S5]. API 문서는 실행 중 동작을 명시하지 않음 [S22] | ✅ 단, `--allow-halt` 없이는 halted 상태 또는 확인된 realtime 모드에서만 |
| 심볼 / ELF·COFF | `loadProgram`이 심볼 로드, `symbols` [S22][S20] | 프로그램 로드는 침습적 | 심볼만 로드하는 방식은 확인 필요 |
| 식 / 변수 | `expressions.evaluate` [S19] | 메모리와 같음 | ✅ |
| 로드 / 플래시 / 리셋 | `loadProgram`, `loadBinaryProgram`(flash 가능), `flash`, `reset` [S22][S20][S21] | ❌ R3–R4 | MVP 미제공. 향후 명시 요청 action으로 제공하며 Loom 기존 자동 기본값과 사용자가 설정한 정책을 따름(새 필수 승인 흐름 없음) |
| 실시간 스트리밍 / trace | XDS560v2 STM 128 MB, 코어 trace는 ETB, 핀 trace는 PRO TRACE [S9][S11]. VM에서는 trace 불가 [S24] | — | ❌ 범위 밖 |

**메모리 읽기가 CPU를 멈추나?** → **보장할 수 없음.** 비침습 접근을 지원하는 디바이스와 모드(C28x 실시간 모드 등)에서만 halt 없이 읽음. 그 외에는 halted 상태가 필요하거나 halt→access→resume을 거침(옵션이 켜진 경우). Loom은 칩이 정해지기 전까지 **"실행 중 읽기 = 미검증"** 으로 표기함.

---

## 5. Loom 아키텍처 제안 (HEAD d6fc6f3 패턴 기반)

### 5.1 기존 패턴에서 가져올 것
- `DomainModule::register` → `CommandSpec{write_class, roles, bounds}`, `RiskTable`(R0–R4), `WorkerSpec{instances, budgets, reopen_after_crash, platforms}`, `Reducer`, `EvidenceVerifier`, `SafeStateSpec`, `HelpSpec.notes`(`crates/loom-domain-api/src/registry.rs`).
- `CallCtx::wait(WaitSpec{filter, after, deadline, max_events})` → `WaitResult{timed_out, scanned_through}`(`crates/loom-domain-api/src/ctx.rs:137`). 대기와 조건 판정은 이미 host 쪽 계약임.
- `WorkerHost::latch` = STOP 플래그, 200 ms 안에 ack(`crates/loom-gateway/src/worker_host.rs`).
- 외부 프로세스 수명주기: `loom-video` worker의 `process_group(0)` → SIGINT → SIGKILL → reap(`crates/loom-video/src/worker.rs:110-146`).
- 외부 endpoint: `loom-psu`/eth/uart의 **loopback TCP 외부 시뮬레이터** 패턴.
- 주의: worker IPC 레인은 **로컬 전용**(원격 클라이언트 거부, `crates/loom-worker-host/src/listen.rs`)이고, `Platform::Macos`는 주석상 개발·시뮬레이션 용도. → 원격 호스트는 **worker가 아니라 backend endpoint**로 모델링함.

### 5.2 구조
```
macOS: Loom UI ─┐
Luna skill/CLI ─┴─ loomd(gateway) ── dbg domain(gateway: risk/policy/reducer/verifier)
                                        │ loom-worker/1 (local IPC)
                                     dbg worker (per probe) ── backend trait
                                        ├─ FakeBackend (CI)
                                        └─ CcsRemoteBackend ── JSON-lines over 127.0.0.1:port
                                                                 (SSH -L tunnel)
x64 Win/Linux bridge: loom-ccs-bridge.mjs (run.sh, CCS Node) ── CCS Scripting ── Debug Server
                                                                 ── Blackhawk driver ── USB/LAN
BH XDS560v2 ── MIPI60→TI14/cTI20/ARM20 ── JTAG/cJTAG ── TI target
```
- **ID**: `probe_id`(예: `bh560v2:lan:<mac>` / `bh560v2:usb:<serial>`), `target_id = <probe_id>/<core regex 결과 이름>`, `session_epoch`(재연결할 때마다 +1).
- **이벤트(저널)**: `dbg.link`(bridge/probe/connect 상태), `dbg.run_state{state, observed_at_host, age_ms, certainty: fresh|stale|unknown}`, `dbg.action{verb, write_state}`(halt/run/step/reset/load), `dbg.watch{matched, value, polls, gaps}`, `dbg.sample_summary`. 샘플 원본은 저널에 넣지 않음(video와 같은 원칙). 요약과 증거만 기록.
- **타임스탬프/카운터**: `t_host_req`, `t_bridge_start`, `t_bridge_done`(bridge의 단조 시계), `t_host_resp`, `t_emit`(저널 기록), `poll_seq`, `gap_count`.
- **지연 지표 (세 가지 모두 기록하고, 전체 지연을 빼지 않음)**:
  1. **획득 지연**: host↔bridge↔probe 왕복(`t_bridge_done − t_bridge_start` 등).
  2. **LLM 지연**: Luna가 호출을 결정하고 결과를 해석하는 데 걸린 시간(에이전트 또는 벤치 측에서 기록).
  3. **End-to-end (사용자에게 가장 중요)**: ① 요청 → 실제 시작(사용자/에이전트 요청 시각 → bridge가 프로브에 실제로 명령을 낸 시각), ② 이벤트 발생 → 올바른 보고(조건 충족·샘플 시각 → `t_emit` → 에이전트가 올바른 결과를 보고한 시각).
  두 구간을 분리해서 원인을 찾을 수 있게 하되, end-to-end 값을 판정 기준에서 제외하지 않음.
- **직렬 큐**: 프로브 하나당 bridge 프로세스 하나와 명령 큐 하나(JTAG 체인 공유). action은 watch/sample보다 우선함. 큐 길이 상한, 명령별 deadline(`setScriptingTimeout` + host 측 deadline).
- **취소/타임아웃**: CCS 호출 도중 취소 불가 → deadline을 넘기면 bridge 프로세스 그룹을 종료함. **종료 후 타깃 상태는 `unknown`으로 기록하고, 자동 재연결 시 halt/reset 하지 않음.**
- **재연결**: `reopen_after_crash = None`(UART와 같음). 사용자가 명시적으로 `dbg open`을 다시 해야 함. 연결 옵션 *halt/reset on connect*는 꺼야 하며, bridge가 `settings.get`으로 이를 검증한 결과를 증거로 남김.

### 5.3 STOP 의미
- STOP = 큐에 남은 명령과 활성 watch/sample을 취소하고, host 측 polling을 중단함. **halt·resume·reset·flash·disconnect는 하지 않음.** 실행 중인 타깃은 그대로 둠.
- STOP 이후 정리 단계에서 타깃을 멈추거나 bridge를 강제 종료해 상태를 바꾸지 않음. bridge 종료는 사용자가 명시적으로 `dbg close`를 실행할 때만 함.
- 링크가 끊기면 `run_state`를 마지막으로 관측한 값으로 두고 `certainty=stale`, `age_ms`를 계속 증가시킴. UI와 에이전트에는 *"마지막 관측 running, N초 전, 현재 상태 미상"* 으로만 보고함.
- `SafeStateSpec`: 디버거에는 자동 safe-state가 없음. "recovery" 버튼은 `dbg.run`이나 `dbg.halt`를 **사용자가 명시적으로 선택**했을 때만 실행함.

### 5.4 소형 Luna 모델용 CLI 초안 — **제안, 미구현** (polling은 host가, 에이전트에는 압축된 증거만)
```
loom dbg status [<target>]                       # R0: link/run_state/age_ms/certainty, 1줄 JSON
loom dbg open <target> --ccxml F --core RE       # R1: connect (halt/reset-on-connect=off 검증)
loom dbg read <target> <sym|0xADDR> [--count N] [--allow-halt]
      # R0, 실행 중 비침습 접근이 불가하면 E_WOULD_HALT (제안 오류 코드, 미구현; 자동 halt 금지)
loom dbg watch <target> --expr 'g_state==3' --every 20ms --timeout 5s
      # R0, worker가 polling/판정 → {matched,value,t_match,polls,gaps,lat_p50/p95}
loom dbg sample <target> --vars a,b --every 10ms --for 2s --summary min,max,last,changes
loom dbg halt|run|step <target>                  # R2 action, 저널에 기록
loom dbg bp add|rm <target> <loc>                # R2
loom dbg reset|load|flash <target> ...           # R3/R4, MVP 미제공 · 향후 명시 요청 action (Loom 기본값+설정 정책)
loom dbg close <target> [--leave-as-is]          # 기본값: 상태 유지, disconnect만
```
- 반환 형식은 키 고정 JSON 한 줄, 배열은 상한 있음. 원본 DSS/CCS 덤프는 반환하지 않음(필요하면 `--artifact`로 파일 경로만 반환).
- 스킬에 넣을 규칙(짧게): "변수를 매번 읽지 말고 `watch`/`sample` 조건을 1회 호출하라", "`E_WOULD_HALT`가 나오면 사용자가 명시적으로 요청하지 않는 한 `--allow-halt`를 쓰지 마라", "STOP 이후 run/halt/reset을 하지 마라", "`certainty≠fresh` 상태는 현재 상태로 단정하지 마라".

### 5.5 향후 다중 backend (설계 차원만)
`Backend` trait: `capabilities()` (nonintrusive_read, realtime_mode, flash, trace, multi_core), `connect/disconnect/run_state/read/write/halt/run/step/bp/reset/load`. J-Link, ST-Link, OpenOCD(GDB RSP)도 같은 trait에 맞출 수 있음. 비침습 메모리 접근 지원 여부는 backend의 capability로 노출하고, 지원하지 않으면 `E_WOULD_HALT`(제안, 미구현)를 반환함. 개별 벤더는 이번 조사 범위가 아님.

---

## 6. 단계별 계획과 수용 증거

| 단계 | 내용 | 수용 증거 | 비고 |
|---|---|---|---|
| **P0 (지금 가능)** | `dbg` 도메인 계약, `FakeBackend`(가상 run_state, 메모리 맵, 지연·끊김 주입), CLI와 스킬 초안 | cargo 테스트: watch/sample host-side 판정, STOP이 halt/resume을 발행하지 않음, 링크 끊김 시 stale 보고, 큐 직렬화, deadline 초과 시 bridge 종료와 `unknown` 기록 | **"fake backend 테스트, 프로브 검증 아님"** 이라고 명시. TI 시뮬레이터는 공식 지원 근거를 찾지 못함 |
| **P1 (브리지 호스트 준비)** | x64 Win11 또는 Ubuntu 24.04에 CCS 21.0.x와 Blackhawk 드라이버 설치. `loom-ccs-bridge.mjs`(JSON-lines, CCS Node) | 프로브 없이: `ds.configure` 오류 경로, `setScriptingTimeout` 동작, bridge 프로세스 수명주기 | 하드웨어 미검증 |
| **P2 (하드웨어 게이트)** | 사용자 보드에서 수행: `dtc_conf get bh560v2{u,e}` 펌웨어/ipClient 확인 → `.ccxml` connect(halt-on-connect 해제 확인) → 알려진 상수 주소 read → (명시 요청 시, 설정된 정책에 따라) halt/run 1회 → watch 조건 1건 | 저널 이벤트, 획득 지연 p50/p95, end-to-end(요청→실제 시작, 이벤트→올바른 보고), 연결 전후 run_state가 같다는 증거 | 이 단계 이후에만 "검증됨"이라고 표기 |
| **P3 (향후)** | breakpoints, step, reset, load/flash, 멀티코어 — 명시 요청 action으로만 | Loom 기존 자동 기본값과 설정 정책 하에서의 write_state 기록 | 새 필수 승인 흐름을 만들지 않음 |
| **범위 밖** | STM/코어 trace, VM 경로, macOS 네이티브 드라이버 | — | — |

---

## 7. 사용자 답변이 필요한 결정 (답이 오기 전까지 진행을 막지 않음)
1. **정확한 모델**: BH-XDS-560v2(-BP/-PoE, USB+LAN)인지 BH-USB-560v2(USB만)인지. 라벨이 "USB560"/"LAN560"이면 **v1이라 미지원**.
2. **TI 칩/보드 P/N**: 실행 중 읽기 정책(C28x 실시간 모드인지, Cortex-R인지, C66x인지)과 미지원 여부(MSP430, SWD 전용)를 이것으로 결정함.
3. **브리지 호스트**: 기존 Windows/Linux x64 PC가 있는지. 없다면 Intel 미니 PC를 권장함. Apple silicon VM은 비권장.
4. **CCS 버전 고정**(21.0.1 권장)과 기존 `.ccxml`·ELF 제공 여부.
5. **정책**: Loom 기존 자동 기본값을 그대로 둘지, halt/step/reset/flash에 대해 사용자 설정 정책을 따로 둘지(새 필수 승인 흐름은 만들지 않음).

## 8. 최대 블로커와 해결책
| 블로커 | 해결책 |
|---|---|
| macOS에서 XDS560v2 미지원 | x64 Win/Linux 브리지 + SSH 터널. LAN 모델이면 USB 제약 없음 |
| 공개 SDK·GDB 서버 없음 | CCS Scripting(사용자 설치 CCS)을 bridge 스크립트로 호출. 바이너리 재배포나 리버스 엔지니어링 금지 |
| Blackhawk 문서가 오래됨(CCS ≤ v10) | CCS 21 + Blackhawk 드라이버 조합을 P2 게이트에서 확인. 실패하면 Blackhawk 지원(support@blackhawk-dsp.com)에 문의 |
| 실행 중 메모리 접근이 디바이스마다 다름 | capability 플래그와 `E_WOULD_HALT`(제안). 칩이 정해지면 실시간 모드 설정 |
| CCS 호출 취소 불가, `run()` 기본 블록 | `run(false)`, `setScriptingTimeout`, 프로세스 그룹 kill과 `unknown` 상태 표기 |
| 공식 시뮬레이터 확인 못함 | 시뮬레이터를 MVP 검증 수단으로 삼지 않음. FakeBackend 계약 테스트 후 하드웨어 게이트 |

---

## 출처 (접근일 2026-11-14)
- [S1] TI, CCS 20.4.1 Release Notes — https://software-dl.ti.com/ccs/esd/CCSv20/CCS_20_4_1/exports/CCS_20.4.1_ReleaseNote.htm [T]
- [S2] TI, CCS 20.4.0 Release Notes — https://software-dl.ti.com/ccs/esd/CCSv20/CCS_20_4_0/exports/CCS_20.4.0_ReleaseNote.htm [T]
- [S3] TI, CCStudio IDE 21.0.0 Release Notes — https://software-dl.ti.com/ccs/esd/CCSv20/CCS_21_0_0/exports/ccstudio-ide_21.0.0_releasenote.html [T]
- [S4] TI, CCStudio IDE 21.0.1 Release Notes — https://software-dl.ti.com/ccs/esd/CCSv20/CCS_21_0_1/exports/ccstudio-ide_21.0.1_releasenote.html [T]
- [S5] TI, CCSTUDIO 21.0.0 download — https://www.ti.com/tool/download/CCSTUDIO/21.0.0 [T]
- [S6] TI, CCS 21.0.1 User's Guide, System Requirements — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_overview.html [T]
- [S7] TI, CCS v20 landing — https://downloads.ti.com/ccs/esd/CCSv20 [T]
- [S8] TI, CCS v12 macOS host support — https://software-dl.ti.com/ccs/esd/documents/ccsv12_macos_host_support.html [T]
- [S9] TI, XDS560v2 Debug Probe — https://software-dl.ti.com/ccs/esd/documents/xdsdebugprobes/emu_xds560v2.html [T]
- [S10] TI, XDS560 (v1) — https://software-dl.ti.com/ccs/esd/documents/xdsdebugprobes/emu_xds560.html [T]
- [S11] TI, TMDSEMU560V2STM-UE — https://www.ti.com/tool/TMDSEMU560V2STM-UE [T, generic]
- [S12] TI, TMDSEMU560V2STM-U — https://www.ti.com/tool/TMDSEMU560V2STM-U [T, generic]
- [S13] Blackhawk, XDS560v2 STM Emulator — https://www.blackhawk-dsp.com/products/jtag-emulators/xds560v2 [V]
- [S14] Blackhawk, USB560v2 datasheet USB560v2-DS-01 (2012) — http://www.blackhawk-dsp.com/downloads/docs/datasheets/USB560v2-DS-01.pdf [V]
- [S15] Blackhawk, Bh560v2Config User Guide UG-05 (2017) — http://www.blackhawk-dsp.com/downloads/docs/guides/Bh560v2Config-UG-05.pdf [V]
- [S16] Blackhawk, Download Center — https://www.blackhawk-dsp.com/support/downloads [V]
- [S17] Blackhawk, USB560v2 Installation Guide IG-01 (2012, TI E2E에 게시) — https://e2e.ti.com/cfs-file/__key/communityserver-discussions-components-files/791/5756.USB560v2_2D00_IG_2D00_01.pdf [V, 2차 호스팅]
- [S18] TI, dtc_conf — https://software-dl.ti.com/ccs/esd/documents/xdsdebugprobes/emu_dtc_conf.html [T]
- [S19] TI, CCS 21.0.1 Scripting — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-scripting.html [T]
- [S20] TI, Migrating Legacy DSS Scripts (21.0.1) — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-scripting-migration.html [T]
- [S21] TI, Scripting API Target (20.2.0) — https://software-dl.ti.com/ccs/esd/documents/scripting_api/ccs_20.2.0/ccs_scripting/docs/interfaces/Target.html [T]
- [S22] TI, Scripting API Memory (20.1.0) — https://software-dl.ti.com/ccs/esd/documents/scripting_api/ccs_20.1.0/ccs_scripting/docs/interfaces/Memory.html [T]
- [S23] TI, CCS 21.0.1 Debug Overview — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-main.html [T]
- [S24] TI, CCS on VMware — https://software-dl.ti.com/ccs/esd/documents/ccs_vmware.html [T]
- [S25] TI, CCS License — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_legal.html [T]
- [S26] TI, CCS Installation — https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_installation.html [T]
- [S27] TI, CCS 20.5.0 Release Notes — https://software-dl.ti.com/ccs/esd/CCSv20/CCS_20_5_0/exports/CCS_20.5.0_ReleaseNote.htm [T]
- [S28] TI E2E, F29H85x read memory while running — https://e2e.ti.com/support/tools/code-composer-studio-group/ccs/f/code-composer-studio-forum/1696775/f29h85x-som-evm-f29h85x-c29x-read-memory-of-a-running-core-without-halting-it-as-on-c28x [F, 검색 요약만]
- [S29] TI E2E, Simulators removed in CCSv6 — https://e2e.ti.com/support/tools/code-composer-studio-group/ccs/f/code-composer-studio-forum/343311/simulators-removed-in-ccsv6 [F, 403, 검색 스니펫만]

**검증 한계**: 모든 내용은 문서를 근거로 한 것이고 하드웨어로 검증한 항목은 없음. API 레퍼런스는 공개된 20.1/20.2 버전을 기준으로 함(21 레퍼런스 URL은 404). CCS 21에서 Blackhawk 드라이버 버전은 릴리스 노트에 기재되어 있지 않음.
