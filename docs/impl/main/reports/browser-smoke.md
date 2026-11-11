# 최신 판정 — 실제 브라우저 수용 PASS

2026-11-12: 사용자 요청에 따라 Herdr foreground Claude Opus5.5의 정상 승인된 Ego CLI에서 실제 검증이 가능했다. UART/UDS/TC-1과 CAN 사용자 개입/STOP, SDK 수정 후 ETH Ego 검사가 통과했다. Ego의 RAF 약1초 정지로 지연관문은 실패했고, 이미 허용된 Playwright fallback에서 실제5개 전체 PASS(18.5초), CAN p95 83.401ms<150ms를 확인했다. 권한/보안 설정 변경이나 sandbox 해제는 없었다.

최종 결과는 [통합 수용](p1-acceptance.md), [실제 브라우저 실행·SHA·캡처](p1-browser-final.md)를 본다. 아래는 Codex 경로의 실패 진단과 준비 기록이며 전역 차단 또는 최종 미완료 상태로 해석하지 않는다.

---

docs/plan/02-core-principle.md 준수
# 실제 브라우저 스모크 — 실행 환경 차단

기능 / 상태 / 담당: 실제 sim gateway UI 스모크 / BLOCKED / browser_smoke 직접 수행.
기준: main cae4fa7 이후 공유 작업 트리(다른 담당의 CAN/UDS 미커밋 유지).
목적: 실제 gateway+sim의 agent TX/RX가 브라우저에 표시되고 사용자 입력/STOP이 동작하는지 확인. mock/replay 시험으로 대체하지 않는다.
소유 파일: 본 보고서. 제품/UI 코드·index·커밋 변경 없음.

## 실행 증거

사용자 지시로 Ego Lite 실패 후 Playwright 대체를 적용했다. 기존 UI 의존성 `@playwright/test`와 이미 설치된 Chromium을 사용했다. 신규 설치·권한 변경·브라우저 보안 설정 변경 없음.

```sh
PLAYWRIGHT_BROWSERS_PATH=/Users/ctmctm/.cache/ms-playwright node <<'JS'
const {chromium}=require('./ui/node_modules/@playwright/test');
(async()=>{
  const b=await chromium.launch();
  const p=await b.newPage();
  await p.goto('data:text/html,<title>Loom browser probe</title><p>ready</p>');
  console.log(await p.title());
  await b.close();
})().catch(e=>{console.error(e);process.exitCode=1});
JS
```

결과: exit 1. `/private/tmp/loom-playwright-launch.log`.
Chromium headless shell 1254가 page 생성 전에 SIGTRAP으로 종료:

```text
FATAL:base/apple/mach_port_rendezvous_mac.cc:159
bootstrap_check_in org.chromium.Chromium.MachPortRendezvousServer.23990:
Permission denied (1100)
```

이는 UI/daemon 연결 전 macOS 프로세스 IPC 제한이며 UI assertion 실패가 아니다. 과거 동일 MachPort 실패와 같아 같은 실행을 반복하거나 보안 플래그/별도 실행 통로로 제한을 우회하지 않았다. Ego Lite CLI 실패와 CUA 접근 자동 승인 거부는 root의 UART 보고서에 별도로 기록되어 있다.

## 완료 조건과 다음 행동

- 실제 브라우저 화면 검증은 필수 완료 조건으로 유지한다. 이번 결과는 브라우저 PASS도 P1 수용 완료도 아니다.
- 브라우저 실행이 허용되는 환경 또는 승인된 기존 브라우저 연결이 확보되면 실제 daemon의 격리 HOME에 대한 로그인·UART live 표시·viewer 입력·STOP, CAN/ETH 화면을 검증해야 한다.
- 기존 사용자 데모/STOP과 장비는 건드리지 않았다. 실제 sim을 브라우저 없이 송신하는 별도 경로를 브라우저 증거로 대체하지 않았다.
- 자체 리뷰: 명령·실패 로그 확인. 독립 리뷰 없음. 제공된 한도/사용량 미상. 커밋/푸시 안 함. 후속 인수 담당 root.

## 실행 가능한 실제 gateway harness 반환

