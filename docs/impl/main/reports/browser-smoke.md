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
