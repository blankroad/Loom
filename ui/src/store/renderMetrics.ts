// Browser-side lag and render metrics (plan §0.2, §3.5, §3.6, §9.5).
//
// Lag (§3.5-5): lag = age of the oldest event this panel received but has not drawn yet,
//   mono_est(now) − ts.mono_ns, where mono_est = browser time + net clock theta (BigInt ns).
//   Nothing pending → lag 0 and a separate "idle N s" (a quiet device is not latency).
//   Clock not synced, or the event carries a different clock_id (device HW clock, §3.5-8)
//   → the age since main-thread receipt is used instead and the reading says so
//   (`basis: 'recv'`); this under-reports (CORE + WS part missing), never over-reports.
//
// Stages per panel (bounded log histograms, reset every report period):
//   receive  main-thread arrival − ts.mono_ns of the batch's newest event (clock based;
//            includes CORE, batching, WS and the worker — not UI-only)
//   decode   (net worker does not expose it yet — n = 0, see report p1-m1)
//   apply    main-thread mirror + RowStore apply of one batch
//   draw     panel draw callback (RenderScheduler)
//   next_raf draw end → next animation frame start (paint proxy, §3.6)
//   ui       main-thread receipt → end of the draw that covered it (UI segment, §0.2)
//
// C2: per panel ≤ PENDING_MARKERS batch markers (excess merged into the newest),
// histograms are fixed Uint32Array(HIST_BUCKETS) per stage, panels ≤ MAX_PANELS,
// sample-mode buffer ≤ SAMPLE_MAX per period.

import type { LoomEvent } from '../proto/provisional';

export type Stage = 'receive' | 'decode' | 'apply' | 'draw' | 'next_raf' | 'ui';
export const STAGES: readonly Stage[] = ['receive', 'decode', 'apply', 'draw', 'next_raf', 'ui'];

export interface ClockView {
  thetaNs: bigint;
  errMs: number;
  clockId: string;
  synced: boolean;
}

export const PENDING_MARKERS = 256;
export const MAX_PANELS = 64;
export const SAMPLE_EVERY = 50;
export const SAMPLE_MAX = 400;

// ---- bounded log histogram ----------------------------------------------------------

const H_MIN_MS = 0.05;
const H_STEPS_PER_OCTAVE = 8; // ≈ 9 % resolution
export const HIST_BUCKETS = 168; // 0.05 ms · 2^(167/8) ≈ 96 s; larger values land in the last bucket

export class LogHistogram {
  readonly counts = new Uint32Array(HIST_BUCKETS);
  n = 0;
  max = 0;
  add(ms: number): void {
    if (!(ms >= 0)) return; // NaN / negative (clock step) are not samples
    const i = ms <= H_MIN_MS ? 0 : Math.min(HIST_BUCKETS - 1, Math.floor(Math.log2(ms / H_MIN_MS) * H_STEPS_PER_OCTAVE) + 1);
    this.counts[i]!++;
    this.n++;
    if (ms > this.max) this.max = ms;
  }
  /** upper edge of the bucket holding the q-quantile (never above the observed max) */
  quantile(q: number): number {
    if (this.n === 0) return 0;
    const target = Math.max(1, Math.ceil(q * this.n));
    let acc = 0;
    for (let i = 0; i < HIST_BUCKETS; i++) {
      acc += this.counts[i]!;
      if (acc >= target) return Math.min(this.max, H_MIN_MS * 2 ** (i / H_STEPS_PER_OCTAVE));
    }
    return this.max;
  }
  reset(): void {
    this.counts.fill(0);
    this.n = 0;
    this.max = 0;
  }
}

// ---- clock helpers ---------------------------------------------------------------------

/** daemon mono ns for an absolute browser time (performance.timeOrigin + now, ms) */
export function daemonNowNs(clock: ClockView, absMs: number): bigint {
  const whole = Math.floor(absMs);
  return BigInt(whole) * 1_000_000n + BigInt(Math.round((absMs - whole) * 1e6)) + clock.thetaNs;
}

