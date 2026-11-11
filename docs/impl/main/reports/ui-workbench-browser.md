# UI workbench — 브라우저 QA 보고 (ui-qa, ownership #3)

담당: Claude Code Opus 5.5 foreground (Herdr wE:p1). 하위/백그라운드 agent 없음. 구현 파일 수정 없음.
상태: **READY_FOR_REVIEW — FINAL2 strict Ego 61/61 PASS · canonical P1 5/5 PASS (CAN p95 82.9 ms) · 정리 확인 완료**.

## 실행 환경 (baseline)
- 기준 HEAD `ea49d60`. 브라우저: **Ego Lite** (`ego-browser nodejs`, guide v2.2.0), TaskSpace `loom ui-workbench qa` (spaceId 2, agent 소유). Claude Code 일반 명령 승인 경로. root/Codex 재시도, 전역 권한, sandbox 우회 없음. Playwright fallback은 아직 쓰지 않았다.
- 고정 baseline 빌드 사본(다른 워커의 `ui/dist` 재빌드 영향 차단): `~/.cache/loom-ui-workbench/baseline/`
  - `dist/assets/index-BExAuFCo.js` sha256 `96b1d2c7…4af4318`, `dist/index.html` `ac17ffe0…4bd4bd` (P1 최종과 동일 빌드)
  - `loomd` 사본 sha256 `462d4445cd7fe95b697d7d02c1b36c0976484b2dad5e61253b550feb8d5bf4c8`
- 하네스(신규, ui-qa 소유): `ui/e2e/workbench-harness.ts` — 자기 loomd 1개(fresh HOME, `--env sim --rights operate`, **UI dir 인자 지정**), 127.0.0.1 랜덤 포트 control(0600 파일, secret 비출력), close 시 HOME 삭제. eslint/tsc PASS.
- 러너/시나리오: 현재 저장소 `ui/e2e/workbench-ego/`(아래 FINAL 참조). baseline 당시에는 cache 사본으로 실행했고 그 사본은 지우지 않고 남겨 두었다(지금은 쓰지 않음). 출력의 `#boot=` 는 `<redacted>`.
- 뷰포트: baseline은 CDP `Emulation.setDeviceMetricsOverride` **DPR 1**로 1440×900, 1024, 768, 390을 찍었다 → 터미널 글꼴이 아주 작게 나온 것은 환경 artifact(아래 F-ENV1). final은 DPR 2(호스트 원래 비율).
- 채널은 **실제 sim daemon**에 agent RPC로 연다(baseline에는 UI 연결 기능이 없음): `uart.open` dut/aux (`sim://bootshell?split=0`), `can.open ch0`, `uds.connect ecu sim=bench`, `eth.capture.start sim0→cap`. mock/replay 없음.

## Baseline 결과 요약
| 항목 | 결과 |
|---|---|
| Ego 최소 probe | PASS (`baseline/probe-1440.png`) |
| 다중 인터페이스 chip → 패널 | PASS(기능): dut/aux/ch0/cap/ecu 각각 별도 탭, 같은 chip 재클릭은 기존 탭 포커스(중복 없음) |
| 2 UART 입력 대상 분리 | PASS: aux 탭에서 `root⏎` → aux만 `Password:`(dut는 E_TIMEOUT). dut 탭 입력 `qadut⏎` → dut 화면에 `login: qadut / Login incorrect` |
| 재클릭 포커스 | PASS (`reopen-aux.png`) |
| 새로고침 후 레이아웃 | **FINDING**: 동적으로 연 aux/eth/ecu 탭이 사라지고 preset 3개로 복귀 (`after-reload.png`) |
| STOP | PASS: 버튼 클릭 → stop-panel 표시, 이후 agent `uart.send` = `E_STOPPED` (`stop-1440.png`, `stop-390.png`) |
| 390 px | **FINDING**: 가로 스크롤 발생(`scrollWidth>innerWidth`), STOP 버튼은 상단에 보임 |

## 실행 가능한 baseline findings (워커 소비용, 우선순위 순)
담당 추정은 task 소유 분할 기준. 확인은 final에서 같은 시나리오로 재측정한다.

