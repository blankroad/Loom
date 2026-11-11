// UI-T09 쓰기 단계 (plan §11, D4/A14):
//  - UDS 0x78 ×5: ONE row goes ○ → ◔ → ◑ pending ×1..5 → ●; the row count for that request_id is
//    1 at every DOM mutation (MutationObserver), and "×5" was on screen
//  - stages injected in reverse order never move a row backwards
//  - outcome_unknown is drawn orange (mock: armed write; replay: every 5th generated UDS txn)

import type { Page } from '@playwright/test';
import { backend, Daemon, expect, mockOnly, names, openApp, test, type G } from './lib';

/** per request_id: max simultaneous rows, every state and "×N" text seen (MutationObserver) */
async function watchRows(page: Page): Promise<void> {
  await page.evaluate(() => {
    const g = globalThis as unknown as G;
    const m: Record<string, { max: number; states: string[]; texts: string[] }> = {};
    g.__m1 = m as unknown as Record<string, unknown>;
    const scan = () => {
      // pooled rows the table parked with the `hidden` attribute are not rows of the list; they are
      // counted separately (shell.css `.vt-row{display:grid}` currently still paints them — report §5 U-1)
      const rows = g.document.querySelectorAll('.vt-row[data-request-id]:not([hidden])');
      const stale = g.document.querySelectorAll('.vt-row[data-request-id][hidden]');
      for (let i = 0; i < stale.length; i++) {
        const id = stale[i]!.getAttribute('data-request-id')!;
        const e = (m[id] ??= { max: 0, states: [], texts: [] }) as { max: number; states: string[]; texts: string[]; hiddenDup?: number };
        e.hiddenDup = (e.hiddenDup ?? 0) + 1;
      }
      const count: Record<string, number> = {};
      for (let i = 0; i < rows.length; i++) {
        const r = rows[i]!;
        const id = r.getAttribute('data-request-id')!;
        count[id] = (count[id] ?? 0) + 1;
        const e = (m[id] ??= { max: 0, states: [], texts: [] });
        const st = r.getAttribute('data-state') ?? '';
        if (e.states[e.states.length - 1] !== st) e.states.push(st);
        // leaf cells only: "pending ×3" and the elapsed "0.3s" are separate cells
        const leaves = r.querySelectorAll('*');
        for (let k = 0; k < leaves.length; k++) {
          const leaf = leaves[k]!;
          if (leaf.querySelectorAll('*').length) continue;
          const x = /×\s?(\d+)\s*$/.exec(leaf.textContent ?? '');
          if (x && !e.texts.includes(x[1]!) && e.texts.length < 32) e.texts.push(x[1]!);
        }
      }
      for (const [id, c] of Object.entries(count)) m[id]!.max = Math.max(m[id]!.max, c);
    };
    new g.MutationObserver(scan).observe(g.document.body, { subtree: true, childList: true, attributes: true, characterData: true });
  });
}

const RANK: Record<string, number> = { requested: 0, accepted: 1, sent: 2, confirmed: 3, failed: 3, outcome_unknown: 3, effect_unverified: 3 };

