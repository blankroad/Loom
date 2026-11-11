// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { registerCanPanels } from '../panels/can';
import '../panels/uart/terminalPanel';
import '../panels/uart/tracePanel';
import '../panels/uds/consolePanel';
import type { PanelContext } from './context';
import { LATCH_LINE_TEXT } from './latchLine';
import { registerPanel } from './registry';
import { ev, mountPanel, type Harness } from './testing/harness';

registerCanPanels();

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
});

// test panels: draw cost and extra work are driven by the test clock
let t = 0;
let drawCost = 0;
let seen: PanelContext | null = null;
const probe = (id: string) =>
  registerPanel(
    {
      id,
      domain: 'demo',
      title: id,
      version: 1,
      milestone: 'M1',
      topics: [],
      queries: [],
      actions: [],
      memory: { maxBytes: 1024 },
      render: { maxHz: 60, budgetMs: 8 },
    },
    (ctx) => {
      seen = ctx;
      ctx.scheduler.onFrame(() => {
        t += drawCost;
      });
      return { mount() {}, resize() {}, dispose() {} };
    },
  );
probe('demo.m1probe');

function frames(n: number, gapMs = 50) {
  for (let i = 0; i < n; i++) {
    seen!.scheduler.invalidate();
    t += gapMs;
    h!.frame();
  }
}

describe('ctx.scheduler.degraded (CAN panel request)', () => {
  it('read-only flag + subscription: draw over budget 3 frames → degraded, cheap frames → recovered', () => {
    t = 0;
    drawCost = 20;
    h = mountPanel('demo.m1probe', {}, { now: () => t });
    const ctx = seen!;
    const changes: boolean[] = [];
    ctx.scheduler.onDegradedChange((d) => changes.push(d));
    expect(ctx.scheduler.degraded).toBe(false);
    frames(3);
    expect(ctx.scheduler.degraded).toBe(true);
    drawCost = 1;
    frames(40);
    expect(ctx.scheduler.degraded).toBe(false);
    expect(changes).toEqual([true, false]);
  });

  it('reported ingest work counts: draw + work > 300 ms/s → degraded', () => {
    t = 0;
    drawCost = 0;
    h = mountPanel('demo.m1probe', {}, { now: () => t });
    const ctx = seen!;
    ctx.scheduler.reportWork(350);
    frames(1, 1_000);
    expect(ctx.scheduler.degraded).toBe(true);
  });
});

describe('ctx.latch + STOP line on CAN · UART · UDS panels', () => {
  it('latch is read-only state with a change subscription', () => {
    h = mountPanel('demo.m1probe');
    const ctx = seen!;
    let n = 0;
    ctx.latch.subscribe(() => n++);
    expect(ctx.latch.on).toBe(false);
    h.shell.applyControl(ev({ kind: 'core.latch', payload: { on: true, by: 'viewer:web', origin: 'user', missing: [] } }), 0);
    expect(ctx.latch).toMatchObject({ on: true, by: 'viewer:web' });
    h.shell.applyControl(ev({ kind: 'core.heartbeat', payload: { latch: true } }), 1); // no change → no callback
    expect(n).toBe(1);
  });

  for (const [id, props] of [
    ['uart.terminal', { channel: 'dut' }],
    ['uart.trace', { channel: 'dut' }],
    ['uds.console', { channel: 'bcm' }],
    ['can.trace', { channel: 'ch0' }],
  ] as const) {
    it(`${id}: "${LATCH_LINE_TEXT}" only while latched`, () => {
      if (!window.matchMedia) {
        window.matchMedia = ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as unknown as typeof window.matchMedia;
      }
      h = mountPanel(id, props);
      const line = h.host.querySelector<HTMLElement>('[data-testid="panel-latch"]')!;
      expect(line.textContent).toBe(LATCH_LINE_TEXT);
      expect(line.hidden).toBe(true);
      h.shell.applyControl(ev({ kind: 'core.latch', payload: { on: true, missing: [] } }), 0);
      expect(line.hidden).toBe(false);
      h.shell.applyControl(ev({ kind: 'core.latch', payload: { on: false } }), 1);
      expect(line.hidden).toBe(true);
    });
  }
});

describe('panel lag reaches the shell badge', () => {
  it('pending batch → browser lag on the badge; drawn → lag 0 + idle', () => {
    t = 0;
    h = mountPanel('uart.trace', { channel: 'dut' }, { now: () => t });
    // FakeNet clock: synced, theta 0, clock_id 'test' — events carry clock_id 't' → receipt-based lag
    h.net.emitBatch(0, [ev({ kind: 'uart.line', channel: 'dut', payload: { text: 'a' } })]);
    t += 120;
    h.shell.tick(t);
    expect(h.shell.status).toMatchObject({ lagMs: 120, lagFromReceipt: true, idleMs: null });
    h.frame();
    t += 2_000;
    h.shell.tick(t);
    expect(h.shell.status.lagMs).toBe(0);
    expect(h.shell.status.idleMs).toBe(2_120); // since the last receipt
  });
});
