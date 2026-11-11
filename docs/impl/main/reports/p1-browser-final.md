# P1 실제 브라우저 최종 검증 — 보고

담당: Claude Code Opus 5.5 foreground (기능 담당), 하위 agent/백그라운드 에이전트 미사용. 과업: `docs/impl/main/tasks/p1-browser-final.md`.
상태: **PASS — 목표1 Playwright probe PASS / 목표2 Ego ETH PASS / 목표3 real-smoke 5/5 PASS (CAN p95 83.4 ms < 150)**.

## 실행 기준
- HEAD `d7638ac2b060f54b55a54d1507ecbb3e965386e4` (`fix(ui): consume native RPC envelopes and reject domain failures`). 그 위 `399a8b6` TC-1 Env::Sim 보완 포함.
- working tree 코드 diff: `ui/e2e/real-smoke.spec.ts` 기존 선택자 수정만 (diff sha256 `4c5cef31…92fc4`). 이번 세션의 spec 추가 수정은 없다. 그 외 src/native/Cargo/lock/config 수정 없음.
- dist (메인 `pnpm build` exit 0): `ui/dist/assets/index-BExAuFCo.js` sha256 `96b1d2c732ebad9b495b5a5b6a6a6ecd43bbdcda964d00af11b197e0b4af4318`, `index.html` sha256 `ac17ffe00202b87fb727ad14961e0e1ee8cd8530e63cc4ed7bbdded574bdd4bd`. 실행 직전 재확인 일치.
- 바이너리 (`$HOME/.cache/loom-main-target/debug`, 05:21 재빌드, 실행 직전 재확인):
  - `loomd` sha256 `eb6d03bee312c62e4adb2bf813bf2aca24b255542f9bbb5ea57953954598892a`
  - `deps/tc1_live_smoke-e4c707f7a9513da7` sha256 `55463bee0b4079f22c1e07b48682a1fe3a248963f6bbf3a8a5aae858543c46c4`
- Playwright `@playwright/test` 1.65.0, Chromium 158.0.8127.18 (chromium-1254, `~/.cache/ms-playwright`).
- 증거 디렉터리: `~/.cache/loom-p1-browser-final/`.

## 목표1 — Playwright 최소 launch probe: PASS
- 승인 경로: 기존 Claude Code auto mode의 일반 Bash 도구 승인 흐름 그대로. 명령 1회:
  `cd ui && node ~/.cache/loom-p1-browser-final/probe.cjs "$PWD"`.
  거부나 추가 프롬프트 없이 실행됐다. 하지 않은 것: permission/sandbox 설정 해제, `--dangerously-skip-permissions`, 임의 CDP 브리지, 브라우저 보안 flag 변경. probe는 `chromium.launch()` 기본 옵션(headless, 추가 args 없음)만 쓴다.
- Ego root CUA 거부는 그대로 존중했다. Computer Use/AppleScript는 사용하지 않았다.
- 결과 (`probe.out`): `{"version":"158.0.8127.18","headless":true,"launchToResultMs":1565,"visibility":"visible","hasFocus":true,"rafN":60,"rafMedian":16.7,"rafP95":16.7,"rafMax":16.8}`.
- 환경 기록: Playwright headless의 frame은 active·visible이다. RAF cadence는 60 Hz로 안정적이며 1 s 정지가 없다. Ego 창에서 측정된 RAF 1 s 정지와 대비된다.

## 목표2 — Ego TaskSpace1 ETH 재검증: PASS
- 경로: 이전 `ego-harness.mts`(RealSmoke import, 127.0.0.1 랜덤 포트, 0600 control 파일)와 `ego-browser nodejs`를 썼다. TaskSpace `loom p1 browser acceptance`(spaceId 1, agent 소유)를 재사용했다. fresh isolated HOME own daemon을 썼고, 기존 demo는 건드리지 않았다. 부트 URL은 `<redacted>` 처리했다.
- 실행: `~/.cache/loom-p1-browser-final/run-ego.sh eth` (출력 `eth-052541.out`, harness 로그 `harness-eth.log`).
- 결과:
  - rights `operate`. `eth.capture.start`/`eth.send` ok. agent→목록 표시 1461 ms (Ego, 참고값).
  - 목록에서 agent와 device가 분리된다: `22|agent|… ARP Who has 192.168.0.2?`, `14/25/26|device|DoIP Vehicle announcement`, `27|device|SOME/IP-SD`.
  - 상세: `eth:cap@seq:22 · 상세 14 ms`. 트리 `Frame 22: 42 bytes` → `Ethernet II, Src 02:00:00:00:00:01, Dst ff:ff:ff:ff:ff:ff` → `Address Resolution Protocol (op 1)` … hex 표시. **`편집 후 송신…`이 활성화됐다** (이전 빌드에서는 비어 있고 disabled였다. SDK envelope unwrap 수정이 확인됐다).
  - 편집 미리보기 `ARP · Who has 192.168.0.2? Tell 192.168.0.1` → 보내기 → user 행 `seq 39 ARP Who has…`와 USR 송신 기록 `보냄 0.1s`.
  - STOP 패널 표시. 이후 agent `eth.send`는 `E_STOPPED`("STOP is latched — ask the user"), `meta.latch:true`.
