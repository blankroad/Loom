// eth.packets acceptance 1–7 (_xteam/eth-ui.md "수용 시험", Playwright on the real DOM), mock sim tap
// (tools/mock/eth.ts). Layout `#/l/eth` = eth.packets (lan1) + UDS console (bcm).
// Mock only: fixture replay has no eth domain; user sends are watch-capped against loomd (V5).

import type { Page } from '@playwright/test';
import { Daemon, expect, mockOnly, openApp, poll, RpcError, sleep, test } from './lib';

const LIST = '.eth-list';

async function startCapture(ctl: Daemon): Promise<void> {
  await ctl.inject('reset');
  await ctl.call('core.stop.release').catch(() => undefined);
  await ctl.agent('eth.capture.start', { ch: 'lan1', iface: 'lo' });
}

const kernelDrop = async (page: Page): Promise<number> => Number(/커널 드롭 ([\d,]+)/.exec((await page.getByTestId('eth-status').textContent()) ?? '')?.[1]?.replace(/,/g, '') ?? 'NaN');

test.describe('eth.packets acceptance (sim tap)', () => {
  test.beforeEach(() => mockOnly('fixture replay has no eth domain'));

  test('1: agent capture + read-did → DoIP rows (data-seq, data-request-id) → ↔UDS chip focuses the UDS row', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await startCapture(ctl);
      await openApp(page, '/l/eth');
      await expect(page.getByTestId('eth-status')).toContainText('LIVE');
      await expect(page.getByTestId('eth-status')).toContainText('[SIM tap]');
      const sub = await ctl.subscribe({ domains: ['eth'], channels: ['lan1'], kinds: ['eth.pkt'] }, false);
      const t0 = Date.now();
      const u = await ctl.agent<{ request_id: string }>('uds.request', { target: 'bcm', hex: '22F190', mock_pending: 0 });
      const req = page.locator(`${LIST} .vt-row:not([hidden])[data-request-id="${u.request_id}"]`).first();
      await expect(req).toBeVisible();
      test.info().annotations.push({ type: 'eth 1: read-did reply → DoIP row visible', description: `${Date.now() - t0} ms (incl. RPC)` });
      const pkt = await poll(() => sub.events.find((e) => (e.payload.link as { request_id?: string } | undefined)?.request_id === u.request_id), (e) => e !== undefined);
      await expect(page.locator(`${LIST} .vt-row[data-seq="${pkt!.seq}"]`)).toHaveAttribute('data-request-id', u.request_id);
      await expect(req).toHaveAttribute('data-origin', 'agent'); // attributed_origin badge
      await req.getByText('↔UDS').click();
      // the UDS console (same layout) selects that transaction and takes the focus
      const udsGrid = page.getByRole('grid', { name: 'UDS 트랜잭션' });
      await expect(udsGrid.locator(`.vt-row.selected:not([hidden])[data-request-id="${u.request_id}"]`)).toHaveCount(1);
      await expect(udsGrid).toBeFocused();
    } finally {
      ctl.close();
    }
  });

  test('2: filter doip.type == 0x8001 → complete rows = eth.query count; bad expression → underline + message', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await startCapture(ctl);
      await ctl.agent('uds.request', { target: 'bcm', hex: '22F190', mock_pending: 0 });
      await openApp(page, '/p/eth/packets?channel=lan1');
      const input = page.getByTestId('eth-filter-input');
      await input.fill('doip.type ==');
      await expect(page.getByTestId('eth-filter-err')).toContainText('위치 12: 값 필요');
      await expect(page.getByTestId('eth-filter-err-mark')).toBeVisible();
      await input.fill('doip.type == 0x8001');
      await expect(page.getByTestId('eth-filter-err')).toBeHidden();
      await input.press('Enter');
      const badge = page.getByTestId('eth-mode');
      await expect(badge).toHaveAttribute('data-mode', 'complete');
      await expect(badge).toContainText(/완전\(저널 기준\) · [\d,]+행/);
      const shown = Number(/· ([\d,]+)행/.exec((await badge.textContent()) ?? '')![1]!.replace(/,/g, ''));
      const to = /seq \d+–(\d+)/.exec((await badge.textContent()) ?? '')?.[1];
      const q = await ctl.call<{ rows: unknown[] }>('eth.query', { ch: 'lan1', filter: 'doip.type == 0x8001', max: 2000, ...(to ? { range: { to } } : {}) });
      expect(shown).toBe(q.rows.length);
      expect(shown).toBeGreaterThan(0);
      // the rows on screen are the filtered ones
      const protos = await page.locator(`${LIST} .vt-row[data-seq]:visible`).allTextContents(); // pooled rows beyond the list are hidden
      // honesty: the screen shows exactly the complete rows the badge counts (stale pooled rows must be hidden)
      if (!protos.every((t) => t.includes('DoIP'))) test.info().annotations.push({ type: 'eth 2 visible rows', description: `${await badge.textContent()} but ${protos.length} rows visible: ${protos.map((t) => t.slice(0, 40)).join(' | ')}` });
      expect(protos.length).toBe(shown);
      expect(protos.length).toBeGreaterThan(0);
      expect(protos.every((t) => t.includes('DoIP'))).toBe(true);
    } finally {
      ctl.close();
    }
  });

  test('3: edit-and-send (DID changed) → USR tx row, NIC 관찰 ✓; the agent hears of it (others_since_last = 1)', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await startCapture(ctl);
      await openApp(page, '/p/eth/packets?channel=lan1');
      await ctl.agent('uds.request', { target: 'bcm', hex: '22F190', mock_pending: 0 }); // also resets the agent cursor
      const row = page.locator(`${LIST} .vt-row[data-request-id]`).first();
      await row.click();
      await expect(page.getByTestId('eth-detail-info')).toContainText('eth:lan1@seq:');
      await page.getByRole('button', { name: '편집 후 송신…' }).click();
      const dlg = page.getByTestId('eth-edit-dialog');
      const ta = dlg.getByLabel('송신할 프레임 hex');
      const hex = (await ta.inputValue()).trim();
      await ta.fill(hex.replace(/f1 90$/i, 'f1 8c')); // DID F190 → F18C
      await expect(page.getByTestId('eth-edit-preview')).toContainText(/DoIP|Diag/);
      await dlg.getByRole('button', { name: '보내기' }).click();
      await expect(dlg).toHaveCount(0);
      const tx = page.locator('.eth-tx .vt-row:not([hidden])[data-origin=user]').last();
      await expect(tx).toHaveAttribute('data-state', 'confirmed');
      await expect(tx).toContainText('NIC 관찰 ✓');
      const next = await ctl.agent<{ others_since_last?: { count: number } }>('uds.request', { target: 'bcm', hex: '22F190', mock_pending: 0 });
      expect(next.others_since_last?.count).toBe(1);
    } finally {
      ctl.close();
    }
  });

  test('4: high-rate replay → sampling badge; pause → complete range = eth.query count', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await startCapture(ctl);
      await openApp(page, '/p/eth/packets?channel=lan1');
      await ctl.inject('eth_flood', { ch: 'lan1', pps: 200_000, seconds: 3, sample: 1000 });
      const badge = page.getByTestId('eth-mode');
      await expect(badge).toContainText('표시 1/1000 샘플링');
      await expect(badge).toContainText('기록 100 %');
      await sleep(1_000);
      await page.getByRole('button', { name: /일시정지/ }).click();
      await expect(badge).toHaveAttribute('data-mode', 'complete');
      await expect(badge).toContainText(/완전\(저널 기준\) · [\d,]+행 seq \d+–\d+/);
      const text = (await badge.textContent()) ?? '';
      const shown = Number(/· ([\d,]+)행/.exec(text)![1]!.replace(/,/g, ''));
      const to = /seq \d+–(\d+)/.exec(text)![1]!;
      const q = await ctl.call<{ rows: unknown[]; truncated: boolean }>('eth.query', { ch: 'lan1', range: { to }, max: 2000 });
      expect(shown).toBe(q.rows.length);
      if (q.truncated) await expect(badge).toContainText('범위를 좁히세요'); // never called complete when cut
      const st = await ctl.inject<{ total: number; shown: number }>('eth_flood_stats');
      test.info().annotations.push({ type: 'eth 4 flood', description: `${st.total} packets journalled, ${st.shown} on the live stream` });
    } finally {
      ctl.close();
    }
  });

  test('5: kernel drop → red gap row + badge count = source.gap.count', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await startCapture(ctl);
      await openApp(page, '/p/eth/packets?channel=lan1');
      await expect(page.getByTestId('eth-status')).toContainText('커널 드롭');
      const before = await kernelDrop(page);
      await ctl.inject('eth_kernel_drop', { ch: 'lan1', count: 42 });
      await expect.poll(() => kernelDrop(page)).toBe(before + 42);
      const gap = page.locator(`${LIST} .vt-row[data-gap="source.gap"]`).last();
      await expect(gap).toContainText('수신 전 손실 42');
      await expect(gap).toContainText('kernel_drop');
      await expect(page.getByTestId('eth-status')).toHaveClass(/has-drop/);
    } finally {
      ctl.close();
    }
  });

  test('6: a second browser joining late sees the same last seq and capture state', async ({ page, browser }) => {
    const ctl = await Daemon.open();
    try {
      await startCapture(ctl);
      await openApp(page, '/p/eth/packets?channel=lan1');
      await ctl.agent('uds.request', { target: 'bcm', hex: '22F190', mock_pending: 0 });
      await ctl.agent('eth.capture.stop', { ch: 'lan1' }); // quiesce: no more packets
      await expect(page.getByTestId('eth-status')).toHaveAttribute('data-state', 'STOPPED');
      const ctx2 = await browser.newContext({ baseURL: test.info().project.use.baseURL });
      const late = await ctx2.newPage();
      try {
        await openApp(late, '/p/eth/packets?channel=lan1');
        await expect(late.getByTestId('eth-status')).toHaveAttribute('data-state', 'STOPPED');
        const last = (p: Page) => p.locator(`${LIST} .vt-row[data-seq]:visible`).evaluateAll((rs) => Math.max(0, ...rs.map((r) => Number((r as unknown as { getAttribute(n: string): string }).getAttribute('data-seq')))));
        await expect.poll(() => last(late)).toBe(await last(page));
      } finally {
        await ctx2.close();
      }
    } finally {
      ctl.close();
    }
  });

  test('7: STOP → agent eth.send refused; capture rows keep coming', async ({ page }) => {
    const ctl = await Daemon.open();
    try {
      await startCapture(ctl);
      await openApp(page, '/p/eth/packets?channel=lan1');
      await page.getByTestId('stop-button').click();
      await expect(page.getByTestId('stop-button')).toHaveAttribute('data-phase', 'locked');
      const err = await ctl.agent('eth.send', { kind: 'raw', ch: 'lan1', hex: '00'.repeat(60) }).then(
        () => null,
        (e: unknown) => e as RpcError,
      );
      expect(err?.data?.code).toBe('stop_latched');
      // the STOP panel pushes the list down; the panel may read that as a scroll-back and switch to the
      // complete (journal) view — go back to live like a user would (noted in the report, §5)
      const badge = page.getByTestId('eth-mode');
      if ((await badge.getAttribute('data-mode')) === 'complete') {
        test.info().annotations.push({ type: 'eth 7', description: 'list left live mode when the STOP panel appeared (layout scroll read as scroll-back)' });
        await page.getByRole('button', { name: /라이브로/ }).click();
        await expect(badge).toHaveAttribute('data-mode', 'live');
      }
      const lastSeq = async () => Math.max(0, ...(await page.locator(`${LIST} .vt-row[data-seq]:visible`).evaluateAll((rs) => rs.map((r) => Number((r as unknown as { getAttribute(n: string): string }).getAttribute('data-seq'))))));
      const s0 = await lastSeq();
      await expect.poll(lastSeq, { timeout: 5_000 }).toBeGreaterThan(s0); // background SOME/IP-SD keeps arriving
    } finally {
      await ctl.call('core.stop.release').catch(() => undefined);
      await ctl.agent('eth.capture.stop', { ch: 'lan1' }).catch(() => undefined);
      ctl.close();
    }
  });
});