1. **[interfaces] 연결 상태 진실성 — ETH chip `data-state="unknown"`**: `eth.capture.start` ok, 패널은 LIVE 캡처 중인데 chip 상태는 unknown (`multi-1440-after-agent-open.png`). (메인: interface 워커에 이미 통보됨.)
2. **[interfaces/panel] UDS ecu 패널 "대상 없음"**: `uds.connect ecu` ok(link up) 이후 chip으로 `UDS 콘솔 · ecu`를 열면 "대상 없음 — 에이전트가 loom uds connect로 연결하면 나타납니다" 표시 (`multi-1440-after-clicks.png`). 패널을 connect **이후**에 열 때 기존 대상 상태를 hydrate 하지 않는 것으로 보인다(P1 real-smoke는 패널을 먼저 열어 이 경로를 안 탔다). 실제 연결 상태와 반대되는 문구라 우선순위 높음. 패널 구현 변경은 메인 소유 요청 필요.
3. **[shell/main] 기본 빈 세션이 존재하지 않는 대상을 보여 준다**: 채널 0개인데 m1-default가 `UART dut`(빈 검은 터미널, 안내 없음), `CAN ch0`, `UDS bcm`을 연다(`empty-*.png`). 실제 대상이 `ecu`여도 `UDS 콘솔 · bcm`이 계속 남는다. 빈 상태 안내(연결 방법/연결 UI) 또는 welcome으로 대체 권장. UART 빈 패널에는 empty-state 문구가 없다(UDS/CAN에는 있음).
4. **[interfaces] 레이아웃 지속성**: chip으로 연 탭(aux 등)은 reload 후 사라진다. "persistent target-specific tabs" 수용 기준과 충돌. 기존 저장 레이아웃/딥링크 호환을 깨지 않는 선에서 자동 저장 또는 명시 안내 필요.
5. **[shell] 390 px 레이아웃**: 가로 스크롤; 채널 chip이 2~3줄 세로 블록(`UART/dut/gen1`)으로 깨짐; `저장`, `최대화`, `보내기`, `중단!` 버튼 글자가 세로로 줄바꿈; CAN 패널 헤더 탭 제목 잘림. STOP 버튼은 보이지만 **STOP 패널이 높이의 약 1/3을 차지**해 데이터 영역이 거의 없다(`stop-390.png`). 좁은 폭에서는 STOP 패널 compact(해제/복구 송신은 유지) 권장.
6. **[shell] 768 px 설정 과밀**: sub-bar의 `레이아웃/이름/저장/새 채널 패널/에이전트 가리키기/자격`이 줄바꿈되어 `저장` 버튼이 2줄, `에이전트 가리키기` 라벨 분리 (`multi-768.png`). 진행형 공개(설정 메뉴로 접기) 권장. 1440에서도 sub-bar 오른쪽이 설정 컨트롤로 꽉 차 chip 영역과 경쟁.
7. **[interfaces] explorer 부재**: chip은 도메인 그룹/검색 없음, 연결 상태는 테두리 색에만 의존(텍스트·아이콘 없음 — 색각 접근성). `gen1` 노출은 일반 사용자에게 소음.
8. **[shell] 비활성 탭 대비 낮음**: 비활성 탭 제목(`UART 터미널 · dut`)이 배경과 대비가 낮아 거의 안 보인다(`multi-1440-after-clicks.png`). 같은 그룹 안 여러 그룹이 동시에 "active" 강조라 현재 포커스 패널 구분이 약하다.
9. **[해소 — 환경 artifact, F-ENV1] UART 터미널 글꼴 크기**: baseline의 DPR 1 override 스크린샷에서 xterm 글자가 아주 작게(≈6 px) 보였다. 측정 결과 Ego CDP DPR-1 override의 artifact다. 제품 결함 아님(FINAL 절 참조).
10. **[관찰] 입력권 chip**: dut에 `qadut⏎` 후에도 `입력권: 나 (입력 중)` 유지(`input-dut-typed.png`). Login incorrect 후 새 prompt에서 lease 표시가 남는 것으로 보이나 사양 확인 필요(낮음).
11. **키보드 포커스**: Tab 이동 시 포커스 요소는 브라우저 기본 `outline: auto 1px`만 (`focus-ring-1440.png`). 어두운 표면에서 잘 안 보임 → 명시적 `:focus-visible` 링 권장.

