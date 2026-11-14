// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import '../../panels/uart/terminalPanel';
import '../../panels/uart/tracePanel';
import '../../panels/eth/ethPanel';
import { TabSettings } from '../../store/tabSettings';
import { ev, FakeNet, TEST_HELLO } from '../../store/testing/fakeNet';
import { parseRoute, type Route } from '../routes';
import { createShellServices } from '../services';
import { ShellCtx } from '../ShellContext';
import { DockLayout, LayoutBar } from './DockLayout';
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
  const render = (route: Route) => act(() => root.render(<ShellCtx.Provider value={services}><LayoutBar route={route} /><DockLayout route={route} /></ShellCtx.Provider>));
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

  it('reacts to silent snapshots, close/reopen and picker eligibility while preserving trace and input', () => {
    const { net, services, render, dock } = mount();
    const options = () => [...document.querySelectorAll('optgroup[label="프리셋"] option')].map((o) => (o as HTMLOptionElement).value);
    expect(options()).toEqual(['m1-default']);
    act(() => services.shell.noteSnapshotItems([
      { domain: 'uart', channel: 'serial-a', state: { status: 'OPEN' } },
      { domain: 'can', channel: 'history', state: { status: 'closed' } },
      { domain: 'eth', channel: 'uncertain', state: {} },
    ]));
    expect(options()).toEqual(['m1-default', 'uart-only']);
    const terminal = dock().find({ panel: 'uart.terminal', props: { channel: 'serial-a' } })!;
    act(() => services.showSlot({ panel: 'uart.trace', props: { channel: 'serial-a' } }));
    const trace = dock().api.activePanel!;
    const draft = document.createElement('input');
    draft.value = 'unsent user input';
    trace.view.content.element.append(draft);
    draft.focus();
    act(() => services.shell.noteSnapshotItems([{ domain: 'can', channel: 'bus-a', state: { status: 'OPEN' } }]));
    expect(options()).toEqual(['m1-default', 'uart-only', 'can-only']);
    expect(dock().api.activePanel).toBe(trace);
    expect(document.activeElement).toBe(draft);
    act(() => net.emitControl(ev({ kind: 'can.state', domain: 'can', channel: 'bus-a', payload: { state: 'closed' } })));
    expect(dock().api.panels).toHaveLength(2);
    expect(dock().find({ panel: 'uart.terminal', props: { channel: 'serial-a' } })).toBe(terminal);
    expect(dock().api.activePanel).toBe(trace);
    expect(draft.value).toBe('unsent user input');
    act(() => net.emitControl(ev({ kind: 'can.state', domain: 'can', channel: 'bus-a', payload: { state: 'open' } })));
    expect(dock().api.panels).toHaveLength(3);
    expect(dock().api.activePanel).toBe(trace);
    expect(net.calls).toEqual([]);
    // User-closed views stay closed on heartbeats, and an alternative view is retained.
    act(() => terminal.api.close());
    act(() => services.shell.tick(4000));
    expect(dock().find({ panel: 'uart.terminal', props: { channel: 'serial-a' } })).toBeNull();
    expect(dock().api.activePanel).toBe(trace);
    render(parseRoute('#/p/uart/trace?channel=serial-a'));
    act(() => net.emitControl(ev({ kind: 'uart.state', domain: 'uart', channel: 'serial-b', payload: { state: 'open' } })));
    expect(dock().api.panels).toHaveLength(1);
  });

  it('honors current auto-open off for new channels and reopen while pruning inactive panes', () => {
    const { net, services, render, dock } = mount();
    const signal = (channel: string, state: 'open' | 'closed') => act(() => net.emitControl(ev({ kind: 'uart.state', domain: 'uart', channel, payload: { state } })));
    signal('alpha', 'open');
    signal('beta', 'open');
    act(() => services.showSlot({ panel: 'uart.trace', props: { channel: 'alpha' } }));
    const trace = dock().api.activePanel!;
    const terminal = dock().find({ panel: 'uart.terminal', props: { channel: 'alpha' } })!;
    act(() => services.settings.setAutoOpen('off'));
    signal('gamma', 'open');
    expect(services.shell.channels.get('uart/gamma')?.state).toBe('open');
    expect(dock().api.panels).toHaveLength(3);
    expect(dock().find({ panel: 'uart.terminal', props: { channel: 'gamma' } })).toBeNull();
    // Silent snapshot arrivals also stay available only through the explorer.
    act(() => services.shell.noteSnapshotItems([{ domain: 'uart', channel: 'delta', state: { status: 'OPEN' } }]));
    act(() => services.shell.tick(4000));
    expect(dock().api.panels).toHaveLength(3);
    signal('beta', 'closed');
    expect(dock().api.panels).toHaveLength(2);
    signal('beta', 'open');
    expect(dock().api.panels).toHaveLength(2);
    expect(dock().api.activePanel).toBe(trace);
    expect(dock().find({ panel: 'uart.terminal', props: { channel: 'alpha' } })).toBe(terminal);
    act(() => { expect(services.showSlot({ panel: 'uart.terminal', props: { channel: 'gamma' } })).toBe('added'); });
    expect(dock().api.panels).toHaveLength(3);
    act(() => services.settings.setAutoOpen('on'));
    act(() => services.shell.tick(5000));
    expect(dock().find({ panel: 'uart.terminal', props: { channel: 'delta' } })).toBeNull();
    signal('epsilon', 'open');
    expect(dock().find({ panel: 'uart.terminal', props: { channel: 'epsilon' } })).not.toBeNull();
    // An explicit layout selection initializes its actual channels even with auto-open off.
    act(() => services.settings.setAutoOpen('off'));
    render(parseRoute('#/l/uart-only'));
    expect(dock().api.panels).toHaveLength(5);
    expect(services.settings.autoOpenPanels).toBe('off');
    expect(net.calls).toEqual([]);
  });

  it('describes a partially available legacy combo without claiming no interfaces', () => {
    const { services, render, dock } = mount();
    render(parseRoute('#/l/can-uds'));
    const selectedText = () => document.querySelector<HTMLSelectElement>('select[aria-label="레이아웃"]')!.selectedOptions[0]!.textContent;
    expect(selectedText()).toBe('(연결된 인터페이스 없음)');
    act(() => services.shell.noteSnapshotItems([{ domain: 'can', channel: 'bus-real', state: { status: 'OPEN' } }]));
    expect(leaves(dock().snapshot().root)).toEqual([{ panel: 'can.trace', props: { channel: 'bus-real' } }]);
    expect(selectedText()).toBe('(현재 연결에 맞춰 표시)');
    expect(document.querySelector('option[value="can-uds"]')).toBeNull();
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

  it('applies saved layouts only to active aliases and keeps stored unavailable panes intact', () => {
    const { services, render, dock } = mount();
    act(() => services.shell.noteSnapshotItems(['alpha', 'beta'].map((channel) => ({ kind: 'uart.state', channel, state: { status: 'open' } }))));
    render(parseRoute('#/l/uart-only'));
    expect(leaves(dock().snapshot().root)).toEqual(['alpha', 'beta'].map((channel) => ({ panel: 'uart.terminal', props: { channel } })));
    act(() => services.showSlot({ panel: 'uart.trace', props: { channel: 'alpha' } }));
    const layout = dock().snapshot('my-bench');
    expect(services.layouts.save(layout, new Set()).ok).toBe(true);
    act(() => services.shell.noteSnapshotItems([{ kind: 'uart.state', channel: 'beta', state: { status: 'closed' } }]));
    render(parseRoute('#/l/my-bench'));
    expect(leaves(dock().snapshot().root).map((s) => s.props.channel)).toEqual(['alpha', 'alpha']);
    expect(services.layouts.get('my-bench').layout).toEqual(layout);
    const trace = dock().find({ panel: 'uart.trace', props: { channel: 'alpha' } })!;
    act(() => trace.api.setActive());
    act(() => services.shell.noteSnapshotItems([{ kind: 'uart.state', channel: 'beta', state: { status: 'open' } }]));
    expect(dock().api.panels).toHaveLength(3);
    expect(dock().api.activePanel).toBe(trace);
    expect(services.layouts.get('my-bench').layout).toEqual(layout);
  });

  it('keeps an ETH capture panel on normal stop and lets an explicit reopen of the stopped capture stay', () => {
    const { net, services, dock } = mount();
    const cap = (state: string, generation = 1) => act(() => net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'qa', generation, payload: { ch: 'qa', state } })));
    const slot = { panel: 'eth.packets', props: { channel: 'qa' } };
    act(() => net.emitControl(ev({ kind: 'uart.state', domain: 'uart', channel: 'port', payload: { state: 'open' } })));
    cap('live');
    const panel = dock().find(slot)!;
    expect(panel).not.toBeNull();
    cap('stopped');
    act(() => services.shell.tick(4000));
    expect(services.shell.channels.get('eth/qa')?.state).toBe('closed');
    expect(dock().find(slot)).toBe(panel);
    // live-only domains keep the tested pruning contract
    act(() => net.emitControl(ev({ kind: 'uart.state', domain: 'uart', channel: 'port', payload: { state: 'closed' } })));
    expect(dock().find({ panel: 'uart.terminal', props: { channel: 'port' } })).toBeNull();
    expect(dock().find(slot)).toBe(panel);
    // user closes it, then reopens the stopped capture from the inactive explorer: it survives reconcile
    act(() => panel.api.close());
    expect(dock().find(slot)).toBeNull();
    act(() => { expect(services.showSlot(slot)).toBe('added'); });
    act(() => services.shell.tick(8000));
    act(() => net.emitControl(ev({ kind: 'uart.state', domain: 'uart', channel: 'other', payload: { state: 'open' } })));
    expect(dock().find(slot)).not.toBeNull();
    // a later restart (new generation) reuses the view, never duplicates it; nothing was sent
    cap('live', 2);
    expect(dock().api.panels.filter((p) => JSON.stringify(p.params).includes('eth.packets'))).toHaveLength(1);
    expect(net.calls.filter((c) => c.method === 'eth.send' || c.method === 'eth.capture.start')).toEqual([]);
  });
});
