# 최초 Ego 실제 검증 기록 (최종 판정은 별도)

이 문서는 최초 실행의 PASS/FAIL 원본을 보존한다. 이후 SDK 수정 후 Ego ETH 재검증과 Playwright 5/5가 통과했다. 최신 판정은 [최종 브라우저 보고](p1-browser-final.md)와 [P1 통합 수용](p1-acceptance.md)을 본다. 아래 최초 실패는 성공으로 바꾸지 않았다.

---

docs/plan/02-core-principle.md 준수
# P1 Opus 실제 브라우저 검증 — 2026-11-12

상태: **부분 PASS — UART·UDS·TC-1 PASS / ETH 제품 계약 버그(main 수정 후 재확인 필요) / CAN 지연 환경 차단(사용자 조치 필요)**.
실행: Claude Code Opus 5.5 foreground, 내부 subagent/백그라운드 에이전트 미사용. 기준 main `748f7c1` + main 병렬 UI 수정(UART 부분 행, ETH eth.show) 빌드.

## 승인 경로 — 허용 근거

- 이 세션은 Claude Code CLI(auto mode)다. 각 Bash 명령이 정상 권한 검토를 거쳐 실행됐다. 권한/보안 설정 변경, skip-permissions, sandbox 해제, 브라우저 보안 flag, 디버깅 포트, 다른 Herdr worker 통로는 사용하지 않았다.
- `ego-browser skill` exit0 (guide v2.2.0, end 라인 확인).
- `ego-browser nodejs -e 'const task = await taskSpace("loom p1 browser acceptance"); …'` → `{ spaceId: 1, ownership: 'agent' }`. Ego Lite CLI/XPC 연결이 정상이다. root 세션의 XPC 실패/Computer Use 거부는 이 세션에서 재현되지 않았다. Computer Use 앱 제어는 쓰지 않고 Ego CLI 경로만 사용했다.
- Ego가 동작하므로 Playwright fallback은 사용하지 않았다.
- Ego 페이지는 `navigator.webdriver=false`, 화면 권한 `자격: operate`이었다.

## 실행 구성 (재현 가능)

스크립트와 증거는 `~/.cache/loom-p1-opus-browser/`에 보존했다. 원본 scratchpad는 세션 한정이다.

- TC-1 exe: `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=~/.cache/loom-main-target cargo test -p loom-gateway --test tc1_live_smoke --no-run --locked --offline --message-format=json` → `~/.cache/loom-main-target/debug/deps/tc1_live_smoke-e4c707f7a9513da7`.
- 제어 harness `ego-harness.mts`: 기존 `ui/e2e/real-smoke-server.ts` RealSmoke와 `real-tc1-server.ts` RealTc1을 그대로 import한다. 127.0.0.1 랜덤 포트와 secret(0600 control 파일, 종료 시 삭제)을 쓰고, 시나리오마다 fresh isolated HOME daemon/own TC fixture를 기동·정리한다. 기존 demo `live-W4lxNJ`는 건드리지 않았다.
  ```sh
  cd ui && LOOMD_BIN=~/.cache/loom-main-target/debug/loomd LOOM_TC1_TEST_BIN=<위 exe> pnpm exec tsx ~/.cache/loom-p1-opus-browser/ego-harness.mts <dir>/ctl.json
  ```
- 시나리오 실행: `cat prelude.mjs <scenario>.body.mjs | ego-browser nodejs` (UART는 `ego-browser nodejs < uart.mjs`). 스크립트 상단의 `S` 경로를 control 파일 위치로 맞춘다. 시나리오: `uart.mjs`, `can2.body.mjs`(측정 강화판, 1차는 `can.body.mjs`), `uds.body.mjs`, `eth.body.mjs`, `tc1.body.mjs`. 진단: `ethprobe*.body.mjs`.
- 단계 종료 후 harness SIGTERM. 자기 daemon `core.shutdown`, TC fixture release를 거쳐 잔존 child 0을 확인했다.

## 시나리오 결과 (모두 실제 daemon/sim, 실제 Ego 화면)

