// @vitest-environment jsdom
// B6 — terminal panel: status line extras, DTR/RTS pulses via confirm card, native log controls in the REC dialog,
// session highlight rules (FUNCTIONS.md §B6.4). Opus-owned specification; Sonnet must not edit.
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { uartBench, type Session } from '../../workbench/uart/workbench';
import './terminalPanel';

beforeAll(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
});

let h: Harness | null = null;
const savedSessions = [...uartBench.sessions];
afterEach(() => {
  h?.dispose();
  h = null;
  uartBench.sessions = [...savedSessions];
  document.querySelectorAll('.ub-layer').forEach((n) => n.remove());
});

const flush = () => new Promise((r) => setTimeout(r, 0));
const settle = async () => { for (let i = 0; i < 6; i++) await flush(); h!.frame(); };
const hex = (s: string) => Array.from(new TextEncoder().encode(s), (b) => b.toString(16).padStart(2, '0')).join('');
const foot = () => h!.host.querySelector<HTMLElement>('[data-testid="uart-status"]')!.textContent ?? '';
const dlg = () => document.querySelector<HTMLDialogElement>('dialog[aria-label="UART 기록"]')!;

function mount() {
  h = mountPanel('uart.terminal', { channel: 'dut' });
  h.batch([
    ev({ kind: 'uart.state', channel: 'dut', generation: 1, payload: { state: 'open', path: '/dev/ttyUSB0', transport: 'serial' } }),
    ev({ kind: 'uart.config', channel: 'dut', generation: 1, payload: { baud: 115200, data_bits: 8, parity: 'none', stop_bits: '1', flow: 'none', pacing: { char_delay_ms: 5, line_delay_ms: 100 } } }),
  ]);
  return h;
}
const calls = (method: string) => h!.net.calls.filter((c) => c.method === method).map((c) => c.params as Record<string, unknown>);

describe('status line (B6)', () => {
  it('shows pacing, reconnect progress and the native log', () => {
    mount();
    expect(foot()).toContain('TX 지연 문자 5 ms · 줄 100 ms');
    h!.batch([ev({ kind: 'uart.state', channel: 'dut', generation: 1, payload: { state: 'reconnecting', reconnect: { enabled: true, window_ms: 10_000, active: true, attempts: 2 } } })]);
    expect(foot()).toContain('재연결 중 (시도 2, 최대 10 s)');
    h!.batch([ev({ kind: 'uart.log', channel: 'dut', generation: 1, payload: { state: 'started', format: 'log', file: '/h/dut-001.log' } })]);
    expect(foot()).toContain('로그 기록 중 (.log)');
  });
});

describe('palette line pulses (U6)', () => {
  it('DTR/RTS pulses go through the signal-line action with value false for 100 ms; Break stays as before', async () => {
    mount();
    const pill = (label: string) => [...h!.host.querySelectorAll<HTMLButtonElement>('.uart-palette button')].find((b) => b.textContent === label)!;
    pill('DTR 펄스').click();
    await settle();
    pill('RTS 펄스').click();
    await settle();
    pill('Break').click();
    await settle();
    const sig = calls('uart.signal');
    expect(sig).toEqual([
      expect.objectContaining({ channel: 'dut', kind: 'dtr', value: false, duration_ms: 100 }),
      expect.objectContaining({ channel: 'dut', kind: 'rts', value: false, duration_ms: 100 }),
      expect.objectContaining({ channel: 'dut', kind: 'break' }),
    ]);
    expect(sig[2]).not.toHaveProperty('value');
  });
});

describe('REC dialog: native log (U1-A)', () => {
  it('starts with the dialog format, shows the state, stops', async () => {
    mount();
    h!.host.querySelector<HTMLButtonElement>('[data-act="rec"]')!.click();
    expect(dlg().querySelector('[data-role="nlog"]')).not.toBeNull();
    expect(dlg().querySelector('[data-role="nlog"]')!.textContent).toContain('$LOOM_HOME/uart/logs/dut');
    dlg().querySelector<HTMLButtonElement>('[data-act="nlog-start"]')!.click();
    await settle();
    expect(calls('uart.log.start')).toEqual([expect.objectContaining({ channel: 'dut', format: 'log' })]);
    h!.batch([ev({ kind: 'uart.log', channel: 'dut', generation: 1, payload: { state: 'started', format: 'log', file: '/h/uart/logs/dut/dut-001.log' } })]);
    await settle();
    expect(dlg().querySelector('[data-role="nlogstat"]')!.textContent).toContain('기록 중');
    expect(dlg().querySelector('[data-role="nlogstat"]')!.textContent).toContain('dut-001.log');
    dlg().querySelector<HTMLButtonElement>('[data-act="nlog-stop"]')!.click();
    await settle();
    expect(calls('uart.log.stop')).toEqual([expect.objectContaining({ channel: 'dut' })]);
  });
});

describe('session highlight rules (U5b)', () => {
  it('compiles the session rules for this channel (decorations are drawn by xterm)', async () => {
    const sess: Session = {
      id: 's-hl', alias: 'dut', group: '', port: '/dev/ttyUSB0', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', ts: true, color: '#0969da',
      highlights: [{ pattern: 'ERROR', regex: false, caseSensitive: false, color: 'red' }, { pattern: '(bad', regex: true, caseSensitive: false, color: 'blue' }],
    };
    uartBench.sessions = [sess];
    mount();
    h!.batch([ev({ kind: 'uart.data', channel: 'dut', dir: 'rx', origin: 'device', payload: { bytes: { encoding: 'hex', data: hex('boot ERROR here\r\n') } } })]);
    await settle();
    const wrap = h!.host.querySelector<HTMLElement>('.term-wrap')!;
    expect(wrap.dataset.hlRules).toBe('1');
    expect(wrap.dataset.hlErrors).toBe('1');
  });
});