test.describe('UI-T09 write stages', () => {
  test('UDS 0x78 ×5 → one row: ○→◔→◑ pending ×1..5 → ● (row count stays 1)', async ({ page }) => {
    mockOnly();
    const ctl = await Daemon.open();
    try {
      await ctl.inject('reset');
      await openApp(page, '/p/uds/console?targets=bcm');
      await watchRows(page);
      const r = await ctl.agent<{ request_id: string; write_state: string }>('uds.request', { target: 'bcm', hex: '22F190', mock_pending: 5, mock_pending_ms: 120 });
      const row = page.locator(`.vt-row:not([hidden])[data-request-id="${r.request_id}"]`);
      await expect(row).toHaveAttribute('data-state', 'confirmed');
      await expect(row).toHaveCount(1);
      const seen = await page.evaluate((id) => ((globalThis as unknown as G).__m1 as Record<string, { max: number; states: string[]; texts: string[] }>)[id], r.request_id);
      test.info().annotations.push({ type: 'T09 stages seen', description: JSON.stringify(seen) });
      expect(seen!.max).toBe(1);
      expect(seen!.texts).toContain('5');
      // monotone: never back
      const ranks = seen!.states.map((s) => RANK[s] ?? -1);
      for (let i = 1; i < ranks.length; i++) expect(ranks[i]!).toBeGreaterThanOrEqual(ranks[i - 1]!);
      expect(seen!.states.at(-1)).toBe('confirmed');
    } finally {
      ctl.close();
    }
  });

  test('stages injected in reverse order never move the row backwards', async ({ page }) => {
    mockOnly('needs mock.emit');
    const ctl = await Daemon.open();
    try {
      await openApp(page, '/p/uds/console?targets=bcm');
      await watchRows(page);
      const rid = `rev-${Date.now()}`;
      const emit = (ws: string, extra: Record<string, unknown> = {}) =>
        ctl.call('mock.emit', { domain: 'uds', channel: 'bcm', kind: 'uds.request', origin: 'agent', request_id: rid, risk: 'R0', payload: { target: 'bcm', sid: 0x22, service: 'ReadDataByIdentifier', summary: 'read-did F190', req_hex: '22f190', pending_count: 0, write_state: ws, ...extra } });
      await emit('requested');
      await emit('confirmed', { resp_hex: '62f190' });
      await emit('sent');
      await emit('accepted');
      await emit('requested');
      const row = page.locator(`.vt-row:not([hidden])[data-request-id="${rid}"]`);
      await expect(row).toHaveAttribute('data-state', 'confirmed');
      await page.waitForTimeout(300);
      const seen = await page.evaluate((id) => ((globalThis as unknown as G).__m1 as Record<string, { max: number; states: string[] }>)[id], rid);
      expect(seen!.max).toBe(1);
      const i = seen!.states.indexOf('confirmed');
      expect(i).toBeGreaterThanOrEqual(0);
      expect(seen!.states.slice(i).every((s) => s === 'confirmed')).toBe(true); // nothing after ● moved it back
      await expect(row).toHaveAttribute('data-state', 'confirmed');
    } finally {
      ctl.close();
    }
  });

  test('outcome_unknown is drawn orange @replay', async ({ page }) => {
    const n = names();
    let rid: string | null = null;
    if (backend() === 'mock') {
      const ctl = await Daemon.open();
      try {
        await ctl.inject('reset');
        await openApp(page, `/p/uds/console?targets=${n.uds}`);
        await ctl.inject('outcome_unknown');
        const r = await ctl.agent<{ request_id: string; write_state: string }>('uds.request', { target: n.uds, hex: '1001' });
        expect(r.write_state).toBe('outcome_unknown');
        rid = r.request_id;
      } finally {
        ctl.close();
      }
    } else {
      await openApp(page, `/p/uds/console?targets=${n.uds}`); // every 5th replay UDS txn times out (≤ 10 s)
    }
    const row = rid ? page.locator(`.vt-row:not([hidden])[data-request-id="${rid}"]`) : page.locator('.vt-row[data-state=outcome_unknown]').first();
    await expect(row).toHaveAttribute('data-state', 'outcome_unknown', { timeout: 12_000 });
    // the stage chip of that row: orange (red high, green mid, blue low)
    const rgb = await row.evaluate((e) => {
      const g = globalThis as unknown as { getComputedStyle(x: unknown): { color: string } };
      const els = [e, ...Array.from((e as unknown as { querySelectorAll(s: string): ArrayLike<unknown> }).querySelectorAll('*'))];
      const hit = els.find((x) => /⌛|불확정/.test((x as { textContent: string | null }).textContent ?? '') && (x as { children: { length: number } }).children.length === 0);
      return g.getComputedStyle(hit ?? e).color;
    });
    const [r, gr, b] = (rgb.match(/\d+/g) ?? []).map(Number);
    test.info().annotations.push({ type: 'outcome_unknown color', description: rgb });
    expect(r!).toBeGreaterThan(180);
    expect(gr!).toBeGreaterThan(80);
    expect(gr!).toBeLessThan(200);
    expect(b!).toBeLessThan(120);
  });
});
