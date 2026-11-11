// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FakeNet } from '../store/testing/fakeNet';
import { connectionRequest, InterfaceConnect } from './InterfaceConnect';
import { createShellServices } from './services';
import { ShellCtx } from './ShellContext';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => vi.restoreAllMocks());

describe('interface connection requests', () => {
  const draft = { domain: 'uart' as const, name: 'console-b', simulated: true, address: '', baud: '115200' };
  it('uses canonical UART alias/config without changing another connection', () => {
    expect(connectionRequest(draft)).toEqual({ rpc: 'uart.open', params: { target: 'sim://bootshell', alias: 'console-b', config: { baud: 115200 } } });
    expect(connectionRequest({ ...draft, simulated: false, address: ' /dev/ttyUSB1 ' }).params).toEqual({ target: '/dev/ttyUSB1', alias: 'console-b', config: { baud: 115200 } });
  });
  it('uses separate CAN bus, DoIP name and Ethernet channel schemas', () => {
    expect(connectionRequest({ ...draft, domain: 'can' }).params).toEqual({ channel: 'console-b', bus: 'console-b' });
    expect(connectionRequest({ ...draft, domain: 'uds' }).params).toEqual({ name: 'console-b', sim: 'bench', timeout_ms: 5000 });
    expect(connectionRequest({ ...draft, domain: 'uds', simulated: false, address: '192.0.2.10' }).params).toEqual({ name: 'console-b', host: '192.0.2.10', timeout_ms: 5000 });
    expect(connectionRequest({ ...draft, domain: 'eth', simulated: false, address: 'en5' }).params).toEqual({ iface: 'en5', ch: 'console-b', backend: 'live' });
  });
  it('rejects invalid alias, baud and missing device address before RPC', () => {
    for (const name of ['../dut', 'a/b', '', 'A', 'a'.repeat(33)]) expect(() => connectionRequest({ ...draft, name })).toThrow();
    for (const baud of ['NaN', '0', '12.5', '4000001']) expect(() => connectionRequest({ ...draft, baud })).toThrow();
    expect(() => connectionRequest({ ...draft, simulated: false })).toThrow();
    expect(() => connectionRequest({ ...draft, domain: 'can', simulated: false, address: 'can0' })).toThrow(/시뮬레이터/);
  });
  it('opening the dialog sends no RPC, and watch cannot connect', async () => {
    const net = new FakeNet();
    const services = createShellServices(net, { autoTick: false, automation: false });
    const calls = vi.spyOn(net, 'call');
    const host = document.createElement('div');
    document.body.append(host);
    const root = createRoot(host);
    const show = vi.fn();
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: show });
    Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() });
    try {
      act(() => root.render(<ShellCtx.Provider value={services}><InterfaceConnect /></ShellCtx.Provider>));
      act(() => host.querySelector('button')?.click());
      expect(show).toHaveBeenCalledOnce();
      expect(calls).not.toHaveBeenCalled();
      const submit = document.querySelector<HTMLButtonElement>('.interface-dialog button[type=submit]');
      expect(submit?.disabled).toBe(true);
      await act(async () => { document.querySelector('form')?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); });
      expect(calls).not.toHaveBeenCalled();
    } finally {
      act(() => root.unmount());
      host.remove();
      Reflect.deleteProperty(HTMLDialogElement.prototype, 'showModal');
      Reflect.deleteProperty(HTMLDialogElement.prototype, 'close');
    }
  });
});
