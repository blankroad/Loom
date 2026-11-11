// UI-T08 자격 (plan §11, §0.4 F1/E9):
//  - a watch tab: send line / UART input / UDS command line disabled ("보기 전용"), STOP enabled;
//    the SERVER refuses the same writes from a watch connection (direct RPC)
//  - R2 card paths (F1: approvals only exist when a policy turns them on): auto (no card, the write
//    runs) and viewer approval (policy fixture: card → approve → the agent's write proceeds).
//    The token path is CLI-side (no UI) — not a browser test.

import { backend, Daemon, DAEMON_URL, expect, mockOnly, openApp, pageQuery, REPLAY_WATCH_URL, RpcError, test } from './lib';

test.describe('UI-T08 rights', () => {
  test('watch tab: inputs disabled, STOP enabled; the server refuses watch writes @replay', async ({ page }) => {
    const replay = backend() === 'replay';
    await openApp(page, '/l/m1-default', replay ? pageQuery({}, REPLAY_WATCH_URL) : pageQuery({ loom_rights: 'watch' }));
    await expect(page.locator('.rights')).toContainText('watch');
    await expect(page.getByTestId('can-send-input')).toBeDisabled();
    await expect(page.getByLabel('UDS 요청 (hex)')).toBeDisabled();
    await expect(page.getByTestId('uart-status')).toContainText('보기 전용(watch)');
    await expect(page.getByTestId('stop-button')).toBeEnabled();

    // server side: a watch connection's write is refused
    const url = replay ? REPLAY_WATCH_URL : `${DAEMON_URL.mock}?rights=watch`;
    const w = await Daemon.open(url, 'e2e-watch');
    try {
      const err = await w.call('can.send', { channel: 'ch0', frame: '300#01' }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err).toBeInstanceOf(RpcError);
      expect(String(err!.data?.code)).toMatch(/rights|denied/i);
    } finally {
      w.close();
    }
  });

  test('R2 card: auto by default (no card); policy on → approval card → approve → the write runs', async ({ page }) => {
    mockOnly('needs the policy fixture (mock.inject policy_confirm)');
    await openApp(page, '/p/can/trace?channel=ch0');
    const filter = page.locator('.ct-filter-ids');
    await filter.fill('0x300,0x301');
    await filter.press('Enter');
    const ctl = await Daemon.open();
    try {
      await ctl.inject('reset'); // no approval left pending by an earlier run
      await ctl.inject('pause_scenario_agent', { on: true }); // background agent uart.send must not raise a second approval
      // auto (F1 default): no card, the agent write completes
      const auto = await ctl.agent<{ write_state: string }>('can.send', { channel: 'ch0', frame: '300#03' });
      expect(auto.write_state).toBe('confirmed');
      await expect(page.getByTestId('approval-card')).toHaveCount(0);
      // policy turns approvals on
      await ctl.inject('policy_confirm', { on: true });
      const pending = ctl.agent<{ request_id: string; write_state: string }>('can.send', { channel: 'ch0', frame: '300#04' }, 20_000);
      // F1: approvals exist only with a policy; the top bar shows "⚠ 승인 N" and opens the queue
      await page.locator('.approvals-btn').click();
      const card = page.locator('[data-testid=approval-card][data-domain=can]');
      await expect(card).toBeVisible();
      await expect(card).toHaveAttribute('data-origin', 'agent');
      await expect(card).toHaveAttribute('data-armed', 'true', { timeout: 3_000 }); // armed after a delay
      await page.getByTestId('approve-btn').click();
      const r = await pending;
      expect(r.write_state).toBe('confirmed');
      await expect(page.locator(`.ct-row[data-request-id="${r.request_id}"]`)).toHaveAttribute('data-state', 'confirmed');
      await expect(card).toHaveCount(0);
    } finally {
      await ctl.inject('policy_confirm', { on: false });
      await ctl.inject('pause_scenario_agent', { on: false });
      ctl.close();
    }
  });
});
