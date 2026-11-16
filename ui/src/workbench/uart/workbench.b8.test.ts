// UART B8 (U8): the workbench ▶ marks its own opens as local before the gated uart.open, so the shell does not
// toast "장치가 … 다시 열었습니다" for them. Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B8 (lead-written).
import { describe, expect, it, vi } from 'vitest';
import type { ShellServices } from '../../shell/services';
import { UartBench, type Session } from './workbench';

const session: Session = {
  id: 's-1', alias: 'dut', group: '', port: '/dev/ttyUSB0', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', ts: true, color: '#0969da',
};

describe('UART B8: openSession notes a local open', () => {
  it('calls shell.noteLocalOpen(uart, alias, now) before the gated uart.open', async () => {
    const order: string[] = [];
    const s = {
      shell: {
        latch: { on: false },
        status: { writeLocked: false },
        channels: new Map(),
        subscribe: () => () => {},
        noteLocalOpen: vi.fn((d: string, c: string, t: number) => void order.push(`note:${d}/${c}@${t}`)),
      },
      stop: { phase: 'idle', subscribe: () => () => {} },
      gate: { run: vi.fn(async (spec: { rpc: string }) => { order.push(`gate:${spec.rpc}`); return {}; }) },
      toasts: { push: vi.fn() },
      now: () => 4242,
    };
    const bench = new UartBench();
    const off = bench.attach(s as unknown as ShellServices);
    await bench.openSession(session);
    expect(order).toEqual(['note:uart/dut@4242', 'gate:uart.open']);
    off();
  });

  it('does not note anything when the channel is already open', async () => {
    const noteLocalOpen = vi.fn();
    const s = {
      shell: { latch: { on: false }, status: { writeLocked: false }, channels: new Map([['uart/dut', { state: 'open' }]]), subscribe: () => () => {}, noteLocalOpen },
      stop: { phase: 'idle', subscribe: () => () => {} },
      gate: { run: vi.fn(async () => ({})) },
      toasts: { push: vi.fn() },
      now: () => 1,
    };
    const bench = new UartBench();
    const off = bench.attach(s as unknown as ShellServices);
    await bench.openSession(session);
    expect(noteLocalOpen).not.toHaveBeenCalled();
    expect(s.gate.run).not.toHaveBeenCalled();
    off();
  });
});
