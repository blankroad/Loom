// @vitest-environment jsdom
// U5/U7 batch 1 — panel-level behaviour of uart.terminal (FUNCTIONS.md §B1.3–B1.4). Opus-owned specification;
// Sonnet must not edit. Exercises the real panel through the SDK harness (xterm buffer in jsdom, no renderer).
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import type { LoomEvent } from '../../sdk';
import './terminalPanel';

beforeAll(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
});

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
});

const q = <T extends HTMLElement = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const tid = <T extends HTMLElement = HTMLElement>(id: string) => q<T>(`[data-testid="${id}"]`);
const flush = () => new Promise((r) => setTimeout(r, 0));
const settle = async () => { for (let i = 0; i < 6; i++) await flush(); h!.frame(); };
const hex = (s: string) => Array.from(new TextEncoder().encode(s), (b) => b.toString(16).padStart(2, '0')).join('');
const rx = (s: string): LoomEvent => ev({ kind: 'uart.data', channel: 'dut', dir: 'rx', origin: 'device', payload: { bytes: { encoding: 'hex', data: hex(s) } } });
const openPort = () =>
  h!.batch([
    ev({ kind: 'uart.state', channel: 'dut', generation: 2, payload: { state: 'open', path: '/dev/ttyUSB0', transport: 'serial' } }),
    ev({ kind: 'uart.config', channel: 'dut', generation: 2, payload: { baud: 115200, data_bits: 8, parity: 'none', stop_bits: '1', flow: 'none' } }),
  ]);
/** typing runs the search and selects the FIRST hit (existing behaviour: input → hitAt = -1 → step +1) */
const searchFor = (text: string) => {
  const box = q<HTMLInputElement>('[data-act="search-q"]');
  box.value = text;
  box.dispatchEvent(new Event('input'));
  return q('.search [data-role="count"]');
};
const press = (shiftKey = false) => {
  q<HTMLInputElement>('[data-act="search-q"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey }));
  return q('.search [data-role="count"]').textContent;
};
const setHeadWidth = (px: number) => {
  Object.defineProperty(q('.phead'), 'clientWidth', { configurable: true, value: px });
  h!.instance.resize(px, 300);
  h!.frame();
};

describe('U5 scrollback search (logical lines, Aa, .*)', () => {
  it('finds plain text in the terminal buffer (harness sanity, unchanged behaviour)', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    h.batch([rx('U-Boot 2024.01\r\nboot ok\r\n')]);
    await settle();
    expect(searchFor('boot ok').textContent).toBe('1/1');
  });

  it('matches across a wrapped row (80-column terminal) — the old per-row indexOf answered 0건', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    h.batch([rx(`${'x'.repeat(78)}PANIC here\r\n`)]);
    await settle();
    expect(searchFor('panic').textContent).toBe('1/1');
  });

  it('Aa makes the search case-sensitive; .* switches to regex; errors are shown, not thrown', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    h.batch([rx('error 1\r\nerr 2\r\nERR 3\r\n')]);
    await settle();
    expect(searchFor('err').textContent).toBe('1/3');
    expect(press()).toBe('2/3');
    expect(press(true)).toBe('1/3');
    const aa = tid<HTMLButtonElement>('uart-search-case');
    const re = tid<HTMLButtonElement>('uart-search-regex');
    expect(aa.getAttribute('aria-pressed')).toBe('false');
    aa.click();
    expect(aa.getAttribute('aria-pressed')).toBe('true');
    expect(q('.search [data-role="count"]').textContent).toBe('1/2');
    const hint = q('.search [data-role="search-hint"]');
    expect(hint.hidden).toBe(true);
    re.click();
    expect(re.getAttribute('aria-pressed')).toBe('true');
    // the known regex limitation is visible while regex mode is on (no latency guarantee is claimed)
    expect(hint.hidden).toBe(false);
    expect(hint.textContent).toContain('멈출 수');
    expect(hint.textContent).toContain('10,000');
    const count = searchFor('^err \\d$');
    expect(count.textContent).toBe('1/1');
    const bad = searchFor('(');
    expect(bad.textContent?.startsWith('정규식 오류')).toBe(true);
    expect(bad.dataset.state).toBe('error');
    expect(h.net.calls.filter((c) => c.method === 'uart.input')).toEqual([]); // searching never sends bytes
  });
});

describe('U7 narrow pane identity', () => {
  it('the header dot shows the connection state, not the tag colour', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    expect(q('.ph-dot').dataset.conn).toBe('unknown');
    openPort();
    expect(q('.ph-dot').dataset.conn).toBe('connected');
    h.batch([ev({ kind: 'uart.state', channel: 'dut', payload: { state: 'disconnected' } })]);
    expect(q('.ph-dot').dataset.conn).toBe('lost');
  });

  it('a narrow pane keeps alias, state and baud and moves secondary tools into the ⋯ row', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    setHeadWidth(400);
    expect(q('.phead').dataset.mode).toBe('narrow');
    expect(q('.ph-alias').textContent).toBe('dut');
    expect(q('.ph-alias').title).toBe('dut');
    expect(q('.ph-state').textContent).toBe('연결됨');
    expect(q('.ph-frame').textContent).toBe('115200');
    expect(q('.ph-frame').hidden).toBe(false);
    expect(q('.ph-port').hidden).toBe(true);
    expect(q('.ph-ctr').hidden).toBe(true);
    expect(q('.ph-target').title).toBe('dut · 연결됨 · /dev/ttyUSB0 · 115200 8N1 · none · CR · RX 0 B · TX 0 B');
    const header = [...q('.ph-tools').children].map((e) => (e as HTMLElement).dataset.act ?? e.className);
    expect(header).toEqual(['rec', 'search', 'disconnect', 'more']);
    const overflow = q('.uart-lease .uart-overflow');
    expect(overflow.hidden).toBe(false);
    expect(overflow.querySelector('[data-act="theme"]')).not.toBeNull();
    expect(overflow.querySelector('[data-testid="uart-view-hex"]')).not.toBeNull();
    // moved tools keep working: HEX from the ⋯ row switches the view
    tid<HTMLButtonElement>('uart-view-hex').click();
    h.frame();
    expect(tid('uart-bytes').hidden).toBe(false);
  });

  it('widening restores the full header; an unknown width (hidden tab) keeps the current mode', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    setHeadWidth(700);
    expect(q('.phead').dataset.mode).toBe('compact');
    expect(q('.ph-frame').textContent).toBe('115200');
    setHeadWidth(0);
    expect(q('.phead').dataset.mode).toBe('compact');
    setHeadWidth(1200);
    expect(q('.phead').dataset.mode).toBe('full');
    expect(q('.ph-frame').textContent).toBe('115200 8N1 · none · CR');
    expect(q('.ph-port').hidden).toBe(false);
    expect(q('.uart-lease .uart-overflow').hidden).toBe(true);
    expect(q('.ph-tools').children).toHaveLength(13);
  });

  it('theme and settings no longer share a sun-like glyph', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    const theme = q('[data-act="theme"]').innerHTML;
    const gear = q('[data-act="settings"]').innerHTML;
    expect(theme).not.toBe(gear);
    expect(theme).toContain('fill="currentColor"'); // half-filled 'contrast' glyph
  });
});
