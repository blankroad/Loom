// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import '../../panels/uart/terminalPanel';
import '../../panels/uart/tracePanel';
import { TabSettings } from '../../store/tabSettings';
import { ev, FakeNet, TEST_HELLO } from '../../store/testing/fakeNet';
import { createShellServices } from '../services';
import { DockHost } from './dockHost';
import { slotLayout } from './layoutModel';

(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };
let clean: (() => void) | undefined;
afterEach(() => { clean?.(); });

describe('actual UART dock views', () => {
  it('auto-opens channels without hiding a deep-linked trace or stealing its active tab', () => {
    const net = new FakeNet(); net.open(TEST_HELLO);
    const services = createShellServices(net, { autoTick: false, settings: new TabSettings({ getItem: () => null, setItem: () => {} }) });
    const off = services.bind();
    const host = document.createElement('div'); document.body.append(host);
    const dock = new DockHost(host, services, { raf: () => 1, cancelRaf: () => {} });
    const detach = services.attachDock(dock);
    clean = () => { detach(); dock.dispose(); off(); host.remove(); };
    const trace = { panel: 'uart.trace', props: { channel: 'dut' } };
    dock.load(slotLayout(trace));
    dock.find(trace)!.api.setActive();
    const active = dock.api.activePanel!.id;
    net.emitControl(ev({ kind: 'uart.state', domain: 'uart', channel: 'dut', payload: { state: 'open' } }));
    expect(dock.api.activePanel!.id).toBe(active);
    expect(dock.find(trace)!.api.isVisible).toBe(true);
    const terminal = { panel: 'uart.terminal', props: { channel: 'dut' } };
    expect(dock.find(terminal)).not.toBeNull();
    expect(dock.find(terminal)!.api.isVisible).toBe(false);
    services.showSlot(terminal);
    expect(dock.find(terminal)!.api.isVisible).toBe(true);
    expect(net.calls).toEqual([]);
  });
  it('retains separate channel state, trace rows and input sequences while switching/reopening tabs', () => {
    const net = new FakeNet(); net.open(TEST_HELLO);
    let clock = 0;
    let frame: (() => void) | null = null;
    const services = createShellServices(net, {
      autoTick: false,
      settings: new TabSettings({ getItem: () => null, setItem: () => {} }),
      frames: { raf: (cb) => (frame = () => cb(clock), 1), cancelRaf: () => { frame = null; }, now: () => clock },
    });
    const unbind = services.bind();
    const host = document.createElement('div'); document.body.append(host);
    const dock = new DockHost(host, services, { raf: () => 1, cancelRaf: () => {} });
    const detach = services.attachDock(dock);
    clean = () => { detach(); dock.dispose(); unbind(); host.remove(); };
    const draw = () => { clock += 100; const f = frame; frame = null; f?.(); };
    const a = { panel: 'uart.terminal', props: { channel: 'alpha' } };
    const b = { panel: 'uart.terminal', props: { channel: 'beta' } };
    const ta = { panel: 'uart.trace', props: { channel: 'alpha' } };
    const tb = { panel: 'uart.trace', props: { channel: 'beta' } };
    dock.load(slotLayout(a)); dock.open(b); dock.open(ta); dock.open(tb);
    const uartSubs = net.subs.map((s, index) => ({ ...s, index })).filter((s) => s.filter.domains?.includes('uart'));
    expect(uartSubs).toHaveLength(4);
    expect(uartSubs.map((s) => s.filter.channels)).toEqual([['alpha'], ['beta'], ['alpha'], ['beta']]);
    // Deliver the records the daemon would send for each asserted channel filter.
    for (const sub of uartSubs) {
      const channel = sub.filter.channels![0]!;
      net.emitBatch(sub.index, [
        ev({ kind: 'uart.state', channel, payload: { status: 'OPEN', baud: channel === 'alpha' ? 9600 : 115200 } }),
        ev({ kind: 'uart.data', channel, dir: 'rx', payload: { bytes: { encoding: 'base64', data: btoa(`${channel}-only\n`) } } }),
      ]);
    }
    for (const slot of [ta, tb]) {
      dock.open(slot); draw();
      const body = dock.find(slot)!.view.content.element;
      expect(body.textContent).toContain(`${slot.props.channel}-only`);
      expect(body.textContent).not.toContain(`${slot.props.channel === 'alpha' ? 'beta' : 'alpha'}-only`);
    }
    for (const slot of [a, b, a]) {
      dock.open(slot); draw();
      const body = dock.find(slot)!.view.content.element;
      expect(body.querySelector('.uart-port')!.textContent).toContain(slot.props.channel === 'alpha' ? '9600' : '115200');
      [...body.querySelectorAll('button')].find((button) => button.textContent === 'Ctrl-C')!.click();
    }
    expect(net.calls.filter((c) => c.method === 'uart.input').map((c) => c.params)).toEqual([
      { channel: 'alpha', data_hex: '03', input_seq: 1 },
      { channel: 'beta', data_hex: '03', input_seq: 1 },
      { channel: 'alpha', data_hex: '03', input_seq: 2 },
    ]);
    expect(dock.api.panels).toHaveLength(4);
    expect(uartSubs.every((s) => !net.subs[s.index]!.closed)).toBe(true);
    dock.find(b)!.api.close();
    expect(net.subs[uartSubs[1]!.index]!.closed).toBe(true);
    expect(net.subs[uartSubs[0]!.index]!.closed).toBe(false);
    dock.open(b);
    // the reopened view subscribes anew; the daemon delivers the port state (snapshot/backlog) before input is allowed
    const reopened = net.subs.length - 1;
    expect(net.subs[reopened]!.filter.channels).toEqual(['beta']);
    net.emitBatch(reopened, [ev({ kind: 'uart.state', channel: 'beta', payload: { status: 'OPEN', baud: 115200 } })]);
    draw();
    const body = dock.find(b)!.view.content.element;
    [...body.querySelectorAll('button')].find((button) => button.textContent === 'Ctrl-C')!.click();
    expect(net.calls.at(-1)?.params).toEqual({ channel: 'beta', data_hex: '03', input_seq: 1 });
    expect(net.calls.some((c) => c.method === 'uart.open')).toBe(false);
  });
});
