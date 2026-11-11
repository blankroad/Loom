/// <reference lib="dom" />
// Opt-in actual daemon/sim tests. No mock, replay or webdriver hiding.
import { expect, test, type Page, type TestInfo } from '@playwright/test';
import { performance } from 'node:perf_hooks';
import { RealSmoke } from './real-smoke-server';
import { RealTc1 } from './real-tc1-server';

const payload = (r: Record<string, unknown>) => (r.data ?? r) as Record<string, unknown>;
async function open(page: Page, server: RealSmoke, route: string) {
  await page.goto(await server.loginURL(route));
  await expect(page.getByTestId('loom-shell')).toBeVisible();
  // Master F1: default Auto policy permits granted operate rights even under automation.
  await expect(page.getByTestId('rights')).toContainText('operate');
}
async function evidence(page: Page, info: TestInfo, name: string, elapsed: number) {
  await info.attach(`${name}-command-to-visible`, {
    body: JSON.stringify({ milliseconds: elapsed, measurement: 'agent RPC start to DOM assertion completion; upper bound, not paint timestamp' }),
    contentType: 'application/json',
  });
  await page.screenshot({ path: info.outputPath(`${name}.png`), fullPage: true });
}
async function armCanPaint(page: Page, marker: string) {
  await page.evaluate((hex) => {
    const w = window as typeof window & { __loomCanPaint?: Promise<void> };
    w.__loomCanPaint = new Promise<void>((resolve, reject) => {
      const observer = new MutationObserver(() => {
        const rows = document.querySelectorAll('.ct-row[data-dir="rx"][data-origin="device"][data-id="0x301"]');
        if (![...rows].some((row) => row.textContent?.includes(hex))) return;
        observer.disconnect(); clearTimeout(timer);
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      });
      const timer = setTimeout(() => { observer.disconnect(); reject(new Error('CAN device frame paint timeout')); }, 3000);
      observer.observe(document.querySelector('[data-testid="can-trace"]')!, { subtree: true, childList: true, characterData: true, attributes: true });
    });
  }, marker);
}
test.beforeEach(() => test.skip(!process.env.LOOM_REAL_SMOKE, 'use LOOM_REAL_SMOKE=1 and real-smoke.config.ts'));

test('real UART agent trace, browser input and STOP', async ({ page }, info) => {
  const server = new RealSmoke();
  try {
    await server.start();
    await open(page, server, '/p/uart/trace?channel=dut');
    const t = performance.now();
    expect(await server.call('uart.open', { target: 'sim://bootshell?split=0', alias: 'dut', profile: 'linux-shell' })).toMatchObject({ ok: true });
    await expect(page.locator('.uart-trace')).toContainText('U-Boot');
    await evidence(page, info, 'uart-agent-live', performance.now() - t);
    expect(await server.call('uart.send', { channel: 'dut', data: Buffer.from('root').toString('hex'), eol: 'cr' })).toMatchObject({ ok: true });
    await expect(page.locator('.uart-trace [data-origin="agent"][data-dir="tx"]')).toContainText('root');
    await open(page, server, '/p/uart/terminal?channel=dut');
    const terminal = page.locator('.xterm-helper-textarea');
    await expect(terminal).toBeAttached();
    await terminal.focus();
    await page.keyboard.type('secret'); await page.keyboard.press('Enter');
    expect(await server.call('uart.expect', { channel: 'dut', patterns: [{ kind: 'literal', pattern: 'root@sim:~# ' }], timeout_ms: 3000 })).toMatchObject({ ok: true });
    await expect(page.getByTestId('input-chip')).toContainText('나');
    await page.keyboard.type('echo browser-p1'); await page.keyboard.press('Enter');
    expect(await server.call('uart.expect', { channel: 'dut', patterns: [{ kind: 'literal', pattern: 'browser-p1' }], timeout_ms: 3000 })).toMatchObject({ ok: true });
    await page.getByTestId('stop-button').click();
    await expect(page.getByTestId('stop-panel')).toBeVisible();
    expect(await server.call('uart.send', { channel: 'dut', data: '78', eol: 'cr' })).toMatchObject({ error: { code: 'E_STOPPED' } });
    await page.screenshot({ path: info.outputPath('uart-browser-input-stop.png'), fullPage: true });
  } finally { await server.close(); }
});

test('real CAN agent frames, browser send and STOP', async ({ page }, info) => {
  const server = new RealSmoke();
  try {
    await server.start();
    await open(page, server, '/p/can/trace?channel=ch0');
    expect(await server.call('can.open', { channel: 'ch0' })).toMatchObject({ ok: true });
    const t = performance.now();
    expect(await server.call('can.send', { channel: 'ch0', frame: '300#55' })).toMatchObject({ ok: true });
    await expect(page.getByTestId('can-trace')).toContainText('55');
    await evidence(page, info, 'can-agent-live', performance.now() - t);
    const samples: number[] = [];
    for (let i = 0; i < 20; i++) {
      const marker = (0xa0 + i).toString(16).toUpperCase();
      await armCanPaint(page, marker);
      const before = performance.now();
      expect(await server.call('can.send', { channel: 'ch0', frame: `300#${marker}` })).toMatchObject({ ok: true });
      await page.evaluate(async () => {
        const w = window as typeof window & { __loomCanPaint?: Promise<void> };
        await w.__loomCanPaint; delete w.__loomCanPaint;
      });
      samples.push(performance.now() - before);
    }
    const p95 = [...samples].sort((a, b) => a - b)[Math.ceil(samples.length * .95) - 1]!;
    await info.attach('can-device-paint-upper-bound', { body: JSON.stringify({ samples, p95_ms: p95,
      method: 'agent command start through matching device RX DOM and two RAFs; includes device response time and test transport overhead',
      measure: 'conservative end-to-end upper bound using the documented next-RAF paint proxy' }), contentType: 'application/json' });
    expect(p95).toBeLessThan(150);
    const input = page.getByTestId('can-send-input');
    await expect(input).toBeEnabled(); await input.fill('0x300#66'); await input.press('Enter');
    await expect(page.getByTestId('can-send-msg')).not.toHaveText('');
    await expect(page.getByTestId('can-trace').locator('[data-origin="user"]').filter({ hasText: '66' }).first()).toBeVisible();
    await page.getByTestId('stop-button').click();
    await expect(page.getByTestId('stop-panel')).toBeVisible();
    expect(await server.call('can.send', { channel: 'ch0', frame: '300#77' })).toMatchObject({ error: { code: 'E_STOPPED' } });
    await page.screenshot({ path: info.outputPath('can-browser-send-stop.png'), fullPage: true });
  } finally { await server.close(); }
});

