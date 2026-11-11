// ViewerReporter — the tab's reports to the daemon (plan §3.5, §3.6, §9.5; contract
// core.metrics.report / core.viewer.presence, generated types schema/v1).
//
// - every REPORT_PERIOD_MS: one `core.metrics.report{kind:"ui.render"}` per panel that
//   drew or has something pending (≤ MAX_REPORTS_PER_PERIOD). Top-level n/p50/p95/p99 =
//   the UI segment (main-thread receipt → end of the covering draw); the five §3.6 stages
//   go in `detail.stages`. Then histograms reset (bounded, O(buckets) per period).
// - sample mode (test harness): `kind:"ui.render.sample"` with ≤ SAMPLE_MAX (seq, stage, t)
//   points per period (1 / SAMPLE_EVERY events).
// - visibilitychange → net.setVisible(v) (worker: clock re-burst, data credit) and then
//   `core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}` with the
//   render fields the worker cannot know. Same params type; the last one wins.
//   (Report p1-m1: ask net to carry the fields on setVisible so there is one call.)
// - back from hidden: `returned = {hiddenMs, events}` → shell shows
//   "숨김 중 N s (M 이벤트) — 라이브로 이동" until dismissed.
//
// Hidden tab: the browser stops rAF (render stops by itself); reports keep their 10 s
// timer only while visible — a hidden tab sends presence(false) and nothing else.

import type { MetricsReport } from '../proto/gen/metrics_report';
import type { ViewerPresence } from '../proto/gen/viewer_presence';
import { Emitter } from './emitter';
import { daemonNowNs, STAGES, type ClockView, type RenderMetrics } from './renderMetrics';

export const REPORT_PERIOD_MS = 10_000;
export const MAX_REPORTS_PER_PERIOD = 16;
const U32_MAX = 4_294_967_295;

export interface ReporterNet {
  call(method: string, params?: unknown): Promise<unknown>;
  setVisible(visible: boolean): void;
  clock(): ClockView;
}

export interface ReporterTimers {
  setInterval(cb: () => void, ms: number): unknown;
  clearInterval(h: unknown): void;
}

export interface ReporterOptions {
  now?: () => number;
  absNowMs?: () => number;
  timers?: ReporterTimers;
  periodMs?: number;
}

const round3 = (x: number) => Math.round(x * 1000) / 1000;

export class ViewerReporter extends Emitter {
  visible = true;
  /** set when the tab comes back after being hidden; cleared by dismissReturn() */
  returned: { hiddenMs: number; events: number } | null = null;
  /** failed report calls (daemon too old / disconnected) — shown nowhere, kept for tests and the report */
  failures = 0;
  private hiddenAt: { ms: number; events: number } | null = null;
  private timer: unknown = null;
  private readonly now: () => number;
  private readonly absNow: () => number;
  private readonly timers: ReporterTimers;
  private readonly periodMs: number;

  constructor(
    private readonly net: ReporterNet,
    private readonly metrics: RenderMetrics,
    opts: ReporterOptions = {},
  ) {
    super();
    this.now = opts.now ?? (() => performance.now());
    this.absNow = opts.absNowMs ?? (() => performance.timeOrigin + performance.now());
    this.timers = opts.timers ?? {
      setInterval: (cb, ms) => setInterval(cb, ms),
      clearInterval: (h) => clearInterval(h as ReturnType<typeof setInterval>),
    };
    this.periodMs = opts.periodMs ?? REPORT_PERIOD_MS;
  }

  start(): void {
    if (this.timer !== null) return;
    this.timer = this.timers.setInterval(() => void this.flush(), this.periodMs);
  }

  stop(): void {
    if (this.timer !== null) this.timers.clearInterval(this.timer);
    this.timer = null;
  }

  // ---- presence ----

  /** tab-level fields: lowest rendered seq among panels with pending data, else the highest rendered */
  presenceParams(visible: boolean): ViewerPresence {
    const clock = this.net.clock();
    const nowNs = clock.synced ? daemonNowNs(clock, this.absNow()) : null;
    let oldest: number | null = null;
    let pendingThrough: bigint | null = null;
    let maxThrough: bigint | null = null;
    for (const p of this.metrics.panels.values()) {
      const r = p.lag.read(this.now(), nowNs, clock);
      const t = p.lag.renderedThrough;
      if (t !== null && (maxThrough === null || t > maxThrough)) maxThrough = t;
      if (r.oldestPendingAgeMs !== null) {
        oldest = Math.max(oldest ?? 0, r.oldestPendingAgeMs);
        if (t !== null && (pendingThrough === null || t < pendingThrough)) pendingThrough = t;
      }
    }
    const through = pendingThrough ?? maxThrough;
    return {
      visible,
      rendered_through_seq: through === null ? null : through.toString(),
      oldest_pending_age_ms: oldest === null ? null : Math.min(U32_MAX, Math.round(oldest)),
    };
  }