- 신규 파일: `ui/e2e/real-smoke.config.ts`, `real-smoke-server.ts`, `real-smoke.spec.ts`.
- 전용 config와 `LOOM_REAL_SMOKE=1` 이중 opt-in. 기본 mock suite에서는 skip. 격리 HOME의 실제 loomd subprocess를 시작하고 `core.hello` agent token 인증 및 공식 `/api/v1/bootstrap/new` URL을 브라우저에 열어 정상 bootstrap 교환을 사용한다. cookie 직접 삽입 없음. daemon token/일회용 URL은 로그·trace에 출력하지 않는다.
- UART sim open → agent root TX/장치 응답이 실제 `.uart-trace` DOM에 표시 → UI STOP 클릭 → agent 송신 `E_STOPPED` 확인 → screenshot 생성. 자기 daemon만 `core.shutdown` 후 종료한다.
- 현재 UI는 `navigator.webdriver=true`이고 실제 daemon이면 권한을 watch로 제한한다(`ui/src/store/shellControl.ts:233`). 이 정책을 숨기거나 변경하지 않았다. 따라서 자동화 브라우저의 실제 UART 입력 검증은 수행하지 않는다. live 렌더·STOP을 우선 검증하며, 사용자 입력 전체 수용은 정상 비자동화 UI 경로가 별도로 필요하다.

실행 명령(저장소 루트):

```sh
CARGO_TARGET_DIR="$HOME/.cache/loom-main-target" cargo build -p loom-gateway --locked --offline
cd ui
pnpm build
LOOMD_BIN="$HOME/.cache/loom-main-target/debug/loomd" pnpm exec tsx e2e/real-smoke-server.ts --preflight
LOOMD_BIN="$HOME/.cache/loom-main-target/debug/loomd" LOOM_REAL_SMOKE=1 pnpm exec playwright test -c e2e/real-smoke.config.ts
```

검증 증거:
- 실제 daemon+공식 bootstrap 발급+정적 UI serve+UART sim open 사전 검사 **PASS**: `/private/tmp/loom-browser-preflight.log`. 브라우저 제외이며 실제 화면 PASS가 아니다.
- `pnpm exec tsc -b --pretty false` **PASS**, `/private/tmp/loom-browser-typecheck.log`.
- `pnpm exec eslint e2e/real-smoke*.ts` **PASS**, `/private/tmp/loom-browser-lint.log`.
- `LOOM_REAL_SMOKE=1 pnpm exec playwright test -c e2e/real-smoke.config.ts --list`: 실제 smoke 1개 검색 성공. browser assertions 자체는 실행 미검증이며 launch 차단 해소 후 검증해야 한다.

## 사용자 터미널과 agent sandbox A/B 확인

사용자 일반 터미널에서 아래 두 최소 probe를 각각 실행하면 agent sandbox만의 실패인지 구분할 수 있다. 현재 증거는 agent 실행에서의 실패이며 외부 터미널 성공은 아직 확인하지 않았다.

```sh
ego-browser nodejs -e 'console.log("ego-browser ready")'
cd /Users/ctmctm/workspace/Loom
PLAYWRIGHT_BROWSERS_PATH="$HOME/.cache/ms-playwright" node <<'JS'
const {chromium}=require('./ui/node_modules/@playwright/test');
(async()=>{
  const b=await chromium.launch();
  const p=await b.newPage();
  await p.goto('data:text/html,<title>Loom browser probe</title><p>ready</p>');
  console.log(await p.title());
  await b.close();
})().catch(e=>{console.error(e);process.exitCode=1});
JS
```

외부 터미널만 성공하면 agent 프로세스 실행 제약이 원인이라는 강한 분리 증거다. 외부도 실패하면 앱/OS 서비스 상태를 추가 진단해야 하며 sandbox만의 문제라고 확정하면 안 된다. 보안 플래그 변경·권한 우회·기존 사용자 브라우저 디버깅 포트 노출은 이 절차에 포함하지 않는다.

## 메인 원인 판단·권장 방안

