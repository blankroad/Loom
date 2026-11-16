// @vitest-environment jsdom
// operator-upgrade logic — batch 3a specification tests (Opus-authored; Sonnet must not edit).
// Findings: L3 radix · search · line view · copy · empty class column; L4 cursors · Δt · pulse width/period.
// Contract: work/reports/operator-upgrade/logic/FUNCTIONS.md (B3-*).
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';
import { CaptureData, readAnnotations, readEdges, type Annotation } from './logicModel';
import {
  asciiOf,
  copyHeader,
  fmtHz,
  formatWord,
  linesToText,
  measureCursors,
  parseBytesPattern,
  pulseAt,
  rowsToCsv,
  rowsToText,
  searchRows,
  secondsExact,
  splitLines,
  wordValue,
  type CopyMeta,
} from './logicTools';
import { paint } from './waveform';

const CAP = 'lc-b3';
const RATE = 1_000_000;
const row = (start: number, text: string, decoder = 'uart-1', len = 80) => ({ decoder, class: null, start_sample: start, end_sample: start + len, text });
const rowsOf = (list: ReturnType<typeof row>[]): Annotation[] =>
  readAnnotations('logic.annotations', { capture_id: CAP, samplerate: RATE, annotations: list }, CAP, 1n, RATE).rows;
const hexRows = (start: number, s: string, decoder = 'uart-1', step = 100) =>
  [...new TextEncoder().encode(s)].map((b, i) => row(start + i * step, b.toString(16).toUpperCase().padStart(2, '0'), decoder));
const chunk = (start: number, end: number, initial: number, list: unknown[], extra: Record<string, unknown> = {}) =>
  readEdges({ capture_id: CAP, samplerate: RATE, sample_start: start, sample_end: end, initial, edges: list, dropped: 0, ...extra }, CAP, 1n, null)!;
const capOf = (...chunks: ReturnType<typeof chunk>[]) => {
  const c = new CaptureData(CAP);
  for (const ch of chunks) c.addChunk(ch);
  return c;
};

// ---------------------------------------------------------------------------------------------
describe('B3a L3 — radix', () => {
  it('data words convert, non-data text is left alone', () => {
    expect(wordValue('4C')).toBe(0x4c);
    expect(wordValue('141')).toBe(0x141);
    expect(wordValue('Frame error')).toBeNull();
    expect(wordValue('')).toBeNull();
    expect(wordValue('123456789')).toBeNull();
    expect([0x4c, 0x5c, 0x0d, 0x0a, 0x09, 0x00, 0x7f, 0xff, 0x141].map(asciiOf)).toEqual(['L', '\\\\', '\\r', '\\n', '\\t', '\\x00', '\\x7F', '\\xFF', '\\x{141}']);
    expect(['hex', 'ascii', 'dec', 'bin'].map((r) => formatWord('4c', r as 'hex'))).toEqual(['4C', 'L', '76', '01001100']);
    expect(formatWord('0041', 'hex')).toBe('0041');
    expect(formatWord('141', 'bin')).toBe('0000000101000001');
    expect(formatWord('Parity error', 'ascii')).toBe('Parity error');
  });
});