const parseNs = (s: string | undefined): bigint | null => (typeof s === 'string' && /^\d{1,20}$/.test(s) ? BigInt(s) : null);
/** BigInt difference first, then ms Number (§3.5-4) */
const ageMs = (nowNs: bigint, thenNs: bigint): number => Number((nowNs - thenNs) / 1_000n) / 1_000;

// ---- per-panel lag tracker ---------------------------------------------------------------

interface Marker {
  firstSeq: bigint;
  lastSeq: bigint;
  monoNs: bigint | null;
  clockId: string;
  recvMs: number;
  count: number;
}

export interface LagReading {
  lagMs: number;
  /** null when something is pending or nothing was ever received */
  idleMs: number | null;
  basis: 'mono' | 'recv' | 'none';
  /** age of the oldest pending event (same as lagMs when pending; null when none) */
  oldestPendingAgeMs: number | null;
  pendingEvents: number;
  clockErrMs: number;
  clockSynced: boolean;
  /** server-side batch lag_ms of the last batch (tooltip only) */
  serverLagMs: number;
}

const seqOf = (e: LoomEvent): bigint => (/^\d+$/.test(e.seq) ? BigInt(e.seq) : 0n);

export class LagTracker {
  private markers: Marker[] = [];
  lastRecvMs: number | null = null;
  renderedThrough: bigint | null = null;
  serverLagMs = 0;
  /** events received by this panel (monotonic; hidden-period counts use deltas) */
  received = 0;

  /** a data batch reached the main thread */
  receive(evs: readonly LoomEvent[], recvMs: number, serverLagMs: number): void {
    this.serverLagMs = serverLagMs;
    if (evs.length === 0) return;
    this.received += evs.length;
    this.lastRecvMs = recvMs;
    const first = evs[0] as LoomEvent;
    const last = evs[evs.length - 1] as LoomEvent;
    const m: Marker = {
      firstSeq: seqOf(first),
      lastSeq: seqOf(last),
      monoNs: parseNs(first.ts?.mono_ns),
      clockId: first.ts?.clock_id ?? '',
      recvMs,
      count: evs.length,
    };
    if (this.markers.length >= PENDING_MARKERS) {
      // C2: merge into the newest marker (keeps the oldest age, which is what lag reads)
      const tail = this.markers[this.markers.length - 1] as Marker;
      tail.lastSeq = m.lastSeq;
      tail.count += m.count;
      return;
    }
    this.markers.push(m);
  }

  /**
   * A draw finished. `seq` = highest seq the draw covered (panel return value); without
   * it, everything received before the draw started counts as drawn.
   * Calls `onDrawn(uiMs)` once per batch marker that this draw retired.
   */
  drawn(seq: bigint | null, drawStartMs: number, drawEndMs: number, onDrawn?: (uiMs: number) => void): void {
    let k = 0;
    while (k < this.markers.length) {
      const m = this.markers[k] as Marker;
      const done = seq !== null ? m.lastSeq <= seq : m.recvMs <= drawStartMs;
      if (!done) break;
      onDrawn?.(drawEndMs - m.recvMs);
      k++;
    }
    if (k > 0) this.markers.splice(0, k);
    if (seq !== null && (this.renderedThrough === null || seq > this.renderedThrough)) this.renderedThrough = seq;
  }

  get pendingEvents(): number {
    let n = 0;
    for (const m of this.markers) n += m.count;
    return n;
  }

  read(nowMs: number, nowDaemonNs: bigint | null, clock: ClockView): LagReading {
    const base = { clockErrMs: clock.errMs, clockSynced: clock.synced, serverLagMs: this.serverLagMs };
    const oldest = this.markers[0];
    if (!oldest) {
      return {
        ...base,
        lagMs: 0,
        idleMs: this.lastRecvMs === null ? null : Math.max(0, nowMs - this.lastRecvMs),
        basis: 'none',
        oldestPendingAgeMs: null,
        pendingEvents: 0,
      };
    }
    let lag: number;
    let basis: 'mono' | 'recv';
    if (clock.synced && nowDaemonNs !== null && oldest.monoNs !== null && oldest.clockId === clock.clockId) {
      lag = Math.max(0, ageMs(nowDaemonNs, oldest.monoNs));
      basis = 'mono';
    } else {
      lag = Math.max(0, nowMs - oldest.recvMs);
      basis = 'recv';
    }
    return { ...base, lagMs: lag, idleMs: null, basis, oldestPendingAgeMs: lag, pendingEvents: this.pendingEvents };
  }

