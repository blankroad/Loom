// @vitest-environment jsdom
// B1 spec (Opus): CAN-1 in the panel — time mode select, reference row, delta, Receive header.
// FUNCTIONS.md §B1-VIEW / §B1-PANEL / §B1-RXTX.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fmtWall, type LoomEvent } from '../../sdk';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { registerCanPanels } from './index';
import { TIME_MODE_KEY } from './timeFmt';

registerCanPanels();

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
});

const WALL0 = 1_794_742_414_837_452_625n; // ns, existing-proof value
const wallMs = (ns: bigint) => Number(ns / 1000n) / 1000;
const frame = (id: number, ms: number, o: { clock?: string; wallNs?: bigint | null } = {}) =>
  ev({
    kind: 'can.frame', channel: 'ch0', dir: 'rx', origin: 'device',
    ts: {
      mono_ns: String(ms * 1_000_000),
      wall_ns: o.wallNs === null ? '0' : String(o.wallNs ?? WALL0 + BigInt(ms) * 1_000_000n),
      clock_id: o.clock ?? 'proc-23427', source: 'synthetic',
    } as LoomEvent['ts'],
    payload: { id, ext: false, fd: false, dlc: 1, bytes: { encoding: 'hex', data: '01' } },
  });
const tid = <T extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<T>(`[data-testid="${id}"]`)!;
const rows = () => [...h!.host.querySelectorAll<HTMLElement>('.ct-row')].filter((r) => !r.hidden && !r.dataset.gap);
const tcell = (r: HTMLElement) => r.children[1] as HTMLElement;
const choose = (mode: string) => {
  const s = tid<HTMLSelectElement>('can-time-mode');
  s.value = mode;
  s.dispatchEvent(new Event('change', { bubbles: true }));
  h!.frame();
};

describe('CAN-1 trace time column', () => {
  it('default = wall clock with the same formatter as UART; unknown wall is not invented', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([frame(0x301, 1000), frame(0x302, 1010, { wallNs: null })]);
    expect(tid<HTMLSelectElement>('can-time-mode').value).toBe('wall');
    expect(tid('can-time-head').textContent).toBe('시각');
    const [a, b] = rows();
    expect(tcell(a!).textContent).toBe(fmtWall(wallMs(WALL0 + 1_000_000_000n)));
    expect(tcell(a!).title).toContain('clock proc-23427');
    expect(tcell(b!).textContent).toBe('--:--:--.---');
    expect(tcell(b!).className).toContain('ct-t-unknown');
    expect(tid('can-time-refbox').hidden).toBe(true);
  });

  it('relative: session start by default, a selected row becomes 0, reset returns to start', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([frame(0x300, 1000), frame(0x301, 1010), frame(0x302, 1250)]);
    choose('rel');
    expect(tid('can-time-head').textContent).toBe('상대(s)');
    expect(tid('can-time-refbox').hidden).toBe(false);
    expect(rows().map((r) => tcell(r).textContent)).toEqual(['+0.000000', '+0.010000', '+0.250000']);
    expect(tid('can-time-ref-label').textContent).toMatch(/^기준: 시작 seq \d+$/);
    const setRef = tid<HTMLButtonElement>('can-time-ref');
    expect(setRef.disabled).toBe(true); // nothing selected yet
    rows()[1]!.click();
    h.frame();
    expect(rows()[1]!.getAttribute('aria-selected')).toBe('true');
    expect(setRef.disabled).toBe(false);
    setRef.click();
    h.frame();
    expect(rows().map((r) => tcell(r).textContent)).toEqual(['-0.010000', '+0.000000', '+0.240000']);
    expect(tid('can-time-ref-label').textContent).toBe(`기준: seq ${rows()[1]!.dataset.seq} (선택 행)`);
    tid('can-time-ref-reset').click();
    h.frame();
    expect(tcell(rows()[0]!).textContent).toBe('+0.000000');
  });

  it('relative across a clock change shows ≠clock instead of a number', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([frame(0x300, 1000), frame(0x300, 5, { clock: 'proc-99' })]);
    choose('rel');
    expect(tcell(rows()[1]!).textContent).toBe('≠clock');
    expect(tcell(rows()[1]!).className).toContain('ct-t-xclock');
  });

  it('delta is to the previous SHOWN row (after the id filter)', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([frame(0x300, 1000), frame(0x301, 1005), frame(0x300, 1020)]);
    choose('delta');
    expect(tid('can-time-head').textContent).toBe('Δ(ms)');
    expect(rows().map((r) => tcell(r).textContent)).toEqual(['—', '+5.000', '+15.000']);
    const f = h.host.querySelector<HTMLInputElement>('.ct-filter-ids')!;
    f.value = '0x300';
    f.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    h.frame();
    expect(rows().map((r) => tcell(r).textContent)).toEqual(['—', '+20.000']);
    expect(tcell(rows()[1]!).title).toContain('필터 적용 후');
  });

  it('the mode is remembered per viewer; storage failure does not break the panel', () => {
    localStorage.setItem(TIME_MODE_KEY, 'delta');
    h = mountPanel('can.trace', { channel: 'ch0' });
    expect(tid<HTMLSelectElement>('can-time-mode').value).toBe('delta');
    choose('rel');
    expect(localStorage.getItem(TIME_MODE_KEY)).toBe('rel');
  });

  it('gap rows are not selectable as a reference', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([frame(0x300, 1000), { ...frame(0, 1001), kind: 'record.gap', payload: { seq_range: { from: '1', to: '2' }, count: 2, cause: 'disk' } } as LoomEvent]);
    choose('rel');
    const gap = h.host.querySelector<HTMLElement>('.ct-row[data-gap]')!;
    gap.click();
    h.frame();
    expect(gap.getAttribute('aria-selected')).not.toBe('true');
    expect(tid<HTMLButtonElement>('can-time-ref').disabled).toBe(true);
  });
});

describe('CAN-1 Receive view: last-time column states its base and carries the wall time', () => {
  it('header names the window base; the cell title has the row’s wall clock', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.batch([frame(0x310, 1000), frame(0x310, 1010)]);
    const head = h.host.querySelector('.crx-rxgrid.crx-head')!;
    expect(head.children[6]!.textContent).toBe('마지막 s (창 시작 기준)');
    const row = h.host.querySelector<HTMLElement>('[data-testid=can-rx-row]')!;
    expect(row.children[6]!.textContent).toBe('0.0100');
    expect((row.children[6] as HTMLElement).title).toContain(fmtWall(wallMs(WALL0 + 1_010_000_000n)));
  });
});