## 증거 (baseline)
`docs/impl/main/evidence/ui-workbench/baseline/`: `probe-1440`, `empty-{1024,768,390}`, `multi-1440-after-agent-open`, `multi-1440-after-clicks`, `multi-{1024,768,390}`, `focus-ring-1440`, `stop-{1440,390}`, `input-aux-typed`, `input-dut-typed`, `reopen-aux`, `after-reload`, `deeplink-aux` (.png), 실행 출력 `*.out`, 하네스 로그 `harness-*.log`.
- 주의: `deeplink-aux`는 같은 브라우저 재부트스트랩 직후 hash가 아직 `#boot`이어서 딥링크 판정에 쓰지 않는다(무효 측정, final에서 재측정).

## 정리 (baseline)
- 각 실행 후 자기 harness/loomd 종료, control 파일 삭제, 임시 HOME 삭제 확인(`leftovers: none`).
- Ego TaskSpace 2는 final 재사용을 위해 열어 둔다. final 종료 시 `finish({keep:[]})`.

## FINAL 준비 상태 (메인 지시 반영)
- 메인 결정 반영: m1-default는 동적·빈/알려진 대상만(가짜 dut/ch0/bcm 없음) → `CHECK empty-no-fake-panels`; 명명 preset은 고정 정의 그대로. 지속성은 **명시 저장 → 저장 레이아웃 선택 → reload**로 판정(자동 last-workspace 아님) → `CHECK saved-layout-reload`. baseline finding 3·4는 이 기준으로 재판정한다.
- late-join: 모든 인터페이스가 연결된 뒤 두 번째 탭(viewer)이 합류 → explorer 상태, alias별 패널(aux/dut/ecu/cap/ch0) 분리, UDS ecu 패널 hydrate(finding 2), late 탭에서 aux 입력이 aux에만 도달하는지 agent `uart.expect`로 교차 검증.
- 실행: `~/.cache/loom-ui-workbench/freeze.sh <frozen-dist> <loomd>` → `run.sh final final`. frozen 사본이 없으면 러너가 실행을 거부한다(exit 2). 시나리오 문법 검사 PASS.
- P1 canonical suite는 snapshot 후 병행 실행(`real-smoke.config.ts`, `LOOM_REAL_SMOKE=1`; 이 suite는 저장소 `ui/dist`를 쓰므로 snapshot과 동일한지 hash 대조).
- **메인 요청**: P1 최종에 쓴 `tc1_live_smoke-e4c707f7a9513da7` 실행 파일이 `~/.cache/loom-main-target/debug/deps/`에 더 이상 없다(현재 `a1a72115…` 01:15 빌드뿐, 399a8b6 이전일 수 있음). TC-1 테스트용 현재 실행 파일 경로를 신호와 함께 달라. 없으면 TC-1 1건은 '바이너리 부재'로 기록하고 나머지 4건만 판정한다.

## FINAL 계획 (메인 신호 후)
1. 통합 빌드 `ui/dist` + 메인 loomd를 `~/.cache/loom-ui-workbench/final/`로 고정 복사·hash 기록 후 같은 하네스로 실행.
2. 기본 빈 세션(welcome/empty) 1440·768·390 스크린샷, 가로 스크롤/설정 과밀/STOP 접근성 재측정.
3. **UI로** InterfaceConnect 대화상자를 통해 UART alias 2개(dut/aux) 추가 → explorer 그룹/검색/상태 표시 → 각 패널 분리, 재클릭 포커스, 탭 재열기, 채널별 입력/데이터 분리(agent `uart.expect`로 교차 검증).
4. CAN/UDS/ETH 열기 + 상태 진실성(finding 1·2) + STOP 회귀.
5. 도메인 회귀: 기존 P1 `real-smoke.config.ts` 5개(`LOOM_REAL_SMOKE=1`) — 실패는 원본 보존, 기준 완화 없음.

