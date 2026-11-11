import Ajv2020 from 'ajv/dist/2020';
import { describe, expect, it } from 'vitest';
import metricsReportSchema from '../../../schema/v1/metrics_report.schema.json?raw';
import viewerPresenceSchema from '../../../schema/v1/viewer_presence.schema.json?raw';
import type { MetricsReport } from '../proto/gen/metrics_report';
import type { LoomEvent } from '../proto/provisional';
import { daemonNowNs, LagTracker, LogHistogram, MAX_PANELS, PENDING_MARKERS, RenderMetrics, type ClockView } from './renderMetrics';
import { computeLiveStatus } from './shellControl';
import { MAX_REPORTS_PER_PERIOD, REPORT_PERIOD_MS, ViewerReporter, type ReporterNet } from './viewerReport';

const SYNCED: ClockView = { thetaNs: 0n, errMs: 1, clockId: 'c', synced: true };
const UNSYNCED: ClockView = { thetaNs: 0n, errMs: Number.POSITIVE_INFINITY, clockId: '', synced: false };

function evAt(seq: number, monoMs: number, clockId = 'c'): LoomEvent {
  return {
    schema_version: 1,
    session_id: 's',
    domain: 'uart',
    channel: 'dut',
    kind: 'uart.rx',
    epoch: '1',
    seq: String(seq),
    producer_id: 'p',
    producer_epoch: '1',
    source_seq: String(seq),
    ts: { mono_ns: String(BigInt(monoMs) * 1_000_000n), wall_ns: '0', clock_id: clockId, source: 'host_read_complete' },
    origin: 'device',
    payload: {},
  } as LoomEvent;
}
const ns = (ms: number) => BigInt(ms) * 1_000_000n;

describe('LagTracker (browser lag, plan §3.5-5)', () => {
  it('lag = daemon now − ts.mono_ns of the oldest undrawn event; 0 + idle once drawn', () => {
    const t = new LagTracker();
    t.receive([evAt(10, 1_000), evAt(11, 1_005)], 500, 7);
    expect(t.read(520, ns(1_250), SYNCED)).toMatchObject({ lagMs: 250, basis: 'mono', idleMs: null, pendingEvents: 2, serverLagMs: 7 });
    t.drawn(11n, 530, 534);
    const r = t.read(2_534, ns(3_000), SYNCED);
    expect(r).toMatchObject({ lagMs: 0, basis: 'none', oldestPendingAgeMs: null });
    expect(r.idleMs).toBe(2_034); // since the last receipt — quiet device ≠ latency
    expect(t.renderedThrough).toBe(11n);
  });

  it('never received → lag 0, idle null', () => {
    expect(new LagTracker().read(10, ns(10), SYNCED)).toMatchObject({ lagMs: 0, idleMs: null, basis: 'none' });
  });

  it('unsynced clock or another clock_id → receipt-based age, flagged', () => {
    const t = new LagTracker();
    t.receive([evAt(1, 1_000)], 100, 0);
    expect(t.read(180, null, UNSYNCED)).toMatchObject({ lagMs: 80, basis: 'recv', clockSynced: false });
    const d = new LagTracker();
    d.receive([evAt(1, 1_000, 'can-hw')], 100, 0); // device HW clock (§3.5-8)
    expect(d.read(150, ns(9_999), SYNCED)).toMatchObject({ lagMs: 50, basis: 'recv' });
  });

  it('a draw without a seq retires only batches received before the draw started', () => {
    const t = new LagTracker();
    t.receive([evAt(1, 0)], 100, 0);
    t.receive([evAt(2, 0)], 200, 0);
    const ui: number[] = [];
    t.drawn(null, 150, 160, (ms) => ui.push(ms));
    expect(ui).toEqual([60]);
    expect(t.pendingEvents).toBe(1);
  });

  it(`C2: at most ${PENDING_MARKERS} markers; overflow merges and keeps the oldest age`, () => {
    const t = new LagTracker();
    for (let i = 0; i < 300; i++) t.receive([evAt(i + 1, i)], i, 0);
    expect(t.pendingEvents).toBe(300);
    expect(t.read(1_000, ns(1_000), SYNCED).lagMs).toBe(1_000); // oldest = event at mono 0
    t.drawn(300n, 1_000, 1_001);
    expect(t.pendingEvents).toBe(0);
  });

  it('daemonNowNs keeps sub-ms precision on large absolute times (BigInt first)', () => {
    expect(daemonNowNs({ ...SYNCED, thetaNs: -5n }, 1_762_770_000_123.25)).toBe(1_762_770_000_123_250_000n - 5n);
  });
});

