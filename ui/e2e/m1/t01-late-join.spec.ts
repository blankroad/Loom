// UI-T01 늦은 합류 (plan §11): after ≥ 1,000 events a tab opens → snapshot on screen within
// 1 s (UART panel 500 ms) of the connection opening, then live continues with no duplicate and
// no missing seq. Judged on the DOM: CAN rows' `data-seq` compared with a reference subscriber
// (Node, straight to the daemon) over the overlapping seq range; UART trace rows' text.
// Timing basis: net `open` (WS + core.hello done) → first data row in the DOM (rAF-sampled,
// ±1 frame). Dev-server module loading before `open` is not part of the budget.

import type { Page } from '@playwright/test';
import { backend, Daemon, expect, expectIncreasing, missingInRange, names, openApp, scanGridSeqs, sleep, test, type G } from './lib';

/** records, per animation frame, when the net opened and when `rowSel` first matched */
async function armFirstRowProbe(page: Page, rowSel: string): Promise<void> {
  await page.addInitScript((sel: string) => {
    const g = globalThis as unknown as G;
    const m: Record<string, unknown> = {};
    g.__m1 = m;
    const tick = () => {
      const now = g.performance.now();
      if (m.openAt === undefined && g.__loomNet?.state.status === 'open') m.openAt = now;
      if (m.rowAt === undefined && g.document?.querySelector(sel)) m.rowAt = now;
      if (m.rowAt === undefined || m.openAt === undefined) g.requestAnimationFrame(tick);
    };
    g.requestAnimationFrame(tick);
  }, rowSel);
}

async function firstRowMs(page: Page): Promise<{ openAt: number; rowAt: number }> {
  await page.waitForFunction(() => (globalThis as unknown as G).__m1?.rowAt !== undefined && (globalThis as unknown as G).__m1?.openAt !== undefined, null, { timeout: 15_000 });
  return page.evaluate(() => (globalThis as unknown as G).__m1 as { openAt: number; rowAt: number });
}

test.describe('UI-T01 late join', () => {
  test('CAN trace: snapshot within 1 s of open, then live with no duplicate / missing seq @replay', async ({ page }) => {
    const n = names();
    const ref = await Daemon.open();
    try {
      if (backend() === 'mock') await ref.inject('late_backlog', { count: 1000 }); // replay: --scenario all prefilled 1,000
      const sub = await ref.subscribe({ domains: ['can'], channels: [n.can] });
      const rowSel = '[data-testid=can-trace] .ct-row[data-seq]';
      await armFirstRowProbe(page, rowSel);
      const errors = await openApp(page, `/p/can/trace?channel=${n.can}`);
      const t = await firstRowMs(page);
      const atOpen = BigInt(sub.events[sub.events.length - 1]?.seq ?? '0'); // newest seq when the page had its snapshot
      const ms = t.rowAt - t.openAt;
      test.info().annotations.push({ type: 'T01 CAN open→first snapshot row', description: `${ms.toFixed(0)} ms` });
      expect(ms).toBeLessThan(1_000);

      await sleep(1_500); // live after the snapshot
      await page.getByTestId('can-pause').click(); // freeze the view (collection continues)
      const dom = await scanGridSeqs(page, '[data-testid=can-trace] [role=grid]', '.ct-row[data-seq]:not([data-request-id]):not([data-gap])');
      expect(dom.length).toBeGreaterThan(200);
      expectIncreasing(dom); // no duplicate, no reordering across snapshot → live
      // reference: device frames (tx echoes merge into their TX row and carry a request_id)
      const refSeqs = sub.events.filter((e) => e.kind === 'can.frame' && !e.request_id).map((e) => e.seq);
      expect(missingInRange(dom, refSeqs)).toEqual([]);
      // the compared window straddles the snapshot → live boundary
      expect(BigInt(dom[0]!)).toBeLessThan(atOpen);
      expect(BigInt(dom[dom.length - 1]!)).toBeGreaterThan(atOpen);
      expect(errors).toEqual([]);
    } finally {
      ref.close();
    }
  });

  test('UART trace: backlog lines within 500 ms of open @replay', async ({ page }) => {
    const n = names();
    const ref = await Daemon.open();
    try {
      if (backend() === 'mock') await ref.inject('late_backlog', { count: 1000 });
      await armFirstRowProbe(page, '.vt-row[data-dir=rx]');
      const errors = await openApp(page, `/p/uart/trace?channel=${n.uart}`);
      const t = await firstRowMs(page);
      const ms = t.rowAt - t.openAt;
      test.info().annotations.push({ type: 'T01 UART open→first backlog line', description: `${ms.toFixed(0)} ms` });
      expect(ms).toBeLessThan(500);
      // backlog text is the device's (mock: "[backlog N] boot line", replay: "boot: line N")
      await expect(page.locator('.vt-row[data-dir=rx]').first()).toContainText(backend() === 'mock' ? 'boot line' : 'boot: line');
      expect(errors).toEqual([]);
    } finally {
      ref.close();
    }
  });
});