## FINAL 결과

### 빌드 / 실행 식별
- 메인 FINAL GO(소스 freeze, `pnpm check` 417 tests PASS, `pnpm build` PASS). HEAD `ea49d607733e9452d20c753627255ba5228f3919` + 미커밋 워커 변경(소스 freeze 시점).
- 고정 사본 `~/.cache/loom-ui-workbench/final/` (`ui/e2e/workbench-ego/freeze.sh final ui/dist ~/.cache/loom-main-target/debug/loomd`):
  - `dist/assets/index-BotxmNDR.js` `fb42621d1ad60cf0bd62fd2243ce851fb62d42d96a192a84e317c8c45ed3ff59`
  - `dist/assets/index-NLivQMYM.css` `8780e762237b9121a6297856ff2fd41b96556a5c01f7f020e03c4b36ee9f50d4`
  - `dist/index.html` `6ffb79622dbed91eed4c472b1a024558646e4fb46f79730cab7da2c3ac660e0c`
  - `dist/assets/net.worker-BleoIIWk.js` `98ccabf8bc6b7858fb0e9d8e9787d392cbf43c2160d01ea31372ef49845e65a4`
  - `loomd` `462d4445cd7fe95b697d7d02c1b36c0976484b2dad5e61253b550feb8d5bf4c8` (baseline과 동일 바이너리)
  - 실행 직전 저장소 `ui/dist/assets/index-BotxmNDR.js` 해시가 고정 사본과 같음을 확인했다(P1 suite가 저장소 dist를 쓰기 때문).

### QA 스크립트 (ui-qa 소유, 커밋 대상 후보)
- `ui/e2e/workbench-harness.ts`: 자기 loomd 1개, UI dir 지정, control 0600, close 시 HOME 삭제.
- `ui/e2e/workbench-ego/run.sh <label> <scenario>`: label/scenario 검증. 매 실행마다 `mktemp -d` 0700 control 디렉터리를 쓴다. **EXIT/INT/TERM trap**으로 자기 harness를 정리하고(TERM 15 s 후 KILL), control 파일 삭제와 잔존 프로세스 확인까지 한다. 시작 실패는 exit 3. **시나리오 `RESULT … FAIL`이거나 ego-browser가 0이 아닌 코드로 끝나면 exit 1**. 실행 출력은 `<scenario>-HHMMSS.out`, harness 로그는 `harness-<scenario>-HHMMSS.log`로 남아 이전 실행을 덮어쓰지 않는다.
- `ui/e2e/workbench-ego/freeze.sh <label> <dist> <loomd>`: label 형식을 검증하고 dist/loomd 존재를 확인한다. **이미 있는 snapshot은 덮어쓰지 않는다(exit 3)**. 경로는 모두 따옴표로 감쌌다.
- `prelude.mjs`: `check()`와 `attempt()`가 실패를 모은다. 실패가 하나라도 있으면 `RESULT FAIL: n check(s) failed: …`가 된다. 비밀값·토큰·boot URL은 출력하지 않는다(evidence 스캔 결과 일치 0).
- 시나리오: `final.body.mjs`(strict acceptance), `termfont.body.mjs`(글꼴 진단), `baseline*.body.mjs`, `probe.body.mjs`. 모두 eslint PASS.
- 정확한 실행: `cd ui && e2e/workbench-ego/run.sh final final` / `e2e/workbench-ego/run.sh final termfont`.

### F-ENV1 — 아주 작은 터미널 글꼴 = Ego DPR-1 에뮬레이션 artifact (제품 수정 없음)
`termfont` 시나리오(`final/termfont-061619.out`): 각 모드의 override를 로그인 **전에** 적용하고, 그려진 결과는 3 s 뒤에 측정했다(Ego RAF ≈1 s cadence).