describe('LogHistogram', () => {
  it('bounded quantiles within ~9 %', () => {
    const h = new LogHistogram();
    for (let i = 1; i <= 100; i++) h.add(i);
    expect(h.quantile(0.5)).toBeGreaterThanOrEqual(50);
    expect(h.quantile(0.5)).toBeLessThanOrEqual(55);
    expect(h.quantile(0.99)).toBeLessThanOrEqual(100);
    h.add(Number.NaN);
    h.add(-1);
    expect(h.n).toBe(100);
  });
});

describe('shell live status from browser lag', () => {
  const base = { connOpen: true, lastHeartbeatMs: 0, nowMs: 100 };
  it('all panels idle → idle shown; anything pending → no idle', () => {
    const idle = { lagMs: 0, targetMs: 150, degraded: false, visible: true, basis: 'none' as const, idleMs: 5_200, clockSynced: true, clockErrMs: 1 };
    expect(computeLiveStatus({ ...base, panels: [idle] })).toMatchObject({ badge: 'LIVE', idleMs: 5_200, clockUncertain: false });
    const busy = { ...idle, basis: 'mono' as const, idleMs: null, lagMs: 40, serverLagMs: 12 };
    expect(computeLiveStatus({ ...base, panels: [idle, busy] })).toMatchObject({ idleMs: null, lagMs: 40, serverLagMs: 12 });
  });
  it('clock unsynced, or ± error above the smallest target → 동기 불확실', () => {
    const p = { lagMs: 0, targetMs: 45, degraded: false, visible: true, basis: 'none' as const, idleMs: 1 };
    expect(computeLiveStatus({ ...base, panels: [{ ...p, clockSynced: false }] }).clockUncertain).toBe(true);
    expect(computeLiveStatus({ ...base, panels: [{ ...p, clockSynced: true, clockErrMs: 60 }] }).clockUncertain).toBe(true);
    expect(computeLiveStatus({ ...base, panels: [{ ...p, clockSynced: true, clockErrMs: 3 }] }).clockUncertain).toBe(false);
  });
  it('receipt-based lag is flagged for the tooltip', () => {
    const p = { lagMs: 20, targetMs: 150, degraded: false, visible: true, basis: 'recv' as const, clockSynced: false };
    expect(computeLiveStatus({ ...base, panels: [p] }).lagFromReceipt).toBe(true);
  });
});

// ---- core.metrics.report / core.viewer.presence ----

// the CORE JSON Schemas themselves (schema/v1), not only the generated TS types
const ajv = new Ajv2020({ strict: false });
const validReport = ajv.compile(JSON.parse(metricsReportSchema) as object);
const validPresence = ajv.compile(JSON.parse(viewerPresenceSchema) as object);

function fakeNet(clock: ClockView = SYNCED) {
  const log: string[] = [];
  const calls: { method: string; params: unknown }[] = [];
  const net: ReporterNet & { clockNow: ClockView } = {
    clockNow: clock,
    call: async (method, params) => {
      log.push(`call:${method}`);
      calls.push({ method, params });
      return {};
    },
    setVisible: (v) => void log.push(`setVisible:${v}`),
    clock: () => net.clockNow,
  };
  return { net, log, calls };
}
function fakeTimers() {
  const live = new Map<number, () => void>();
  let id = 0;
  return {
    timers: {
      setInterval: (cb: () => void) => {
        live.set(++id, cb);
        return id;
      },
      clearInterval: (h: unknown) => void live.delete(h as number),
    },
    fire: () => [...live.values()].forEach((cb) => cb()),
    get count() {
      return live.size;
    },
  };
}

