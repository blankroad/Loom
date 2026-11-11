// ClockSync — maps browser time onto daemon mono time (plan §3.5, CR-UI-03).
// Pure: samples in, estimate out. All ns arithmetic in BigInt (u64 > 2^53).
//
//   b0 = browser send, b3 = browser receive   (ms, performance.timeOrigin + now())
//   d1 = daemon receive, d2 = daemon send     (ns, daemon mono)
//   theta = ((d1 - b0) + (d2 - b3)) / 2       daemon_mono ≈ browser_ns + theta
//   delta = (b3 - b0) - (d2 - d1)             round trip minus daemon hold time
// The sample with the smallest delta in the last 60 s wins (NTP style);
// uncertainty = ±delta/2. A clock_id or stream epoch change discards everything.
// Display smoothing (5 ms steps eased over 1 s) is a shell concern, not done here.

import type { ClockEstimate } from './api';

export interface ClockSample {
  b0Ms: number;
  b3Ms: number;
  d1Ns: bigint;
  d2Ns: bigint;
  clockId: string;
}

interface Kept {
  atMs: number;
  thetaNs: bigint;
  deltaNs: bigint;
}

/** ms float (epoch-based, ~1.7e12) -> ns BigInt without losing sub-ms precision. */
export function msToNs(ms: number): bigint {
  const whole = Math.floor(ms);
  return BigInt(whole) * 1_000_000n + BigInt(Math.round((ms - whole) * 1e6));
}

export const UNSYNCED: ClockEstimate = Object.freeze({ thetaNs: 0n, errMs: Number.POSITIVE_INFINITY, clockId: '', synced: false });

export class ClockSync {
  private samples: Kept[] = [];
  private clockId = '';
  private best: ClockEstimate = UNSYNCED;

  /**
   * @param windowMs samples older than this (vs. the newest) are discarded
   * @param maxSamples C2 bound; overflow drops the oldest sample
   */
  constructor(
    private readonly windowMs = 60_000,
    private readonly maxSamples = 64,
  ) {}

  reset(): void {
    this.samples = [];
    this.clockId = '';
    this.best = UNSYNCED;
  }

  /** Returns true when the published estimate changed. */
  add(s: ClockSample): boolean {
    if (s.clockId !== this.clockId) {
      this.samples = [];
      this.clockId = s.clockId;
    }
    const b0 = msToNs(s.b0Ms);
    const b3 = msToNs(s.b3Ms);
    const thetaNs = (s.d1Ns - b0 + (s.d2Ns - b3)) / 2n;
    let deltaNs = b3 - b0 - (s.d2Ns - s.d1Ns);
    if (deltaNs < 0n) deltaNs = 0n; // daemon hold time > round trip: clocks are lying, treat as exact
    this.samples.push({ atMs: s.b3Ms, thetaNs, deltaNs });
    const cutoff = s.b3Ms - this.windowMs;
    this.samples = this.samples.filter((k) => k.atMs >= cutoff);
    while (this.samples.length > this.maxSamples) this.samples.shift();

    let pick = this.samples[0]!;
    for (const k of this.samples) if (k.deltaNs < pick.deltaNs) pick = k;
    const next: ClockEstimate = { thetaNs: pick.thetaNs, errMs: Number(pick.deltaNs) / 2 / 1e6, clockId: this.clockId, synced: true };
    const changed = next.thetaNs !== this.best.thetaNs || next.errMs !== this.best.errMs || next.clockId !== this.best.clockId;
    this.best = next;
    return changed;
  }

  estimate(): ClockEstimate {
    return this.best;
  }

  get size(): number {
    return this.samples.length;
  }
}

/** daemon mono ns for a browser timestamp (ms). */
export function toDaemonNs(est: ClockEstimate, browserMs: number): bigint {
  return msToNs(browserMs) + est.thetaNs;
}
