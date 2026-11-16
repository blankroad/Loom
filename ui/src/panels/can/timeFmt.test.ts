// B1 spec (Opus): CAN-1 time column — pure formatting. FUNCTIONS.md §B1-TIME.
import { describe, expect, it } from 'vitest';
import { fmtWall, type LoomEvent } from '../../sdk';
import type { KV } from './kv';
import {
  CLOCK_IDS_MAX,
  ClockDict,
  NO_CLOCK,
  readTimeMode,
  refLabel,
  TIME_MODE_KEY,
  timeCell,
  wallMsOf,
  writeTimeMode,
  type TimeInputs,
  type TimeRef,
} from './timeFmt';

const ts = (wall: string | undefined) => ({ ts: { mono_ns: '1', wall_ns: wall, clock_id: 'c', source: 'sim' } }) as unknown as Pick<LoomEvent, 'ts'>;
// existing proof (work/reports/usability-benchmark/ui/can-timestamp-existing-proof.json seq 736)
const WALL_NS = '1794742414837452625';
const WALL_MS = Number('1794742414837.452625');

const row = (o: Partial<TimeInputs> = {}): TimeInputs => ({ monoMs: 9_953_877.626791, wallMs: WALL_MS, clock: 0, clockId: 'proc-23427', seq: 736, ...o });
const startRef: TimeRef = { monoMs: 9_953_877.626791, clock: 0, seq: 736, kind: 'start' };

const memKV = (init: Record<string, string> = {}): KV & { data: Record<string, string> } => {
  const data = { ...init };
  return {
    data,
    getItem: (k) => (k in data ? data[k]! : null),
    setItem: (k, v) => void (data[k] = v),
    removeItem: (k) => void delete data[k],
  };
};
const throwingKV: KV = {
  getItem: () => {
    throw new Error('SecurityError');
  },
  setItem: () => {
    throw new Error('QuotaExceeded');
  },
  removeItem: () => {
    throw new Error('x');
  },
};

describe('wallMsOf — ts.wall_ns → ms, never invented', () => {
  it('keeps sub-ms precision of a real daemon wall stamp', () => {
    expect(wallMsOf(ts(WALL_NS))).toBeCloseTo(WALL_MS, 3);
    expect(wallMsOf(ts('1500000'))).toBeCloseTo(1.5, 9);
    expect(wallMsOf(ts('999'))).toBeCloseTo(0.000999, 9);
  });
  it('missing / zero / malformed → 0 (unknown)', () => {
    for (const w of [undefined, '', '0', '000', '-1', '12a', '1e18', '123456789012345678901', ' 1']) expect(wallMsOf(ts(w))).toBe(0);
    expect(wallMsOf({} as Pick<LoomEvent, 'ts'>)).toBe(0);
  });
});

describe('ClockDict — bounded clock_id interning', () => {
  it('stable indexes, unknown for empty ids, NO_CLOCK once full', () => {
    const d = new ClockDict();
    expect(d.index('proc-1')).toBe(0);
    expect(d.index('proc-2')).toBe(1);
    expect(d.index('proc-1')).toBe(0);
    expect(d.idOf(1)).toBe('proc-2');
    expect(d.index(undefined)).toBe(NO_CLOCK);
    expect(d.index(null)).toBe(NO_CLOCK);
    expect(d.index('')).toBe(NO_CLOCK);
    expect(d.idOf(NO_CLOCK)).toBeNull();
    expect(d.idOf(999)).toBeNull();
    for (let i = 3; i <= CLOCK_IDS_MAX; i++) d.index(`p${i}`);
    expect(d.size).toBe(CLOCK_IDS_MAX);
    expect(d.index('one-too-many')).toBe(NO_CLOCK);
    expect(d.size).toBe(CLOCK_IDS_MAX);
    expect(d.index('proc-2')).toBe(1); // known ids still resolve when full
  });
});