| 모드 | DPR | 텍스트 canvas backing / CSS | link layer | screen/container | 결과 |
|---|---|---|---|---|---|
| native (override 없음, 1309×858 창) | 2 | 1966×870 / 983×435 (2×) | 2× | 0.95 | **정상 13 px** (`termfont-native.png`) |
| override DPR 2, 1440×900 | 2 | 2220×930 / 1110×465 (2×) | 2× | 0.95 | **정상 13 px** (`termfont-dpr2.png`) |
| override DPR 1, 1440×900 | 1 | **2072×930 / 1036×465 (2×)** | 1× | 0.89 | **절반 크기, 좌하단에 그려짐** (`termfont-dpr1.png`) |

- 원래 문제 화면: `final/two-uart-1440.png`(DPR 1 실행)와 baseline `input-*.png`. 글자 폭 ≈3.8 px. 13 px Menlo의 ≈7.8 px 셀 대비 1/2이다.
- 업스트림 가설(Canvas ParseFont가 앞 공백 font 문자열 거부) 측정: addon 문자열은 `` `${italic} ${weight} ${size*dpr}px ${family}` ``다. Ego Chromium 156(`Chrome/156.0.0.0`, DPR 1·2 모두)에서 `" normal 26px ui-monospace, Menlo, Consolas, monospace"`는 **accepted**였고 `26px ui-monospace, …`로 정규화됐다. 앞 공백 없는 문자열과 측정 폭도 같았다(W 15.7 px). → 이 환경에서는 재현되지 않는다. 메인 판단(xterm font patch 미적용)과 같다.
- 결론: CDP DPR-1 override가 호스트(DPR 2) 위에서 WebGL canvas backing을 2×로 남긴다. 그래서 link layer(1×)와 비율이 어긋난다. 제품 버그로 보지 않는다. final 시각 판정은 DPR 2 override 또는 native 화면으로 한다. (낮은 우선순위 관찰: 실제 사용자가 창을 Retina↔비Retina 모니터로 옮기는 DPR 변경 경로는 별도 확인 대상이다. 이번 범위에서 제품 변경은 요청하지 않는다.)

### FINAL run 1 (frozen `final` 빌드, DPR 2) — `final/final-061713.out`: **RESULT FAIL, 50 PASS / 5 FAIL**
| 영역 | 결과 |
|---|---|
| 빈 기본 세션: 가짜 dut/ch0/bcm 패널 없음, welcome 표시 | PASS (`empty-1440.png`) |
| 1024/768/390 가로 스크롤 없음 + STOP 화면 내 (빈 세션·다중 채널·STOP 후) | PASS 전부 (`empty-*.png`, `multi-*.png`, `stop-390.png`) |
| 연결 대화상자 Escape 닫힘 | PASS (`connect-dialog-1440.png`) |
| **UI로** UART dut/aux 추가 → explorer `연결됨` | PASS (`two-uart-1440.png`) |
| 채널별 입력: aux `root` → aux만 `Password:`(dut E_TIMEOUT), dut 입력 → dut, 둘 다 shell 로그인 | PASS |
| 채널별 데이터: agent `echo qa-dut-only` → dut 일치, aux E_TIMEOUT | PASS |
| 재클릭 4회: 탭 1개만 유지하고 그 탭에 포커스 | PASS (`reopen-aux-1440.png`) |
| 채널별 보기 목록(터미널/트레이스) | PASS |
| UI로 CAN ch0 / UDS ecu / ETH cap 연결 → 모두 `open` (baseline finding 1 해소) | PASS |
| 5개 패널 모두 별개 탭(중복 없음) | PASS (`multi-domains-1440.png`) |
| UDS ecu 패널: 연결 후 열어도 `ecu ● sim DoIP 0x1001 …` 표시 (baseline finding 2 해소) | PASS |
| 검색 `aux` → `uart/aux`만 남음 | PASS (`search-aux-1440.png`) |
| 키보드 포커스 링 `solid 2px rgb(133,184,255)` (finding 11 해소) | PASS (`focus-1440.png`) |
| 명시 저장 `qa-bench` → 선택(`#/l/qa-bench`) → reload → alias 탭 유지 | PASS (`saved-layout-reload-1440.png`) |
| **late-join explorer 상태** | **FAIL**: 두 번째 탭 explorer가 `uart/aux:unknown uart/dut:unknown can/ch0:unknown eth/cap:open`이고 `uds/ecu`는 **목록에 없다** |
| late-join `uds/ecu` 열기 | **FAIL** (선택자 0개) |
| late-join UART dut/aux 별개 탭 + aux 입력이 aux에만 도달 | PASS (`late-join-1440.png`) |
| STOP(390): 버튼 화면 안, agent `uart.send`/`can.send` → `E_STOPPED` | PASS (`stop-390.png`, `stop-1440.png`) |
- 실패 5건은 메인이 진단한 nested native snapshot parser(`core.channels.state.channels`, `uds.snapshot.state:{kind:uds.state,payload}` 무시) 증상과 같다. wF ui-latejoin 수정 번들로 strict 재실행한다.
- 첫 시도 `final/final-*.out`(DPR 1, 수정 전 스크립트)은 원본 그대로 보존했다. late-join `uds/ecu` 클릭에서 중단됐다.
- 검사 한계: "uds ecu panel" 검사의 `textContent`에는 숨은 STOP 문구 노드가 섞여 있었다. 화면(`multi-domains-1440.png`)에는 STOP 배너가 없다. 판정은 '대상 없음' 부재 기준이라 유효하다.

