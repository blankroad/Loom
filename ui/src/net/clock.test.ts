import { describe, expect, it } from 'vitest';
import { ClockSync, msToNs, toDaemonNs } from './clock';

const MS = 1_000_000n;
const BASE_MS = 1_762_000_000_000; // epoch-scale browser time (> 2^53 when in ns)

describe('msToNs', () => {
  it('keeps sub-ms precision at epoch scale (beyond 2^53 ns)', () => {
    // the double itself only resolves ~244 ns at this scale (finer than performance.now())
    const d = msToNs(BASE_MS + 0.123456) - (BigInt(BASE_MS) * MS + 123_456n);
    expect(d < 0n ? -d : d).toBeLessThan(1000n);
    expect(msToNs(0.5)).toBe(500_000n);
  });
});

describe('ClockSync', () => {
  // daemon mono = browser_ns + OFFSET
  const OFFSET = -1_700_000_000_000n * MS + 42n;
  const sample = (b0: number, rttMs: number, holdMs: number, clockId = 'mono-1', asym = 0) => {
    // one-way legs: up = (rtt - hold)/2 + asym, down = rest
    const up = (rttMs - holdMs) / 2 + asym;
    const d1 = msToNs(b0 + up) + OFFSET;
    const d2 = d1 + msToNs(holdMs);
    return { b0Ms: b0, b3Ms: b0 + rttMs, d1Ns: d1, d2Ns: d2, clockId };
  };

  it('implements theta=((d1-b0)+(d2-b3))/2 and err=delta/2', () => {
    const c = new ClockSync();
    expect(c.estimate().synced).toBe(false);
    c.add(sample(BASE_MS, 4, 1));
    const e = c.estimate();
    expect(e.synced).toBe(true);
    expect(e.thetaNs).toBe(OFFSET);
    expect(e.errMs).toBeCloseTo(1.5, 6); // delta = 4 - 1 = 3 ms
    expect(toDaemonNs(e, BASE_MS)).toBe(msToNs(BASE_MS) + OFFSET);
  });

  it('adopts the minimum-delta sample, so an asymmetric slow sample does not win', () => {
    const c = new ClockSync();
    c.add(sample(BASE_MS, 30, 0, 'mono-1', 10)); // slow & asymmetric -> biased theta
    const biased = c.estimate().thetaNs;
    expect(biased).not.toBe(OFFSET);
    c.add(sample(BASE_MS + 100, 2, 0));
    expect(c.estimate().thetaNs).toBe(OFFSET);
    c.add(sample(BASE_MS + 200, 50, 0, 'mono-1', 20)); // worse again: kept but not adopted
    expect(c.estimate().thetaNs).toBe(OFFSET);
    expect(c.estimate().errMs).toBeCloseTo(1, 6);
  });

  it('forgets samples older than 60 s', () => {
    const c = new ClockSync();
    c.add(sample(BASE_MS, 1, 0));
    c.add(sample(BASE_MS + 61_000, 9, 0));
    expect(c.size).toBe(1);
    expect(c.estimate().errMs).toBeCloseTo(4.5, 6);
  });

  it('discards the mapping when clock_id changes', () => {
    const c = new ClockSync();
    c.add(sample(BASE_MS, 1, 0));
    c.add(sample(BASE_MS + 10, 8, 0, 'mono-2'));
    expect(c.size).toBe(1);
    expect(c.estimate().clockId).toBe('mono-2');
    expect(c.estimate().errMs).toBeCloseTo(4, 6);
  });

  it('bounds the sample set (C2)', () => {
    const c = new ClockSync(60_000, 8);
    for (let i = 0; i < 20; i++) c.add(sample(BASE_MS + i, 5, 0));
    expect(c.size).toBe(8);
  });

  it('reset() returns to unsynced', () => {
    const c = new ClockSync();
    c.add(sample(BASE_MS, 1, 0));
    c.reset();
    expect(c.estimate().synced).toBe(false);
  });
});
