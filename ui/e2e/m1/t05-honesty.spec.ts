// UI-T05 정직성 (plan §11, §0.3): the screen never hides loss and never freezes silently.
//  - server gaps of the three layers each get their own wording/colour (display grey,
//    record/source red) — mock injects them on can/ch0; replay's `gaps` generator publishes them
//    every 3 s (domain = kind prefix "display|record|source", channel ch0)
//  - heartbeat stops ≥ 3 s → STALLED badge + banner + write lock; it recovers afterwards
//  - reconnect stitches: after a drop the CAN rows continue with no duplicate / missing seq
//    (mock: daemon drops the socket; replay: `--scenario saturate` closes with slow_consumer)
//  - UART flood ≈ 92 KB/s for LOOM_FLOOD_S (default 60) s: no view skip, heartbeat continuous,
//    main thread never stalls (rAF gap), no STALLED

import { backend, Daemon, expect, expectIncreasing, missingInRange, mockOnly, names, openApp, pageQuery, REPLAY_SATURATE_URL, scanGridSeqs, sleep, test, type G } from './lib';

test.describe('UI-T05 honesty', () => {
  test('three loss layers render as distinct rows (display grey · record / source red) @replay', async ({ page }) => {
    test.fail(backend() === 'replay', 'CORE replay publishes gaps with domain "display|record|source" (kind prefix), not the affected stream (can/ch0) — the CAN trace (domain can) never receives them (report §4 C-4)');
    const n = names();
    await openApp(page, `/p/can/trace?channel=${n.can}`);
    if (backend() === 'mock') {
      const ctl = await Daemon.open();
      try {
        await ctl.inject('display_gap', { domain: 'can', channel: 'ch0', count: 7 });
        await ctl.inject('record_gap', { domain: 'can', channel: 'ch0', count: 3 });
        await ctl.inject('source_gap', { domain: 'can', channel: 'ch0', count: 37 });
      } finally {
        ctl.close();
      }
    }
    await expect(page.locator('.ct-row[data-gap="source.gap"]').first()).toBeAttached({ timeout: 8_000 });
    await page.getByTestId('can-pause').click();
    const disp = page.locator('.ct-row[data-gap="display.gap"]').last();
    const rec = page.locator('.ct-row[data-gap="record.gap"]').last();
    const src = page.locator('.ct-row[data-gap="source.gap"]').last();
    await expect(disp).toContainText('화면 생략');
    await expect(rec).toContainText('기록 손실');
    await expect(src).toContainText('SOURCE LOSS');
    // colour: display = grey/amber row, record/source = red loss rows (.ct-loss, --err text)
    await expect(disp).not.toHaveClass(/ct-loss/);
    await expect(rec).toHaveClass(/ct-loss/);
    await expect(src).toHaveClass(/ct-loss/);
    const color = (l: typeof disp) => l.locator('.gap-text').evaluate((e) => (globalThis as unknown as { getComputedStyle(x: unknown): { color: string } }).getComputedStyle(e).color);
    const rgb = (c: string) => (c.match(/\d+/g) ?? []).map(Number);
    const [rr, rg] = rgb(await color(rec));
    const [sr, sg] = rgb(await color(src));
    expect(rr! > rg! + 40 && sr! > sg! + 40).toBe(true); // red family
    expect(await color(disp)).not.toBe(await color(rec));
  });

  test('heartbeat stalled ≥ 3 s → STALLED + banner + write lock; recovers', async ({ page }) => {
    mockOnly();
    await openApp(page, '/p/can/trace?channel=ch0');
    const ctl = await Daemon.open();
    try {
      await expect(page.getByTestId('live-badge')).not.toHaveAttribute('data-badge', 'STALLED');
      await ctl.inject('stall_heartbeat', { ms: 6_000 });
      await expect(page.getByTestId('live-badge')).toHaveAttribute('data-badge', 'STALLED', { timeout: 5_500 });
      await expect(page.locator('[data-banner=stalled]')).toContainText('STALLED');
      await expect(page.getByTestId('loom-shell')).toHaveAttribute('data-locked', 'true');
      // write lock: a user send is refused by the gate — nothing reaches the daemon
      const connId = await page.evaluate(() => (globalThis as unknown as G).__loomNet?.state.hello?.conn_id ?? '');
      const sends = async () => {
        const calls = (await ctl.inject<{ conns: { id: string; calls: Record<string, number> }[] }>('conn_stats')).conns.find((x) => x.id === connId)?.calls ?? {};
        return (calls['can.send'] ?? 0) + (calls['can.tx.send'] ?? 0); // panel moving from the provisional name to can.send
      };
      const before = await sends();
      const input = page.getByTestId('can-send-input');
      // write lock: the send input is disabled while STALLED (ctx.writeLocked) and nothing reaches the daemon
      await expect(input).toBeDisabled();
      expect(await sends()).toBe(before);
      await expect(page.getByTestId('stop-button')).toBeEnabled(); // STOP stays available
      await expect(page.getByTestId('live-badge')).not.toHaveAttribute('data-badge', 'STALLED', { timeout: 8_000 });
      await expect(page.getByTestId('loom-shell')).toHaveAttribute('data-locked', 'false');
    } finally {
      ctl.close();
    }
  });

  test('reconnect stitches the CAN stream: no duplicate / missing seq across the drop @replay', async ({ page }) => {
    const replay = backend() === 'replay';
    const ref = replay ? null : await Daemon.open();
    try {
      const sub = ref ? await ref.subscribe({ domains: ['can'], channels: ['ch0'] }) : null;
      // replay: a separate loomd whose generator overflows every viewer's control queue (slow_consumer)
      await openApp(page, '/p/can/trace?channel=ch0', replay ? pageQuery({}, REPLAY_SATURATE_URL) : pageQuery());
      await sleep(800);
      const reconnecting = page.waitForFunction(() => (globalThis as unknown as G).__loomNet?.state.status === 'reconnecting', null, { timeout: 12_000 });
      if (ref) await ref.inject('disconnect'); // drops every other connection (the page), not this one
      await reconnecting;
      await expect(page.locator('[data-banner=conn]')).toContainText('재연결');
      await page.waitForFunction(() => (globalThis as unknown as G).__loomNet?.state.status === 'open', null, { timeout: 12_000 });
      await sleep(1_200);
      await page.getByTestId('can-pause').click();
      const dom = await scanGridSeqs(page, '[data-testid=can-trace] [role=grid]', '.ct-row[data-seq]:not([data-request-id]):not([data-gap])');
      expectIncreasing(dom);
      if (sub) {
        const refSeqs = sub.events.filter((e) => e.kind === 'can.frame' && !e.request_id).map((e) => e.seq);
        expect(missingInRange(dom, refSeqs)).toEqual([]);
      } else {
        // replay saturate: rows continue after the reconnect; any hole must be a visible gap/resync row
        expect(dom.length).toBeGreaterThan(50);
        test.info().annotations.push({ type: 'replay saturate gap/resync rows', description: String(await page.locator('.ct-row[data-gap]').count()) });
      }
    } finally {
      ref?.close();
    }
  });

  test('UART flood ≈ 92 KB/s: no view skip, heartbeat continuous, main thread never stalls', async ({ page }) => {
    mockOnly('needs mock.inject flood_uart');
    const secs = Number(process.env.LOOM_FLOOD_S ?? 60);
    test.setTimeout((secs + 40) * 1000);
    // main-thread probe as an init script: survives a dev-server (HMR) reload; reloads are counted
    let loads = 0;
    page.on('load', () => loads++);
    await page.addInitScript(() => {
      const g = globalThis as unknown as G;
      const m: { maxGap: number; badges: Record<string, number>; last: number } = { maxGap: 0, badges: {}, last: g.performance.now() };
      g.__m1 = m as unknown as Record<string, unknown>;
      const f = () => {
        const t = g.performance.now();
        m.maxGap = Math.max(m.maxGap, t - m.last);
        m.last = t;
        const b = g.document.querySelector('[data-testid=live-badge]')?.getAttribute('data-badge') ?? '?';
        m.badges[b] = (m.badges[b] ?? 0) + 1;
        g.requestAnimationFrame(f);
      };
      g.requestAnimationFrame(f);
    });
    await openApp(page, '/l/uart-only');
    await page.waitForTimeout(1_000); // first-paint frames are not part of the measurement
    await expect(page.getByTestId('live-badge')).not.toHaveAttribute('data-badge', 'STALLED', { timeout: 5_000 });
    await page.evaluate(() => {
      const m = (globalThis as unknown as G).__m1 as { maxGap: number; badges: Record<string, number> };
      m.maxGap = 0;
      m.badges = {}; // frames before the first heartbeat may read STALLED: not part of the flood
    });
    const loads0 = loads;
    const ctl = await Daemon.open();
    try {
      await ctl.inject('flood_uart', { bytes_per_s: 92_000, seconds: secs });
      const hb: string[] = [];
      for (let t = 0; t < secs; t += 2) {
        await sleep(2_000);
        hb.push((await page.getByTestId('hb-clock').textContent()) ?? '');
        await expect(page.getByTestId('uart-status')).not.toContainText('화면 생략');
      }
      const m = await page.evaluate(() => (globalThis as unknown as G).__m1 as { maxGap: number; badges: Record<string, number> });
      expect(loads - loads0, 'the page reloaded during the flood (dev-server HMR from a concurrent edit?) — rerun').toBe(0);
      test.info().annotations.push({ type: 'T05 flood', description: `${secs} s, max rAF gap ${m.maxGap.toFixed(0)} ms, badges ${JSON.stringify(m.badges)}` });
      expect(m.badges.STALLED ?? 0).toBe(0);
      expect(m.maxGap).toBeLessThan(1_000); // "메인 스레드 멈춤 없음"
      expect(new Set(hb).size).toBeGreaterThan(hb.length / 2); // heartbeat clock keeps moving
      await expect(page.locator('.xterm')).toBeVisible();
    } finally {
      ctl.close();
    }
  });
});
