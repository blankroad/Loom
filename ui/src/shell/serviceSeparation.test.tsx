// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it } from 'vitest';
import '../panels/uart/terminalPanel';
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

it('lists only OPEN service domains and counts aliases without navigation RPCs', () => {
  const { host, open, net, render } = mount();
  const links = () => [...host.querySelectorAll<HTMLAnchorElement>('.service-nav a')].map((a) => [a.getAttribute('href'), a.textContent]);
  expect(links()).toEqual([['#/l/m1-default', '통합 작업 공간']]);
  open('uart', 'a'); open('uart', 'b'); open('can', 'bus'); open('eth', 'closed', 'closed');
  expect(links()).toEqual([['#/l/m1-default', '통합 작업 공간'], ['#/s/uart', 'UART2'], ['#/s/can', 'CAN1']]);
  render('#/s/uart');
  expect(host.querySelector('a[href="#/s/uart"]')?.getAttribute('aria-current')).toBe('page');
  expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['uart/a', 'uart/b']);
  open('uart', 'a', 'closed');
  expect(host.querySelector('a[href="#/s/uart"]')?.textContent).toBe('UART1');
  expect(net.calls).toEqual([]);
});

it('keeps multiple UART aliases in one service tab group through foreign arrivals and returns to integration', () => {
  const { net, host, render, open, dock, services, navs } = mount();
  render('#/s/uart');
  expect(host.querySelector('[data-testid="dock-empty"]')?.textContent).toContain('활성 UART 인터페이스가 없습니다');
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

it('keeps a foreign agent focus out of the service dock and empties the view after the last close', () => {
  const { net, host, render, open, dock, services, navs } = mount();
  open('uart', 'a'); open('can', 'bus');
  render('#/s/uart');
  services.settings.setFollow('auto');
  act(() => net.emitControl(ev({ kind: 'ui.focus_request', origin: 'agent', payload: { panel: 'can.trace', props: { channel: 'bus' }, reason: 'inspect', actor: 'agent' } })));
  expect(navs).toEqual(['#/p/can/trace?channel=bus']);
  expect(dock().api.panels.map((panel) => (panel.params as { panel: string }).panel)).toEqual(['uart.terminal']);
  render('#/s/uart');
  open('uart', 'a', 'closed');
  expect(dock().api.panels).toHaveLength(0);
  expect(host.querySelector('[data-testid="dock-empty"]')?.textContent).toContain('활성 UART 인터페이스가 없습니다');
  expect(host.querySelector('a[href="#/s/uart"]')).toBeNull();
  expect(net.calls).toEqual([]);
});

it('groups active UDS under diagnostics and shows all open CAN and Ethernet observations without inferred matching', () => {
  const { net, host, render, open, dock, services } = mount();
  open('uds', 'ecu-a');
  open('can', 'bus-a'); open('can', 'bus-b');
  open('eth', 'nic-a'); open('eth', 'nic-b');
  open('uart', 'serial-a');
  render('#/s/can');
  expect(host.querySelector('.service-relations a[href="#/s/uds"]')).not.toBeNull();
  render('#/s/eth');
  expect(host.querySelector('.service-relations a[href="#/s/uds"]')).not.toBeNull();
  render('#/s/uds');
  expect(host.querySelector('.service-nav-label:last-of-type')?.textContent).toBe('진단');
  expect(host.querySelector('a[href="#/s/uds"]')?.textContent).toBe('UDS 진단1');
  expect(host.querySelector('.service-relations a[href="#/s/can"]')).not.toBeNull();
  expect(host.querySelector('.service-relations a[href="#/s/eth"]')).not.toBeNull();
  expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['can/bus-a', 'can/bus-b', 'uds/ecu-a', 'eth/nic-a', 'eth/nic-b']);
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