describe('B3a L3 — line view', () => {
  it('CRLF, lone CR, LF and unterminated tails; terminator rows belong to their line', () => {
    const rows = rowsOf([...hexRows(0, 'LOOM\r\nok\rx')]);
    const lines = splitLines(rows);
    expect(lines.map((l) => [l.text, l.end, l.bytes, l.startSample, l.endSample])).toEqual([
      ['LOOM', 'crlf', 6, 0, 580],
      ['ok', 'cr', 3, 600, 880],
      ['x', 'none', 1, 900, 980],
    ]);
    expect(lines[0]!.rowKeys).toEqual(rows.slice(0, 6).map((r) => r.key));
    expect(lines[0]!.key).toBe(`uart-1:0:${rows[0]!.key}`);
  });

  it('empty lines, decoders kept apart, errors become note lines, long lines split', () => {
    const rows = rowsOf([
      ...hexRows(0, '\n\nA'),
      row(250, '42', 'uart-2'),
      row(400, 'Frame error'),
      ...hexRows(500, 'B'),
    ]);
    expect(splitLines(rows).map((l) => [l.decoder, l.text, l.end])).toEqual([
      ['uart-1', '', 'lf'],
      ['uart-1', '', 'lf'],
      ['uart-1', 'A', 'none'],
      ['uart-2', 'B', 'none'],
      ['uart-1', '⟦Frame error⟧', 'note'],
      ['uart-1', 'B', 'none'],
    ]);
    const long = splitLines(rowsOf(hexRows(0, 'ABCDE\u0001')), 3);
    expect(long.map((l) => [l.text, l.end])).toEqual([
      ['ABC', 'split'],
      ['DE', 'split'], // the next byte renders as 4 chars and would pass 3
      ['\\x01', 'none'],
    ]);
  });
});

describe('B3a L3 — search', () => {
  const rows = rowsOf([...hexRows(0, 'xxlogin: '), ...hexRows(50, 'LOGIN:', 'uart-2')]);
  it('finds text across consecutive words of one decoder, with sample span and row keys', () => {
    const r = searchRows(rows, { mode: 'text', pattern: 'login:', caseSensitive: true });
    expect(r.error).toBeNull();
    expect(r.hits).toEqual([{ firstKey: rows[2]!.key, lastKey: rows[7]!.key, decoder: 'uart-1', startSample: 200, endSample: 780 }]);
    const ci = searchRows(rows, { mode: 'text', pattern: 'LOGIN', caseSensitive: false });
    expect(ci.hits.map((h) => [h.decoder, h.startSample])).toEqual([
      ['uart-2', 50],
      ['uart-1', 200],
    ]);
  });

  it('an error row breaks contiguity; bytes mode; non-overlapping hits; limits and errors are explicit', () => {
    const broken = rowsOf([...hexRows(0, 'log'), row(250, 'Frame error'), ...hexRows(300, 'in')]);
    expect(searchRows(broken, { mode: 'text', pattern: 'login', caseSensitive: false }).hits).toEqual([]);
    expect(searchRows(rows, { mode: 'bytes', pattern: '6c 6f', caseSensitive: false }).hits).toHaveLength(1);
    expect(searchRows(rowsOf(hexRows(0, 'AAA')), { mode: 'text', pattern: 'AA', caseSensitive: true }).hits).toHaveLength(1);
    const many = searchRows(rowsOf(hexRows(0, 'ababab')), { mode: 'text', pattern: 'ab', caseSensitive: true }, 2);
    expect([many.hits.length, many.truncated]).toEqual([2, true]);
    expect(searchRows(rows, { mode: 'text', pattern: '', caseSensitive: true }).error).toBe('검색어가 비어 있습니다');
    expect(searchRows(rows, { mode: 'bytes', pattern: '6G', caseSensitive: true }).error).toBe('바이트 패턴: 16진수 바이트 (예: 4C 4F 4F 4D)');
    expect(searchRows(rows, { mode: 'text', pattern: 'x'.repeat(257), caseSensitive: true }).error).toBe('검색어가 256바이트를 넘습니다');
  });

  it('byte patterns', () => {
    const b = (s: string) => {
      const r = parseBytesPattern(s);
      return r ? [...r] : null;
    };
    expect(b('4C 4F')).toEqual([0x4c, 0x4f]);
    expect(b('4c4f')).toEqual([0x4c, 0x4f]);
    expect(b('0x4C,0x4F')).toEqual([0x4c, 0x4f]);
    expect(b('4 F')).toEqual([0x04, 0x0f]);
    expect(b('4c4')).toBeNull();
    expect(b('')).toBeNull();
    expect(b('zz')).toBeNull();
    expect(b('00'.repeat(257))).toBeNull();
  });
});