  /** receive stage of one batch (clock based); null when not comparable */
  static receiveAge(evs: readonly LoomEvent[], nowDaemonNs: bigint | null, clock: ClockView): number | null {
    const last = evs[evs.length - 1];
    if (!last || nowDaemonNs === null || !clock.synced || last.ts?.clock_id !== clock.clockId) return null;
    const mono = parseNs(last.ts.mono_ns);
    return mono === null ? null : ageMs(nowDaemonNs, mono);
  }
}

// ---- tab-wide registry -------------------------------------------------------------------

export interface PanelMetrics {
  id: string;
  /** channel prop of the panel, when it has one (report `channel`) */
  channel: string | null;
  lag: LagTracker;
  hist: Record<Stage, LogHistogram>;
  /** events the panel could not show this period (display budget, mirror overwrite) */
  omitted: number;
}

export interface RenderSamplePoint {
  panel: string;
  seq: string;
  stage: 'receive' | 'draw';
  /** daemon-mono estimate (ms, relative to the first sample of the period) or browser ms */
  t_ms: number;
}

/** Per-tab registry of panel trackers + histograms. One per ShellServices. */
export class RenderMetrics {
  readonly panels = new Map<string, PanelMetrics>();
  /** panels dropped because MAX_PANELS was reached (reported) */
  overflow = 0;
  sampleMode = false;
  samples: RenderSamplePoint[] = [];
  samplesDropped = 0;
  private sampleCounter = 0;
  /** seqs picked for sampling, waiting for the draw that covers them (≤ SAMPLE_MAX) */
  private sampleWait = new Map<string, bigint[]>();

  panel(id: string, channel: string | null = null): PanelMetrics | null {
    let p = this.panels.get(id);
    if (p) return p;
    if (this.panels.size >= MAX_PANELS) {
      this.overflow++;
      return null;
    }
    const hist = {} as Record<Stage, LogHistogram>;
    for (const s of STAGES) hist[s] = new LogHistogram();
    p = { id, channel, lag: new LagTracker(), hist, omitted: 0 };
    this.panels.set(id, p);
    return p;
  }

  remove(id: string): void {
    this.panels.delete(id);
    this.sampleWait.delete(id);
  }

  add(id: string, stage: Stage, ms: number): void {
    this.panels.get(id)?.hist[stage].add(ms);
  }

  /** total events received by all panels (hidden-period counting) */
  get received(): number {
    let n = 0;
    for (const p of this.panels.values()) n += p.lag.received;
    return n;
  }

  // ---- sample mode (ui.render.sample, §3.6) ----
  sampleReceive(id: string, evs: readonly LoomEvent[], tMs: number): void {
    if (!this.sampleMode) return;
    for (const e of evs) {
      if (++this.sampleCounter % SAMPLE_EVERY !== 0) continue;
      if (this.samples.length >= SAMPLE_MAX) {
        this.samplesDropped++;
        continue;
      }
      this.samples.push({ panel: id, seq: e.seq, stage: 'receive', t_ms: tMs });
      const w = this.sampleWait.get(id) ?? [];
      if (w.length < SAMPLE_MAX) w.push(seqOf(e));
      this.sampleWait.set(id, w);
    }
  }
  sampleDraw(id: string, seq: bigint | null, tMs: number): void {
    const w = this.sampleWait.get(id);
    if (!w || w.length === 0) return;
    let k = 0;
    while (k < w.length && (seq === null || (w[k] as bigint) <= seq)) {
      if (this.samples.length < SAMPLE_MAX) this.samples.push({ panel: id, seq: String(w[k]), stage: 'draw', t_ms: tMs });
      else this.samplesDropped++;
      k++;
    }
    w.splice(0, k);
  }

  /** end of a report period: histograms, omissions and samples start over */
  resetPeriod(): void {
    for (const p of this.panels.values()) {
      for (const s of STAGES) p.hist[s].reset();
      p.omitted = 0;
    }
    this.samples = [];
    this.samplesDropped = 0;
  }
}
