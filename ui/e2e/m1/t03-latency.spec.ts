// UI-T03 지연 — LOGIC ONLY here (plan §9.2: numeric pass/fail only on the fixed runner,
// LOOM_PERF=1). What is checked on every run:
//  1. each path produces samples: DOM appearance of a known event (CAN row data-seq, UART trace
//     line with a unique marker, UDS row reaching ●확정) vs. the event's daemon ts.mono_ns
//  2. values are computed inside the clock uncertainty: net clock synced, errMs finite and small,
//     every sample ≥ −errMs (an event cannot be drawn before it happened)
//  3. the tab reports render metrics to the daemon (`core.metrics.report{kind:"ui.render"}` per
//     panel, sample mode `ui.render.sample`) — mock keeps them (conn_stats{reports:true})
// With LOOM_PERF=1 the end-to-end p95 targets (plan §0.2) are asserted too.

import type { Page } from '@playwright/test';
import { backend, Daemon, expect, mockOnly, names, openApp, pageQuery, poll, sleep, test, type G } from './lib';

const PERF = process.env.LOOM_PERF === '1';

interface Clock {
  thetaNs: string;
  errMs: number;
  synced: boolean;
}

async function clockOf(page: Page): Promise<Clock> {
  await page.waitForFunction(() => (globalThis as unknown as G).__loomNet?.clock().synced === true, null, { timeout: 10_000 });
  return page.evaluate(() => {
    const c = (globalThis as unknown as G).__loomNet!.clock();
    return { thetaNs: String(c.thetaNs), errMs: c.errMs, synced: c.synced };
  });
}

/** record page time (timeOrigin + now, ms) when an attribute/text first matches, per key */
async function armObserver(page: Page, mode: 'seq' | 'text' | 'state'): Promise<void> {
  await page.evaluate((mode) => {
    const g = globalThis as unknown as G;
    const seen: Record<string, number> = {};
    g.__m1 = { seen };
    const now = () => g.performance.timeOrigin + g.performance.now();
    const scan = () => {
      const t = now();
      if (mode === 'seq') {
        const rows = g.document.querySelectorAll('.ct-row[data-seq]');
        for (let i = 0; i < rows.length; i++) {
          const s = rows[i]!.getAttribute('data-seq')!;
          if (!(s in seen)) seen[s] = t;
        }
      } else if (mode === 'text') {
        const rows = g.document.querySelectorAll('.vt-row[data-dir=rx]');
        for (let i = 0; i < rows.length; i++) {
          const m = /MARK-(\d+)/.exec(rows[i]!.textContent ?? '');
          if (m && !(m[1]! in seen)) seen[m[1]!] = t;
        }
      } else {
        const rows = g.document.querySelectorAll('.vt-row:not([hidden])[data-request-id][data-state=confirmed]');
        for (let i = 0; i < rows.length; i++) {
          const r = rows[i]!.getAttribute('data-request-id')!;
          if (!(r in seen)) seen[r] = t;
        }
      }
    };
    new g.MutationObserver(scan).observe(g.document.body, { subtree: true, childList: true, attributes: true, characterData: true });
  }, mode);
}

async function seenMap(page: Page): Promise<Record<string, number>> {
  return page.evaluate(() => ((globalThis as unknown as G).__m1?.seen ?? {}) as Record<string, number>);
}

/** latency (ms) of DOM appearance vs daemon mono, with the page→daemon clock mapping */
function latencies(clock: Clock, pairs: { pageMs: number; monoNs: string }[]): number[] {
  const theta = BigInt(clock.thetaNs);
  return pairs.map((p) => {
    const daemonNs = BigInt(Math.round(p.pageMs * 1e6)) + theta; // daemon mono ≈ page ms·1e6 + θ
    return Number(daemonNs - BigInt(p.monoNs)) / 1e6;
  });
}

function q(xs: number[], p: number): number {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil(p * s.length) - 1))] ?? NaN;
}

function judge(label: string, clock: Clock, lat: number[], targetP95: number, minSamples: number): void {
  const p50 = q(lat, 0.5);
  const p95 = q(lat, 0.95);
  test.info().annotations.push({ type: `T03 ${label}`, description: `n=${lat.length} p50=${p50.toFixed(1)} p95=${p95.toFixed(1)} max=${Math.max(...lat).toFixed(1)} ms clock±${clock.errMs.toFixed(2)} ms (target p95 ${targetP95} ms${PERF ? '' : ', not judged: LOOM_PERF unset'})` });
  expect(clock.synced).toBe(true);
  expect(Number.isFinite(clock.errMs)).toBe(true);
  expect(clock.errMs).toBeLessThan(20); // loopback: uncertainty far below the targets, else "동기 불확실"
  expect(lat.length).toBeGreaterThanOrEqual(minSamples);
  for (const x of lat) {
    expect(Number.isFinite(x)).toBe(true);
    expect(x).toBeGreaterThanOrEqual(-clock.errMs - 1); // never drawn before it happened (±1 frame of rounding)
    expect(x).toBeLessThan(10_000);
  }
  if (PERF) expect(p95).toBeLessThanOrEqual(targetP95);
}

