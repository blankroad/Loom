// @vitest-environment jsdom
// Permanent module tabs (user rule): each service tab is a usable module application before any connection.
// Tab ≠ channel: navigation seeds the UNBOUND module panel (no channel/targets prop), opens/sends nothing and never
// invents a channel; Integrated stays the user's observation workspace of actual channels only.
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeAll, expect, it } from 'vitest';
import type { PanelContext } from '../sdk/context';
import { CHANNEL_PROP } from '../sdk/propsSchema';
import { registerPanel } from '../sdk/registry';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { TabSettings } from '../store/tabSettings';
import { DockLayout } from './dock/DockLayout';
import type { DockHost } from './dock/dockHost';
import { leaves } from './dock/layoutModel';
import { isUnboundModuleSlot, parseRoute, servicePresetForChannels, SERVICE_TAB_ORDER, type Slot } from './routes';
import { createShellServices } from './services';
import { ShellCtx } from './ShellContext';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };

// probe panels standing in for the owners' module panels: record ctx, mounts, visibility; never call the net
const ctxs = new Map<string, PanelContext>();
const log: string[] = [];
const PANELS = ['uart.terminal', 'can.trace', 'eth.packets', 'uds.console', 'psu.gauge', 'logic.analyzer', 'video.workbench'];
beforeAll(() => {
  for (const id of PANELS) {
    registerPanel({
      id, domain: id.split('.')[0]!, title: id, version: 1, milestone: 'M1', topics: [], queries: [], actions: [],
      memory: { maxBytes: 1 }, render: { maxHz: 60, budgetMs: 8 },
      propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP, ch: CHANNEL_PROP, targets: { type: 'string' }, view: { type: 'string' } } },
    }, (ctx) => {
      const key = `${id}:${String(ctx.props.channel ?? ctx.props.targets ?? '')}`;
      ctxs.set(key, ctx);
      return {
        mount: (el) => { log.push(`mount ${key}`); const input = document.createElement('input'); input.dataset.draft = key; el.append(input); },
        resize: () => {},
        dispose: () => { log.push(`dispose ${key}`); if (ctxs.get(key) === ctx) ctxs.delete(key); },
      };
    });
  }
});

let cleanup: (() => void) | undefined;
afterEach(() => { cleanup?.(); log.length = 0; ctxs.clear(); location.hash = ''; });

function mount() {
  const net = new FakeNet(); net.open(TEST_HELLO);
  const settings = new TabSettings({ getItem: () => null, setItem: () => {} });
  const services = createShellServices(net, { autoTick: false, settings, currentHash: () => location.hash });
  const off = services.bind();
  const host = document.createElement('div'); document.body.append(host);
  const root = createRoot(host);
  const render = (next: string) => {
    location.hash = next;
    act(() => root.render(<ShellCtx.Provider value={services}><DockLayout route={parseRoute(next)} /></ShellCtx.Provider>));
  };
  /** follow a hash navigation a panel/shell made (as the shell's hashchange listener would) */
  const follow = () => render(location.hash);
  const state = (domain: string, channel: string, s = 'open') => act(() => net.emitControl(ev({ kind: `${domain}.state`, domain, channel, payload: { state: s } })));
  cleanup = () => { act(() => root.unmount()); off(); host.remove(); };
  render('#/l/m1-default');
  const dock = () => services.dock.current as DockHost;
  const slots = () => leaves(dock().snapshot().root) as Slot[];
  return { net, services, host, render, follow, state, dock, slots };
}

const DEVICE_RPC = /\.(open|close|connect|disconnect|send|input|write|set|out|start|stop)$|capture\.start|capture\.stop/;