describe('B3a L3 — copy text', () => {
  const meta: CopyMeta = { captureId: CAP, samplerate: RATE, source: 'panel', scope: 'all', fromSample: 10, toSample: 180, coverage: 'partial' };
  const rows = rowsOf([row(10, '4C'), { ...row(100, 'a,"b"'), end_sample: 180 }]);
  it('exact seconds like the daemon export', () => {
    expect([secondsExact(10, RATE), secondsExact(0, RATE), secondsExact(1_500_000, RATE), secondsExact(1, 3), secondsExact(5, null), secondsExact(36_000_000_000, 10_000_000)]).toEqual([
      '0.00001',
      '0',
      '1.5',
      '0.333333333',
      '#5',
      '3600',
    ]);
  });
  it('header, text in radix, CSV raw and escaped, lines', () => {
    const head = '# loom logic copy\n# capture_id=lc-b3\n# source=panel\n# scope=all\n# from_sample=10\n# to_sample=180\n# rows=2\n# coverage=partial\n';
    expect(copyHeader(meta, 2)).toBe(head);
    expect(copyHeader(meta, 2, 'ascii')).toBe(`${head}# radix=ascii\n`);
    expect(rowsToText(rows, 'ascii', meta)).toBe(`${head}# radix=ascii\n0.00001 uart-1: L\n0.0001 uart-1: a,"b"\n`);
    expect(rowsToCsv(rows, meta)).toBe(`${head}start_sample,end_sample,t_start_s,t_end_s,decoder,text\n10,90,0.00001,0.00009,uart-1,4C\n100,180,0.0001,0.00018,uart-1,"a,""b"""\n`);
    const lines = splitLines(rowsOf(hexRows(0, 'hi\n')));
    expect(linesToText(lines, { ...meta, fromSample: 0, toSample: 280 })).toBe(
      '# loom logic copy\n# capture_id=lc-b3\n# source=panel\n# scope=all\n# from_sample=0\n# to_sample=280\n# rows=1\n# coverage=partial\n# radix=ascii\n0 uart-1: hi\n',
    );
  });
});

// ---------------------------------------------------------------------------------------------
describe('B3a L4 — cursors and pulses', () => {
  it('Δ, Δt and 1/Δt', () => {
    expect(['115200', '0.5', '2000000', '1234.5678', '999.9996'].map((s) => fmtHz(Number(s)))).toEqual(['115.2 kHz', '0.5 Hz', '2 MHz', '1.235 kHz', '1 kHz']);
    const m = measureCursors({ a: 968, b: 100 }, RATE)!;
    expect([m.dSamples, m.dtS, m.text]).toEqual([868, 0.000868, 'A 968 µs · B 100 µs · Δ 868 smp · Δt 868 µs · 1/Δt 1.152 kHz']);
    expect(m.freqHz).toBeCloseTo(1152.07, 2);
    expect(measureCursors({ a: 5, b: 5 }, RATE)!.text).toBe('A 5 µs · B 5 µs · Δ 0 smp · Δt 0 s');
    expect(measureCursors({ a: 100, b: 968 }, null)!.text).toBe('A #100 · B #968 · Δ 868 smp');
    expect(measureCursors({ a: 100, b: null }, RATE)).toBeNull();
  });

  it('pulse width, period and duty from real transitions only', () => {
    const c = capOf(chunk(0, 100, 0, [[10, 1], [20, 0], [40, 1], [50, 0], [60, 2], [70, 0]]));
    const hi = pulseAt(c, 0, 15, RATE)!;
    expect([hi.level, hi.start, hi.end, hi.nextEnd, hi.text]).toEqual([1, 10, 20, 40, 'HIGH 10 µs (10 smp) · 주기 30 µs · 33.333 kHz · duty 33.3%']);
    const lo = pulseAt(c, 0, 25, RATE)!;
    expect([lo.level, lo.start, lo.end, lo.nextEnd, lo.dutyPct]).toEqual([0, 20, 40, 50, (10 / 30) * 100]);
    // bit-1 transitions (60, 70) do not end the bit-0 LOW run that began at 50 — and it never ends in data
    expect(pulseAt(c, 0, 65, RATE)).toBeNull();
    expect(pulseAt(c, 1, 65, RATE)!.text).toBe('HIGH 10 µs (10 smp)');
    expect(pulseAt(c, 0, 45, RATE)!.text).toBe('HIGH 10 µs (10 smp)'); // next run end not in data → no period
    expect(pulseAt(c, 0, 5, RATE)).toBeNull(); // level began before the first transition seen
    expect(pulseAt(c, 0, 15, null)!.text).toBe('HIGH 10 smp · 주기 30 smp · duty 33.3%');
  });

  it('spans contiguous chunks but never a gap, an edge cap or evicted data', () => {
    expect(pulseAt(capOf(chunk(0, 30, 0, [[10, 1]]), chunk(30, 60, 1, [[45, 0]])), 0, 35, RATE)!.start).toBe(10);
    expect(pulseAt(capOf(chunk(0, 30, 0, [[10, 1]]), chunk(31, 60, 1, [[45, 0]])), 0, 35, RATE)).toBeNull();
    expect(pulseAt(capOf(chunk(0, 100, 0, [[10, 1], [20, 0]], { dropped: 3 })), 0, 15, RATE)).not.toBeNull();
    expect(pulseAt(capOf(chunk(0, 100, 0, [[10, 1], [20, 0]], { dropped: 3 })), 0, 25, RATE)).toBeNull(); // after the cap: unknown
  });
});

