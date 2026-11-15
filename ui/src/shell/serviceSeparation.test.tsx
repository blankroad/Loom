// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it } from 'vitest';
import '../panels/uart/terminalPanel';
import '../panels/logic/logicPanel';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { TabSettings } from '../store/tabSettings';
import { ChannelBar } from './ChannelBar';
import { routeKey, DockLayout } from './dock/DockLayout';
import type { DockHost } from './dock/dockHost';
import { leaves } from './dock/layoutModel';
import { DIAGNOSTIC_OBSERVATION_DOMAINS, parseRoute, SERVICE_DOMAINS, slotHash, type Route } from './routes';
import { ServiceNav } from './ServiceNav';
import { createShellServices } from './services';
import { ShellCtx } from './ShellContext';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };
let cleanup: (() => void) | undefined;
afterEach(() => cleanup?.());

function mount() {
  const net = new FakeNet(); net.open(TEST_HELLO);
  let hash = '#/l/m1-default';
  const navs: string[] = [];
  const settings = new TabSettings({ getItem: () => null, setItem: () => {} });
  const services = createShellServices(net, { autoTick: false, settings, currentHash: () => hash, navigate: (next) => { navs.push(next); hash = next; } });
  const off = services.bind();
  const host = document.createElement('div'); document.body.append(host);
  const root = createRoot(host);
  const render = (next: string) => {
    hash = next;
    const route: Route = parseRoute(next);
    const domains = route.kind === 'layout' && route.service ? route.service === 'uds' ? DIAGNOSTIC_OBSERVATION_DOMAINS : [route.service] : undefined;
    act(() => root.render(<ShellCtx.Provider value={services}><ServiceNav route={route} /><ChannelBar domains={domains} /><DockLayout route={route} /></ShellCtx.Provider>));
  };
  const open = (domain: string, channel: string, state = 'open') => act(() => net.emitControl(ev({ kind: `${domain}.state`, domain, channel, payload: { state } })));
  cleanup = () => { act(() => root.unmount()); off(); host.remove(); };
  render(hash);
  return { net, services, host, render, open, navs, dock: () => services.dock.current as DockHost };
}

it('parses canonical service routes separately from legacy layouts and panel links', () => {
  for (const domain of SERVICE_DOMAINS) {
    const route = parseRoute(`#/s/${domain}`);
    expect(route.kind).toBe('layout');
    expect(route.kind === 'layout' && route.service).toBe(domain);
    expect(routeKey(route)).toBe(`s:${domain}`);
  }
  expect(routeKey(parseRoute('#/l/uart-only'))).toBe('l:uart-only');
  expect(parseRoute(slotHash({ panel: 'uart.terminal', props: { channel: 'a' } }))).toEqual({ kind: 'panel', slot: { panel: 'uart.terminal', props: { channel: 'a' } } });
});

const NAV_ORDER = ['#/l/m1-default', '#/s/uart', '#/s/can', '#/s/eth', '#/s/uds', '#/s/psu', '#/s/logic', '#/s/video'];

it('always shows the seven module tabs (user rule) with truthful open counts and no navigation RPCs', () => {
  const { host, open, net, render, services } = mount();
  const hrefs = () => [...host.querySelectorAll<HTMLAnchorElement>('.service-nav a')].map((a) => a.getAttribute('href'));
  const count = (svc: string) => host.querySelector(`.service-nav a[href="#/s/${svc}"]`)?.getAttribute('data-count');
  const badge = (svc: string) => host.querySelector(`.service-nav a[href="#/s/${svc}"] .service-count`)?.textContent ?? null;
  // zero channels: every module tab is present (tab ≠ channel), no counts
  expect(hrefs()).toEqual(NAV_ORDER);
  expect(NAV_ORDER.slice(1).map((h) => count(h.slice(4)))).toEqual(['0', '0', '0', '0', '0', '0', '0']);
  expect(services.shell.channels.size).toBe(0);
  open('uart', 'a');
  open('uart', 'b'); open('can', 'bus'); open('eth', 'closed', 'closed');
  expect(hrefs()).toEqual(NAV_ORDER);
  expect([badge('uart'), badge('can'), badge('eth'), badge('uds')]).toEqual(['2', '1', null, null]); // closed ≠ open
  expect(host.querySelector('a[href="#/s/uart"]')?.getAttribute('aria-label')).toBe('UART, 열린 2개');
  render('#/s/uart');
  expect(host.querySelector('a[href="#/s/uart"]')?.getAttribute('aria-current')).toBe('page');
  expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['uart/a', 'uart/b']);
  open('uart', 'a', 'closed');
  expect(badge('uart')).toBe('1');
  // visiting every module tab is navigation only: no RPC, no channel appears
  for (const h of NAV_ORDER) render(h);
  expect(hrefs()).toEqual(NAV_ORDER);
  expect([...services.shell.channels.keys()].sort()).toEqual(['can/bus', 'eth/closed', 'uart/a', 'uart/b']);
  expect(net.calls).toEqual([]);
});