it('every module tab is its module application with zero channels: unbound panels, no RPC, no phantom channel; Integrated not stuffed', () => {
  const { net, services, render, dock, slots, host } = mount();
  expect(dock().api.panels).toHaveLength(0); // Integrated: actual channels only → nothing
  const expected: Record<string, Slot[]> = {
    uart: [], can: [{ panel: 'can.trace', props: {} }], eth: [{ panel: 'eth.packets', props: {} }], uds: [{ panel: 'uds.console', props: {} }],
    psu: [{ panel: 'psu.gauge', props: {} }], logic: [{ panel: 'logic.analyzer', props: {} }], video: [{ panel: 'video.workbench', props: {} }],
  };
  for (const svc of SERVICE_TAB_ORDER) {
    render(`#/s/${svc}`);
    expect(slots()).toEqual(expected[svc]);
    expect(slots().every(isUnboundModuleSlot)).toBe(true);
  }
  // UART's application is its chrome + the saved-session pane picker, not a generic "no interface" card
  render('#/s/uart');
  expect(host.querySelector('[data-testid="uart-empty-pane"]')).not.toBeNull();
  // back on Integrated: still nothing auto-seeded there, and every module app stayed in its own (hidden) dock
  render('#/l/m1-default');
  expect(dock().api.panels).toHaveLength(0);
  expect(host.querySelectorAll('.dock-host[data-workspace]').length).toBe(8);
  expect(services.shell.channels.size).toBe(0);
  expect(net.calls).toEqual([]);
  expect(net.calls.filter((c) => DEVICE_RPC.test(c.method))).toEqual([]);
});

it('servicePresetForChannels seeds the unbound app only while the service itself has no OPEN channel (UDS ignores CAN/ETH observations)', () => {
  const open = (domain: string, channel: string) => ({ domain, channel, state: 'open' as const });
  expect(servicePresetForChannels('eth', [{ domain: 'eth', channel: 'old', state: 'closed' }]).slots).toEqual([{ panel: 'eth.packets', props: {} }]);
  expect(servicePresetForChannels('eth', [open('eth', 'nic')]).slots).toEqual([{ panel: 'eth.packets', props: { channel: 'nic' } }]);
  // UDS: raw observations are listed but do not imply an ECU binding → the unbound console stays first
  expect(servicePresetForChannels('uds', [open('can', 'pt'), open('eth', 'nic')]).slots).toEqual([
    { panel: 'uds.console', props: {} }, { panel: 'can.trace', props: { channel: 'pt' } }, { panel: 'eth.packets', props: { channel: 'nic' } },
  ]);
  expect(servicePresetForChannels('uds', [open('uds', 'gw')]).slots).toEqual([{ panel: 'uds.console', props: { targets: 'gw' } }]);
  expect(servicePresetForChannels('uart', []).slots).toEqual([]);
});

for (const [svc, panel, bound] of [
  ['eth', 'eth.packets', { channel: 'nic-a' }],
  ['can', 'can.trace', { channel: 'pt' }],
  ['psu', 'psu.gauge', { channel: 'bench2' }],
] as const) {
  it(`${svc}: the seeded application hands off to the connected alias in the same pane (openPanel after Connect), no duplicate, other workspaces untouched`, () => {
    const { net, render, follow, state, dock, slots, services } = mount();
    // an unrelated integrated arrangement to keep
    state('uart', 'mcu');
    const integrated = dock();
    const integratedPanels = integrated.api.panels.map((p) => p.id);
    render(`#/s/${svc}`);
    const ws = dock();
    const seed = ws.find({ panel, props: {} })!;
    expect(seed).not.toBeNull();
    const group = seed.group;
    // the panel's own Connect completes: it asks for its bound view (the panel owners' real path), then the daemon state arrives
    act(() => ctxs.get(`${panel}:`)!.ui.openPanel(panel, { ...bound }));
    follow(); // hash → in-module live focus
    expect(dock()).toBe(ws);
    state(svc, bound.channel);
    render(`#/s/${svc}`);
    const boundSlot = svc === 'can' ? { panel, props: { ...bound } } : { panel, props: { ...bound } };
    expect(ws.find(boundSlot)).not.toBeNull();
    expect(ws.find({ panel, props: {} })).toBeNull(); // the placeholder app is gone, not duplicated
    expect(slots().filter((s) => s.panel === panel)).toHaveLength(1);
    expect(ws.find(boundSlot)!.group).toBe(group);
    expect(ws.api.activePanel?.params).toMatchObject({ panel, props: bound });
    // the integrated workspace kept its instances (it simply gained the new real channel in the background)
    expect(integratedPanels.every((id) => integrated.api.getPanel(id))).toBe(true);
    expect(integrated.isShown).toBe(false);
    // closing the alias: its view remains per retained-history policy (eth/psu) and the app is offered again
    state(svc, bound.channel, 'closed');
    expect(ws.find({ panel, props: {} })).not.toBeNull();
    expect(net.calls.filter((c) => DEVICE_RPC.test(c.method))).toEqual([]);
    expect(services.shell.channels.has(`${svc}/`)).toBe(false);
  });
}

