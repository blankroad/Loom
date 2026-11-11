// UI-T10 수명주기 (plan §11): open/close panels 100 times (CAN trace, UDS console, UART terminal
// via hash routes) → event listeners, DOM nodes and server subscriptions return to the baseline.
// Measured with CDP Performance.getMetrics after a forced GC (main thread) and the daemon's view
// of this connection's subscriptions (mock: conn_stats; replay: heartbeat sub_stats).
// Bounded to a few minutes (10-minute rule).

import type { Page } from '@playwright/test';
import { backend, Daemon, expect, names, openApp, sleep, test, type G } from './lib';

interface Metrics {
  listeners: number;
  nodes: number;
  heapMb: number;
}

async function metrics(page: Page): Promise<Metrics> {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Performance.enable');
  await cdp.send('HeapProfiler.collectGarbage');
  const { metrics: m } = await cdp.send('Performance.getMetrics');
  await cdp.detach();
  const get = (n: string) => m.find((x) => x.name === n)?.value ?? NaN;
  return { listeners: get('JSEventListeners'), nodes: get('Nodes'), heapMb: get('JSHeapUsedSize') / 2 ** 20 };
}

async function serverSubs(page: Page, ctl: Daemon | null): Promise<number> {
  if (ctl) {
    const connId = await page.evaluate(() => (globalThis as unknown as G).__loomNet?.state.hello?.conn_id ?? '');
    const s = await ctl.inject<{ conns: { id: string; subs: number }[] }>('conn_stats');
    return s.conns.find((c) => c.id === connId)?.subs ?? -1;
  }
  // replay: the daemon reports this connection's subscriptions in every heartbeat (sub_stats)
  await sleep(1_300);
  return page.evaluate(() => Number(((globalThis as unknown as G).__m1?.subStats as number | undefined) ?? -1));
}

test('UI-T10: 100 panel open/close cycles → listeners, nodes and subscriptions back to baseline @replay', async ({ page }) => {
  test.setTimeout(240_000);
  const n = names();
  const routes: [string, string][] = [
    [`#/p/can/trace?channel=${n.can}`, '[data-testid=can-trace]'],
    [`#/p/uds/console?targets=${n.uds}`, '[data-testid=uds-targets]'],
    [`#/p/uart/terminal?channel=${n.uart}`, '[data-testid=input-chip]'],
  ];
  await openApp(page, routes[0]![0].slice(1));
  await page.evaluate(() => {
    const g = globalThis as unknown as G & { __loomNet: { onControl(cb: (e: { kind: string; payload: { sub_stats?: unknown[] } }) => void): void } };
    g.__m1 = {};
    g.__loomNet.onControl((e) => {
      if (e.kind === 'core.heartbeat' && Array.isArray(e.payload.sub_stats)) {
        const ids = e.payload.sub_stats.map((s) => String((s as { sub?: string }).sub)).filter((s) => s !== 'control');
        g.__m1!.subStats = ids.length;
        g.__m1!.subIds = ids.join(',');
      }
    });
  });
  const ctl = backend() === 'mock' ? await Daemon.open() : null;
  try {
    const go = async (i: number) => {
      const [hash, sel] = routes[i % routes.length]!;
      await page.evaluate((h) => ((globalThis as unknown as { location: { hash: string } }).location.hash = h), hash);
      await page.locator(sel).first().waitFor({ state: 'attached' });
    };
    for (let i = 0; i < 3; i++) await go(i); // warm-up: every panel module loaded once
    await go(0);
    await sleep(500);
    const base = await metrics(page);
    const baseSubs = await serverSubs(page, ctl);
    const baseIds = await page.evaluate(() => String((globalThis as unknown as G).__m1?.subIds ?? ''));
    for (let i = 1; i <= 100; i++) await go(i);
    await go(0);
    await sleep(Number(process.env.T10_SETTLE_MS ?? 500));
    const resync = await page.locator('.ct-row').filter({ hasText: '재동기화' }).allTextContents();
    if (resync.length) test.info().annotations.push({ type: 'T10 resync rows', description: resync.join(' | ').slice(0, 300) });
    console.log('[T10 resync]', resync.join(' | ').slice(0, 300));
    const after = await metrics(page);
    const afterSubs = await serverSubs(page, ctl);
    const afterIds = await page.evaluate(() => String((globalThis as unknown as G).__m1?.subIds ?? ''));
    console.log(`[T10 ${backend()}] listeners ${base.listeners}→${after.listeners}, nodes ${base.nodes}→${after.nodes}, subs ${baseSubs}→${afterSubs} [${baseIds}]→[${afterIds}]`);
    test.info().annotations.push({ type: 'T10', description: `listeners ${base.listeners}→${after.listeners}, nodes ${base.nodes}→${after.nodes}, heap ${base.heapMb.toFixed(1)}→${after.heapMb.toFixed(1)} MiB, server subs ${baseSubs}→${afterSubs}${baseIds ? ` [${baseIds}]→[${afterIds}]` : ''}` });
    expect(after.listeners).toBeLessThanOrEqual(base.listeners + 20); // a few lazily created listeners at most, not ×100
    // DOM nodes vary with how full the CAN row pool is at the moment of measurement (±1 viewport of
    // rows ≈ 1,000 nodes); a leaked panel per cycle would add ≥ 100 × several hundred nodes
    expect(after.nodes).toBeLessThanOrEqual(base.nodes + 2_000);
    expect(afterSubs).toBe(baseSubs);
    expect(afterSubs).toBeGreaterThanOrEqual(0);
    expect(after.heapMb).toBeLessThan(base.heapMb + 64);
  } finally {
    ctl?.close();
  }
});