현재 세션은 workspace-write + approval_policy=never다. spawned CLI도 같은 sandbox 경계를 상속한다는 공식 문서(https://learn.chatgpt.com/docs/sandboxing)와 Chromium Mach bootstrap 거부/ego XPC 연결 실패/ps 거부를 종합하면 agent 실행 제한이 공통 원인일 가능성이 높다. 단, sandbox denial 원본 로그 또는 사용자 일반 터미널 비교가 없으므로 Ego CLI의 정확한 내부 원인을 확정하지 않는다.

권장 순서:
1. 사용자 일반 터미널에서 본 보고서의 최소 Ego/Chromium probe 비교. 외부도 실패하면 앱/OS 연결 문제를 먼저 진단한다.
2. 외부만 성공하면 사용자/관리자가 에이전트 permissions에서 개별 실행 승인 가능한 정책을 선택한다. 공식 docs는 workspace-write + on-request 및 필요한 command exception을 설명한다. 무조건 Full access나 Full Disk Access를 켜는 것을 전제하지 않는다. 이 세션은 상위 정책상 에이전트가 변경/에스컬레이션할 수 없다.
3. Ego Lite 앱 접근은 별도 자동 승인 거부이므로 해당 computer-use 허용을 따로 확인한다. 실제 반환은 `Computer Use was not approved to use ego lite`; 구체 사유는 제공되지 않았다.
4. 정상 Ego 경로에서 실제 사용자 입력을 검증하고, Playwright로 실제 live trace·STOP 회귀를 실행한다. webdriver watch cap은 정상 테스트에서 기대 동작으로 확인하며 숨김으로 우회하지 않는다.

후속 리뷰 반영: 신규 TS 3파일을 기존 2-space 스타일로 정리. agent WebSocket open 대기를 10초로 제한하고, cleanup은 정상 core.shutdown 이후 최대 5초 종료 대기 → 필요한 경우에만 자기 child SIGTERM → 추가 5초 대기로 변경하여 정상 journal seal 기회를 보장했다. 변경 후 실제 daemon preflight 재실행 PASS, eslint·TypeScript PASS. 브라우저 launch 차단 및 DOM assertions 실행 미검증 상태는 동일하다.

## 2026-11-12 최신 P1 browser 상태 (앞의 webdriver 설명 대체)

마스터 §2.5 F1은 기본 Auto일 때 automation도 부여된 operate권한을 유지하고, confirm/approve를 설정했을 때만 V5 watch상한을 적용한다. 앞 보고의 항상watch 판정은 오래된 V5만 읽은 오류였다. 메인이 daemon core.hello에 policy/auto capability를 추가하고 UI가 명시선언을받을때만 cap해제하도록수정. 정책변경통지뒤에는 새연결hello전까지 failclosed, watch부트스트랩을operate로승격하지않음. 기본/deny only/confirm/approve실gatewayhello정책검사2개와UI권한38개검사통과. 실제UIcheck362 PASS, buildPASS.

브라우저시험4개 준비: UART 실제agenttrace→xterm 사용자키입력→deviceexpect→STOP, CAN 실제ECU응답→browser사용자send→STOP, UDS same request_id confirmed row→browser요청→STOP, Ethernet실packet목록→detail→raw편집송신→STOP. screenshot/명령시작→DOM확인상한시간첨부. e2e타입/lint/testdiscovery PASS, **렌더/input 실제실행은 미수행(아래차단)**. 시험코드준비를P1합격으로세지않는다.

- Ego Lite실행은확인했지만 앱제어 autoapproval 거부: `Computer Use was not approved to use ego lite` (구체적거부이유제공없음).
- EgoCLI실행은XPC연결실패, Playwrightlaunch는MachPortRendezvousServer Permissiondenied1100 SIGTRAP.
- 사용자에게 Settings>Computeruse>Control AnyApp과 앱접근승인/OS Accessibility·ScreenRecording의구분을안내했고접근허용후알려달라는질문이대기중. 해당보안설정을agent가변경하거나다른worker에서우회실행하지않음.
- 사용자허용신호후EgoLite우선, 불가시이미허용된Playwrightfallback실행. 브라우저관문은필수이며 BLOCKED권한을미확인/완료로기록하지않는다.

## 최종 준비 구성 (5개 필수 시나리오)

기존 네 도메인에 실제 powered TC-1의 `shell.test` LIVE → journal 판정 → DONE/passed 화면 검사를 추가했다. CAN은 20개 고유 프레임의 장치 RX DOM과 2회 RAF를 기다리고, 명령 시작부터의 보수적 end-to-end 상한 p95 < 150 ms를 검사한다. paint timestamp 자체라고 표기하지 않는다. 화면 캡처는 각 도메인 사용자 개입·STOP과 TC-1 최종 판정 시점에 남긴다.

Rust fixture/Node bridge의 사전 검증은 **브라우저 없이** 완료했다: fresh UI HTTP serve, 정상 일회용 cookie 교환, 실제 viewer WS 구독, 실제 TC-1 passed, release 후 정상 child 종료. `/private/tmp/loom-p1-browser-protocol-preflight2.log`(exit0). 결과 JSON은 browser_rendering=false, browser_input=false다. ready.json은 private0600이며 읽은 즉시 삭제하고 URL/token을 로그·trace로 출력하지 않는다. 이 사전 검증을 실제 화면 PASS로 세지 않는다.

실제 브라우저 권한 차단 해소 후 실행(저장소 root):

```sh
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$HOME/.cache/loom-main-target" cargo build -p loom-gateway --locked --offline
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$HOME/.cache/loom-main-target" cargo test -p loom-gateway --test tc1_live_smoke --no-run --locked --offline --message-format=json > /private/tmp/loom-tc1-build.jsonl
export LOOM_TC1_TEST_BIN="$(python3 -c 'import json; rows=[json.loads(s) for s in open("/private/tmp/loom-tc1-build.jsonl")]; print(next(r["executable"] for r in rows if r.get("target",{}).get("name")=="tc1_live_smoke" and r.get("executable")))')"
cd ui
pnpm build
LOOMD_BIN="$HOME/.cache/loom-main-target/debug/loomd" LOOM_REAL_SMOKE=1 pnpm exec playwright test -c e2e/real-smoke.config.ts
```

`LOOM_REAL_SMOKE=1`에서 TC-1 바이너리를 지정하지 않으면 다섯째 검사는 실패한다(필수 시나리오를 skip해서 전체 PASS로 오인하지 않도록 함). 실제 실행 시 5개 모두 통과·스크린샷·CAN 시간 증거를 확인해야 브라우저 축 완료다. Ego Lite가 우선이며 Playwright는 사용자가 명시 허용한 fallback이다. 이 명령을 Herdr 다른 담당에게 넘겨 현재 샌드박스/앱 접근 거부를 우회 실행하지 않는다.

## 해결 절차 구체화 — 2026-11-12 후속

추가 읽기전용 확인: `~/.codex/config.toml`에는 computer_use/approval_policy/sandbox_mode/default_permissions의 명시 설정이 없었다. `~/.codex/requirements.toml`, `/etc/codex/requirements.toml`도 없었다. 앱/세션/관리형 서버 정책까지 없다는 의미는 아니다. 현재 task에 적용된 workspace-write/approval_policy=never는 그대로다. 자동 승인 검토의 내부 거부 원인은 노출되지 않았다.

권장 해결 순서:
1. ChatGPT 데스크톱 Settings → Computer use → Control에서 Any App 설치/켜기 상태를 확인하고, Ego Lite 접근 승인 요청을 Allow로 처리한다. 지원 UI에서 Always allow를 선택하면 같은 앱 승인을 저장할 수 있다. Any App이 없거나 컨트롤이 잠겨 있으면 관리자에게 해당 앱 접근 허용을 요청한다.
2. macOS 시스템 설정 → 개인정보 보호 및 보안 → 화면 기록 및 손쉬운 사용에서 Codex Computer Use 권한을 확인한다. 이는 ChatGPT의 개별 앱 승인과 별개의 설정이다.
3. 설정 변경 사실이 확인된 뒤에만 Ego Lite 접근을 재시도한다. 성공 판정은 실제 Loom 화면 읽기 및 해당 테스트 세션의 입력/STOP 상호작용이다. 성공하면 준비된 5개 시나리오를 실행한다.
4. Ego 경로가 여전히 실패하면 정확한 오류로 분기한다. 자동 승인 거부가 계속되면 앱/워크스페이스 정책 승인 문제, GUI는 되는데 ego CLI XPC만 실패하면 CLI 연결 문제다. 후자는 승인된 GUI 경로로 검증을 진행하면서 CLI는 별도 고장으로 추적한다.
5. Playwright fallback은 현재 세션의 명령 실행 sandbox 문제다. 사용자/관리자가 Chromium 실행에 필요한 승인을 요청할 수 있는 세션 정책을 적용한 뒤 해당 명령에 한정한 정상 승인을 받아 최소 launch probe부터 확인한다. 승인 정책 변경 자체가 Mach 권한 문제를 반드시 해소한다고 단정하지 않는다. 현재 `never` 정책에서 agent가 이를 변경하거나 Herdr로 우회하지 않는다.

근거(official OpenAI documentation):
- https://learn.chatgpt.com/docs/computer-use — 앱 승인 및 macOS 권한의 분리, Settings 경로.
- https://learn.chatgpt.com/docs/config-file/config-reference — computer_use.macos.bundle_ids의 allow는 일반 승인 흐름을 대신하지 않으며 managed deny를 완화하지 않음.
- https://learn.chatgpt.com/docs/permissions — 명령 실행 권한과 Computer Use가 별도 제어를 사용함.

이 절은 해결 경로다. 설정 변경/접근 성공을 관측한 결과가 아니므로 아직 해결 완료라고 기록하지 않는다.

## 오류 문구 웹 검색 결과 — 후속 정정

공식 권한 문서 외에 오류 자체를 검색하여 다음 upstream 1차 자료를 확인했다.

- https://github.com/openai/codex/issues/21292 : 동일한 Chromium `MachPortRendezvousServer Permission denied(1100)` 재현 보고. 보고자는 같은 스크립트가 승인된 sandbox 밖 실행에서는 성공했고 여러 Chromium 플래그/채널 변경은 해결하지 못했다고 기록했다. 해당 호스트의 재현 보고이지 우리 호스트에서 A/B 성공을 관측한 것은 아니다. 정상적인 좁은 실행 승인이 다음 검증 경로라는 근거가 강화됐다.
- https://github.com/citrolabs/ego-lite/issues/188 : GUI가 열려 있어도 agent sandbox에서 CLI bootstrap이 실패하는 사례. 개발자는 CLI 서버 서비스 시작 과정을 설명하고 app 인자 `--startup-ego-browser-service`를 제시했다. 앱 실행과 CLI 서비스 준비는 구분해 진단해야 한다. 기존 사례는0.4.x/다른agent환경이며 현재0.6.2.9에서 동일 결함이라고 확정하지 않는다. 앱 접근이 승인된 정상 경로에서 서비스 준비 상태를 검증하는 것이 후속이다. 이 플래그를 자동 승인 거부 우회로 실행하지 않았다.
- https://github.com/citrolabs/ego-lite/issues/433 : macOS27에서 AppKit/talagent XPC 대기 관련 별도 사례. maintainer는 sample만으로 entitlement/sandbox원인을 확정할 수 없다고 정정했다. 우리 오류와 같은 원인이라고 단정하지 않는다.

읽기전용 로컬 버전 확인: macOS27.0.1(build26A434), EgoLite0.6.2.9(bundle6.2.9). `ego-browser --help`는 정상 반환하며 named service 연결 옵션도 제공하지만 실행 중인 서비스 이름을 확인하지 않고 임의 지정하지 않는다.

정정: Ego CLI XPC 실패/Playwright Mach 실패/ComputerUse 자동승인 거부를 한 가지 권한 스위치로 해결할 수 있다고 보지 않는다. GUI 앱 접근 승인, Ego CLI 서버 서비스 bootstrap, Chromium 외부 sandbox 경계를 각기 검증해야 한다. 실제 해결 성공은 여전히 미관측이다.