describe('timeCell — wall | rel | delta', () => {
  it('wall uses the shared fmtWall (UART parity) and puts the full facts in the title', () => {
    const c = timeCell('wall', row(), null, null, false);
    expect(c.text).toBe(fmtWall(WALL_MS));
    expect(c.cls).toBe('');
    expect(c.title).toContain(new Date(WALL_MS).toISOString());
    expect(c.title).toContain('mono 9953.877627 s');
    expect(c.title).toContain('clock proc-23427');
    expect(c.title).toContain('seq 736');
  });
  it('wall without ts.wall_ns is shown unknown — not derived from mono', () => {
    const c = timeCell('wall', row({ wallMs: 0 }), startRef, null, false);
    expect(c.text).toBe('--:--:--.---');
    expect(c.cls).toBe('ct-t-unknown');
    expect(c.title).toContain('벽시계 없음');
    expect(c.title).toContain('wall 없음');
  });
  it('rel: signed seconds with µs digits against the reference, same clock only', () => {
    expect(timeCell('rel', row(), startRef, null, false)).toMatchObject({ text: '+0.000000', cls: '' });
    expect(timeCell('rel', row({ monoMs: startRef.monoMs + 1234.568 }), startRef, null, false).text).toBe('+1.234568');
    expect(timeCell('rel', row({ monoMs: startRef.monoMs - 10 }), startRef, null, false).text).toBe('-0.010000');
    expect(timeCell('rel', row(), startRef, null, false).title).toContain('기준: 시작 seq 736');
    expect(timeCell('rel', row(), null, null, false)).toMatchObject({ text: '—', cls: 'ct-t-unknown' });
    const x = timeCell('rel', row({ clock: 1, clockId: 'proc-9' }), startRef, null, false);
    expect(x).toMatchObject({ text: '≠clock', cls: 'ct-t-xclock' });
    expect(x.title).toContain('proc-9');
    expect(timeCell('rel', row({ clock: NO_CLOCK, clockId: null }), { ...startRef, clock: NO_CLOCK }, null, false).text).toBe('≠clock');
  });
  it('delta: signed ms with 3 digits to the previous SHOWN row; gaps in between are flagged', () => {
    const prev = row({ monoMs: 1000, seq: 10 });
    const cur = row({ monoMs: 1100.0126, seq: 12 });
    expect(timeCell('delta', cur, null, prev, false)).toMatchObject({ text: '+100.013', cls: '' });
    expect(timeCell('delta', cur, null, prev, false).title).toContain('seq 10');
    const g = timeCell('delta', cur, null, prev, true);
    expect(g.cls).toBe('ct-t-gap');
    expect(g.title).toContain('생략·손실');
    expect(timeCell('delta', cur, null, null, false)).toMatchObject({ text: '—', cls: 'ct-t-unknown' });
    expect(timeCell('delta', row({ monoMs: 900 }), null, prev, false).text).toBe('-100.000');
    expect(timeCell('delta', cur, null, row({ clock: 3, clockId: 'other' }), false)).toMatchObject({ text: '≠clock', cls: 'ct-t-xclock' });
  });
  it('refLabel', () => {
    expect(refLabel(null)).toBe('기준 없음');
    expect(refLabel(startRef)).toBe('기준: 시작 seq 736');
    expect(refLabel({ ...startRef, seq: 9, kind: 'row' })).toBe('기준: seq 9 (선택 행)');
  });
});

describe('time mode is a per-viewer convenience (storage may fail)', () => {
  it('reads valid values, defaults to wall otherwise', () => {
    expect(readTimeMode(null)).toBe('wall');
    expect(readTimeMode(memKV())).toBe('wall');
    expect(readTimeMode(memKV({ [TIME_MODE_KEY]: 'delta' }))).toBe('delta');
    expect(readTimeMode(memKV({ [TIME_MODE_KEY]: 'rel' }))).toBe('rel');
    expect(readTimeMode(memKV({ [TIME_MODE_KEY]: 'mono' }))).toBe('wall');
    expect(readTimeMode(throwingKV)).toBe('wall');
  });
  it('writes, and swallows storage errors', () => {
    const kv = memKV();
    writeTimeMode(kv, 'rel');
    expect(kv.data[TIME_MODE_KEY]).toBe('rel');
    expect(() => writeTimeMode(throwingKV, 'rel')).not.toThrow();
    expect(() => writeTimeMode(null, 'rel')).not.toThrow();
  });
});