| # | 시나리오 | 결과 | 핵심 증거 |
|---|---|---|---|
| 1 | UART agent→RX→화면→사용자 키입력→STOP | **PASS** (시도4) | uart.open→`Loom sim 1.0` DOM 125 ms. agent `root` TX 행과 `Password:` 표시. 브라우저 xterm에 사용자가 입력한 `secret⏎`로 `root@sim:~# ` 도달. Enter 전 칩 `입력권: 나 (입력 중)`. 브라우저 `echo browser-p1`이 device expect에 매칭. UI STOP 후 agent send `E_STOPPED`. `uart-agent-live-attempt4.png`, `uart-browser-input-stop-attempt4.png` |
| 2 | CAN 20회 device RX DOM+2RAF p95<150 ms→사용자 send/STOP | **FAIL(환경) — 지연 판정 불가**. 사용자 send/STOP은 PASS | 아래 CAN 절 참고. `can-browser-send-stop-attempt2.png`, `can2-samples.json` |
| 3 | UDS 같은 request_id→사용자 요청→STOP | **PASS** | `uds.request 22F190` rid `w1` confirmed 행, VIN `LOOMSIM0000000001`. 브라우저 `22 F18C` 사용자 행 confirmed. STOP 후 `E_STOPPED`. `uds-agent-live.png`, `uds-browser-request-stop.png` |
| 4 | ETH 목록/상세/사용자 send/STOP | **FAIL — 제품 계약 버그** | 목록 live(agent ARP seq22 AGT, DEV DoIP/SOME-IP) PASS. 상세 tree/bytes 비어 있음 → `편집 후 송신…` disabled. 아래 ETH 절 참고 |
| 5 | 실제 powered TC-1 LIVE→journal 판정 DONE/passed | **PASS** (시도2) | 보는 중 2/2(Rust fixture viewer+브라우저). fixture `{"result":"passed","session_id":"ts-18e716db8b11a7d8-66568-1"}`. 화면 `DONE`/`결과 passed`. 단계 4 DTC 불일치 fail → 5 삭제 후 재확인 pass(journal 근거). `tc1-live-attempt2.png`, `actual-tc1-journal-verdict-attempt2.png` |

### 시도별 실패 기록 (캡처 보존)

- UART 시도1: `U-Boot` 기대 행이 가상화 목록에서 밀려나 DOM에 없음. 테스트 기대값 문제. 캡처는 시도2가 덮어써서 시도1 원본은 유실됐고, 이후 시도별 파일명으로 바꿨다.
- UART 시도2: `login:`/`Password:`처럼 LF 없는 부분 행이 표시되지 않음 → **main이 `ui/src/panels/uart/tracePanel.ts` LineAssembler 수정**. `uart-fail-attempt2.png`.
- UART 시도3(수정 빌드): 사용자 입력은 device 도달. Enter 뒤 칩 `나` 기대가 실패(Enter로 lease가 해제되는 게 정상) — 테스트 타이밍 문제. `uart-fail-2026-11-11T201315331Z.png`.
- TC-1 시도1: 화면에 id `TC-1`/`S2`가 아니라 제목 `콜드 부팅 스모크`/단계명 표시 — 기대값 문제. 캡처상 DONE/passed는 이미 표시됨. `tc1-fail-2026-11-11T201436504Z.png`.
- ETH 시도1·2: `eth-fail.png`(첫 행이 DEV 패킷, 시도2가 덮어씀), `eth-fail-2026-11-11T201527171Z.png`(수정 빌드 전).
- 보안 사고 기록: UART 시도3 실패 메시지에 Ego 오류 문자열이 포함한 일회용 bootstrap URL fragment가 에이전트 도구 출력에만 1회 노출됐다. 해당 daemon은 즉시 shutdown됐고 파일/보고서/로그에는 기록하지 않았다. 이후 스크립트는 `#boot=`를 redaction한다.

## CAN 지연 — 원본 측정과 원인 구분

- 시도1(`can.body.mjs`, 기존 정의: agent command 시작→device RX DOM→2RAF, 제어 왕복 포함). p95 **2068.7 ms**. samples(ms) `[56,1066.3,145,1110,1063,170.8,1114.4,1100,140,126.5,1114.9,2082,171.4,2049.6,1074.1,156.6,1115.1,1113.7,2068.7,140.3]`. 실패 캡처 `can-fail.png`(사용자 send 전 단계).
- 진단: idle 페이지에서 2RAF = 471.5 ms.
- 시도2(`can2.body.mjs`, `can2-samples.json` 원본):
  - 측정 구간: DOM observer가 해당 device row `data-seq`와 `performance.timeOrigin+now()`를 DOM 시점과 2RAF 시점에 저장. 루프 종료 후(샘플 구간 밖) `core.events.query({since:"seq:N",until:"seq:N",max:1,peek:true,filter:{domains:["can"],channels:["ch0"],kinds:["can.frame"]}})`로 원본 RX `ts.wall_ns`를 얻어 device event→paint proxy를 계산. 제어 왕복은 device→paint 구간에 포함되지 않는다.
  - 시계: 같은 호스트. page `Date.now()−(timeOrigin+now)` = −0.8 ms. page−host 중간값 +0.6 ms ± 0.5 ms(왕복 반폭) → 오차 약 ±1.5 ms.
  - 환경: `visibilityState=visible`, `hasFocus=false`. RAF 간격 29개 중 7개가 ~1000–1017 ms, 나머지 16.7/33 ms(≈30 fps).
  - p95: command→paint **2097.7**, device→DOM **1045.7**, device→paint **2087**, DOM→paint **1047.6** ms.
  - RAF 정지 없는 3개 샘플: device→paint 130.8 / 145.3 / 148.6 ms (DOM→2RAF ≈ 63 ms = 30 fps 2프레임).
  - device→DOM ~1040 ms는 UI 렌더 스케줄러가 RAF 기반이라 RAF 1 s 정지에 그대로 묶인 것이다. 제품 경로 자체 지연의 근거가 아니다. 판정: **브라우저 창 RAF throttling(환경)**. 150 ms 기준은 완화하지 않았고 PASS로 세지 않는다.