### P1 canonical suite (5개) — **4/5, UART 실패 = 회귀**
- 명령: `cd ui && LOOM_REAL_SMOKE=1 LOOMD_BIN=$HOME/.cache/loom-main-target/debug/loomd LOOM_TC1_TEST_BIN=/Users/ctmctm/workspace/Loom/target/debug/deps/tc1_live_smoke-9d0e6bf51cd9cc4e PLAYWRIGHT_JSON_OUTPUT_NAME=<ev>/pw-results.json pnpm exec playwright test -c e2e/real-smoke.config.ts --reporter=list,json --output=<ev>/pw-out` (config 무수정, 기준 완화 없음). TC-1 bin sha256 `2993478f3c80d1c8d208512b3af5ee175551958e047346629f46acc0eeed9027`. Playwright 1.65.0 headless 기본 viewport.
- 결과(`p1-suite/pw-run.log`, `pw-results.json`): expected 4 / unexpected 1 / skipped 0 / flaky 0, 25.1 s, exit 1.

| # | 테스트 | 결과 |
|---|---|---|
| 1 | UART agent trace, browser input, STOP | **FAIL** — `.uart-trace` 요소 없음 (5 s) |
| 2 | CAN agent frames, browser send, STOP | PASS, **p95 = 97.0 ms < 150** |
| 3 | UDS agent transaction, browser request, STOP | PASS |
| 4 | Ethernet list/detail/send/STOP | PASS |
| 5 | 실제 powered TC-1 live + journal verdict | PASS |

- **원인(관측)**: 테스트는 딥링크 `#/p/uart/trace?channel=dut`를 먼저 연 다음 agent `uart.open`을 호출한다. 통합 빌드에서는 채널이 열리는 순간 '새 채널 패널 자동 열기'가 같은 그룹에 `UART 터미널 · dut`를 **추가하고 활성화한다**. 그러면 dockview가 숨은 trace 탭 내용을 떼어 내서 `.uart-trace`가 DOM에서 사라진다. 페이지 스냅샷(`p1-suite/pw-out/real-smoke-real-UART-…/error-context.md`)의 tablist는 `UART 트레이스 · dut`, `UART 터미널 · dut [selected]`, status `UART 터미널 · dut opened`다. baseline(P1 최종 PASS)에서는 같은 채널의 패널이 이미 열려 있으면 자동 열기를 하지 않았다. 지금은 패널 종류(slot)별로 구분하므로 터미널을 추가로 연다.
- 판정: **제품 회귀**로 본다. 수용 기준인 "Existing deep links … remain compatible"과 "no implicit … on selection"에 어긋난다. 사용자가 딥링크로 고른 보기를 자동 열기가 가려 버린다. 테스트 쪽 완화는 하지 않았다.
- 메인 요청(소유: interfaces — `store/channels.ts`/`dock/*` 자동 열기 조건): 채널이 열릴 때 그 채널의 **어떤 보기든** 이미 열려 있으면 자동 열기를 생략하라(또는 추가하더라도 활성 탭을 바꾸지 마라). 수정 후 P1 suite를 다시 돌린다.
- **메인 수신(후속)**: 자동으로 열린 장치 패널은 이제 비활성/배경으로 추가된다. 딥링크 trace와 키보드 대상은 유지되고, explorer/추가 동작에서만 활성화된다. UART 헤더 native 상태/설정과 late-join input-owner 메타데이터는 새 snapshot API로 hydrate된다. 최종 번들에서 P1 5개를 다시 돌린다. 이전 4 PASS / 1 FAIL 원본(`p1-suite/`)은 보존한다. 재실행 결과는 `p1-suite-final2/`에 둔다.

