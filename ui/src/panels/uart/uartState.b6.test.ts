// B6 — port state for pacing / reconnect / native log / line levels (FUNCTIONS.md §B6.2). Opus-owned; Sonnet must not edit.
import { describe, expect, it } from 'vitest';
import type { LoomEvent } from '../../proto/provisional';
import { portExtrasText, reducePort, type UartPortState } from './uartState';

const ev = (kind: string, payload: Record<string, unknown>, generation = 1): LoomEvent =>
  ({ kind, payload, generation, domain: 'uart', channel: 'dut', seq: '1', epoch: '1', origin: 'device' }) as unknown as LoomEvent;
const base: UartPortState = { status: 'open', baud: 115200, generation: 1 };

describe('reducePort (B6 fields)', () => {
  it('reads pacing from uart.config', () => {
    const p = reducePort(base, ev('uart.config', { baud: 115200, pacing: { char_delay_ms: 5, line_delay_ms: 100 } }));
    expect(p.pacing).toEqual({ charMs: 5, lineMs: 100 });
    expect(p.status).toBe('open');
    expect(reducePort(base, ev('uart.config', { baud: 9600 })).pacing).toBeUndefined();
  });

  it('tracks a reconnect episode and its end', () => {
    let p = reducePort(base, ev('uart.state', { state: 'reconnecting', reconnect: { enabled: true, window_ms: 10_000, active: true, attempts: 2 } }));
    expect(p.status).toBe('reconnecting');
    expect(p.reconnect).toEqual({ enabled: true, windowMs: 10_000, active: true, attempts: 2, reason: null });
    p = reducePort(p, ev('uart.state', { state: 'disconnected', reason: 'gave_up', reconnect: { enabled: true, window_ms: 10_000, active: false, attempts: 30 } }));
    expect(p.reconnect).toEqual({ enabled: true, windowMs: 10_000, active: false, attempts: 30, reason: 'gave_up' });
    p = reducePort(p, ev('uart.state', { state: 'open', reopen: { attempt: 3 } }, 2));
    expect(p.reconnect?.active).toBe(false);
    expect(p.reconnect?.reason).toBeNull();
    expect(p.generation).toBe(2);
  });

  it('reads uart.log without touching the port status', () => {
    let p = reducePort(base, ev('uart.log', { state: 'started', format: 'jsonl', file: '/h/uart/logs/dut/dut-1.jsonl' }));
    expect(p.status).toBe('open');
    expect(p.log).toEqual({ active: true, format: 'jsonl', file: '/h/uart/logs/dut/dut-1.jsonl', stopped: null });
    p = reducePort(p, ev('uart.log', { state: 'stopped', reason: 'quota', format: 'jsonl', file: '/h/uart/logs/dut/dut-1.jsonl' }));
    expect(p.log).toEqual({ active: false, format: 'jsonl', file: '/h/uart/logs/dut/dut-1.jsonl', stopped: 'quota' });
  });

  it('records line levels from successful uart.signal events only', () => {
    let p = reducePort(base, ev('uart.signal', { kind: 'dtr', value: false, result: 'ok' }));
    expect(p.lines).toEqual({ dtr: false, rts: null });
    p = reducePort(p, ev('uart.signal', { kind: 'rts', value: false, duration_ms: 100, result: 'ok' }));
    expect(p.lines).toEqual({ dtr: false, rts: true });
    p = reducePort(p, ev('uart.signal', { kind: 'dtr', value: true, result: 'failed' }));
    expect(p.lines?.dtr).toBe(false);
    expect(reducePort(base, ev('uart.signal', { kind: 'break', value: true, duration_ms: 250, result: 'ok' })).lines).toBeUndefined();
  });
});

describe('portExtrasText', () => {
  it('null when nothing is reported', () => {
    expect(portExtrasText(base)).toBeNull();
    expect(portExtrasText({ ...base, pacing: { charMs: 0, lineMs: 0 } })).toBeNull();
  });
  it('names pacing, reconnect, log and lines', () => {
    expect(portExtrasText({ ...base, pacing: { charMs: 5, lineMs: 100 } })).toBe('TX 지연 문자 5 ms · 줄 100 ms');
    expect(portExtrasText({ ...base, reconnect: { enabled: true, windowMs: 10_000, active: true, attempts: 2, reason: null } })).toBe('재연결 중 (시도 2, 최대 10 s)');
    expect(portExtrasText({ ...base, reconnect: { enabled: true, windowMs: 10_000, active: false, attempts: 30, reason: 'gave_up' } })).toBe('재연결 중단: 시간 초과');
    expect(portExtrasText({ ...base, reconnect: { enabled: true, windowMs: 10_000, active: false, attempts: 1, reason: 'identity_mismatch' } })).toBe('재연결 중단: 다른 장치');
    expect(portExtrasText({ ...base, log: { active: true, format: 'log', file: 'x', stopped: null } })).toBe('로그 기록 중 (.log)');
    expect(portExtrasText({ ...base, log: { active: false, format: 'log', file: 'x', stopped: 'quota' } })).toBe('로그 중지: quota');
    expect(portExtrasText({ ...base, log: { active: false, format: 'log', file: 'x', stopped: 'user' } })).toBeNull();
    expect(portExtrasText({ ...base, lines: { dtr: false, rts: null } })).toBe('DTR off · RTS ?');
    expect(
      portExtrasText({ ...base, pacing: { charMs: 0, lineMs: 50 }, log: { active: true, format: 'jsonl', file: 'x', stopped: null } }),
    ).toBe('TX 지연 문자 0 ms · 줄 50 ms · 로그 기록 중 (.jsonl)');
  });
});
