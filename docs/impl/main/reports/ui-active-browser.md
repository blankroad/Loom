# Active-interface workbench — 브라우저 조합 매트릭스 (ui-combo-qa, ownership 3)

담당: Claude Code Opus 5.5 foreground (wH:p1). 하위/백그라운드 agent 없음. 제품 소스 수정 없음(읽기 전용 리뷰).
상태: **READY_FOR_REVIEW — FINAL active-final3: strict 조합 매트릭스 440/440 PASS (기본 작업 공간 ≤1 dock group 검사 30건 포함) · canonical P1 5/5 PASS · 정리 완료**. 이전 실행(active-final all5 PSU FAIL, active-final2 410 PASS)은 원본 보존된 이력.

## 소유 산출물
- `ui/e2e/workbench-ego/combinations.body.mjs` (신규). 기존 `run.sh`/`prelude.mjs`/`workbench-harness.ts`/`freeze.sh`/`p1.sh`는 수정 없이 재사용. `node --check`(prelude 결합) + eslint PASS.
- 증거: `docs/impl/main/evidence/ui-workbench/active-*/`, `p1-suite-active-*/`.

## 매트릭스 설계 (실제 daemon, mock 없음)
- 케이스마다 **새 own loomd**(harness `/close`→`/start`, 새 origin → localStorage 초기화). 장치는 agent RPC로 실제 sim 장치를 연다: `uart.open sim://bootshell`, `can.open`, `eth.capture.start backend=sim`, `uds.connect sim=bench`, `psu.connect`. 닫기: `uart.close`, `can.close`, `eth.capture.stop` (UDS/PSU는 disconnect RPC 없음 → 전환 대상 아님).
- 별칭은 일부러 레거시 이름이 아닌 `mcu`/`lnx`(UART 2개), `pt`(CAN), `mirror`(ETH), `gw`(UDS), `rail`(PSU). 따라서 dut/ch0/bcm/lan1/bench 같은 지어낸 대상은 "활성 집합 밖 탭"으로 바로 잡힌다.
- 케이스: `uart`(2 alias), `can`, `eth`, `uart-eth`, `eth-can`, `uart-can`, `all3`(UART 2 + CAN + ETH), `all5`(+UDS +PSU, PSU output off 상태).
- 엄격 판정(`expectActive`): DPR 2; explorer 기본 목록(비활성 disclosure 접힘) = 활성 집합과 정확히 일치 + 모두 `open`; 도메인 그룹 정확히 일치; 헤더 수 = 활성 수; 활성 집합 밖 탭 0; 활성 장치마다 보기 1개 이상; placeholder/`대상 없음` 0; welcome은 활성 0일 때만; STOP 화면 안; 가로 스크롤 없음.
- 레이아웃 선택지(`exerciseLayouts`): disabled 선택지 0, 테스트 세션 없으면 `test` 선택지 없음, 제공된 프리셋을 하나씩 실제 선택해 결과 탭이 비어 있지 않고 활성 장치/적격 도메인만인지, 그 뒤 기본 작업 공간 복귀 검증. 새 패널 hydrate는 **최대 8초 bounded 대기**(조건 미충족 시 FAIL; 고정 타이머 아님).
- 전환(한 daemon, live): 빈 화면 → +UART mcu → 터미널 포커스 후 +CAN pt(자동 추가가 탭/키보드 포커스를 뺏지 않음) → CAN 송신 입력에 **미전송 초안** `0x123#11 22`(Enter 없음) → +ETH → −UART(탭 제거, 기본 목록 제외, "비활성 1개" 접힘 disclosure, 펼치면 `uart/mcu:closed` 이력, 탭 추가 없음) → 4초 heartbeat → 재연결(단일 탭) → +UART lnx → 레이아웃 저장 `qa-active` → −CAN 후 저장 레이아웃 선택(사용 가능한 칸만, localStorage 원문 불변) → CAN 재연결 시 칸 복귀 → −ETH → reload → 두 번째 viewer late-join → 양쪽에서 −UART lnx → late 탭 딥링크 `#/p/uart/terminal?channel=mcu`는 단일 대상, 이후 ETH 재연결이 딥링크에 탭 추가 안 함 / 기본 작업 공간에는 추가 → 390 px STOP, agent `uart.send`/`can.send` = `E_STOPPED`.
- CAN 인스턴스 동일성(DOM 요소 identity)과 초안 값을 ETH 열기·UART 닫기·heartbeat·UART 재연결 후마다 확인.
- 렌더 fence: 측정/스크린샷 전 **2×requestAnimationFrame 콜백 fence**(프레임 콜백 실행 증거; 물리적 paint 증명은 아님). 6초 안에 끝나지 않으면 `2xrAF fence <label>` FAIL check로 기록(조용한 통과 없음). 실행: native DPR 2 override (`Emulation.setDeviceMetricsOverride deviceScaleFactor:2`, 이 호스트 원래 비율).

