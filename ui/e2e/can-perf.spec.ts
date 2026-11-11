// CAN Trace render-proxy latency under mock floods (task p1-can §7; plan §0.2, §9.5).
// Latency = event ts.mono_ns (mock hub publish) → first rAF after the draw that put the
// row in the list, using the net clock sync (core.time) for the browser→daemon mono offset.
// Includes mock batching (max_batch_ms 40), WS, Worker decode, main-thread apply, draw.
// Limits: synthetic 8-byte frames, single machine (mock + Vite + headless Chromium share
// CPUs), JSON encoding, render proxy (not a paint timestamp). Not a product acceptance number.

import { expect, test, type Page } from '@playwright/test';
import { MockCtl } from './mockCtl';

interface Stats {
  rows: number;
  framesIn: number;
  floodSeen: number;
  omitted: number;
  degraded: boolean;
  gapRows: { kind: string; first: number | null; last: number | null; count: number | 'unknown'; cause: string }[];
  lat: { n: number; p50: number; p95: number; p99: number; max: number; clockErrMs: number };
}

const stats = (page: Page) =>
  page.evaluate(() => {
    const p = (globalThis as unknown as { __loomCan?: Record<string, { stats(): Stats }> }).__loomCan ?? {};
    const k = Object.keys(p)[0];
    return k ? p[k]!.stats() : null;
  });

const SECONDS = 6;

const CASES = [
  { fps: 5_000, cpuThrottle: 1 },
  { fps: 10_000, cpuThrottle: 1 },
  // stress: CDP CPU throttling makes the main thread fall behind, so the net decode budget /
  // server slow_client / display budget paths actually run and their gap rows can be checked
  { fps: 10_000, cpuThrottle: Number(process.env.CAN_PERF_THROTTLE ?? 8) },
];

// Performance numbers are judged only on a fixed runner (plan §9.2): opt in with LOOM_PERF=1.
// Default e2e runs keep the logic/honesty checks in can.spec.ts.
test.skip(!process.env.LOOM_PERF, 'perf cases run only with LOOM_PERF=1 (fixed runner)');

for (const { fps, cpuThrottle } of CASES) {
  test(`flood_can ${fps} fps × ${SECONDS} s, CPU ×${cpuThrottle}`, async ({ page }) => {
    test.setTimeout(60_000);
    if (cpuThrottle > 1) {
      const cdp = await page.context().newCDPSession(page);
      await cdp.send('Emulation.setCPUThrottlingRate', { rate: cpuThrottle });
    }
    await page.goto('./#/p/can/trace?channel=ch0');
    await page.waitForFunction(() => (globalThis as unknown as { __loomNet?: { state: { status: string } } }).__loomNet?.state.status === 'open', null, { timeout: 15_000 });
    await expect(page.getByTestId('can-trace')).toBeVisible();
    await page.waitForTimeout(1_500); // clock sync samples (core.time ×8)
    const ctl = await MockCtl.open();
    try {
      const before = (await stats(page))!;
      await ctl.call('mock.inject', { kind: 'flood_can', fps, seconds: SECONDS });
      await page.waitForTimeout(SECONDS * 1000 + 300);
      await expect.poll(async () => (await ctl.call<{ running: string[] }>('mock.inject', { kind: 'flood_stats' })).running.length).toBe(0);
      const emitted = (await ctl.call<{ can: number }>('mock.inject', { kind: 'flood_stats' })).can;
      await page.waitForTimeout(800); // drain
      const after = (await stats(page))!;
      const seen = after.floodSeen - before.floodSeen;
      const newGaps = after.gapRows.slice(before.gapRows.length);
      const gapCount = newGaps.filter((g) => g.kind !== 'omit').reduce((a, g) => a + (typeof g.count === 'number' ? g.count : 0), 0);
      const line = {
        fps,
        cpuThrottle,
        emitted,
        seen,
        omittedByBudget: after.omitted - before.omitted,
        degraded: after.degraded,
        gapRows: newGaps.length,
        gapCount,
        gapKinds: [...new Set(newGaps.map((g) => `${g.kind}:${g.cause}`))],
        lat: after.lat,
      };
      console.log(`[can-perf] ${JSON.stringify(line)}`);
      if (fps === 5_000 && cpuThrottle === 1) {
        // normal profile: every flood frame reaches the panel, no omission, no gap rows
        expect(seen).toBe(emitted);
        expect(newGaps.filter((g) => g.kind !== 'resync')).toEqual([]);
        expect(after.omitted - before.omitted).toBe(0);
      } else {
        // high load: nothing silently lost — frames seen + frames the gap rows account for
        // cover every emitted frame (gap counts may also include background 0x100/0x200 frames)
        expect(seen + gapCount).toBeGreaterThanOrEqual(emitted);
        expect(seen).toBeLessThanOrEqual(emitted);
        for (const g of newGaps) if (g.kind !== 'resync' && g.first !== null) expect(g.last! >= g.first).toBe(true);
      }
    } finally {
      ctl.close();
    }
  });
}