  setVisible(visible: boolean): void {
    if (visible === this.visible) return;
    this.visible = visible;
    const t = this.now();
    if (!visible) {
      this.hiddenAt = { ms: t, events: this.metrics.received };
      this.stop();
    } else {
      if (this.hiddenAt) this.returned = { hiddenMs: t - this.hiddenAt.ms, events: Math.max(0, this.metrics.received - this.hiddenAt.events) };
      this.hiddenAt = null;
      this.start();
    }
    this.net.setVisible(visible);
    this.send('core.viewer.presence', this.presenceParams(visible));
    this.changed();
  }

  dismissReturn(): void {
    if (!this.returned) return;
    this.returned = null;
    this.changed();
  }

  // ---- metrics.report ----

  buildReports(): MetricsReport[] {
    const clock = this.net.clock();
    const nowNs = clock.synced ? daemonNowNs(clock, this.absNow()) : null;
    // JSON has no Infinity: an unsynced clock reports -1 and detail.clock_synced = false
    const skew = clock.synced && Number.isFinite(clock.errMs) ? round3(clock.errMs) : -1;
    const out: MetricsReport[] = [];
    let skipped = 0;
    for (const p of this.metrics.panels.values()) {
      const r = p.lag.read(this.now(), nowNs, clock);
      const active = STAGES.some((s) => p.hist[s].n > 0) || r.oldestPendingAgeMs !== null || p.omitted > 0;
      if (!active) continue;
      if (out.length >= MAX_REPORTS_PER_PERIOD) {
        skipped++;
        continue;
      }
      const stages: Record<string, { n: number; p50: number; p95: number; p99: number }> = {};
      for (const s of STAGES) {
        if (s === 'ui') continue;
        const h = p.hist[s];
        stages[s] = { n: h.n, p50: round3(h.quantile(0.5)), p95: round3(h.quantile(0.95)), p99: round3(h.quantile(0.99)) };
      }
      const ui = p.hist.ui;
      out.push({
        kind: 'ui.render',
        channel: p.channel,
        n: ui.n,
        p50: round3(ui.quantile(0.5)),
        p95: round3(ui.quantile(0.95)),
        p99: round3(ui.quantile(0.99)),
        rendered_through_seq: p.lag.renderedThrough === null ? null : p.lag.renderedThrough.toString(),
        skew_err_ms: skew,
        detail: {
          panel: p.id,
          visible: this.visible,
          period_ms: this.periodMs,
          segment: 'main_receipt_to_draw_end',
          stages,
          decode: 'not_exposed_by_net',
          oldest_pending_age_ms: r.oldestPendingAgeMs === null ? null : Math.round(r.oldestPendingAgeMs),
          lag_basis: r.basis,
          clock_err_ms: skew,
          clock_synced: clock.synced,
          omitted: p.omitted,
        },
      });
    }
    if (skipped > 0 || this.metrics.overflow > 0) {
      const last = out[out.length - 1];
      if (last?.detail) last.detail.reports_skipped = skipped + this.metrics.overflow;
    }
    if (this.metrics.sampleMode && (this.metrics.samples.length > 0 || this.metrics.samplesDropped > 0)) {
      out.push(this.sampleReport(skew));
    }
    return out;
  }

  private sampleReport(skew: number): MetricsReport {
    // receive → draw delay per sampled seq
    const recv = new Map<string, number>();
    const d: number[] = [];
    for (const s of this.metrics.samples) {
      const k = `${s.panel}|${s.seq}`;
      if (s.stage === 'receive') recv.set(k, s.t_ms);
      else {
        const r = recv.get(k);
        if (r !== undefined) d.push(s.t_ms - r);
      }
    }
    d.sort((a, b) => a - b);
    const q = (x: number) => (d.length ? round3(d[Math.min(d.length - 1, Math.ceil(x * d.length) - 1)] as number) : 0);
    return {
      kind: 'ui.render.sample',
      n: d.length,
      p50: q(0.5),
      p95: q(0.95),
      p99: q(0.99),
      skew_err_ms: skew,
      detail: {
        every: 50,
        samples: this.metrics.samples.map((s) => ({ ...s, t_ms: round3(s.t_ms) })),
        dropped: this.metrics.samplesDropped,
        t_base: 'performance.now',
      },
    };
  }

  /** send this period's reports and start a new period */
  async flush(): Promise<number> {
    const reports = this.buildReports();
    this.metrics.resetPeriod();
    await Promise.all(reports.map((r) => this.send('core.metrics.report', r)));
    return reports.length;
  }

  private send(method: string, params: MetricsReport | ViewerPresence): Promise<void> {
    return this.net.call(method, params).then(
      () => undefined,
      () => {
        this.failures++;
      },
    );
  }
}