## FINAL2 (수용 판정 빌드) — **PASS**

### 빌드 식별
- 메인 FINAL2 GO: 소스 freeze, 집계 check PASS(schema 33 / type / lint / 428 tests, 40 files), build PASS. Sol native `core.channels` + UDS snapshot 수정, UART header/snapshot metadata, 비활성 auto-open 포함.
- `ui/e2e/workbench-ego/freeze.sh final2 ui/dist target/debug/loomd` → `~/.cache/loom-ui-workbench/final2/`:
  - `dist/assets/index-Ecu5RWJd.js` `8b0c72ae21c1765d80bfd3179e41b1d7ce445512a4420a5cfa96e562c98e1e17`
  - `dist/assets/index-NLivQMYM.css` `8780e762237b9121a6297856ff2fd41b96556a5c01f7f020e03c4b36ee9f50d4`
  - `dist/assets/net.worker-BleoIIWk.js` `98ccabf8bc6b7858fb0e9d8e9787d392cbf43c2160d01ea31372ef49845e65a4`
  - `dist/index.html` `4ee58fe2495e6394015a679f62208601f74f41c60372847d3445b5aed6d3250c`
  - `loomd` (`target/debug/loomd` 06:20) `1abbb3ad512bb3b875e739e1d7f9060f7c09c7a02f6575fa970f41e3dcce2917`
  - TC-1 `target/debug/deps/tc1_live_smoke-9d0e6bf51cd9cc4e` `2993478f3c80d1c8d208512b3af5ee175551958e047346629f46acc0eeed9027`

### Strict Ego final2 — `cd ui && e2e/workbench-ego/run.sh final2 final` → **RESULT final PASS, 61/61 checks, runner rc=0**
- 원본: `evidence/ui-workbench/final2/final-062029.out`, `harness-final-062029.log`. DPR 2 override(호스트 원래 비율). 판정 기준은 run 1과 같고, late-join UART header 검사 1개를 더했다.
- run 1에서 실패한 5개가 모두 PASS로 바뀌었다. late explorer는 `uart/aux:open uart/dut:open can/ch0:open uds/ecu:open eth/cap:open`이다. late 탭 aux header `UART aux ● open 115200 gen1 [SIM]`(이전 `?`) PASS, late aux 입력이 aux에만 도달 PASS.
- 나머지(빈 세션 가짜 패널 없음, 3개 폭에서 h-scroll 없음 + STOP 화면 안, UI 연결 5개, 채널별 입력/데이터 분리, 재클릭 단일 탭 포커스, 별개 패널 5개, UDS hydrate, 검색, 포커스 링, 명시 저장→선택→reload, STOP 후 agent UART/CAN `E_STOPPED`)는 run 1과 같이 PASS다.

### Canonical P1 final2 — `ui/e2e/workbench-ego/p1.sh final2 <tc1-bin>` → **5/5 PASS, exit 0**
- `p1-suite-final2/pw-run.log`, `pw-results.json`(expected 5 / unexpected 0 / skipped 0 / flaky 0, 19.0 s), `inputs.sha256`, `pw-out/*/*.png`. 저장소 `ui/dist`가 frozen final2와 바이트 단위로 같음을 확인한 뒤 실행했다. config 무수정, 기준 완화 없음.
- UART 1.6 s(command→visible 151.5 ms, 상한 측정), CAN 2.3 s(**p95 82.9 ms < 150**, command→visible 15.7 ms), UDS 0.9 s(94.7 ms), ETH 1.1 s(106.2 ms), TC-1 12.0 s.
- run 1의 UART 딥링크 회귀(자동 열기가 trace 탭을 가림)가 해소됐다. 이전 4/1 원본은 `p1-suite/`에 보존했다.

