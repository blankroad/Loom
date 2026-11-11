// Opt-in actual daemon/sim test. No mock, replay, injected cookies or webdriver hiding.
import { expect, test } from '@playwright/test';
import { RealSmoke } from './real-smoke-server';

test('real UART live trace and browser STOP', async ({ page }, testInfo) => {
  test.skip(!process.env.LOOM_REAL_SMOKE, 'use LOOM_REAL_SMOKE=1 and real-smoke.config.ts');
  const server = new RealSmoke();
  try {
    await server.start();
    await page.goto(await server.loginURL('/p/uart/trace?channel=dut'));
    await expect(page.getByTestId('loom-shell')).toBeVisible();
    await expect(page.getByTestId('rights')).toContainText('watch (자동화 브라우저)');
    expect(await server.call('uart.open', {
      target: 'sim://bootshell?split=0', alias: 'dut', profile: 'linux-shell',
    })).toMatchObject({ ok: true });
    await expect(page.locator('.uart-trace')).toContainText('U-Boot', { timeout: 10_000 });
    expect(await server.call('uart.send', {
      channel: 'dut', data: Buffer.from('root').toString('hex'), eol: 'cr',
    })).toMatchObject({ ok: true });
    await expect(page.locator('.uart-trace [data-origin="agent"][data-dir="tx"]')).toContainText('root');
    await expect(page.locator('.uart-trace')).toContainText('Password:');
    await page.getByTestId('stop-button').click();
    await expect(page.getByTestId('stop-panel')).toBeVisible();
    expect(await server.call('uart.send', {
      channel: 'dut', data: '78', eol: 'cr',
    })).toMatchObject({ error: { code: 'E_STOPPED' } });
    await page.screenshot({ path: testInfo.outputPath('real-uart-stop.png'), fullPage: true });
  } finally {
    await server.close();
  }
});