## active-final 결과 (메인 FINAL GO, 고정 사본)
- `freeze.sh active-final ui/dist target/debug/loomd` (repo `ui/dist`와 diff 없음 확인):
  - `loomd` `5dae52ae7fc84bfc2bdf7cfbd54b6ce88d8c84e63ba70b3e9f2b8eaccab43194`
  - `dist/assets/index-CPlHtExe.js` `2c74ca3b9ea22a266aaf0c0a6d53a13e5f3729bd286603f56f58e55944d2c15b`
  - `dist/assets/index-DdjG3pEn.css` `15770b862b51ec5c871619df9b2d2de9ef9feda59d7f51d16831d25055a8445c`
  - `dist/index.html` `27da5e1ca679ebb1aeb61474657d05198b71da831121c31b601c7f058601a8df`
  - `dist/assets/net.worker-BleoIIWk.js` `98ccabf8bc6b7858fb0e9d8e9787d392cbf43c2160d01ea31372ef49845e65a4`
- Ego Lite, TaskSpace `loom ui-workbench qa` (spaceId 3).

| 실행 | 결과 |
|---|---|
| `active-final/combinations-063315.out` (1차) | FAIL 10: all5 PSU 9 + all5 `can-uds` 1(`대상 없음`, 500 ms 고정 대기) |
| `active-final/combinations-063608.out` (bounded hydration 대기로 수정 후, 같은 빌드) | **399 PASS / 9 FAIL** — 모두 all5 PSU. `can-uds` 수렴 PASS(`noTarget:false`). 2×rAF fence 실패 0회 |
| `p1-suite-active-final/` canonical 5 (`p1.sh active-final /Users/ctmctm/workspace/Loom/target/debug/deps/tc1_live_smoke-9d0e6bf51cd9cc4e`) | **5/5 PASS** (UART 1.6 s, CAN 2.3 s, UDS 0.93 s, ETH 0.92 s, TC-1 11.9 s), exit 0. TC-1 bin sha256 `2993478f…eed9027` |

부분집합별 (063608):

| 케이스 | 기본 탭 | 제공 레이아웃 | FAIL |
|---|---|---|---|
| uart | UART 터미널 · lnx, · mcu | m1-default, uart-only | 0 |
| can | CAN 트레이스 · pt | m1-default, can-only | 0 |
| eth | 이더넷 패킷 · mirror | m1-default, eth | 0 |
| uart-eth | UART · mcu, 이더넷 · mirror | m1-default, uart-only, eth | 0 |
| eth-can | 이더넷 · mirror, CAN · pt | m1-default, can-only, eth | 0 |
| uart-can | UART · mcu, CAN · pt | m1-default, uart-only, can-only | 0 |
| all3 | UART lnx/mcu, 이더넷 mirror, CAN pt (+768/390 STOP·가로 스크롤 PASS) | m1-default, uart-only, can-only, eth | 0 |
| all5 | UDS gw, UART mcu, 이더넷 mirror, CAN pt — **PSU rail 없음** | m1-default, uart-only, can-only, uds-only, can-uds, eth (psu-only 없음) | 9 (F1) |

전환/저장/reload/late-join/딥링크/STOP: 전 항목 PASS (빈 화면 welcome, 자동 추가가 포커스 탭·xterm 입력 포커스 유지, CAN 인스턴스+초안 유지 4회, 비활성 disclosure, 저장 레이아웃 필터 적용/원문 불변/재연결 복귀, reload, late-join 정확 집합, 딥링크 단일 대상·자동 추가 없음, STOP E_STOPPED 2건).

### F1 — PSU 연결 상태 진실성 (네이티브 결함, 메인 확인·수정 진행)
- 실제 `psu.connect {ch:"rail"}` 응답은 `connected:true, out:false`. 그러나 UI는 `psu/rail`을 비활성(disclosure "비활성 인터페이스 1개")로 분류 → 기본 목록·그룹·헤더·보기·`psu-only` 선택지 누락.
- 원인(메인 root 확인): `crates/loom-psu/src/worker.rs` `state_payload()`가 `psu.state` 이벤트/스냅샷에 `connected`를 넣지 않음(`psu.status`만 별도 추가). 연결 증거가 없으므로 UI의 `unknown`은 정직한 판정이고, 출력 off(`out:false`)로 연결을 추론하지 않는 것도 맞다. UI 스크립트 매핑 문제가 아님.
- 수정: 네이티브가 장치가 있는 동안 `connected:true` 명시(PSU 담당 Sol, 메인 리뷰). → **active-final2에서 해소 확인**(all5 0 FAIL).

