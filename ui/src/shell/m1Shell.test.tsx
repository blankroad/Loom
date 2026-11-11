// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { Banners } from './Banners';
import { createShellServices } from './services';
import { ShellCtx } from './ShellContext';
import { LiveBadge } from './TopBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let unbind: (() => void) | null = null;
beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  unbind?.();
  unbind = null;
  host.remove();
});

function mount() {
  let now = 0;
  const net = new FakeNet();
  net.open(TEST_HELLO);
  const intervals: (() => void)[] = [];
  const services = createShellServices(net, {
    autoTick: false,
    now: () => now,
    absNowMs: () => now,
    reportTimers: { setInterval: (cb) => intervals.push(cb), clearInterval: () => {} },
  });
  unbind = services.bind();
  act(() =>
    root.render(
      <ShellCtx.Provider value={services}>
        <LiveBadge />
        <Banners />
      </ShellCtx.Provider>,
    ),
  );
  const at = (ms: number) => {
    now = ms;
    act(() => {
      net.emitControl(ev({ kind: 'core.heartbeat', payload: { latch: false } }));
      services.shell.tick(ms);
    });
  };
  return { net, services, at, intervals };
}
const badge = () => host.querySelector<HTMLElement>('[data-testid="live-badge"]')!;

describe('top bar badge = browser lag', () => {
  it('idle panels → "LIVE idle N s"; pending → "lag N ms"; server lag in the tooltip', () => {
    const { services, at } = mount();
    let reading = { lagMs: 0, basis: 'none' as 'none' | 'mono', idleMs: 5_200 as number | null, serverLagMs: 9 };
    services.shell.setPanelFreshness('p', () => ({ ...reading, targetMs: 150, degraded: false, visible: true, clockSynced: true, clockErrMs: 1 }));
    at(100);
    expect(badge().textContent).toContain('LIVE idle 5.2s');
    expect(badge().title).toContain('서버 배치 lag 9 ms');
    reading = { lagMs: 37, basis: 'mono', idleMs: null, serverLagMs: 9 };
    at(400); // provider is polled on tick — no batch needed for the number to move
    expect(badge().textContent).toContain('LIVE lag 37ms');
    expect(host.querySelector('[data-testid="clock-uncertain"]')).toBeNull();
  });

  it('clock not synced → "동기 불확실"', () => {
    const { services, at } = mount();
    services.shell.setPanelFreshness('p', () => ({ lagMs: 0, targetMs: 45, degraded: false, visible: true, basis: 'none', idleMs: 1_000, clockSynced: false }));
    at(100);
    expect(host.querySelector('[data-testid="clock-uncertain"]')!.textContent).toContain('동기 불확실');
  });
});

describe('visibility → presence + return line', () => {
  it('visibilitychange: net.setVisible, then core.viewer.presence with render fields; back → "숨김 중 … 라이브로 이동"', async () => {
    const { net, services } = mount();
    const hidden = (v: boolean) => {
      Object.defineProperty(document, 'hidden', { configurable: true, get: () => v });
      act(() => document.dispatchEvent(new Event('visibilitychange')));
    };
    hidden(true);
    await act(async () => {});
    expect(net.visible).toEqual([false]);
    const pres = net.calls.find((c) => c.method === 'core.viewer.presence')!;
    expect(pres.params).toEqual({ visible: false, rendered_through_seq: null, oldest_pending_age_ms: null });
    hidden(false);
    expect(net.visible).toEqual([false, true]);
    const line = host.querySelector<HTMLElement>('[data-banner="hidden-return"]')!;
    expect(line.textContent).toMatch(/숨김 중 0\.0 s \(0 이벤트\)/);
    act(() => line.querySelector('button')!.click());
    expect(host.querySelector('[data-banner="hidden-return"]')).toBeNull();
    expect(services.reporter.returned).toBeNull();
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => false });
  });

  it('10 s timer sends core.metrics.report through the net client', async () => {
    const { net, services, intervals } = mount();
    const p = services.metrics.panel('x#1', 'dut')!;
    services.metrics.add(p.id, 'draw', 3);
    await act(async () => intervals.forEach((cb) => cb()));
    expect(net.calls.filter((c) => c.method === 'core.metrics.report').map((c) => (c.params as { kind: string }).kind)).toEqual(['ui.render']);
  });
});