- 화면: `eth-agent-live-final.png`, `eth-detail-treehex-final.png`, `eth-browser-detail-send-stop-final.png`.
- 새로 드러난 제품 오류: 없음. 이 시나리오에서 SDK reject로 인한 숨은 실패는 나타나지 않았다.

## 목표3 — canonical real-smoke.config.ts 전체 5개: 5/5 PASS
- 명령 (config 무수정, 기준 완화 없음):
  ```sh
  cd ui && LOOM_REAL_SMOKE=1 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=$HOME/.cache/loom-main-target \
    LOOMD_BIN=$HOME/.cache/loom-main-target/debug/loomd \
    LOOM_TC1_TEST_BIN=$HOME/.cache/loom-main-target/debug/deps/tc1_live_smoke-e4c707f7a9513da7 \
    PLAYWRIGHT_JSON_OUTPUT_NAME=~/.cache/loom-p1-browser-final/pw-results.json \
    pnpm exec playwright test -c e2e/real-smoke.config.ts --reporter=list,json --output=~/.cache/loom-p1-browser-final/pw-out
  ```
  (첫 시도는 `LOOM_REAL_SMOKE` 누락으로 5개 모두 skip됐다. spec의 opt-in 가드 때문이며, 판정에서 제외한다.)
- 결과 `5 passed (18.5s)`, exit 0 (`pw-run.log`, `pw-results.json`):

| # | 테스트 | 결과 | 시간 | command→visible (상한) |
|---|---|---|---|---|
| 1 | UART agent trace, browser input, STOP | PASS | 1.8 s | 214.3 ms |
| 2 | CAN agent frames, browser send, STOP (+p95 관문) | PASS | 2.3 s | 12.5 ms |
| 3 | UDS agent transaction, browser request, STOP | PASS | 0.9 s | 89.2 ms |
| 4 | Ethernet list, detail, browser send, STOP | PASS | 0.9 s | 94.3 ms |
| 5 | 실제 powered TC-1 live + journal verdict | PASS | 11.8 s | — |

- CAN p95 관문: 기존 정의 그대로다(agent command 시작 → 매칭 device RX DOM → 2RAF, device 응답·전송 오버헤드 포함 보수적 상한). **p95 = 83.4 ms < 150 ms**.
  20 샘플(ms): `61.2, 83.4, 82.3, 82.4, 82.5, 81.5, 83.6, 82.3, 82.5, 82.1, 81.8, 80.4, 82.7, 82.4, 81.5, 82.8, 82.7, 82.5, 82.6, 82.2`.
- screens (`pw-out/<test>/`): `uart-agent-live.png`, `uart-browser-input-stop.png`, `can-agent-live.png`, `can-browser-send-stop.png`, `uds-agent-live.png`, `uds-browser-request-stop.png`, `eth-agent-live.png`, `eth-browser-detail-send-stop.png`, `actual-tc1-journal-verdict.png`.
- Ego 단위 functional PASS를 반복한 근거: runtime이 Ego에서 Playwright fallback으로 바뀌었다. 자동화 권한 경로와 CAN 계측 환경(RAF cadence)도 달라졌다.

## 정리
- 자기 daemon/TC-1 fixture/Playwright chromium 잔존 프로세스 0개 (`pgrep` 확인). control 파일 삭제 확인.
- `run-ego.sh`(ETH 실행 당시)가 `pnpm exec` wrapper pid만 SIGTERM해서 tsx harness node 2개(71869/71876)가 남았다. 수동 SIGTERM으로 정리했다(harness의 shutdown 핸들러 경유, daemon은 이미 `/smoke/close`로 종료된 상태).
- **스크립트 최소 수정 + 수명 확인 (PASS)**: `~/.cache/loom-p1-browser-final/run-ego.sh`의 harness 실행을 `pnpm exec tsx …`에서 `node --import tsx …`로 바꿨다. 이제 `$!`가 harness node pid 자체가 된다. 시나리오 대신 수명만 보는 `lifecycle` 모드를 추가했다(`lifecycle.mjs`: `/smoke/start`·`/smoke/close`만 호출, secret 비출력). 1회 실행 결과(`lifecycle-run.out`):
  ```
  harness pid=73590
  /smoke/start 200 {"ok":true}
  children while running: 73625 loomd
  /smoke/close 200 {"ok":true}
  children after close: none
  harness exit=0 (pid 73590 alive? no)
  leftover own parent/children: none ; ctl.json removed
  ```
  브라우저 5개 성공 시험은 반복하지 않았다.
