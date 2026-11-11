// UI-T06 STOP (plan §11, §0.1 T6, contract 04 A3/A15, v1.1):
//  - click → the band changes within one frame ("STOP 요청 중…" / locked), then the server's
//    core.latch shows "STOP — 잠김" + the STOP panel
//  - agent writes are refused (row requested → ✗ failed, exit 6 equivalent stop_latched)
//  - user UART keys still pass; recovery send runs with one key (Enter on the focused button)
//  - WS down → HTTP fallback `POST /api/v1/stop` still latches
//  - a watch tab can STOP (release needs operate)
// The latch is released through a direct RPC in `finally` so later tests start clean.

import { backend, Daemon, DAEMON_URL, expect, mockOnly, openApp, pageQuery, REPLAY_WATCH_URL, RpcError, test, type G } from './lib';

async function release(url = DAEMON_URL[backend()]): Promise<void> {
  // release needs operate: on replay the 4790 daemon grants operate; on mock the default is operate
  const d = await Daemon.open(url === REPLAY_WATCH_URL ? DAEMON_URL.replay : url, 'e2e-release');
  try {
    await d.call('core.stop.release').catch(() => undefined);
  } finally {
    d.close();
  }
}

test.describe('UI-T06 STOP', () => {
  test.afterEach(async () => release());

  test('click → band within 1 frame → server latch → STOP panel @replay', async ({ page }) => {
    await openApp(page, '/p/can/trace?channel=ch0');
    const phases = await page.evaluate(async () => {
      const g = globalThis as unknown as G;
      const btn = g.document.querySelector('[data-testid=stop-button]')!;
      const before = btn.getAttribute('data-phase');
      btn.click();
      await new Promise<void>((r) => g.requestAnimationFrame(() => r()));
      return { before, afterOneFrame: g.document.querySelector('[data-testid=stop-button]')!.getAttribute('data-phase') };
    });
    expect(phases.before).toBe('idle');
    expect(['requesting', 'locked']).toContain(phases.afterOneFrame);
    await expect(page.getByTestId('stop-button')).toHaveAttribute('data-phase', 'locked');
    await expect(page.getByTestId('stop-button')).toContainText('STOP — 잠김');
    await expect(page.getByTestId('stop-panel')).toContainText('agent·script 쓰기 잠김');
    await expect(page.getByTestId('aria-live')).toContainText('STOP');
  });

  test('during STOP: agent write refused (row ✗ failed), user UART key passes, recovery runs with one key', async ({ page }) => {
    mockOnly();
    await openApp(page, '/l/uart-only');
    const ctl = await Daemon.open();
    try {
      await page.getByTestId('stop-button').click();
      await expect(page.getByTestId('stop-button')).toHaveAttribute('data-phase', 'locked');
      // agent refused
      const err = await ctl.agent('uart.send', { channel: 'dut', data: 'uname -a' }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err?.data?.code).toBe('stop_latched');
      // user keys pass (A3): the device echo comes back and the status line names the user send
      await page.locator('.xterm').first().click();
      await page.keyboard.type('help');
      await page.keyboard.press('Enter');
      await expect(page.getByTestId('uart-status')).toContainText('USR ▶');
      // recovery: [복구 송신 ⏎] has the focus unless the terminal has it → focus it, one Enter
      const recover = page.getByTestId('stop-panel').getByRole('button', { name: /복구 송신/ });
      await recover.focus();
      await page.keyboard.press('Enter');
      await expect(page.getByTestId('stop-panel')).toContainText('✓ 보냄');
      await expect(page.getByTestId('stop-panel')).toContainText('막힘'); // interlocked item shown, not hidden
    } finally {
      ctl.close();
    }
  });

  test('agent write during STOP leaves a ✗ failed AGT row (CAN)', async ({ page }) => {
    mockOnly();
    await openApp(page, '/p/can/trace?channel=ch0');
    const ctl = await Daemon.open();
    try {
      await ctl.call('core.stop', { scope: 'all', reason: 'e2e' });
      await expect(page.getByTestId('stop-button')).toHaveAttribute('data-phase', 'locked');
      const err = await ctl.agent('can.send', { channel: 'ch0', frame: '300#09' }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err?.data?.code).toBe('stop_latched');
      const row = page.locator(`.ct-row[data-request-id="${String(err!.data!.request_id)}"]`);
      await expect(row).toHaveAttribute('data-origin', 'agent');
      await expect(row).toHaveAttribute('data-state', 'failed');
    } finally {
      ctl.close();
    }
  });

  test('WS down → STOP goes through HTTP POST /api/v1/stop', async ({ page }) => {
    mockOnly('fixture replay has no POST /api/v1/stop (report §4 C-7)');
    await openApp(page, '/p/can/trace?channel=ch0');
    const ctl = await Daemon.open();
    try {
      await ctl.inject('refuse_ws', { ms: 5_000 });
      await ctl.inject('disconnect');
      await page.waitForFunction(() => (globalThis as unknown as G).__loomNet?.state.status !== 'open', null, { timeout: 5_000 });
      await page.getByTestId('stop-button').click();
      await expect(page.getByTestId('stop-button')).toHaveAttribute('title', /HTTP/);
      // the daemon latched without any WS: our control connection sees it
      await expect.poll(() => ctl.control.some((e) => e.kind === 'core.latch' && e.payload.on === true), { timeout: 5_000 }).toBe(true);
      // after the WS comes back the tab shows the server's latch
      await expect(page.getByTestId('stop-button')).toHaveAttribute('data-phase', 'locked', { timeout: 15_000 });
    } finally {
      await ctl.inject('reset');
      ctl.close();
    }
  });

  test('a watch tab can STOP (and cannot release) @replay', async ({ page }) => {
    test.fail(backend() === 'replay', 'CORE replay: core.stop requires operate; contract v1.1 allows every right incl. watch (report §4 C-5)');
    const replay = backend() === 'replay';
    await openApp(page, '/p/can/trace?channel=ch0', replay ? pageQuery({}, REPLAY_WATCH_URL) : pageQuery({ loom_rights: 'watch' }));
    await expect(page.locator('.rights')).toContainText('watch');
    await expect(page.getByTestId('stop-button')).toBeEnabled();
    await page.getByTestId('stop-button').click();
    await expect(page.getByTestId('stop-button')).toHaveAttribute('data-phase', 'locked', { timeout: 5_000 });
    await expect(page.getByTestId('stop-panel').getByRole('button', { name: '해제…' })).toBeDisabled();
    if (replay) await release(REPLAY_WATCH_URL);
  });
});