test('real UDS agent transaction, browser request and STOP', async ({ page }, info) => {
  const server = new RealSmoke();
  try {
    await server.start();
    await open(page, server, '/p/uds/console?targets=ecu');
    expect(await server.call('uds.connect', { name: 'ecu', sim: 'bench' })).toMatchObject({ ok: true });
    const t = performance.now();
    const response = await server.call('uds.request', { target: 'ecu', bytes_hex: '22F190' });
    expect(response).toMatchObject({ ok: true });
    const rid = String(payload(response).request_id);
    expect(rid).not.toBe('undefined');
    await expect(page.locator('.uds-panel').locator(`[data-request-id="${rid}"][data-state="confirmed"]`)).toBeVisible();
    await evidence(page, info, 'uds-agent-live', performance.now() - t);
    const input = page.getByLabel('UDS 요청 (hex)');
    await expect(input).toBeEnabled(); await input.fill('22 F18C');
    await expect(page.getByTestId('uds-preview')).not.toHaveText('');
    await input.press('Enter');
    await expect(page.locator('.uds-panel [data-origin="user"][data-state="confirmed"]')).toBeVisible();
    await page.getByTestId('stop-button').click();
    await expect(page.getByTestId('stop-panel')).toBeVisible();
    expect(await server.call('uds.request', { target: 'ecu', bytes_hex: '22F190' })).toMatchObject({ error: { code: 'E_STOPPED' } });
    await page.screenshot({ path: info.outputPath('uds-browser-request-stop.png'), fullPage: true });
  } finally { await server.close(); }
});

test('real Ethernet packet list, detail, browser send and STOP', async ({ page }, info) => {
  const server = new RealSmoke();
  const hex = 'ffffffffffff02000000000108060001080006040001020000000001c0a80001000000000000c0a80002';
  try {
    await server.start();
    await open(page, server, '/p/eth/packets?channel=cap');
    expect(await server.call('eth.capture.start', { iface: 'sim0', ch: 'cap', backend: 'sim' })).toMatchObject({ ok: true });
    const t = performance.now();
    expect(await server.call('eth.send', { ch: 'cap', kind: 'raw', hex })).toMatchObject({ ok: true });
    const packet = page.locator('.eth-list [data-seq]').first();
    await expect(packet).toBeVisible();
    await evidence(page, info, 'eth-agent-live', performance.now() - t);
    await packet.click();
    await expect(page.getByTestId('eth-detail-info')).not.toHaveText('');
    await page.getByRole('button', { name: '편집 후 송신…', exact: true }).click();
    await page.getByLabel('송신할 프레임 hex').fill(hex);
    await expect(page.getByTestId('eth-edit-preview')).not.toHaveText('');
    await page.getByTestId('eth-edit-dialog').getByRole('button', { name: '보내기', exact: true }).click();
    await expect(page.getByTestId('eth-edit-dialog')).not.toBeVisible();
    await expect(page.locator('.eth-panel [data-origin="user"]')).not.toHaveCount(0);
    await page.getByTestId('stop-button').click();
    await expect(page.getByTestId('stop-panel')).toBeVisible();
    expect(await server.call('eth.send', { ch: 'cap', kind: 'raw', hex })).toMatchObject({ error: { code: 'E_STOPPED' } });
    await page.screenshot({ path: info.outputPath('eth-browser-detail-send-stop.png'), fullPage: true });
  } finally { await server.close(); }
});

test('actual powered TC-1 appears live with journal verdicts', async ({ page }, info) => {
  test.setTimeout(90_000);
  expect(process.env.LOOM_TC1_TEST_BIN, 'set LOOM_TC1_TEST_BIN to the default-feature tc1_live_smoke executable').toBeTruthy();
  const fixture = new RealTc1();
  try {
    await fixture.start();
    await page.goto(fixture.loginURL());
    await expect(page.getByTestId('loom-shell')).toBeVisible();
    await expect(page.getByTestId('rights')).toContainText('operate');
    await expect(page.getByTestId('test-screen')).toBeVisible();
    // The Rust fixture already has one viewer. A second subscribed viewer proves this
    // browser joined the live stream before the agent starts the TC.
    await expect(page.getByText(/보는 중:\s*\d+\/(?:[2-9]|\d{2,})\b/)).toBeVisible();
    await fixture.begin();
    await expect(page.getByTestId('test-screen')).toContainText('TC-1');
    await expect(page.getByTestId('test-steps')).toContainText('S2');
    expect(await fixture.result()).toMatchObject({ result: 'passed' });
    await expect(page.getByTestId('test-result')).toContainText('passed');
    await expect(page.getByTestId('test-state')).toContainText('DONE');
    await page.screenshot({ path: info.outputPath('actual-tc1-journal-verdict.png'), fullPage: true });
  } finally { await fixture.close(); }
});
