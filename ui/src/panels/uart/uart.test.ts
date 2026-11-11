// @vitest-environment jsdom
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './terminalPanel';
import { LineAssembler } from './tracePanel';
import { INITIAL_INPUT, inputChip, reduceInput, reducePort, restoreUartState, symbolize, type UartInputState } from './uartState';

beforeAll(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
});

it('reads native UART connection/config and restores metadata without device output', () => {
  const initial = { status: '?', baud: null, generation: null };
  expect(reducePort(initial, ev({ kind: 'uart.state', generation: 3, payload: { state: 'open' } }))).toEqual({ status: 'open', baud: null, generation: 3 });
  expect(restoreUartState({ channel: 'aux', generation: 4, state: { state: 'open', path: 'sim://bootshell' }, config: { baud: 9600 }, input_owner: { owner: 'user', viewer: 'other', dirty: true, hold: true }, lock: { on: false } }, 100)).toMatchObject({ port: { status: 'open', baud: 9600, generation: 4 }, input: { owner: 'user', viewer: 'other', dirty: true, hold: true, lock: null } });
});

it('hydrates only its UART metadata, supersedes older backlog state and keeps newer live state', () => {
  const h = mountPanel('uart.terminal', { channel: 'aux' });
  try {
    h.net.subs[0]!.h.onSnapshotPart?.({ sub: h.net.subs[0]!.id, n: 1, total: 1, events: [ev({ seq: '3', kind: 'uart.state', channel: 'aux', payload: { state: 'closed' } })], items: [
      { domain: 'uart', channel: 'dut', kind: 'uart.snapshot', state: { channel: 'dut', generation: 99, state: { state: 'closed' }, config: { baud: 4800 } } },
      { domain: 'uart', channel: 'aux', kind: 'uart.snapshot', state: { channel: 'aux', generation: 4, state: { state: 'open' }, config: { baud: 9600 }, input_owner: { owner: 'user', viewer: 'other', dirty: true } } },
    ] });
    h.snapshotEnd('10');
    expect(h.host.querySelector('.uart-port')?.textContent).toContain('open 9600 gen4');
    expect(h.host.querySelector('.uart-chip')?.textContent).toContain('다른');
    h.batch([ev({ seq: '11', kind: 'uart.state', channel: 'aux', payload: { state: 'closed' } })]);
    expect(h.host.querySelector('.uart-port')?.textContent).toContain('closed');
    expect(h.net.calls).toEqual([]);
  } finally { h.dispose(); }
});

describe('input-lease chip transitions', () => {
  const apply = (st: UartInputState, kind: string, payload: Record<string, unknown>, now = 0) =>
    reduceInput(st, ev({ kind, payload }), now);

  it('free → me → agent waiting → me → free', () => {
    let st = INITIAL_INPUT;
    expect(inputChip(st, 'c-me', false, 0)).toMatchObject({ kind: 'free', text: '입력권: 비어 있음' });
    st = apply(st, 'uart.input_owner', { owner: 'user', viewer: 'c-me', dirty: true, hold: false });
    expect(inputChip(st, 'c-me', false, 0)).toMatchObject({ kind: 'me', canYield: true, canClear: true });
    st = apply(st, 'uart.agent_activity', { kind: 'deferred_send', text: 'reboot⏎', elapsed_ms: 0, timeout_ms: 10_000 }, 1_000);
    expect(inputChip(st, 'c-me', false, 4_200)).toMatchObject({ kind: 'deferred', text: '⏳ 에이전트 대기 "reboot⏎" 3.2/10s' });
    st = apply(st, 'uart.agent_activity', { kind: 'idle' });
    expect(inputChip(st, 'c-me', false, 0).kind).toBe('me');
    st = apply(st, 'uart.input_owner', { owner: 'none', dirty: false, hold: false });
    expect(inputChip(st, 'c-me', false, 0).kind).toBe('free');
  });

  it('other viewer, dirty line, lock, HOLD and PAUSED priorities', () => {
    let st = apply(INITIAL_INPUT, 'uart.input_owner', { owner: 'user', viewer: 'c-other', dirty: true, hold: false });
    expect(inputChip(st, 'c-me', false, 0)).toMatchObject({ kind: 'other', canTakeover: true, canYield: false });
    st = apply(st, 'uart.input_owner', { owner: 'none', dirty: true, hold: false });
    expect(inputChip(st, 'c-me', false, 0)).toMatchObject({ kind: 'dirty', text: '미완결 줄 남음' });
    st = apply(st, 'uart.lock', { on: true, reason: 'u-boot env 수정', ttl_ms: 90_000 }, 0);
    expect(inputChip(st, 'c-me', false, 3_000)).toMatchObject({ kind: 'lock', text: '🔒 에이전트 잠금 "u-boot env 수정" 87s', canTakeover: true });
    st = apply(st, 'uart.hold', { on: true });
    expect(inputChip(st, 'c-me', false, 0).kind).toBe('hold');
    expect(inputChip(st, 'c-me', true, 0).kind).toBe('paused');
  });

  it('symbolizes invisible characters', () => {
    expect(symbolize('ls\r\n\t\x1b[0m\x03')).toBe('ls⏎␊⇥␛[0m^C');
  });

  it('LineAssembler joins lines across chunk boundaries', () => {
    const a = new LineAssembler();
    const enc = new TextEncoder();
    expect(a.push(enc.encode('Linux d'))).toEqual([]);
    expect(a.push(enc.encode('ut\r\nroot@'))).toEqual(['Linux dut']);
  });
});

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
});