it('a Connect completing after the user switched tabs lands in its own hidden module dock: no navigation, front dock untouched', () => {
  const { render, state, dock, services } = mount();
  render('#/s/eth');
  const eth = dock();
  const ethCtx = ctxs.get('eth.packets:')!;
  const psuSeed = (() => { render('#/s/psu'); return ctxs.get('psu.gauge:')!; })();
  render('#/l/m1-default');
  const integrated = dock();
  const before = integrated.api.panels.length;
  // ETH and PSU completions arrive late, while Integrated is in front
  act(() => ethCtx.ui.openPanel('eth.packets', { channel: 'nic-late' }));
  act(() => psuSeed.ui.openPanel('psu.gauge', { channel: 'bench-late' }));
  // a foreign-domain request from a hidden module dock is refused, not routed elsewhere
  act(() => ethCtx.ui.openPanel('uart.terminal', { channel: 'x' }));
  expect(location.hash).toBe('#/l/m1-default');
  expect(services.dock.current).toBe(integrated);
  expect(integrated.api.panels).toHaveLength(before);
  expect(eth.find({ panel: 'eth.packets', props: { channel: 'nic-late' } })).not.toBeNull();
  expect(eth.find({ panel: 'uart.terminal', props: { channel: 'x' } })).toBeNull();
  expect(services.toasts.items.some((t) => t.text.includes('uart.terminal'))).toBe(true);
  // when the channel really opens, the ETH tab shows it with the seed handed off
  state('eth', 'nic-late');
  render('#/s/eth');
  expect(dock()).toBe(eth);
  expect(eth.find({ panel: 'eth.packets', props: {} })).toBeNull();
  expect(eth.find({ panel: 'eth.packets', props: { channel: 'nic-late' } })).not.toBeNull();
});

it('switching module tabs keeps each module instance and its unsent draft; hidden docks stop drawing', () => {
  const { render, dock } = mount();
  render('#/s/can');
  const can = dock();
  const draft = document.querySelector<HTMLInputElement>('[data-draft="can.trace:"]')!;
  draft.value = '0x123#11 22';
  const mounts = log.filter((l) => l === 'mount can.trace:').length;
  render('#/s/eth');
  render('#/s/uds');
  render('#/l/m1-default');
  render('#/s/can');
  expect(dock()).toBe(can);
  expect(log.filter((l) => l === 'mount can.trace:').length).toBe(mounts);
  expect(log).not.toContain('dispose can.trace:');
  expect(document.querySelector<HTMLInputElement>('[data-draft="can.trace:"]')).toBe(draft);
  expect(draft.value).toBe('0x123#11 22');
  render('#/s/eth');
  expect(can.isShown).toBe(false);
  expect(can.api.panels[0]!.view.content.element.closest('.dock-host')?.hasAttribute('hidden')).toBe(true);
  // only the front dock is the e2e/QA "layout"
  expect(document.querySelectorAll('[data-testid="layout"]')).toHaveLength(1);
});

it('a closed module app is not re-seeded by heartbeats; the empty module state offers an explicit reopen', () => {
  const { render, dock, services, host, state } = mount();
  render('#/s/video');
  const ws = dock();
  act(() => ws.find({ panel: 'video.workbench', props: {} })!.api.close());
  expect(ws.api.panels).toHaveLength(0);
  act(() => services.shell.tick(5000));
  state('uart', 'other'); // unrelated arrival
  expect(ws.api.panels).toHaveLength(0);
  const empty = host.querySelector('[data-testid="dock-empty"]');
  expect(empty?.textContent).toContain('작업대가 닫혀 있습니다');
  expect(empty?.textContent).not.toContain('인터페이스가 없습니다');
  act(() => host.querySelector<HTMLButtonElement>('[data-act="reopen-module"]')!.click());
  expect(leaves(ws.snapshot().root)).toEqual([{ panel: 'video.workbench', props: {} }]);
});
