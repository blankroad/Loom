// @vitest-environment jsdom
// operator-upgrade logic — batch 6.1 specification tests (Opus-authored; Sonnet must not edit).
// ROOT follow-up (live marker run, first3): every live append changed shown.version and reset searchIdx to −1, so the
// status stayed "0/2" and Next/Enter could not advance RX → TX while rows kept arriving.
// Contract: work/reports/operator-upgrade/logic/FUNCTIONS.md §B6.1.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';
import { relocateHit, type SearchHit } from './logicTools';

const hit = (decoder: string, startSample: number, endSample: number, k = 'x'): SearchHit => ({ firstKey: `${k}a`, lastKey: `${k}b`, decoder, startSample, endSample });

describe('B6.1 relocateHit', () => {
  it('finds the same hit by decoder + span, whatever its keys or position', () => {
    const hits = [hit('uart-1', 0, 380), hit('uart-2', 10_000, 10_380), hit('uart-1', 30_000, 30_380)];
    expect(relocateHit(null, hits)).toBe(-1);
    expect(relocateHit(hit('uart-2', 10_000, 10_380, 'other-keys'), hits)).toBe(1);
    expect(relocateHit(hit('uart-1', 30_000, 30_380), hits)).toBe(2);
    expect(relocateHit(hit('uart-1', 10_000, 10_380), hits)).toBe(-1); // same span, other decoder
    expect(relocateHit(hit('uart-1', 0, 381), hits)).toBe(-1); // span changed → lost
    expect(relocateHit(hit('uart-1', 0, 380), [])).toBe(-1);
    expect(relocateHit(hit('uart-1', 0, 380), [hit('uart-1', 0, 380, 'p'), hit('uart-1', 0, 380, 'q')])).toBe(0);
  });
});

const CAP = 'lc-b61';
const RATE = 1_000_000;
const row = (start: number, text: string, decoder: string) => ({ decoder, class: null, start_sample: start, end_sample: start + 80, text, role: 'data' });
const hexRows = (start: number, s: string, decoder = 'uart-1', step = 100) =>
  [...new TextEncoder().encode(s)].map((b, i) => row(start + i * step, b.toString(16).toUpperCase().padStart(2, '0'), decoder));

describe('B6.1 panel — search selection survives live appends', () => {
  let h: Harness | null = null;
  let seq = 20;
  afterEach(() => {
    h?.dispose();
    h = null;
    document.body.replaceChildren();
    vi.unstubAllGlobals();
    try {
      localStorage.clear();
    } catch {
      /* ignore */
    }
  });
  const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
  const status = () => q('logic-search-status').textContent;
  const selectedStart = () => h!.host.querySelector('.lg-table .vt-row[aria-selected="true"]')?.getAttribute('data-start') ?? null;
  const append = (rows: ReturnType<typeof row>[]) => {
    seq++;
    h!.batch([ev({ kind: 'logic.annotations', domain: 'logic', channel: CAP, seq: String(seq), payload: { capture_id: CAP, samplerate: RATE, annotations: rows, dropped: 0 } })]);
    h!.frame();
  };
  const search = (text: string) => {
    const s = q<HTMLInputElement>('logic-search');
    s.value = text;
    s.dispatchEvent(new Event('input', { bubbles: true }));
    h!.frame();
  };
  const enter = () => {
    q('logic-search').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    h!.frame();
  };
  const mount = () => {
    seq = 20;
    h = mountPanel('logic.analyzer');
    h.net.emitControl(
      ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: '10', origin: 'device', payload: { capture_id: CAP, state: 'running', source: 'synthetic', samplerate: RATE, channels: ['D0', 'D1'], decoder_tracks: [{ id: 'uart-1', line: 'rx', channel: 'D0' }, { id: 'uart-2', line: 'tx', channel: 'D1' }] } }),
    );
    h.batch([ev({ kind: 'logic.edges', domain: 'logic', channel: CAP, seq: '11', payload: { capture_id: CAP, samplerate: RATE, sample_start: 0, sample_end: 40_000, initial: 3, edges: [], dropped: 0 } })]);
    append([...hexRows(0, 'LOOM\r\n', 'uart-1'), ...hexRows(10_000, 'LOOM\r\n', 'uart-2')]); // RX + TX marker
    h.frame();
  };

  it("ROOT's case: Next/Enter advance RX → TX while unrelated rows keep arriving", () => {
    mount();
    search('LOOM');
    expect(status()).toBe('0/2');
    enter();
    expect(status()).toBe('1/2');
    const first = selectedStart();
    expect(first === '0' || first === '10000').toBe(true);
    append(hexRows(20_000, 'zz', 'uart-1')); // live append: new version, same query
    expect(status()).toBe('1/2'); // selection kept (was reset to 0/2)
    expect(selectedStart()).toBe(first);
    enter();
    expect(status()).toBe('2/2');
    const second = selectedStart();
    expect(second).not.toBe(first);
    expect([first, second].sort()).toEqual(['0', '10000']); // both RX and TX reached
    append(hexRows(21_000, 'zz', 'uart-2'));
    expect(status()).toBe('2/2');
    q('logic-search-prev').click();
    h!.frame();
    expect(status()).toBe('1/2');
    expect(selectedStart()).toBe(first);
  });

  it('a new matching row keeps the selected hit (by identity) and counts the new hit', () => {
    mount();
    search('LOOM');
    enter();
    const first = selectedStart();
    append(hexRows(30_000, 'LOOM\r\n', 'uart-1'));
    expect(status()).toMatch(/^[123]\/3$/);
    expect(status()).not.toBe('0/3');
    expect(selectedStart()).toBe(first);
  });

  it('changing the query still resets the selection', () => {
    mount();
    search('LOOM');
    enter();
    expect(status()).toBe('1/2');
    search('OOM');
    expect(status()).toBe('0/2');
    enter();
    expect(status()).toBe('1/2');
    const c = q<HTMLInputElement>('logic-search-case');
    c.checked = !c.checked;
    c.dispatchEvent(new Event('change', { bubbles: true }));
    h!.frame();
    expect(status()).toBe('0/2');
  });
});
