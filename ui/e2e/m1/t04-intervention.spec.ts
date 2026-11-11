// UI-T04 사용자 개입 3종 (plan §11, §0.1 E-UI-1 T2–T5, E-UI-2, E-UI-3):
//  UART: user key → chip `나` → agent send waits (chip ⏳ 에이전트 대기) → Ctrl-C → the agent's
//        result carries others_since_last.count ≥ 1 and the user's Activity line shows
//        `⇢ 에이전트에게 전달됨`
//  CAN : one send from the Trace send line → USR(나) row
//  UDS : command line request (USR row) + [제어 가져오기] → the agent's next R1+ request fails with
//        exit 6 (lease preempted) and its row ends ✗ failed
// Mock only: user writes from an automation browser are capped to watch against a non-mock daemon
// (V5, shellControl.automationCapped), and fixture replay has no agent.

import { Daemon, expect, mockOnly, openApp, RpcError, test } from './lib';

test.describe('UI-T04 user intervention', () => {
  test.beforeEach(() => mockOnly('V5: automation browser is watch-only against loomd; replay has no agent'));

  test('UART: key → 나 → agent waits → Ctrl-C → others_since_last ≥ 1 + ⇢ 전달됨', async ({ page }) => {
    await openApp(page, '/l/uart-only');
    const ctl = await Daemon.open();
    try {
      const chip = page.getByTestId('input-chip');
      await expect(chip).toHaveAttribute('data-kind', 'free');
      await page.locator('.xterm').first().click();
      await page.keyboard.type('dm');
      await expect(chip).toHaveAttribute('data-kind', 'me');
      await expect(chip).toContainText('입력권: 나');

      const agent = ctl.agent<{ request_id: string; write_state: string; others_since_last?: { count: number; origins: string[] } }>('uart.send', { channel: 'dut', data: 'reboot' }, 15_000);
      await expect(chip).toHaveAttribute('data-kind', 'deferred');
      await expect(chip).toContainText('⏳ 에이전트 대기 "reboot⏎"');

      await page.keyboard.press('Control+c'); // 0x03 at once (TerminalView), ends the user's line
      const r = await agent;
      expect(r.write_state).toBe('sent');
      expect(r.others_since_last?.count ?? 0).toBeGreaterThanOrEqual(1);
      expect(r.others_since_last?.origins).toContain('user');

      await expect(chip).toHaveAttribute('data-kind', 'free');
      await expect(page.getByTestId('uart-status')).toContainText('AGT ▶ "reboot⏎"');
      const userLine = page.locator('.act-line[data-origin=user]').filter({ hasText: 'input' }).first();
      await expect(userLine).toContainText('⇢ 에이전트에게 전달됨');
    } finally {
      ctl.close();
    }
  });

  test('CAN: one send from the Trace send line → USR(나) row, confirmed', async ({ page }) => {
    await openApp(page, '/p/can/trace?channel=ch0');
    const filter = page.locator('.ct-filter-ids');
    await filter.fill('0x300,0x301');
    await filter.press('Enter');
    const input = page.getByTestId('can-send-input');
    await input.fill('0x300#00');
    await input.press('Enter');
    const row = page.locator('.ct-row[data-origin="user"][data-dir="tx"]').last();
    await expect(row).toHaveAttribute('data-state', 'confirmed');
    await expect(row).toContainText('USR(나)');
    const rid = await row.getAttribute('data-request-id');
    expect(rid).toMatch(/^r-\d+$/);
    await expect(page.getByTestId('can-send-msg')).toContainText(`→ ${rid}`);
  });

  test('UDS: command line 22 F190 → USR row; [제어 가져오기] → agent R2 request exit 6, row ✗', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await ctl.inject('reset');
      await openApp(page, '/p/uds/console?targets=bcm');
      const input = page.getByLabel('UDS 요청 (hex)');
      await input.fill('22 F190');
      await expect(page.getByTestId('uds-preview')).toContainText(/F190|VIN|R0/); // dry-run preview, no row
      await input.press('Enter');
      const userRow = page.locator('.vt-row:not([hidden])[data-origin=user]').last();
      await expect(userRow).toHaveAttribute('data-state', 'confirmed');
      await expect(userRow).toContainText('USR');
      // dry-run previews never become rows (uds-ui U3): exactly one user row
      await expect(page.locator('.vt-row:not([hidden])[data-origin=user]')).toHaveCount(1);

      await page.getByTestId('uds-targets').getByRole('button', { name: /제어 가져오기/ }).click();
      await expect(page.getByTestId('uds-targets')).toContainText('제어: 나');

      const err = await ctl.agent('uds.request', { target: 'bcm', hex: '1003' }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err).toBeInstanceOf(RpcError);
      expect(err!.data).toMatchObject({ code: 'lease_preempted', exit_code: 6 });
      const agentRow = page.locator(`.vt-row:not([hidden])[data-request-id="${String(err!.data!.request_id)}"]`);
      await expect(agentRow).toHaveAttribute('data-origin', 'agent');
      await expect(agentRow).toHaveAttribute('data-state', 'failed');
      // the user's takeover reached the agent with that result
      expect((err!.data!.others_since_last as { count: number }).count).toBeGreaterThanOrEqual(1);
    } finally {
      await ctl.inject('reset');
      ctl.close();
    }
  });
});
