// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';

// uPlot reads matchMedia at module load; jsdom has none (lead: shared vitest setup requested)
vi.hoisted(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
});
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { psuFixture } from './fixtures';
import './gaugePanel';
import { Series } from './psuModel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
});
const btn = (t: string) => [...h!.host.querySelectorAll('button')].find((b) => b.textContent === t)!;

describe('psu.gauge', () => {
  it('shows set / measured values and output state', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture());
    expect(h.host.querySelector('[data-testid="psu-head"]')!.textContent).toContain('● 출력 ON');
    const vals = h.host.querySelector('[data-testid="psu-vals"]')!.textContent!;
    expect(vals).toContain('설정 13.50 V / 2.00 A CV');
    expect(vals).toMatch(/실측 13\.4\d\d V 0\.\d{3} A/);
  });

  it('emergency OFF: allowed for watch, no confirmation, sends psu.out{state:"off"}; operate-only controls disabled', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' }, { hello: { rights: 'watch' } });
    h.batch(psuFixture(5));
    expect(btn('출력 OFF').disabled).toBe(true);
    expect(btn('설정').disabled).toBe(true);
    const off = btn('! 비상 OFF');
    expect(off.disabled).toBe(false);
    off.click();
    await Promise.resolve();
    expect(h.net.calls).toEqual([{ method: 'psu.out', params: { channel: 'bench', state: 'off' } }]);
  });

  it('`!` key on the focused panel also triggers emergency OFF', async () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.host.querySelector<HTMLElement>('.psu-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: '!', bubbles: true }));
    await Promise.resolve();
    expect(h.net.calls.map((c) => c.method)).toEqual(['psu.out']);
  });

  it('STOP latched + output on → "STOP — 출력 유지 중"', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture(3));
    expect(h.host.querySelector<HTMLElement>('.psu-stop')!.hidden).toBe(true);
    h.shell.applyControl(ev({ kind: 'core.latch', payload: { on: true, missing: [] } }), 0);
    h.frame();
    expect(h.host.querySelector<HTMLElement>('.psu-stop')!.hidden).toBe(false);
  });

  it('SCPI no-response gap breaks the series (null point) and is counted', () => {
    h = mountPanel('psu.gauge', { channel: 'bench' });
    h.batch(psuFixture());
    expect(h.host.textContent).toContain('측정 손실(SCPI 무응답) 1회');
  });
});

describe('psu Series', () => {
  it('keeps a 60 s window and inserts nulls at gaps', () => {
    const s = new Series();
    for (let t = 0; t <= 100; t++) s.push(t, 13.5, 0.3);
    s.gap(101);
    const [t, v] = s.data();
    expect(t[0]).toBe(41);
    expect(v[v.length - 1]).toBeNull();
  });
});
