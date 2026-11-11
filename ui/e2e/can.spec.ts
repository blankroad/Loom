// CAN Trace E2E (E-UI-3, plan §0.1): the agent sends 0x300#01 → the Trace shows the AGT
// tx row (data-request-id, data-origin="agent") and the sim BCM's DEV rx 0x301; the user
// sends 0x300#00 from the Trace send line → USR row. Opened in the real app
// with the deep link
// `#/p/can/trace?channel=ch0`.

import { expect, test } from '@playwright/test';
import { MockCtl } from './mockCtl';

interface NetProbe {
  state: { status: string };
}

const open = async (page: import('@playwright/test').Page) => {
  await page.goto('./#/p/can/trace?channel=ch0');
  await page.waitForFunction(() => (globalThis as unknown as { __loomNet?: NetProbe }).__loomNet?.state.status === 'open', null, { timeout: 15_000 });
  await expect(page.getByTestId('can-trace')).toBeVisible();
};

test('E-UI-3: agent 0x300#01 → AGT tx + DEV rx 0x301; user send line → USR row; agent read bar', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await open(page);
  // background scenario traffic (0x100/0x200) would scroll our rows away: filter like a user would
  const filter = page.locator('.ct-filter-ids');
  await filter.fill('0x300,0x301');
  await filter.press('Enter');

  const ctl = await MockCtl.open();
  try {
    const t0 = Date.now();
    const res = await ctl.agent<{ request_id: string; write_state: string }>('can.tx.send', { channel: 'ch0', id: 0x300, data_hex: '01' });
    const agentRow = page.locator(`.ct-row[data-request-id="${res.request_id}"]`);
    await expect(agentRow).toHaveAttribute('data-origin', 'agent');
    await expect(agentRow).toHaveAttribute('data-dir', 'tx');
    await expect(agentRow).toHaveAttribute('data-state', 'confirmed');
    await expect(agentRow).toContainText('AGT');
    const rx = page.locator('.ct-row[data-origin="device"][data-id="0x301"]').last();
    await expect(rx).toContainText('01 A5');
    console.log(`[E-UI-3] agent RPC start → AGT row confirmed + DEV 0x301 visible: ${Date.now() - t0} ms (includes RPC round trip)`);

    // user intervention from the send line
    const input = page.getByTestId('can-send-input');
    await input.fill('0x300#00');
    await input.press('Enter');
    const userRow = page.locator('.ct-row[data-origin="user"][data-dir="tx"]').last();
    await expect(userRow).toHaveAttribute('data-state', 'confirmed');
    await expect(userRow).toContainText('USR(나)');
    await expect(page.getByTestId('can-send-msg')).toContainText('USR ▶ 0x300#00 → r-');
    await expect(page.locator('.ct-row[data-origin="device"][data-id="0x301"]').last()).toContainText('00 A5');

    // the agent reads the trace → "에이전트가 읽은 범위" shows up
    await ctl.agent('can.trace.query', { channel: 'ch0', max: 20 });
    await expect(page.getByTestId('can-agent-read')).toContainText('에이전트 읽음 seq');
    await expect(page.locator('.ct-row[data-agent-read="1"]').first()).toBeVisible();
  } finally {
    ctl.close();
  }
  expect(errors).toEqual([]);
});

test('display / record / source gaps render as distinct rows', async ({ page }) => {
  await open(page);
  const ctl = await MockCtl.open();
  try {
    await ctl.call('mock.inject', { kind: 'display_gap', domain: 'can', channel: 'ch0', count: 7 });
    await ctl.call('mock.inject', { kind: 'record_gap', domain: 'can', channel: 'ch0', count: 3 });
    await ctl.call('mock.inject', { kind: 'source_gap', domain: 'can', channel: 'ch0', count: 37 });
    // pause so the background traffic does not scroll the gap rows away
    await expect(page.locator('.ct-row[data-gap="source.gap"]')).toHaveCount(1);
    await page.getByTestId('can-pause').click();
    await expect(page.locator('.ct-row[data-gap="display.gap"]').last()).toContainText('7 프레임 화면 생략');
    await expect(page.locator('.ct-row[data-gap="record.gap"]').last()).toContainText('기록 손실 3');
    await expect(page.locator('.ct-row[data-gap="source.gap"]').last()).toContainText('SOURCE LOSS (driver_overrun)');
    await expect(page.getByTestId('can-live')).toContainText('PAUSED');
  } finally {
    ctl.close();
  }
});
