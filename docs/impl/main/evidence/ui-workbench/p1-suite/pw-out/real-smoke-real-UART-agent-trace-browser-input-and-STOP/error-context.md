# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: real-smoke.spec.ts >> real UART agent trace, browser input and STOP
- Location: e2e/real-smoke.spec.ts:39:1

# Error details

```
Error: expect(locator).toContainText(expected) failed

Locator: locator('.uart-trace')
Expected substring: "Loom sim 1.0"
Timeout: 5000ms
Error: element(s) not found

Call log:
  - Expect "toContainText" locator('.uart-trace') with timeout 5000ms
  - waiting for locator('.uart-trace')

```

```yaml
- banner:
  - text: "Loom Workbench [SIM] 연결됨 LIVE idle 4.6s REC ok 보는 중: 1/1 에이전트 real-browser-smoke uart.open 4s 전 자격: operate"
  - button "■ STOP 전체"
- text: 자동화 브라우저 — 자동 실행 정책에서 쓰기 허용
- complementary "인터페이스 탐색기":
  - navigation "채널":
    - strong: 인터페이스
    - text: "1"
    - button "인터페이스 추가": + 추가
    - text: 채널 검색
    - searchbox "채널 검색"
    - paragraph: 채널을 선택해 보기를 엽니다.
    - region "UART 채널":
      - button "UART 1" [expanded]
      - list:
        - listitem:
          - button "UART dut 터미널 열기 · 연결됨": dut 연결됨 gen1
          - button "UART dut 터미널 보기": 터미널
          - button "UART dut 트레이스 보기": 트레이스
  - text: 탐색 안내
  - paragraph: 인터페이스를 선택해 작업 패널을 여세요.
  - text: 패널을 열어도 장치에 연결하거나 데이터를 보내지 않습니다.
- text: WORKSPACE
- heading "개발 워크벤치" [level=1]
- button "단축키 ?"
- region "워크스페이스 도구":
  - text: 레이아웃
  - combobox "레이아웃":
    - option "(딥링크)" [selected]
  - textbox "저장할 레이아웃 이름":
    - /placeholder: 이름
  - button "저장"
  - group: 패널 동작
- main:
  - region "UART 터미널 · dut":
    - tablist:
      - tab "UART 트레이스 · dut"
      - tab "UART 터미널 · dut" [selected]:
        - text: UART 터미널 · dut
        - button "Close UART 터미널 · dut"
    - button "최대화"
    - tabpanel "UART 터미널 · dut":
      - text: "UART dut ● ? 115200 gen1 [SIM] 입력권: 비어 있음"
      - button "반환" [disabled]
      - button "지우기" [disabled]
      - button "가져오기" [disabled]
      - button "HOLD"
      - button "⏸"
      - group: 키 ▾
      - textbox "Terminal input"
      - text: rx 0 B/s │ 차단 질의×0 자동응답×0 │ REC ok
  - status: UART 터미널 · dut opened
  - alert
- contentinfo:
  - region "Activity":
    - button "Activity 1"
    - list:
      - listitem: 6시 17분 17초.157 AGT real-browser-smoke uart uart.open 1 call(s) in the last second
  - text: Ctrl-] x STOP · m 마커 · ? 도움말 LOOM / sim 6시 17분 20초
- text: 장치가 uart dut를 다시 열었습니다 — 패널을 열었습니다
- status
```

# Test source