describe('uart.terminal panel', () => {
  it('shows status, lease chip and last agent send from uart.* events', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([
      ev({ kind: 'uart.state', channel: 'dut', generation: 1, payload: { status: 'OPEN', baud: 115200 } }),
      { ...ev({ kind: 'uart.data', channel: 'dut', dir: 'tx', origin: 'agent', payload: { bytes: { encoding: 'hex', data: '756e616d65202d610d' } } }), request_id: 'r-88' },
      { ...ev({ kind: 'uart.write', channel: 'dut', origin: 'agent', payload: { write_state: 'sent' } }), request_id: 'r-88' },
      ev({ kind: 'uart.input_owner', channel: 'dut', payload: { owner: 'user', viewer: 'c-me', dirty: true, hold: false } }),
      ev({ kind: 'uart.data', channel: 'dut', dir: 'rx', payload: { bytes: { encoding: 'base64', data: btoa('Linux dut\r\n') } } }),
    ]);
    expect(h.host.querySelector('.uart-port')!.textContent).toContain('OPEN 115200 gen1');
    expect(h.host.querySelector('[data-testid="input-chip"]')!.textContent).toBe('입력권: 나 (입력 중)');
    const status = h.host.querySelector('[data-testid="uart-status"]')!.textContent!;
    expect(status).toContain('AGT ▶ "uname -a⏎" ◑보냄');
    expect(status).toContain('rx 11 B/s');
    expect(h.host.querySelector<HTMLElement>('[data-testid="uart-status"]')!.dataset).toMatchObject({ requestId: 'r-88', origin: 'agent', state: 'sent' });
  });

  it('U-2: tx bytes arriving after the write row keep its stage and request id', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([
      { ...ev({ kind: 'uart.write', channel: 'dut', origin: 'agent', payload: { write_state: 'sent', bytes: { encoding: 'hex', data: '6c730d' } } }), request_id: 'r-90' },
      { ...ev({ kind: 'uart.data', channel: 'dut', dir: 'tx', origin: 'agent', payload: { bytes: { encoding: 'hex', data: '6c730d' } } }), request_id: 'r-90' },
    ]);
    const st = h.host.querySelector<HTMLElement>('[data-testid="uart-status"]')!;
    expect(st.textContent).toContain('AGT ▶ "ls⏎" ◑보냄');
    expect(st.dataset).toMatchObject({ requestId: 'r-90', state: 'sent' });
  });

  it('palette Ctrl-C sends uart.input immediately with data_hex and input_seq', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    const btn = [...h.host.querySelectorAll('button')].find((b) => b.textContent === 'Ctrl-C')!;
    btn.click();
    const calls = h.net.calls.filter((c) => c.method === 'uart.input');
    expect(calls).toEqual([{ method: 'uart.input', params: { channel: 'dut', data_hex: '03', input_seq: 1 } }]);
  });

  it('watch rights: terminal readonly and lease buttons disabled; nothing is sent', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' }, { hello: { rights: 'watch' } });
    const btn = [...h.host.querySelectorAll('button')].find((b) => b.textContent === 'Ctrl-C')!;
    btn.click();
    expect(h.net.calls.filter((c) => c.method === 'uart.input')).toHaveLength(0);
    const take = [...h.host.querySelectorAll('button')].find((b) => b.textContent === '가져오기')!;
    expect(take.disabled).toBe(true);
    expect(h.host.querySelector('[data-testid="uart-status"]')!.textContent).toContain('보기 전용');
  });
});


describe('uart.trace live unterminated lines', () => {
  const data = (text: string, dir: 'rx' | 'tx' = 'rx') => ev({
    kind: 'uart.data', channel: 'dut', dir, origin: dir === 'rx' ? 'device' : 'agent',
    payload: { bytes: { encoding: 'base64', data: btoa(text) } },
  });
  const rows = () => [...h!.host.querySelectorAll<HTMLElement>('.vt-row')].filter((r) => !r.hidden);
  it('shows a split login prompt before LF and completes the same row once', () => {
    h = mountPanel('uart.trace', { channel: 'dut' });
    h.batch([data('log')]);
    const key = rows()[0]!.dataset.key;
    expect(rows()[0]!.textContent).toContain('log');
    expect(rows()[0]!.dataset.partial).toBe('true');
    h.batch([data('in: ')]);
    expect(rows()).toHaveLength(1);
    expect(rows()[0]!.textContent).toContain('login: ');
    h.batch([data('root', 'tx'), data('root\r\nPassword: ')]);
    expect(rows()).toHaveLength(3);
    expect(rows()[0]!.dataset.key).toBe(key);
    expect(rows()[0]!.textContent).toContain('login: root');
    expect(rows()[0]!.dataset.partial).toBe('false');
    expect(rows()[1]!.dataset.origin).toBe('agent');
    expect(rows()[2]!.textContent).toContain('Password: ');
  });
  it('does not join an unterminated line across a source gap in the same batch', () => {
    h = mountPanel('uart.trace', { channel: 'dut' });
    h.batch([data('before'), ev({ kind: 'source.gap', channel: 'dut', payload: { count: '1', cause: 'lost' } }), data('after')]);
    expect(h.host.textContent).not.toContain('beforeafter');
    expect(rows().some((r) => r.textContent?.includes('before…'))).toBe(true);
    expect(rows().some((r) => r.textContent?.includes('after') && r.dataset.partial === 'true')).toBe(true);
  });
});