### 관찰 (낮음)
- UDS 콘솔은 새로 mount될 때 잠시 `대상 없음`을 보였다가 실제 대상(gw)으로 수렴(<8 s, 1차 실행 500 ms 시점에 포착). 기능 결함은 아니고 짧은 깜빡임.

## active-final3 결과 (최종 — 기본 그룹 fallback 수정 후)
- 배경: 메인 시각 리뷰에서 active-final2 `all5-1440.png`가 활성 그룹이 없을 때 백그라운드 도착 패널마다 도메인별 새 그룹을 만들어 5개 좁은 열로 나뉜 것을 발견했다(기존 검사는 탭 집합만 봐서 통과). 메인이 `DockHost.open` fallback을 `activeGroup ?? groups[0]`로 수정.
- 매트릭스 추가 검사: `snap.dvGroups` = `[data-testid=layout] .dv-groupview` 수. `expectActive`가 route `#/l/m1-default`(또는 boot)일 때 `default workspace max 1 dock group`(≤1)을 강제. 사용자 저장 레이아웃은 의도적 분할이 가능하므로 대상 아님.
- 고정 사본 `freeze.sh active-final3 ui/dist target/debug/loomd` (repo `ui/dist` diff 없음, `loomd` cmp 동일, 메인 GO 해시와 일치):
  - `loomd` `87aa3a504fe5d517353295a73a0fd2e9994070ce94d9818db8030cdeff727cbe`
  - `dist/assets/index-B3bYyU1u.js` `f49125c57cc872551f9388b2c21b70b0088a82a6fafda1ba443c1de575ecd24a`
  - `dist/assets/index-DdjG3pEn.css` `15770b862b51ec5c871619df9b2d2de9ef9feda59d7f51d16831d25055a8445c`
  - `dist/index.html` `bc34c33500f5cab892f32b320045544c82a28ec8bc7249f2a3bd62a49d914153`
  - `dist/assets/net.worker-BleoIIWk.js` `98ccabf8bc6b7858fb0e9d8e9787d392cbf43c2160d01ea31372ef49845e65a4`
  - TC-1 `/Users/ctmctm/workspace/Loom/target/debug/deps/tc1_live_smoke-9d0e6bf51cd9cc4e` `f1d32cf1d35ea36eb0900014e9c194f780f4cb8051fb423488add979d69789c7`
- 실행은 절대 경로 단일 명령(`…/run.sh active-final3 combinations`, `…/p1.sh active-final3 <TC-1>`), 출력은 별도 확인.

| 실행 | 결과 |
|---|---|
| `active-final3/combinations-064328.out` | **RESULT PASS — 440 PASS / 0 FAIL**, ego exit 0, 2×rAF fence 실패 0, `leftovers: none`, `ctl removed`, Ego spaceId 4 |
| ≤1 dock group 검사 | **30건 실행·전부 PASS** (dvGroups=1 28건, 빈 bench dvGroups=0 2건). all5 기본: 탭 5개(CAN pt, 이더넷 mirror, PSU rail, UART mcu, UDS gw) 1그룹 (`all5-1440.png` 육안 확인) |
| `p1-suite-active-final3/` canonical 5 | **5/5 PASS** (UART 1.8 s, CAN 2.3 s, UDS 0.89 s, ETH 0.95 s, TC-1 11.9 s), `exit=0` |

스크린샷 23장 (`docs/impl/main/evidence/ui-workbench/active-final3/`): `uart-1440`, `uart-768`, `uart-390`, `can-1440`, `eth-1440`, `uart-eth-1440`, `eth-can-1440`, `uart-can-1440`, `all3-1440`, `all3-768`, `all3-390`, `all5-1440`, `live-empty-1440`, `live-uart-can-1440`, `live-uart-closed-1440`, `live-inactive-disclosed-1440`, `live-reopen-1440`, `saved-filtered-1440`, `reload-1440`, `late-join-1440`, `late-deeplink-1440`, `stop-390`, `stop-1440` (.png). 실행 출력 `combinations-064328.out`, harness 로그 `harness-combinations-064328.log`, `ego-finish.out`.

## active-final2 결과 (이력 — PSU 네이티브 수정 후)
- `freeze.sh active-final2 ui/dist target/debug/loomd` (repo `ui/dist` diff 없음, `loomd` cmp 동일). UI는 active-final과 바이트 동일, loomd만 새 빌드:
  - `loomd` `e64bc166ea7f3d4d0aea9394bb0ddbb196b28563510e7657806d97e936ab94a5`
  - `index-CPlHtExe.js` `2c74ca3b9ea22a266aaf0c0a6d53a13e5f3729bd286603f56f58e55944d2c15b`, `index-DdjG3pEn.css` `15770b86…8445c`, `index.html` `27da5e1c…01a8df`, `net.worker-BleoIIWk.js` `98ccabf8…65a4` (위 active-final 해시와 동일)
  - TC-1 테스트 실행 파일(같은 이름, 재빌드) `/Users/ctmctm/workspace/Loom/target/debug/deps/tc1_live_smoke-9d0e6bf51cd9cc4e` sha256 `f1d32cf1d35ea36eb0900014e9c194f780f4cb8051fb423488add979d69789c7`