```ts
  1   | /// <reference lib="dom" />
  2   | // Opt-in actual daemon/sim tests. No mock, replay or webdriver hiding.
  3   | import { expect, test, type Page, type TestInfo } from '@playwright/test';
  4   | import { performance } from 'node:perf_hooks';
  5   | import { RealSmoke } from './real-smoke-server';
  6   | import { RealTc1 } from './real-tc1-server';
  7   | 
  8   | const payload = (r: Record<string, unknown>) => (r.data ?? r) as Record<string, unknown>;
  9   | async function open(page: Page, server: RealSmoke, route: string) {
  10  |   await page.goto(await server.loginURL(route));
  11  |   await expect(page.getByTestId('loom-shell')).toBeVisible();
  12  |   // Master F1: default Auto policy permits granted operate rights even under automation.
  13  |   await expect(page.getByTestId('rights')).toContainText('operate');
  14  | }
  15  | async function evidence(page: Page, info: TestInfo, name: string, elapsed: number) {
  16  |   await info.attach(`${name}-command-to-visible`, {
  17  |     body: JSON.stringify({ milliseconds: elapsed, measurement: 'agent RPC start to DOM assertion completion; upper bound, not paint timestamp' }),
  18  |     contentType: 'application/json',
  19  |   });
  20  |   await page.screenshot({ path: info.outputPath(`${name}.png`), fullPage: true });
  21  | }
  22  | async function armCanPaint(page: Page, marker: string) {
  23  |   await page.evaluate((hex) => {
  24  |     const w = window as typeof window & { __loomCanPaint?: Promise<void> };
  25  |     w.__loomCanPaint = new Promise<void>((resolve, reject) => {
  26  |       const observer = new MutationObserver(() => {
  27  |         const rows = document.querySelectorAll('.ct-row[data-dir="rx"][data-origin="device"][data-id="0x301"]');
  28  |         if (![...rows].some((row) => row.textContent?.includes(hex))) return;
  29  |         observer.disconnect(); clearTimeout(timer);
  30  |         requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  31  |       });
  32  |       const timer = setTimeout(() => { observer.disconnect(); reject(new Error('CAN device frame paint timeout')); }, 3000);
  33  |       observer.observe(document.querySelector('[data-testid="can-trace"]')!, { subtree: true, childList: true, characterData: true, attributes: true });
  34  |     });
  35  |   }, marker);
  36  | }
  37  | test.beforeEach(() => test.skip(!process.env.LOOM_REAL_SMOKE, 'use LOOM_REAL_SMOKE=1 and real-smoke.config.ts'));
  38  | 
  39  | test('real UART agent trace, browser input and STOP', async ({ page }, info) => {
  40  |   const server = new RealSmoke();
  41  |   try {
  42  |     await server.start();
  43  |     await open(page, server, '/p/uart/trace?channel=dut');
  44  |     const t = performance.now();
  45  |     expect(await server.call('uart.open', { target: 'sim://bootshell?split=0', alias: 'dut', profile: 'linux-shell' })).toMatchObject({ ok: true });
  46  |     // Early boot lines (U-Boot) scroll out of the virtualized list; assert the live tail.
> 47  |     await expect(page.locator('.uart-trace')).toContainText('Loom sim 1.0');
      |                                               ^ Error: expect(locator).toContainText(expected) failed
  48  |     await evidence(page, info, 'uart-agent-live', performance.now() - t);
  49  |     await expect(page.locator('.uart-trace')).toContainText('login:');
  50  |     expect(await server.call('uart.send', { channel: 'dut', data: Buffer.from('root').toString('hex'), eol: 'cr' })).toMatchObject({ ok: true });
  51  |     await expect(page.locator('.uart-trace [data-origin="agent"][data-dir="tx"]')).toContainText('root');
  52  |     await expect(page.locator('.uart-trace')).toContainText('Password:');
  53  |     await open(page, server, '/p/uart/terminal?channel=dut');
  54  |     const terminal = page.locator('.xterm-helper-textarea');
  55  |     await expect(terminal).toBeAttached();
  56  |     await terminal.focus();
  57  |     await page.keyboard.type('secret'); await page.keyboard.press('Enter');
  58  |     expect(await server.call('uart.expect', { channel: 'dut', patterns: [{ kind: 'literal', pattern: 'root@sim:~# ' }], timeout_ms: 3000 })).toMatchObject({ ok: true });
  59  |     // The input lease is held only while my line is unfinished; Enter releases it.
  60  |     await page.keyboard.type('echo browser-p1');
  61  |     await expect(page.getByTestId('input-chip')).toContainText('나');
  62  |     await page.keyboard.press('Enter');
  63  |     expect(await server.call('uart.expect', { channel: 'dut', patterns: [{ kind: 'literal', pattern: 'browser-p1' }], timeout_ms: 3000 })).toMatchObject({ ok: true });
  64  |     await page.getByTestId('stop-button').click();
  65  |     await expect(page.getByTestId('stop-panel')).toBeVisible();
  66  |     expect(await server.call('uart.send', { channel: 'dut', data: '78', eol: 'cr' })).toMatchObject({ error: { code: 'E_STOPPED' } });
  67  |     await page.screenshot({ path: info.outputPath('uart-browser-input-stop.png'), fullPage: true });
  68  |   } finally { await server.close(); }
  69  | });
  70  | 
  71  | test('real CAN agent frames, browser send and STOP', async ({ page }, info) => {
  72  |   const server = new RealSmoke();
  73  |   try {
  74  |     await server.start();
  75  |     await open(page, server, '/p/can/trace?channel=ch0');
  76  |     expect(await server.call('can.open', { channel: 'ch0' })).toMatchObject({ ok: true });
  77  |     const t = performance.now();
  78  |     expect(await server.call('can.send', { channel: 'ch0', frame: '300#55' })).toMatchObject({ ok: true });
  79  |     await expect(page.getByTestId('can-trace')).toContainText('55');
  80  |     await evidence(page, info, 'can-agent-live', performance.now() - t);
  81  |     const samples: number[] = [];
  82  |     for (let i = 0; i < 20; i++) {
  83  |       const marker = (0xa0 + i).toString(16).toUpperCase();
  84  |       await armCanPaint(page, marker);
  85  |       const before = performance.now();
  86  |       expect(await server.call('can.send', { channel: 'ch0', frame: `300#${marker}` })).toMatchObject({ ok: true });
  87  |       await page.evaluate(async () => {
  88  |         const w = window as typeof window & { __loomCanPaint?: Promise<void> };
  89  |         await w.__loomCanPaint; delete w.__loomCanPaint;
  90  |       });
  91  |       samples.push(performance.now() - before);
  92  |     }
  93  |     const p95 = [...samples].sort((a, b) => a - b)[Math.ceil(samples.length * .95) - 1]!;
  94  |     await info.attach('can-device-paint-upper-bound', { body: JSON.stringify({ samples, p95_ms: p95,
  95  |       method: 'agent command start through matching device RX DOM and two RAFs; includes device response time and test transport overhead',
  96  |       measure: 'conservative end-to-end upper bound using the documented next-RAF paint proxy' }), contentType: 'application/json' });
  97  |     expect(p95).toBeLessThan(150);
  98  |     const input = page.getByTestId('can-send-input');
  99  |     await expect(input).toBeEnabled(); await input.fill('0x300#66'); await input.press('Enter');
  100 |     await expect(page.getByTestId('can-send-msg')).not.toHaveText('');
  101 |     await expect(page.getByTestId('can-trace').locator('[data-origin="user"]').filter({ hasText: '66' }).first()).toBeVisible();
  102 |     await page.getByTestId('stop-button').click();
  103 |     await expect(page.getByTestId('stop-panel')).toBeVisible();
  104 |     expect(await server.call('can.send', { channel: 'ch0', frame: '300#77' })).toMatchObject({ error: { code: 'E_STOPPED' } });
  105 |     await page.screenshot({ path: info.outputPath('can-browser-send-stop.png'), fullPage: true });
  106 |   } finally { await server.close(); }
  107 | });
  108 | 
  109 | test('real UDS agent transaction, browser request and STOP', async ({ page }, info) => {
  110 |   const server = new RealSmoke();
  111 |   try {
  112 |     await server.start();
  113 |     await open(page, server, '/p/uds/console?targets=ecu');
  114 |     expect(await server.call('uds.connect', { name: 'ecu', sim: 'bench' })).toMatchObject({ ok: true });
  115 |     const t = performance.now();
  116 |     const response = await server.call('uds.request', { target: 'ecu', bytes_hex: '22F190' });
  117 |     expect(response).toMatchObject({ ok: true });
  118 |     const rid = String(payload(response).request_id);
  119 |     expect(rid).not.toBe('undefined');
  120 |     await expect(page.locator('.uds-panel').locator(`[data-request-id="${rid}"][data-state="confirmed"]`)).toBeVisible();
  121 |     await evidence(page, info, 'uds-agent-live', performance.now() - t);
  122 |     const input = page.getByLabel('UDS 요청 (hex)');
  123 |     await expect(input).toBeEnabled(); await input.fill('22 F18C');
  124 |     await expect(page.getByTestId('uds-preview')).not.toHaveText('');
  125 |     await input.press('Enter');
  126 |     await expect(page.locator('.uds-panel [data-origin="user"][data-state="confirmed"]')).toBeVisible();
  127 |     await page.getByTestId('stop-button').click();
  128 |     await expect(page.getByTestId('stop-panel')).toBeVisible();
  129 |     expect(await server.call('uds.request', { target: 'ecu', bytes_hex: '22F190' })).toMatchObject({ error: { code: 'E_STOPPED' } });
  130 |     await page.screenshot({ path: info.outputPath('uds-browser-request-stop.png'), fullPage: true });
  131 |   } finally { await server.close(); }
  132 | });
  133 | 
  134 | test('real Ethernet packet list, detail, browser send and STOP', async ({ page }, info) => {
  135 |   const server = new RealSmoke();
  136 |   const hex = 'ffffffffffff02000000000108060001080006040001020000000001c0a80001000000000000c0a80002';
  137 |   try {
  138 |     await server.start();
  139 |     await open(page, server, '/p/eth/packets?channel=cap');
  140 |     expect(await server.call('eth.capture.start', { iface: 'sim0', ch: 'cap', backend: 'sim' })).toMatchObject({ ok: true });
  141 |     const t = performance.now();
  142 |     expect(await server.call('eth.send', { ch: 'cap', kind: 'raw', hex })).toMatchObject({ ok: true });
  143 |     // Device DoIP/SOME-IP announcements also arrive; pick the agent's own ARP frame.
  144 |     const packet = page.locator('.eth-list [data-seq]').filter({ hasText: 'Who has' }).first();
  145 |     await expect(packet).toBeVisible();
  146 |     await evidence(page, info, 'eth-agent-live', performance.now() - t);
  147 |     await packet.click();
```