- 이번 세션이 만든 임시 HOME 7개(Ego ETH 1, Playwright smoke 4, TC-1 1, lifecycle 1)를 삭제했다. 다른 시험의 HOME 23개(smoke 20, tc1 3)는 저널·실패 자료 보존 지시에 따라 남겼다.
- **Ego TaskSpace1 종료**: goal 전체가 끝났으므로 정리했다. `help("TaskSpace.finish")` 확인 → `taskSpace(1)` 조회 결과 ownership `agent`, 탭은 agent가 연 `p1` 1개뿐(사용자 탭 없음) → `finish({keep:[]})`. receipt `{"spaceId":1,"closedSpace":true,"keptManagedLabels":[],"closedManagedLabels":["p1"],"preservedUnmanagedCount":0}` (`ego-finish.out`). 다른 Space는 건드리지 않았다.
- 다른 세션 소유의 `.worktrees/core/target/debug/loomd` 프로세스는 건드리지 않았다.

## 보존된 이전 판정
- Ego CAN RAF 1 s 정지로 인한 p95 device→paint 2087 ms FAIL(환경)은 원본 그대로 유지한다. 이번 CAN 성능 PASS는 Playwright headless runtime에서 측정한 결과다. **Ego 성능 PASS로 보지 않는다**.
- UART/UDS/TC-1 Ego 기능 PASS, CAN Ego 사용자 send/STOP PASS(`p1-opus-browser.md`)는 재실행하지 않았다.

## 메인 확인 (수신)
- 메인이 `pw-results.json`/`pw-run.log`를 확인했다: expected 5 / skipped 0 / unexpected 0 / flaky 0, CAN 20샘플 p95 = 83.40125 ms < 150, 전체 18.5 s PASS. 기본 launch 옵션과 판정 기준이 유지됐고 원본 Ego FAIL이 보존된 것도 확인했다. 추가 성공 검사는 반복하지 않는다.

## 원본 증거 경로
- perf/결과 JSON: `~/.cache/loom-p1-browser-final/pw-results.json` (CAN `can-device-paint-upper-bound` 첨부 = 20 샘플 + p95, 각 `*-command-to-visible` 첨부 포함). 실행 로그 `~/.cache/loom-p1-browser-final/pw-run.log`.
- Playwright screens: `~/.cache/loom-p1-browser-final/pw-out/real-smoke-*/*.png` (9장).
- Ego ETH: `~/.cache/loom-p1-browser-final/eth-052541.out`, `harness-eth.log`, `eth-agent-live-final.png`, `eth-detail-treehex-final.png`, `eth-browser-detail-send-stop-final.png`.
- probe: `~/.cache/loom-p1-browser-final/probe.cjs`, `probe.out`.
- 이전 Ego 원본(FAIL 포함) 보존: `~/.cache/loom-p1-opus-browser/` (`can2-samples.json` 등).

## 재개 설명
- 이 기능은 종료됐다. 재개가 필요하면 위 `pw-results.json`과 이 보고서만 읽으면 된다.
- 재실행 명령은 목표3 절의 명령 그대로 쓴다(`LOOM_REAL_SMOKE=1` 필수). 실행 전 dist/loomd/TC-1 exe hash를 재확인한다.
- Ego 재실행: `~/.cache/loom-p1-browser-final/run-ego.sh <scenario>`. 수정본이라 harness가 node pid로 직접 관리된다. `<scenario>.body.mjs`에 `prelude.mjs`를 붙여 실행하며, `lifecycle` 인자는 수명 확인만 한다. `prelude.mjs`의 control 경로 `C`는 이 세션 scratchpad로 고정돼 있으므로 새 세션에서는 `C`와 `run-ego.sh`의 `C`를 함께 바꾼다. TaskSpace1은 닫혔다. 새 goal에서는 `taskSpace("loom p1 browser acceptance")`가 새 agent space를 만든다.
- 정리 상태 최종 확인: control 파일 없음, 자기 daemon·TC-1·harness·chromium 프로세스 0개, Ego TaskSpace1 closed. 세션·패인 로그는 보존한다.
- main 인수 확인: 최종 보고와 5 PASS 인수 완료. native embed build exit 0, assets 11 PASS 완료(메인).
- 소유 반환: main. 이 role을 해제한다.

## 남은 제한 / 메인 요청
1. **임시 HOME 미삭제 (테스트 harness 정리 누락, main 소유)**: `ui/e2e/real-smoke-server.ts` `RealSmoke.close()`와 `ui/e2e/real-tc1-server.ts` `RealTc1.close()`는 `mkdtemp`로 만든 `loom-browser-smoke-*`/`loom-tc1-browser-*` 디렉터리를 지우지 않는다. 이전 실행분 23개(smoke 20, tc1 3)가 `$TMPDIR`에 남아 있다. 여기에는 종료된 daemon의 `run/daemon.json` token이 들어 있다. close에서 `rm(home,{recursive:true,force:true})`를 추가하도록 권고한다. 이 담당의 소유 범위 밖이라 수정하지 않았고, 타 실행분도 삭제하지 않았다.
2. CAN 지연 PASS는 headless Playwright 환경 기준이다. headed Ego 창의 RAF throttling은 OS 창 상태 문제로 남아 있다(제품 결함 근거 없음).
3. `real-smoke.config.ts` 실행에는 `LOOM_REAL_SMOKE=1`이 필수다. 문서화된 명령(`browser-smoke.md`)에 이미 포함돼 있다.