### 시각 리뷰 (final2, 비차단 polish)
- 좋음: 빈 세션 welcome(연결→보기→작업 공간 3단계, STOP 안내). 도메인 그룹 explorer에 텍스트 상태 `연결됨`과 보기 버튼이 있다. 1440 DPR 2 터미널 13 px는 읽기 쉽다(`two-uart-1440.png`). 비활성 탭 대비가 좋아졌다. `패널 동작` 메뉴로 설정을 진행형 공개해 sub-bar 과밀이 해소됐다(finding 6). 390에서 가로 스크롤이 없고 STOP 버튼이 늘 보인다.
- 남은 polish(낮음, 차단 아님):
  1. 연결할 때마다 토스트가 2개씩 쌓인다("…다시 열었습니다 — 패널을 열었습니다", "…연결 요청을 보냈습니다…"). 2 UART면 4개가 터미널 오른쪽 아래를 몇 초 가린다(`two-uart-1440.png`).
  2. 768에서 UDS 콘솔 헤더 열 `요약`이 세로로 줄바꿈되고 `제어: 비어 있음`도 줄바꿈된다. 탭 제목 `UDS…`가 잘린다(`multi-768.png`).
  3. 390 + STOP: STOP 패널은 스크롤 가능한 compact 형태다. 그러나 explorer가 workspace 위로 쌓여서 데이터 영역이 아주 작다(`stop-390.png`). 좁은 폭에서는 explorer 접기(drawer)를 권장한다.
  4. 빈 세션에서 explorer의 "채널 없음" 안내와 welcome 본문이 내용상 겹친다(경미).
  5. Activity 줄 `AGT … call(s) in the last second`처럼 영문/한글이 섞여 있다(기존).

## 정리 증명 (ui-qa 소유 자원)
- Ego TaskSpace 2(`loom ui-workbench qa`, agent 소유, p1/p2 모두 agent가 엶) `finish({keep:[]})` → `{"spaceId":2,"closedSpace":true,"keptManagedLabels":[],"closedManagedLabels":["p1","p2"],"preservedUnmanagedCount":0}` (`final2/ego-finish.out`). 이후 `listTaskSpaces()`에 같은 이름 space 없음. 다른 space는 건드리지 않았다.
- 자기 harness/loomd/TC-1/Playwright 프로세스 0개(`pgrep`). `$TMPDIR/loom-ui-workbench-*` 임시 HOME 0개(harness close가 삭제). control 파일 0개(run.sh trap이 삭제, mktemp 디렉터리 rmdir).
- 증거 secret 스캔(`#boot=` 원문 / `"token"` / `x-ctl`): `docs/impl/main/evidence/ui-workbench/` 일치 0. 스크립트 안의 `x-ctl`은 헤더 이름 리터럴일 뿐 값이 아니다.
- 보존(삭제하지 않음): `~/.cache/loom-ui-workbench/{baseline,final,final2}` 고정 빌드 사본(재현용), baseline 당시 cache 스크립트 사본(사용 중지, 메인 지시로 보존), 모든 실패 원본 출력.
- 이 담당이 만든 파일(커밋 후보, 메인 판단): `ui/e2e/workbench-harness.ts`, `ui/e2e/workbench-ego/{run.sh,freeze.sh,p1.sh,prelude.mjs,probe,baseline,baseline-input,final,termfont}.body.mjs`, 이 보고서, `docs/impl/main/evidence/ui-workbench/{baseline,final,final2,p1-suite,p1-suite-final2}/`. 구현(소스) 파일은 수정하지 않았다. commit/push 없음.
- workspace wE는 메인이 보고서를 인수하고 정상 종료/close할 때까지 유지한다.

## 다음 행동
메인: 보고서 인수 → 커밋 범위 결정(evidence 크기: `pw-out` PNG 포함) → wE 정상 종료/close.