describe('ViewerReporter — core.metrics.report every 10 s', () => {
  function setup(clock: ClockView = SYNCED) {
    let now = 0;
    const metrics = new RenderMetrics();
    const { net, calls, log } = fakeNet(clock);
    const t = fakeTimers();
    const rep = new ViewerReporter(net, metrics, { now: () => now, absNowMs: () => now, timers: t.timers });
    return { metrics, net, calls, log, t, rep, setNow: (x: number) => (now = x) };
  }

  it('one ui.render report per active panel, schema-valid, then histograms reset', async () => {
    const { metrics, calls, t, rep, setNow } = setup();
    rep.start();
    const p = metrics.panel('uart.terminal#1', 'dut')!;
    p.lag.receive([evAt(5, 990)], 1_000, 3);
    metrics.add(p.id, 'receive', 10);
    metrics.add(p.id, 'apply', 0.4);
    metrics.add(p.id, 'draw', 2);
    p.lag.drawn(5n, 1_010, 1_012, (ms) => metrics.add(p.id, 'ui', ms));
    metrics.panel('idle.panel#2'); // nothing happened → no report
    setNow(1_100);
    t.fire();
    await Promise.resolve();
    const reports = calls.filter((c) => c.method === 'core.metrics.report').map((c) => c.params as MetricsReport);
    expect(reports).toHaveLength(1);
    const r = reports[0]!;
    expect(validReport(r)).toBe(true);
    expect(r).toMatchObject({ kind: 'ui.render', channel: 'dut', n: 1, rendered_through_seq: '5', skew_err_ms: 1 });
    expect(r.p95).toBeGreaterThanOrEqual(12);
    expect(r.detail).toMatchObject({ panel: 'uart.terminal#1', period_ms: REPORT_PERIOD_MS, lag_basis: 'none', clock_synced: true, decode: 'not_exposed_by_net' });
    const stages = r.detail!.stages as Record<string, { n: number }>;
    expect(Object.keys(stages)).toEqual(['receive', 'decode', 'apply', 'draw', 'next_raf']);
    expect(stages.decode!.n).toBe(0);
    t.fire();
    await Promise.resolve();
    expect(calls.filter((c) => c.method === 'core.metrics.report')).toHaveLength(1); // reset: nothing new
  });

  it('pending data is reported (oldest_pending_age_ms); unsynced clock → skew -1, still schema-valid', async () => {
    const { metrics, rep, setNow } = setup(UNSYNCED);
    const p = metrics.panel('can.trace#1', 'ch0')!;
    p.lag.receive([evAt(9, 0)], 100, 0);
    setNow(400);
    const [r] = rep.buildReports();
    expect(validReport(r)).toBe(true);
    expect(r).toMatchObject({ skew_err_ms: -1, n: 0 });
    expect(r!.detail).toMatchObject({ oldest_pending_age_ms: 300, lag_basis: 'recv', clock_synced: false });
  });

  it(`C2: ≤ ${MAX_REPORTS_PER_PERIOD} reports per period, the rest counted`, () => {
    const { metrics, rep } = setup();
    for (let i = 0; i < 20; i++) metrics.add(metrics.panel(`p${i}`)!.id, 'draw', 1);
    const out = rep.buildReports();
    expect(out).toHaveLength(MAX_REPORTS_PER_PERIOD);
    expect(out[out.length - 1]!.detail!.reports_skipped).toBe(4);
    for (let i = 20; i < MAX_PANELS + 5; i++) metrics.panel(`q${i}`);
    expect(metrics.overflow).toBe(5); // MAX_PANELS + 5 asked, MAX_PANELS kept
  });

  it('sample mode: 1/50 events (seq, stage, t), ui.render.sample report, bounded', () => {
    const { metrics, rep } = setup();
    metrics.sampleMode = true;
    const p = metrics.panel('uart.terminal#1', 'dut')!;
    const evs = Array.from({ length: 100 }, (_, i) => evAt(i + 1, 0));
    metrics.sampleReceive(p.id, evs, 10);
    metrics.sampleDraw(p.id, 100n, 25);
    const s = rep.buildReports().find((r) => r.kind === 'ui.render.sample')!;
    expect(validReport(s)).toBe(true);
    expect(s).toMatchObject({ n: 2, p50: 15, p99: 15 });
    expect((s.detail!.samples as unknown[]).map((x) => (x as { seq: string; stage: string }).seq + (x as { stage: string }).stage)).toEqual([
      '50receive',
      '100receive',
      '50draw',
      '100draw',
    ]);
  });
});

describe('ViewerReporter — presence', () => {
  it('visibilitychange: net.setVisible first, then presence with render fields; return line counts hidden events', async () => {
    let now = 0;
    const metrics = new RenderMetrics();
    const { net, calls, log } = fakeNet();
    const t = fakeTimers();
    const rep = new ViewerReporter(net, metrics, { now: () => now, absNowMs: () => now, timers: t.timers });
    rep.start();
    const p = metrics.panel('uart.terminal#1', 'dut')!;
    p.lag.receive([evAt(41, 0)], 0, 0);
    p.lag.drawn(41n, 1, 2);
    p.lag.receive([evAt(42, 10)], 10, 0); // pending
    now = 1_000;
    rep.setVisible(false);
    await Promise.resolve();
    expect(log.slice(0, 2)).toEqual(['setVisible:false', 'call:core.viewer.presence']);
    const pres = calls.find((c) => c.method === 'core.viewer.presence')!.params;
    expect(validPresence(pres)).toBe(true);
    expect(pres).toEqual({ visible: false, rendered_through_seq: '41', oldest_pending_age_ms: 990 });
    expect(t.count).toBe(0); // hidden: no periodic reports
    p.lag.receive(Array.from({ length: 30 }, (_, i) => evAt(43 + i, 20)), 1_500, 0);
    now = 13_300;
    rep.setVisible(true);
    expect(rep.returned).toEqual({ hiddenMs: 12_300, events: 30 });
    expect(t.count).toBe(1);
    rep.dismissReturn();
    expect(rep.returned).toBeNull();
  });
});