it('keeps the logic analyzer tab permanent and routes /s/logic to its captures, seeding the unbound analyzer otherwise', () => {
  const { host, render, net, dock } = mount();
  const logicLink = () => host.querySelector<HTMLAnchorElement>('.service-nav a[href="#/s/logic"]');
  const cap = (id: string, state: string) => act(() => net.emitControl(ev({ kind: 'logic.capture_state', domain: 'logic', channel: id, payload: { capture_id: id, state } })));
  expect(logicLink()?.getAttribute('data-count')).toBe('0'); // logic unused → tab still there, no count
  cap('lc-old', 'stopped');
  expect(logicLink()?.getAttribute('data-count')).toBe('0'); // only a finished capture → nothing open
  cap('lc-1', 'running');
  expect(logicLink()?.querySelector('.service-count')?.textContent).toBe('1');
  render('#/s/logic');
  expect(logicLink()?.getAttribute('aria-current')).toBe('page');
  expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['logic/lc-1']);
  expect(leaves(dock().snapshot().root)).toEqual([{ panel: 'logic.analyzer', props: { channel: 'lc-1' } }]);
  cap('lc-1', 'stopped');
  expect(logicLink()?.getAttribute('data-count')).toBe('0');
  // the stopped capture keeps its read-only history view; the unbound analyzer comes back for a new capture
  expect(leaves(dock().snapshot().root)).toEqual([{ panel: 'logic.analyzer', props: { channel: 'lc-1' } }, { panel: 'logic.analyzer', props: {} }]);
  render('#/l/m1-default');
  expect(logicLink()).not.toBeNull();
  expect(net.calls).toEqual([]);
});

it('keeps multiple UART aliases in one service tab group through foreign arrivals and returns to integration', () => {
  const { net, host, render, open, dock, services, navs } = mount();
  render('#/s/uart');
  // the UART application (saved-session pane picker), not a generic "no active interface" card
  expect(host.querySelector('[data-testid="dock-empty"] [data-testid="uart-empty-pane"]')).not.toBeNull();
  expect(host.querySelector('[data-testid="dock-empty"]')?.textContent).not.toContain('인터페이스가 없습니다');
  open('uart', 'a'); open('uart', 'b');
  expect(leaves(dock().snapshot().root)).toEqual(['a', 'b'].map((channel) => ({ panel: 'uart.terminal', props: { channel } })));
  expect(dock().api.groups).toHaveLength(1);
  open('can', 'bus');
  expect(dock().api.panels).toHaveLength(2);
  expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['uart/a', 'uart/b']);
  expect(services.showSlot({ panel: 'can.trace', props: { channel: 'bus' } })).toBe('routed');
  expect(navs).toEqual(['#/p/can/trace?channel=bus']);
  render('#/l/m1-default');
  expect(dock().api.panels).toHaveLength(3);
  expect(new Set(leaves(dock().snapshot().root).map((slot) => slot.panel.split('.')[0]))).toEqual(new Set(['uart', 'can']));
  expect(net.calls).toEqual([]);
});

it('keeps a foreign agent focus out of the service dock and keeps the closed view as read-only history', () => {
  const { net, host, render, open, dock, services, navs } = mount();
  open('uart', 'a'); open('can', 'bus');
  render('#/s/uart');
  services.settings.setFollow('auto');
  act(() => net.emitControl(ev({ kind: 'ui.focus_request', origin: 'agent', payload: { panel: 'can.trace', props: { channel: 'bus' }, reason: 'inspect', actor: 'agent' } })));
  expect(navs).toEqual(['#/p/can/trace?channel=bus']);
  expect(dock().api.panels.map((panel) => (panel.params as { panel: string }).panel)).toEqual(['uart.terminal']);
  render('#/s/uart');
  open('uart', 'a', 'closed');
  // the open UART view stays as read-only history (retained-history policy); the tab stays, with no open count
  expect(dock().api.panels.map((panel) => (panel.params as { panel: string }).panel)).toEqual(['uart.terminal']);
  expect(host.querySelector('a[href="#/s/uart"]')?.getAttribute('data-count')).toBe('0');
  expect(host.querySelector('a[href="#/s/uart"] .service-count')).toBeNull();
  render('#/s/can');
  expect(host.querySelector('a[href="#/s/uart"]')).not.toBeNull(); // permanent module tab
  expect(net.calls).toEqual([]);
});

it('groups active UDS under diagnostics and shows all open CAN and Ethernet observations without inferred matching', () => {
  const { net, host, render, open, dock, services } = mount();
  open('uds', 'ecu-a');
  open('can', 'bus-a'); open('can', 'bus-b');
  open('eth', 'nic-a'); open('eth', 'nic-b');
  open('uart', 'serial-a');
  const counts = () => [...host.querySelectorAll<HTMLAnchorElement>('.service-nav a[data-service]')].map((x) => [x.dataset.service, x.dataset.count]);
  // UDS counts real UDS connections only; CAN/Ethernet observations are not diagnostics
  expect(counts()).toEqual([['uart', '1'], ['can', '2'], ['eth', '2'], ['uds', '1'], ['psu', '0'], ['logic', '0'], ['video', '0']]);
  render('#/s/can');
  expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['can/bus-a', 'can/bus-b']);
  render('#/s/uds');
  expect(host.querySelector('a[href="#/s/uds"]')?.getAttribute('aria-current')).toBe('page');
  expect(leaves(dock().snapshot().root)).toEqual([
    { panel: 'uds.console', props: { targets: 'ecu-a' } },
    { panel: 'can.trace', props: { channel: 'bus-a' } },
    { panel: 'can.trace', props: { channel: 'bus-b' } },
    { panel: 'eth.packets', props: { channel: 'nic-a' } },
    { panel: 'eth.packets', props: { channel: 'nic-b' } },
  ]);
  expect(dock().api.groups).toHaveLength(1);
  expect(services.showSlot({ panel: 'can.trace', props: { channel: 'bus-b' } })).toBe('shown');
  open('can', 'bus-c');
  expect(dock().api.panels).toHaveLength(6);
  expect(dock().find({ panel: 'can.trace', props: { channel: 'bus-c' } })).not.toBeNull();
  expect(net.calls).toEqual([]);
});
