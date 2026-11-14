// @vitest-environment jsdom
// Real dock + ShellApp: a retained (stopped) ETH capture view must stay usable when the last live interface closes.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import '../panels/uart/terminalPanel';
import '../panels/eth/ethPanel';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { ShellApp } from './ShellApp';
import { createShellServices } from './services';
import type { DockHost } from './dock/dockHost';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  localStorage.clear();
  location.hash = '#/l/m1-default';
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('empty welcome vs retained ETH history', () => {
  it('last live UART close keeps a stopped (reopened) ETH capture view visible; it falls back to welcome only when nothing is docked', () => {
    const net = new FakeNet();
    net.open(TEST_HELLO);
    const services = createShellServices(net, { autoTick: false });
    act(() => root.render(<ShellApp services={services} />));
    const welcome = () => host.querySelector('[data-testid="workbench-welcome"]');
    const inert = () => host.querySelector('.workspace-dock')!.hasAttribute('inert');
    const dock = () => services.dock.current as DockHost;
    const eth = { panel: 'eth.packets', props: { channel: 'qa_eth_fixed' } };
    const uart = { panel: 'uart.terminal', props: { channel: 'qa_uart_fixed' } };
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', domain: 'uart', channel: 'qa_uart_fixed', payload: { state: 'opened' } })));
    act(() => net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'qa_eth_fixed', generation: 1, payload: { ch: 'qa_eth_fixed', state: 'live' } })));
    expect(dock().find(eth)).not.toBeNull();
    act(() => net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'qa_eth_fixed', generation: 1, payload: { ch: 'qa_eth_fixed', state: 'stopped' } })));
    // explicit close + reopen of the stopped capture (inactive explorer path)
    act(() => dock().find(eth)!.api.close());
    act(() => { services.showSlot(eth); });
    expect(dock().find(eth)).not.toBeNull();
    // the last live interface closes (UART owner normal close): no welcome over the retained ETH history
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', domain: 'uart', channel: 'qa_uart_fixed', payload: { state: 'closed' } })));
    act(() => net.emitControl(ev({ kind: 'core.heartbeat', payload: {} })));
    act(() => services.shell.tick(10_000));
    expect(dock().find(uart)).toBeNull();
    expect(dock().find(eth)).not.toBeNull();
    expect(welcome()).toBeNull();
    expect(inert()).toBe(false);
    expect(host.querySelector('[data-chan="eth/qa_eth_fixed"]')?.getAttribute('data-state') ?? 'hidden-in-active-list').not.toBe('open');
    // user closes the history view: nothing docked, no live interface → the welcome returns
    act(() => dock().find(eth)!.api.close());
    act(() => net.emitControl(ev({ kind: 'core.heartbeat', payload: {} })));
    expect(welcome()).not.toBeNull();
    expect(inert()).toBe(true);
    expect(net.calls.filter((c) => /eth\.(send|capture\.start)|uart\.(open|send)/.test(c.method))).toEqual([]);
  });
});