describe('B3a L4 — painter draws cursors', () => {
  it('A and B labelled lines inside the plot', () => {
    const calls: [string, unknown[]][] = [];
    const g = new Proxy({} as Record<string, unknown>, {
      get: (_t, k) => (k === 'canvas' ? null : (...a: unknown[]) => void calls.push([String(k), a])),
      set: () => true,
    }) as unknown as CanvasRenderingContext2D;
    const pal = { bg: '#fff', grid: '#eee', fg: '#000', muted: '#666', hi: '#070', lo: '#070', dense: '#8c8', unknown: '#c80', ann: '#36c', annText: '#126', cursor: '#d40', sel: '#d40' };
    paint(g, { cap: capOf(chunk(0, 100, 0, [])), lanes: [{ bit: 0, name: 'D0' }], decoders: [], view: { s0: 0, spp: 1, width: 400 }, rate: RATE, palette: pal, hoverX: null, selected: null, cursors: { a: 10, b: 50 } }, 80);
    expect(calls.some(([k, a]) => k === 'fillText' && a[0] === 'A')).toBe(true);
    expect(calls.some(([k, a]) => k === 'fillText' && a[0] === 'B')).toBe(true);
    expect(calls.some(([k, a]) => k === 'moveTo' && a[0] === 96 + 10 + 0.5)).toBe(true);
    const n = calls.length;
    paint(g, { cap: capOf(chunk(0, 100, 0, [])), lanes: [{ bit: 0, name: 'D0' }], decoders: [], view: { s0: 0, spp: 1, width: 400 }, rate: RATE, palette: pal, hoverX: null, selected: null, cursors: { a: 1000, b: null } }, 80);
    expect(calls.slice(n).some(([k, a]) => k === 'fillText' && (a[0] === 'A' || a[0] === 'B'))).toBe(false); // off-screen / unset
  });
});

