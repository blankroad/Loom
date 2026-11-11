// UI-T07 불변식·보안 (plan §11, §4.3 I-UI1, §10):
//  - a device sends terminal QUERIES (DA1, DA2, DSR, CPR, DECRQSS, OSC 4/10/11/12 '?') → the
//    terminal answers none of them: the daemon receives 0 uart.input from the tab (server-side
//    count, mock conn_stats) and the status line counts the blocked queries (DOM)
//  - a readonly (watch) tab: queries + key presses → 0 uart.input, "보기 전용(watch)"
//  - a panel that imports net/store or uses WebSocket/fetch/navigator.serial fails ESLint
//  - CSP and Permissions-Policy are present on the served page (production: loomd serves /ui/*)

import fs from 'node:fs';
import path from 'node:path';
import { ESLint } from 'eslint';
import { Daemon, expect, mockOnly, openApp, pageQuery, sleep, test, type G } from './lib';

const UI = path.resolve(import.meta.dirname, '..', '..');

async function uartInputs(ctl: Daemon, connId: string): Promise<number> {
  const s = await ctl.inject<{ conns: { id: string; calls: Record<string, number> }[] }>('conn_stats');
  return s.conns.find((c) => c.id === connId)?.calls['uart.input'] ?? 0;
}

const connIdOf = (page: import('@playwright/test').Page) => page.evaluate(() => (globalThis as unknown as G).__loomNet?.state.hello?.conn_id ?? '');

test.describe('UI-T07 security', () => {
  test('terminal query sequences from the device → no unintended TX, blocked count shown', async ({ page }) => {
    mockOnly('needs mock.inject to make the device send query sequences');
    await openApp(page, '/l/uart-only');
    const ctl = await Daemon.open();
    try {
      const connId = await connIdOf(page);
      await expect(page.locator('.xterm')).toBeVisible();
      const before = await uartInputs(ctl, connId);
      await ctl.inject('uart_queries', { channel: 'dut' });
      await expect(page.getByTestId('uart-status')).toContainText(/차단 질의×([5-9]|\d\d)/);
      await sleep(500); // any (wrong) auto-answer would have gone out by now
      expect(await uartInputs(ctl, connId)).toBe(before);
    } finally {
      ctl.close();
    }
  });

  test('readonly (watch) tab: queries and key presses send nothing', async ({ page }) => {
    mockOnly('needs ?rights=watch on the mock');
    await openApp(page, '/l/uart-only', pageQuery({ loom_rights: 'watch' }));
    const ctl = await Daemon.open();
    try {
      const connId = await connIdOf(page);
      await expect(page.getByTestId('uart-status')).toContainText('보기 전용(watch)');
      await ctl.inject('uart_queries', { channel: 'dut' });
      await page.locator('.xterm').first().click();
      await page.keyboard.type('reboot');
      await page.keyboard.press('Enter');
      await page.keyboard.press('Control+c');
      await sleep(500);
      expect(await uartInputs(ctl, connId)).toBe(0);
    } finally {
      ctl.close();
    }
  });

  test('a panel violating I-UI1 fails ESLint (fixture linted as src/panels/bad/badPanel.ts)', async () => {
    const code = fs.readFileSync(path.join(UI, 'fixtures', 'lint', 'bad-panel.ts.txt'), 'utf8');
    const eslint = new ESLint({ cwd: UI });
    const [res] = await eslint.lintText(code, { filePath: path.join(UI, 'src', 'panels', 'bad', 'badPanel.ts') });
    const rules = res!.messages.map((m) => m.ruleId);
    const bad = code.split('\n').filter((l) => l.includes('// BAD')).length;
    expect(res!.errorCount).toBeGreaterThanOrEqual(bad);
    expect(rules).toEqual(expect.arrayContaining(['no-restricted-imports', 'no-restricted-globals', 'no-restricted-properties']));
    // the same code outside src/panels is not an I-UI1 violation (rules are scoped to panels)
    const [shell] = await eslint.lintText(code, { filePath: path.join(UI, 'src', 'shell', 'notAPanel.ts') });
    expect(shell!.messages.filter((m) => m.ruleId?.startsWith('no-restricted')).length).toBe(0);
  });

  test('served page carries CSP and Permissions-Policy (WebSerial/WebUSB/WebHID blocked)', async ({ page }) => {
    test.fail(true, 'the Vite dev server sends neither header and index.html has no CSP meta; loomd serving /ui/* is CORE P1 (report §5 L-3)');
    const res = await page.goto('./');
    const h = res!.headers();
    const meta = await page.locator('meta[http-equiv="Content-Security-Policy"]').count();
    expect(h['content-security-policy'] ?? (meta ? 'meta' : undefined)).toBeTruthy();
    expect(h['permissions-policy'] ?? '').toMatch(/serial=\(\)/);
  });
});