test.describe('UI-T03 latency (logic; numbers only with LOOM_PERF=1)', () => {
  test('CAN trace: row appearance vs ts.mono_ns within clock uncertainty @replay', async ({ page }) => {
    const n = names();
    const ref = await Daemon.open();
    try {
      const sub = await ref.subscribe({ domains: ['can'], channels: [n.can] }, false);
      await openApp(page, `/p/can/trace?channel=${n.can}`);
      const clock = await clockOf(page);
      await armObserver(page, 'seq');
      const t0 = sub.events.length;
      await sleep(3_000);
      const seen = await seenMap(page);
      const pairs = sub.events.slice(t0).filter((e) => e.kind === 'can.frame' && seen[e.seq] !== undefined).map((e) => ({ pageMs: seen[e.seq]!, monoNs: e.ts.mono_ns }));
      judge('CAN', clock, latencies(clock, pairs), 150, backend() === 'replay' ? 200 : 40);
    } finally {
      ref.close();
    }
  });

  test('UART trace: device line with a marker vs ts.mono_ns', async ({ page }) => {
    mockOnly('needs mock.emit to place marker lines');
    const ctl = await Daemon.open();
    try {
      await openApp(page, '/p/uart/trace?channel=dut');
      const clock = await clockOf(page);
      await armObserver(page, 'text');
      const mono = new Map<string, string>();
      for (let i = 0; i < 30; i++) {
        const seq = await ctl.call<string>('mock.emit', { domain: 'uart', channel: 'dut', kind: 'uart.data', dir: 'rx', origin: 'device', payload: { bytes: { encoding: 'base64', data: Buffer.from(`MARK-${i}\r\n`).toString('base64') } } });
        void seq;
        await sleep(60);
      }
      const ref = await Daemon.open();
      try {
        const sub = await ref.subscribe({ domains: ['uart'], channels: ['dut'] });
        await poll(() => sub.h, (h) => h !== null);
        for (const e of sub.events) {
          const txt = typeof e.payload.bytes === 'object' ? Buffer.from(String((e.payload.bytes as { data: string }).data), 'base64').toString() : '';
          const m = /MARK-(\d+)/.exec(txt);
          if (m) mono.set(m[1]!, e.ts.mono_ns);
        }
      } finally {
        ref.close();
      }
      await sleep(300);
      const seen = await seenMap(page);
      const pairs = [...mono].filter(([k]) => seen[k] !== undefined).map(([k, m]) => ({ pageMs: seen[k]!, monoNs: m }));
      judge('UART', clock, latencies(clock, pairs), 100, 25);
    } finally {
      ctl.close();
    }
  });

  test('UDS console: final stage on screen vs the confirmed event ts.mono_ns', async ({ page }) => {
    mockOnly();
    const ctl = await Daemon.open();
    try {
      await ctl.inject('reset');
      await openApp(page, '/p/uds/console?targets=bcm');
      const clock = await clockOf(page);
      await armObserver(page, 'state');
      for (let i = 0; i < 20; i++) {
        await ctl.agent('uds.request', { target: 'bcm', hex: '1001' }); // 3E (TesterPresent) folds into the target bar, not a row
        await sleep(80);
      }
      await sleep(300);
      const seen = await seenMap(page);
      const fin = ctl.control.filter((e) => e.domain === 'uds' && e.payload.write_state === 'confirmed' && e.request_id && seen[e.request_id] !== undefined);
      judge('UDS', clock, latencies(clock, fin.map((e) => ({ pageMs: seen[e.request_id!]!, monoNs: e.ts.mono_ns }))), 100, 15);
    } finally {
      ctl.close();
    }
  });

  test('render reports: core.metrics.report{ui.render} per drawing panel (+ ui.render.sample in sample mode)', async ({ page }) => {
    mockOnly('the daemon side of core.metrics.report is only inspectable on mock (loomd logs it)');
    test.setTimeout(45_000);
    await openApp(page, '/l/m1-default', pageQuery({ render_sample: '1' }));
    const ctl = await Daemon.open();
    try {
      await ctl.agent('can.send', { channel: 'ch0', frame: '300#02' });
      const connId = await page.evaluate(() => (globalThis as unknown as G).__loomNet?.state.hello?.conn_id ?? '');
      const reports = await poll(
        async () => {
          const s = await ctl.inject<{ conns: { id: string; reports?: { kind: string; n: number; p95: number; skew_err_ms: number; detail?: Record<string, unknown> }[] }[] }>('conn_stats', { reports: true });
          return s.conns.find((c) => c.id === connId)?.reports ?? [];
        },
        (r) => r.some((x) => x.kind === 'ui.render') && r.some((x) => x.kind === 'ui.render.sample'),
        25_000,
        500,
      );
      const panels = new Set(reports.filter((r) => r.kind === 'ui.render').map((r) => String(r.detail?.panel ?? '')));
      test.info().annotations.push({ type: 'T03 render reports', description: `${reports.length} reports, panels: ${[...panels].join(', ')}` });
      for (const r of reports) {
        expect(Number.isFinite(r.p95)).toBe(true);
        expect(r.n).toBeGreaterThanOrEqual(0);
        expect(Number.isFinite(r.skew_err_ms)).toBe(true);
      }
      expect([...panels].some((p) => p.startsWith('can.'))).toBe(true);
    } finally {
      ctl.close();
    }
  });
});