| 실행 | 결과 |
|---|---|
| `active-final2/combinations-063914.out` (`run.sh active-final2 combinations`) | **RESULT PASS — 410 PASS / 0 FAIL**, 2×rAF fence 실패 0, 스크린샷 23장, `leftovers: none`, `ctl removed` |
| `p1-suite-active-final2/` (`p1.sh active-final2 <절대 TC-1 경로>`) | **5/5 PASS** (UART 1.8 s, CAN 2.4 s, UDS 0.97 s, ETH 0.91 s, TC-1 12.8 s), exit 0 |

- all5: explorer `uart/mcu, can/pt, uds/gw, eth/mirror, psu/rail` 모두 `open`, 기본 탭 5개(`벤치 전원 (PSU) · rail` 포함), PSU output off(`out:false`) 상태에서도 연결됨으로 표시, 레이아웃 선택지 `m1-default, uart-only, can-only, uds-only, psu-only, can-uds, eth` 모두 활성 장치만·비어 있지 않음, `can-uds` UDS hydrate bounded 수렴.
- 나머지 7개 부분집합·전환 시퀀스는 active-final과 같은 결과로 전부 PASS.

## 읽기 전용 구현 리뷰 (Sol diff: routes.ts / DockLayout.tsx / dockHost.ts / layoutModel.ts / services.ts / shellControl.ts)
- PRESETS는 별칭 없는 정의 + `presetForChannels`가 OPEN 채널로 해석, `activePresets`는 필요한 도메인이 모두 열린 프리셋만 제공(test는 세션 있을 때만). 브라우저에서 확인됨.
- `DockHost.reconcile`은 shell store emit마다 실행되지만 살아 있는 패널은 제거/재생성하지 않고 활성 패널을 복원 → heartbeat·무관한 open/close 후 인스턴스/초안/포커스 유지가 브라우저로 확인됨.
- 딥링크 route는 reconcile 없이 단일 slot 로드, services 자동 열기도 panel route에서는 비활성 → 딥링크 자동 추가 없음 확인.
- `shellControl`: stream_epoch 변경 시 open→unknown 강등(이전 bench 연결을 증명 없이 유지하지 않음) — 타당.
- 차단 이슈 없음.

## 한계
- UI InterfaceConnect 대화상자 경로는 이 매트릭스에서 쓰지 않았다(장치는 agent RPC; 이전 FINAL2에서 UI 연결 검증됨).
- UDS/PSU close/reopen은 daemon에 disconnect RPC가 없어 미검증. 31개 전체 부분집합은 단위 테스트(메인/Sol) 담당, 브라우저는 8 케이스.
- 명시 `#/l/test` route(세션 없음)와 테스트 세션 있는 경우는 브라우저 미검증.

## 정리
- 매 실행 `run.sh` trap: 자기 harness/loomd 종료, control 파일 삭제, `leftovers: none`, `ctl removed`. 임시 HOME(`loom-ui-workbench-*`) 잔존 없음.
- 중단된 baseline mechanics 실행 `active-baseline/combinations-063144.out`(빈 출력, 부분 스크린샷)은 원본 보존. harness 로그상 마지막 `/start` 뒤 trap 정리됨, 잔존 프로세스/HOME 없음 확인. 수용 증거로 쓰지 않는다.
- 다른 세션의 `.worktrees/core/target/debug/loomd` 프로세스는 건드리지 않았다.
- Ego TaskSpace 3(`loom ui-workbench qa`, agent 소유) `finish({keep:[]})` → `{"spaceId":3,"closedSpace":true,"keptManagedLabels":[],"closedManagedLabels":["p1"],"preservedUnmanagedCount":0}` (`active-final2/ego-finish.out`). late-join 페이지 p2는 시나리오 `finally`에서 이미 닫힘. 다른 space는 건드리지 않음.
- Ego TaskSpace 4 (active-final3) `finish({keep:[]})` → `{"spaceId":4,"closedSpace":true,"keptManagedLabels":[],"closedManagedLabels":["p1"],"preservedUnmanagedCount":0}` (`active-final3/ego-finish.out`). 이후 자기 harness/loomd 프로세스·임시 HOME 잔존 없음 재확인.
- 고정 사본 `~/.cache/loom-ui-workbench/active-{baseline,final,final2,final3}/`는 재현용으로 보존(덮어쓰기 없음).
