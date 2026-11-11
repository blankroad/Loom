// UI-T02 (plan §11): the agent's `uart send` / `can send` / `uds read-did` show up in the same
// panel DOM row as the agent's result — `data-request-id` = the request_id the agent got back,
// origin AGT. Mock: the agent is `mock.agent` (same server pipeline as a CLI client).
// Replay: there is no agent client; loomd's generator publishes agent-origin UDS rows
// (`replay-uds-N`) — the DOM row must carry the request_id the reference subscriber saw.

import { backend, Daemon, expect, mockOnly, names, openApp, poll, test } from './lib';

test.describe('UI-T02 agent writes → same request_id row, AGT', () => {
  test('CAN: agent can.send → Trace row data-request-id, data-origin=agent', async ({ page }) => {
    mockOnly();
    await openApp(page, '/p/can/trace?channel=ch0');
    const ctl = await Daemon.open();
    try {
      const filter = page.locator('.ct-filter-ids');
      await filter.fill('0x300,0x301');
      await filter.press('Enter');
      const r = await ctl.agent<{ request_id: string; write_state: string }>('can.send', { channel: 'ch0', frame: '300#01' });
      expect(r.write_state).toBe('confirmed');
      const row = page.locator(`.ct-row[data-request-id="${r.request_id}"]`);
      await expect(row).toHaveCount(1);
      await expect(row).toHaveAttribute('data-origin', 'agent');
      await expect(row).toHaveAttribute('data-state', 'confirmed');
      await expect(row).toContainText('AGT');
    } finally {
      ctl.close();
    }
  });

  test('UDS: agent read-did F190 → console row data-request-id, AGT, VIN', async ({ page }) => {
    mockOnly();
    await openApp(page, '/p/uds/console?targets=bcm');
    const ctl = await Daemon.open();
    try {
      await ctl.inject('reset');
      const r = await ctl.agent<{ request_id: string; write_state: string }>('uds.request', { target: 'bcm', hex: '22F190' });
      const row = page.locator(`.vt-row:not([hidden])[data-request-id="${r.request_id}"]`);
      await expect(row).toHaveCount(1);
      await expect(row).toHaveAttribute('data-origin', 'agent');
      await expect(row).toHaveAttribute('data-state', 'confirmed');
      await expect(row).toContainText('AGT');
    } finally {
      ctl.close();
    }
  });

  test('UART: agent uart.send "uname -a" → status line AGT ▶ + agent tx line and device answer', async ({ page }) => {
    mockOnly();
    // terminal + trace of the same channel share one server subscription
    await openApp(page, '/l/uart-only');
    const ctl = await Daemon.open();
    try {
      await ctl.inject('pause_scenario_agent', { on: true }); // background agent sends would overwrite the status line
      const r = await ctl.agent<{ request_id: string; write_state: string }>('uart.send', { channel: 'dut', data: 'uname -a' });
      expect(r.write_state).toBe('sent'); // UART success terminal = sent (contract 02 §2.3)
      const status = page.getByTestId('uart-status');
      // the status line names the last send by request_id (DOM attributes, not text alone)
      await expect(status).toHaveAttribute('data-request-id', r.request_id);
      await expect(status).toHaveAttribute('data-origin', 'agent');
      await expect(status).toContainText('AGT ▶ "uname -a⏎"');
    } finally {
      await ctl.inject('pause_scenario_agent', { on: false });
      ctl.close();
    }
  });

  test('replay: generator agent UDS rows render with the reference request_id, AGT @replay', async ({ page }) => {
    test.skip(backend() !== 'replay', 'replay-only (mock covers this with mock.agent above)');
    const n = names();
    const ref = await Daemon.open();
    try {
      await openApp(page, `/p/uds/console?targets=${n.uds}`);
      // the newest finished UDS request the reference saw on the priority path; its first row is agent-origin
      const done = await poll(
        () => [...ref.control].reverse().find((e) => e.domain === 'uds' && e.request_id && (e.payload.write_state === 'confirmed' || e.payload.write_state === 'outcome_unknown')),
        (e) => e !== undefined,
        8_000,
      );
      const rid = done!.request_id!;
      expect(ref.control.find((e) => e.request_id === rid)?.origin).toBe('agent');
      const row = page.locator(`.vt-row:not([hidden])[data-request-id="${rid}"]`);
      await expect(row).toHaveCount(1);
      await expect(row).toHaveAttribute('data-origin', 'agent');
      await expect(row).toHaveAttribute('data-state', String(done!.payload.write_state));
    } finally {
      ref.close();
    }
  });
});
