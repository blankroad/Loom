// B6 — session operator options (FUNCTIONS.md §B6.3). Opus-owned specification; Sonnet must not edit.
import { describe, expect, it, vi } from 'vitest';
import type { ShellServices } from '../../shell/services';
import { openParams, PACING_SPEC, sanitizeSession, SESSION_SCROLLBACK, UartBench, type Session } from './workbench';

const san = (x: unknown) => sanitizeSession(x as Parameters<typeof sanitizeSession>[0]);
const base: Session = { id: 's-1', alias: 'dut', group: '', port: '/dev/ttyUSB0', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', ts: true, color: '#0969da' };

describe('openParams', () => {
  it('sends only what is on', () => {
    expect(openParams(base)).toEqual({ target: '/dev/ttyUSB0', alias: 'dut', config: { baud: 115200, data_bits: 8, parity: 'none', stop_bits: '1', flow: 'none' } });
    expect(openParams({ ...base, charDelayMs: 0, lineDelayMs: 0, reconnect: false, log: 'off' })).not.toHaveProperty('pacing');
    const p = openParams({ ...base, charDelayMs: 5, lineDelayMs: 0, reconnect: true, reconnectWindowS: 30, log: 'jsonl', flow: 'RTS/CTS', parity: 'E' });
    expect(p.pacing).toEqual({ char_delay_ms: 5, line_delay_ms: 0 });
    expect(p.reconnect).toEqual({ enabled: true, window_ms: 30_000 });
    expect(p.log).toEqual({ format: 'jsonl' });
    expect(p.config).toEqual({ baud: 115200, data_bits: 8, parity: 'even', stop_bits: '1', flow: 'hardware' });
  });
});

describe('sanitizeSession (B6 fields)', () => {
  it('fills defaults and clamps', () => {
    const d = san({ ...base })!;
    expect(d).toMatchObject({ charDelayMs: 0, lineDelayMs: 0, reconnect: false, reconnectWindowS: 10, log: 'off', scrollback: SESSION_SCROLLBACK.def, highlights: [] });
    const c = san({ ...base, charDelayMs: 5000, lineDelayMs: 'x', reconnect: 'yes', reconnectWindowS: 500, log: 'csv', scrollback: 10, highlights: [{ pattern: 'ERR', regex: false, caseSensitive: false, color: 'red' }, { bad: 1 }] })!;
    expect(c).toMatchObject({ charDelayMs: 1000, lineDelayMs: 0, reconnect: false, reconnectWindowS: 120, log: 'off', scrollback: 1000 });
    expect(c.highlights).toEqual([{ pattern: 'ERR', regex: false, caseSensitive: false, color: 'red' }]);
    expect(san({ ...base, charDelayMs: -3, reconnect: true, reconnectWindowS: 0, scrollback: 90_000, log: 'log' })).toMatchObject({ charDelayMs: 0, reconnect: true, reconnectWindowS: 1, scrollback: 50_000, log: 'log' });
  });
});

function svc() {
  return {
    shell: { latch: { on: false }, status: { writeLocked: false }, channels: new Map(), subscribe: () => () => {}, noteLocalOpen: vi.fn() },
    stop: { phase: 'idle', subscribe: () => () => {} },
    gate: { run: vi.fn(async () => ({})) },
    toasts: { push: vi.fn() },
    now: () => 1,
  };
}

describe('UartBench', () => {
  it('openSession opens with openParams', async () => {
    const s = svc();
    const b = new UartBench(null);
    const off = b.attach(s as unknown as ShellServices);
    const sess = { ...base, lineDelayMs: 40, reconnect: true, reconnectWindowS: 10, log: 'log' as const };
    await b.openSession(sess);
    expect(s.gate.run).toHaveBeenCalledWith(expect.objectContaining({ rpc: 'uart.open' }), openParams(sess));
    off();
  });
  it('applyPacing sends uart.pacing through the gate', async () => {
    const s = svc();
    const b = new UartBench(null);
    const off = b.attach(s as unknown as ShellServices);
    await b.applyPacing('dut', 3, 50);
    expect(s.gate.run).toHaveBeenCalledWith(PACING_SPEC, { channel: 'dut', char_delay_ms: 3, line_delay_ms: 50 });
    off();
  });
});