- 문서화된 자기 page 활성화 `page.cdp("Page.bringToFront")` 1회 후 `hasFocus=true`가 됐지만 RAF는 여전히 ~1000 ms 정지가 반복됐다 → 창/OS 수준(가려짐·화면 밖 등) 원인. visibility 위조나 flag 변경은 하지 않았고 재시도를 중단했다.
- 시도2에서도 사용자 send `0x300#66` user 행 표시와 STOP 후 `E_STOPPED`는 PASS.
- **필요 사용자 조치**: Ego Lite 창을 화면 위에 가리지 않게 띄우고(다른 창 뒤·최소화·다른 Space 아님), 디스플레이 잠금 없이 둔 상태에서 `can2.body.mjs`를 재실행한다. 측정은 RAF 간격 기록을 포함하므로 throttling 해소 여부가 같이 남는다.

## ETH — 제품 계약 버그 (main 소유, 이 담당은 미수정)

- 1차 원인(main 확인·수정 완료): `ethPanel.onSelect`가 `eth.query({ref,detail:true})` 호출 → 목록형 응답(rows/range…)에 bytes 없음 → `detailHex=null`.
- 수정 빌드 `index-W9kIEiJJ.js`(eth.show 포함, 로드 확인) 후에도 재현: 행 선택 시 `eth:cap@seq:22 · 상세 37 ms`인데 tree/bytes 영역이 비고 `편집 후 송신…` disabled=true(`ethprobe2-detail.png`).
- 같은 ref로 harness에서 `eth.show({ref:"eth:cap@seq:22"})`를 호출하면 `{cmd,data:{bytes:{encoding:"base64",data:"////////AgAAAAABCAYAAQgABgQAAQIAAAAAAcCoAAEAAAAAAADAqAAC",comments:[],loom_comment:null},caplen,len,linktype,ref,summary,tree,truncated:false,warnings}}`.
- 원인(main 확인): `eth.show` 자체는 정상 bytes/tree를 반환한다. 공통 `PanelContext.query`가 native domain 응답의 D5 Envelope를 unwrap하지 않고 원본 그대로 반환하는 것이 원인이다. 그래서 `ethPanel.ts`의 `res.bytes`/`res.tree`가 비어 있다. 수정 담당은 별도 foreground Opus uds-impl이다(SDK `context.ts`/`sdk.test.ts`: 성공 data unwrap, 실패 reject, bare 응답 호환). 이 담당은 미수정이다.
- **ETH 재시도 보류**: SDK 수정 + `ui/dist` 빌드 완료 신호 후 fresh daemon으로 `eth.body.mjs`를 재실행한다(agent ARP 행 선택 → 상세 tree/bytes → 편집 송신 → user 행 → STOP). 진단 재현은 `ethprobe2.body.mjs`.

## E2E 변경 (Opus 소유)

`ui/e2e/real-smoke.spec.ts` 최소 수정(+10/−5, sha256 `84ef5ef53a7c7af955e755d868d6484f850c035444f7cbb99e6ecee0d5394e88`), eslint+`tsc -b` PASS:
- UART: `U-Boot` → live tail `Loom sim 1.0`(가상화 반례). 칩 `나` 확인을 Enter 이전(입력 중)으로 이동.
- ETH: `.first()` → agent ARP 프레임(`Who has`) 행. DEV 공지 패킷이 먼저 도착하기 때문.
- TC-1: `TC-1`/`S2` → 화면 실제 표기 `콜드 부팅 스모크`/`콘솔 관찰`.
- `real-tc1-server.ts`, `real-smoke-server.ts`는 변경 없음. 제품 src/native/Cargo/lock/index/commit/push/권한 설정 변경 없음.

## 남은 후속

1. uds-impl SDK `PanelContext.query` unwrap 수정 + dist 빌드 신호 → `eth.body.mjs` 재검증(현재 보류).
2. 사용자: Ego Lite 창 가시·비가림 상태 확보 → `can2.body.mjs` 재측정(device→paint p95<150 ms 판정).
3. Ego TaskSpace 1(agent 소유)은 재개용으로 열어 두었다. 완료 시 `finish({keep:[]})`.
4. Playwright 경로의 전체 spec 실행은 이번 세션에서 미실행이다(Ego 우선 원칙).
5. CAN 추가 계측은 하지 않았다. 창 RAF throttling이 남아 있는 한 같은 측정을 반복해도 판정이 불가하다. 사용자가 창 가시 상태를 확보한 뒤 `can2.body.mjs` 1회로 판정한다.

## 반환 상태

12분 기능 한도를 넘겨 반환한다. 준비된 결과·실패·후속 계측은 위에 기록했다. 실행 중인 harness/daemon/fixture는 없다(잔존 child 0 확인). ETH(SDK 신호)와 CAN(사용자 창 조치)은 대기 중이며, 남은 focused 검증은 main이 새 기능으로 배정한다.
