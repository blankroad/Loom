// psu.gauge model (_xteam/explore-ui.md §psu.gauge). Pure readers + 60 s series ring.
// Provisional (ask EXPLORE): psu.state payload {set_v, set_i, out, prot, mode?}, measured
// values in the record's `metrics[{name: voltage|current|power, value, unit}]` (on
// psu.state or psu.meas), loss = source.gap{cause:"scpi_no_response"}.

import type { LoomEvent } from '../../sdk';

export const WINDOW_S = 60;
const MAX_POINTS = 4_096; // C2: ≥ 60 s at 50 Hz; oldest dropped

export interface PsuState {
  setV: number | null;
  setI: number | null;
  out: boolean | null;
  prot: string | null;
  mode: string | null;
}

export interface Meas {
  v: number | null;
  i: number | null;
  p: number | null;
  /** client clock of the last measurement (age display) */
  atMs: number | null;
}

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);

export function reduceState(prev: PsuState, ev: LoomEvent): PsuState {
  if (ev.kind !== 'psu.state') return prev;
  const p = obj(ev.payload);
  const out = p.out === true || p.out === 'on' ? true : p.out === false || p.out === 'off' ? false : prev.out;
  return {
    setV: num(p.set_v) ?? prev.setV,
    setI: num(p.set_i) ?? prev.setI,
    out,
    prot: typeof p.prot === 'string' ? p.prot : prev.prot,
    mode: typeof p.mode === 'string' ? p.mode : prev.mode,
  };
}

/** metrics[] → v/i/p (names: voltage|v, current|i, power|p). Missing → null. */
export function readMetrics(ev: LoomEvent): { v: number | null; i: number | null; p: number | null } | null {
  if (!ev.metrics || ev.metrics.length === 0) return null;
  const get = (...names: string[]) => num(ev.metrics?.find((m) => names.includes(m.name))?.value);
  const v = get('voltage', 'v');
  const i = get('current', 'i');
  const p = get('power', 'p') ?? (v !== null && i !== null ? v * i : null);
  return v === null && i === null && p === null ? null : { v, i, p };
}

/** time series for uPlot: x = seconds (daemon mono), null rows break the line at gaps. */
export class Series {
  t: number[] = [];
  v: (number | null)[] = [];
  i: (number | null)[] = [];
  push(tS: number, v: number | null, i: number | null): void {
    this.t.push(tS);
    this.v.push(v);
    this.i.push(i);
    this.prune(tS);
  }
  /** gap: a null sample so the plot does not join across the loss */
  gap(tS: number): void {
    this.push(tS, null, null);
  }
  private prune(nowS: number): void {
    let k = 0;
    while (k < this.t.length && (nowS - (this.t[k] as number) > WINDOW_S || this.t.length - k > MAX_POINTS)) k++;
    if (k > 0) {
      this.t.splice(0, k);
      this.v.splice(0, k);
      this.i.splice(0, k);
    }
  }
  data(): [number[], (number | null)[], (number | null)[]] {
    return [this.t, this.v, this.i];
  }
}

export function monoS(ev: LoomEvent): number {
  return /^\d+$/.test(ev.ts.mono_ns) ? Number(BigInt(ev.ts.mono_ns) / 1_000_000n) / 1000 : 0;
}

export const fmt = (x: number | null, unit: string, d = 3) => (x === null ? `— ${unit}` : `${x.toFixed(d)} ${unit}`);