// ---------------------------------------------------------------------------------------------
describe('B3a panel', () => {
  let h: Harness | null = null;
  const written: string[] = [];
  afterEach(() => {
    h?.dispose();
    h = null;
    document.body.replaceChildren();
    written.length = 0;
    vi.unstubAllGlobals();
    try {
      localStorage.clear();
    } catch {
      /* ignore */
    }
  });
  const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
  const setVal = (id: string, v: string, type = 'change') => {
    const e = q<HTMLInputElement | HTMLSelectElement>(id);
    e.value = v;
    e.dispatchEvent(new Event(type, { bubbles: true }));
    h!.frame();
  };
  const mount = () => {
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: (t: string) => (written.push(t), Promise.resolve()) } });
    h = mountPanel('logic.analyzer');
    h.net.emitControl(ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: '10', origin: 'device', payload: { capture_id: CAP, state: 'running', source: 'synthetic', samplerate: RATE, channels: ['D0'], counters: { samples: 0, annotations: 0 } } }));
    h.batch([
      ev({ kind: 'logic.edges', domain: 'logic', channel: CAP, seq: '11', payload: { capture_id: CAP, samplerate: RATE, sample_start: 0, sample_end: 2000, initial: 1, edges: [[100, 0], [110, 1], [140, 0], [150, 1]], dropped: 0 } }),
      ev({ kind: 'logic.annotations', domain: 'logic', channel: CAP, seq: '12', payload: { capture_id: CAP, samplerate: RATE, annotations: hexRows(0, 'LOOM\r\nok'), dropped: 0 } }),
    ]);
  };
  const flush = async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve();
  };

  it('empty class column is not shown; radix switches the text column and persists', () => {
    mount();
    expect(h!.host.querySelector('.lg-table .vt-head')!.textContent).not.toContain('class');
    expect(h!.host.querySelector('.lg-table .vt-row[data-start="0"]')!.textContent).toContain('4C');
    setVal('logic-radix', 'ascii');
    expect(h!.host.querySelector('.lg-table .vt-row[data-start="0"]')!.textContent).toContain('L');
    expect(h!.host.querySelector('.lg-table .vt-row[data-start="400"]')!.textContent).toContain('\\r');
    h!.dispose();
    mount();
    expect(q<HTMLSelectElement>('logic-radix').value).toBe('ascii');
  });

  it('line view shows CR/LF lines; selecting a line sets cursors to its span', () => {
    mount();
    setVal('logic-table-mode', 'lines');
    const lines = [...h!.host.querySelectorAll('[data-testid="logic-lines"] .vt-row')].map((r) => r.textContent);
    expect(lines.some((t) => t!.includes('LOOM'))).toBe(true);
    expect(lines.some((t) => t!.includes('ok'))).toBe(true);
    (h!.host.querySelector('[data-testid="logic-lines"] .vt-row') as HTMLElement).click();
    h!.frame();
    expect(q('logic-cursors').textContent).toContain('Δ 580 smp');
  });

  it('search reports n/m, jumps to the hit row and spans it with cursors', () => {
    mount();
    setVal('logic-search', 'OOM', 'input');
    expect(q('logic-search-status').textContent).toBe('0/1'); // found, none selected yet
    q('logic-search-next').click();
    h!.frame();
    expect(q('logic-search-status').textContent).toBe('1/1');
    expect(h!.host.querySelector('.lg-table .vt-row[aria-selected="true"]')!.getAttribute('data-start')).toBe('100');
    expect(q('logic-cursors').textContent).toContain('Δ 280 smp');
    setVal('logic-search-mode', 'bytes');
    expect(q('logic-search-status').textContent).toBe('바이트 패턴: 16진수 바이트 (예: 4C 4F 4F 4D)');
    setVal('logic-search', '4F 4D', 'input');
    expect(q('logic-search-status').textContent).toBe('0/1');
  });

  it('copy writes the held rows with metadata (radix applied to text)', async () => {
    mount();
    q('logic-copy').click();
    await flush();
    expect(written).toHaveLength(1);
    expect(written[0]!.startsWith('# loom logic copy\n# capture_id=lc-b3\n# source=panel\n# scope=all\n')).toBe(true);
    expect(written[0]).toContain('# coverage=live');
    expect(written[0]).toContain('0 uart-1: 4C');
    setVal('logic-copy-format', 'csv');
    q('logic-copy').click();
    await flush();
    expect(written[1]).toContain('start_sample,end_sample,t_start_s,t_end_s,decoder,text\n0,80,0,0.00008,uart-1,4C\n');
  });

  it('cursors: click = A, Shift+click = B, keys 1/2, Esc clears; a table row spans A–B', () => {
    mount();
    const wave = q('logic-wave');
    const at = (x: number, shift = false) => {
      wave.dispatchEvent(new MouseEvent('pointermove', { clientX: 96 + x, clientY: 30, bubbles: true }));
      wave.dispatchEvent(new MouseEvent('pointerdown', { clientX: 96 + x, clientY: 30, button: 0, bubbles: true }));
      wave.dispatchEvent(new MouseEvent('pointerup', { clientX: 96 + x, clientY: 30, shiftKey: shift, bubbles: true }));
      h!.frame();
    };
    expect(q('logic-cursors').textContent).toContain('클릭=A');
    at(10);
    expect(q('logic-cursors').textContent).toMatch(/^A /);
    at(20, true);
    expect(q('logic-cursors').textContent).toContain(' · Δ ');
    wave.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    h!.frame();
    expect(q('logic-cursors').textContent).toContain('클릭=A');
    wave.dispatchEvent(new MouseEvent('pointermove', { clientX: 96 + 30, clientY: 30, bubbles: true }));
    wave.dispatchEvent(new KeyboardEvent('keydown', { key: '1', bubbles: true }));
    wave.dispatchEvent(new MouseEvent('pointermove', { clientX: 96 + 60, clientY: 30, bubbles: true }));
    wave.dispatchEvent(new KeyboardEvent('keydown', { key: '2', bubbles: true }));
    h!.frame();
    expect(q('logic-cursors').textContent).toContain(' · Δ ');
    (h!.host.querySelector('.lg-table .vt-row[data-start="100"]') as HTMLElement).click();
    h!.frame();
    expect(q('logic-cursors').textContent).toContain('A 100 µs · B 180 µs · Δ 80 smp');
    q('logic-cursor-clear').click();
    h!.frame();
    expect(q('logic-cursors').textContent).toContain('클릭=A');
  });

  it('a drag still pans and does not place a cursor', () => {
    mount();
    const wave = q('logic-wave');
    wave.dispatchEvent(new MouseEvent('pointerdown', { clientX: 300, clientY: 30, button: 0, bubbles: true }));
    wave.dispatchEvent(new MouseEvent('pointermove', { clientX: 250, clientY: 30, bubbles: true }));
    wave.dispatchEvent(new MouseEvent('pointerup', { clientX: 250, clientY: 30, bubbles: true }));
    h!.frame();
    expect(q('logic-cursors').textContent).toContain('클릭=A');
    expect(q('logic-follow').getAttribute('aria-pressed')).toBe('false');
  });

  it('hovering a lane shows the pulse under the pointer, or nothing when unknown', () => {
    mount();
    // zoom so the 100..150 pulses are visible: centre on the row at 100 (view resets around it)
    (h!.host.querySelector('.lg-table .vt-row[data-start="100"]') as HTMLElement).click();
    h!.frame();
    const wave = q('logic-wave');
    const win = q('logic-window').textContent!;
    const m = /^([\d.]+) (s|ms|µs|ns) – ([\d.]+) (s|ms|µs|ns) · /.exec(win)!;
    const us = (v: string, u: string) => Number(v) * { s: 1e6, ms: 1e3, µs: 1, ns: 1e-3 }[u as 's']!;
    const s0 = us(m[1]!, m[2]!);
    const s1 = us(m[3]!, m[4]!);
    const width = 900 - 96; // jsdom clientWidth 0 → panel fallback 900
    const xOf = (sample: number) => ((sample - s0) / (s1 - s0)) * width;
    wave.dispatchEvent(new MouseEvent('pointermove', { clientX: 96 + xOf(125), clientY: 22 + 14, bubbles: true }));
    h!.frame();
    expect(q('logic-readout').textContent).toContain('HIGH 30 µs (30 smp)');
  });
});
