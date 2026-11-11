// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import '../../panels/uart/terminalPanel';
import '../../panels/uart/tracePanel';
import { TabSettings } from '../../store/tabSettings';
import { ev, FakeNet, TEST_HELLO } from '../../store/testing/fakeNet';
import { parseRoute, type Route } from '../routes';
import { createShellServices } from '../services';
import { ShellCtx } from '../ShellContext';
import { DockLayout } from './DockLayout';
import type { DockHost } from './dockHost';
import { leaves } from './layoutModel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };
let clean: (() => void) | undefined;
afterEach(() => clean?.());
function mount() {
  const net = new FakeNet(); net.open(TEST_HELLO);
  const services = createShellServices(net, { autoTick: false, settings: new TabSettings({ getItem: () => null, setItem: () => {} }) });
  const off = services.bind();
  const host = document.createElement('div'); document.body.append(host);
  const root = createRoot(host);
  const render = (route: Route) => act(() => root.render(<ShellCtx.Provider value={services}><DockLayout route={route} /></ShellCtx.Provider>));
  clean = () => { act(() => root.unmount()); off(); host.remove(); };
  render(parseRoute('#/l/m1-default'));
  return { net, services, render, dock: () => services.dock.current as DockHost };
}

describe('default workbench initialization', () => {
  it('starts empty, opens actual first interfaces without ghosts and never reloads tabs on rerender/state ticks', () => {
    const { net, services, render, dock } = mount();
    expect(dock().api.panels).toHaveLength(0);
    expect(document.querySelector('[data-testid="dock-empty"]')).not.toBeNull();
    expect(net.subs.filter((s) => s.filter.domains?.includes('uart'))).toHaveLength(0);
    for (const channel of ['alpha', 'beta']) act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel, payload: { state: 'opened' } })));
    expect(document.querySelector('[data-testid="dock-empty"]')).toBeNull();
    expect(leaves(dock().snapshot().root)).toEqual([
      { panel: 'uart.terminal', props: { channel: 'alpha' } },
      { panel: 'uart.terminal', props: { channel: 'beta' } },
    ]);
    act(() => services.showSlot({ panel: 'uart.trace', props: { channel: 'alpha' } }));
    const before = dock().api.panels.map((p) => p.id);
    render(parseRoute('#/l/m1-default'));
    act(() => services.shell.tick(2000));
    expect(dock().api.panels.map((p) => p.id)).toEqual(before);
    expect(dock().api.panels.some((p) => /dut|ch0|bcm/.test(JSON.stringify(p.params)))).toBe(false);
    render(parseRoute('#/p/uart/terminal?channel=alpha'));
    expect(dock().api.panels).toHaveLength(1);
    render(parseRoute('#/l/m1-default'));
    expect(dock().api.panels).toHaveLength(2);
    for (const channel of ['alpha', 'beta']) expect(services.showSlot({ panel: 'uart.terminal', props: { channel } })).toBe('shown');
    expect(dock().api.panels).toHaveLength(2);
  });

  it('bounds initial known-channel views without hiding channels from the explorer store', () => {
    const { services, render, dock } = mount();
    act(() => services.shell.noteSnapshotItems(Array.from({ length: 33 }, (_, i) => ({ kind: 'uart.state', channel: `port-${i}`, state: { status: 'open' } }))));
    render(parseRoute('#/l/uart-only'));
    render(parseRoute('#/l/m1-default'));
    expect(dock().api.panels).toHaveLength(32);
    expect(services.shell.channels.size).toBe(33);
    expect(services.toasts.items.at(-1)?.text).toContain('32개 보기만 열었습니다');
  });

  it('keeps explicit presets compatible and restores user aliases/views from a saved layout route', () => {
    const { services, render, dock } = mount();
    render(parseRoute('#/l/uart-only'));
    expect(leaves(dock().snapshot().root)).toEqual([{ panel: 'uart.terminal', props: { channel: 'dut' } }]);
    services.showSlot({ panel: 'uart.terminal', props: { channel: 'beta' } });
    services.showSlot({ panel: 'uart.trace', props: { channel: 'alpha' } });
    const layout = dock().snapshot('my-bench');
    expect(services.layouts.save(layout, new Set()).ok).toBe(true);
    render(parseRoute('#/l/m1-default'));
    expect(dock().api.panels).toHaveLength(0);
    render(parseRoute('#/l/my-bench'));
    expect(leaves(dock().snapshot().root)).toEqual(leaves(layout.root));
    expect(services.showSlot({ panel: 'uart.trace', props: { channel: 'alpha' } })).toBe('shown');
    expect(dock().api.panels).toHaveLength(3);
  });
});
