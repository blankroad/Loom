// @vitest-environment jsdom
// CoolTerm-fidelity behaviors of uart.terminal (docs/impl/main/research/fidelity/uart.md):
// toolbar/status bar, Terminal vs HEX (original bytes), Send String through the existing gated path.
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import type { LoomEvent } from '../../sdk';
import './terminalPanel';

beforeAll(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
});

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
});

const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T & HTMLElement>(sel)!;
const tid = <T extends Element = HTMLElement>(id: string) => q<T>(`[data-testid="${id}"]`);
const flush = () => new Promise((r) => setTimeout(r, 0));
const inputCalls = () => h!.net.calls.filter((c) => c.method === 'uart.input');
const data = (dir: 'rx' | 'tx', hex: string, origin: LoomEvent['origin'] = dir === 'rx' ? 'device' : 'user'): LoomEvent =>
  ev({ kind: 'uart.data', channel: 'dut', dir, origin, payload: { bytes: { encoding: 'hex', data: hex } } });
const openPort = (extra: Record<string, unknown> = {}) =>
  h!.batch([
    ev({ kind: 'uart.state', channel: 'dut', generation: 2, payload: { state: 'open', path: '/dev/ttyUSB0', transport: 'serial' } }),
    ev({ kind: 'uart.config', channel: 'dut', generation: 2, payload: { baud: 115200, data_bits: 8, parity: 'none', stop_bits: '1', flow: 'none', ...extra } }),
  ]);
const typeInto = (el: HTMLTextAreaElement, v: string) => {
  el.value = v;
  el.dispatchEvent(new Event('input'));
};

describe('status bar (CoolTerm "port / 115200 8-N-1")', () => {
  it('formats the reported config and keeps the existing port text', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort({ parity: 'even', stop_bits: '2', data_bits: 7 });
    expect(q('.uart-port').textContent).toContain('open 115200 gen2');
    expect(q('.uart-cfg').textContent).toBe('/dev/ttyUSB0 / 7-E-2 · 흐름 none');
  });

  it('never invents a frame format that was not reported', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([ev({ kind: 'uart.state', channel: 'dut', payload: { state: 'open', baud: 9600 } })]);
    expect(q('.uart-cfg').hidden).toBe(true);
    expect(h.host.textContent).not.toContain('8-N-1');
  });

  it('labels TX/RX counts as observed by this view and flags gaps', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([data('rx', '4c696e75780d0a'), data('tx', '6c730d')]);
    expect(tid('uart-counts').textContent).toBe('관측 TX 3 B / RX 7 B');
    h.batch([ev({ kind: 'source.gap', channel: 'dut', payload: { count: '1', cause: 'lost' } })]);
    expect(tid('uart-counts').textContent).toContain('누락 구간 있음');
  });
});

describe('Terminal vs HEX view', () => {
  it('HEX view shows the original bytes (control codes and invalid UTF-8 intact), RX and TX on separate rows', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([data('rx', '1b5bff41'), data('tx', '6c730d')]);
    tid('uart-view-hex').click();
    h.frame();
    expect(q('.uart-body').hidden).toBe(true);
    expect(tid('uart-bytes').hidden).toBe(false);
    const rows = [...h.host.querySelectorAll<HTMLElement>('.uart-bytes .vt-row')].filter((r) => !r.hidden);
    expect(rows.map((r) => r.dataset.dir)).toEqual(['rx', 'tx']);
    expect(rows[0]!.querySelector('.uart-hex')!.textContent!.trim()).toBe('1B 5B FF 41');
    expect(rows[0]!.querySelector('.uart-ascii')!.textContent).toBe('.[.A');
    expect(rows[1]!.querySelector('.uart-hex')!.textContent!.trim()).toBe('6C 73 0D');
    expect(tid('uart-view-hex').getAttribute('aria-pressed')).toBe('true');
    // switching views never talks to the daemon
    tid('uart-view-terminal').click();
    expect(q('.uart-body').hidden).toBe(false);
    expect(h.net.calls).toEqual([]);
  });
});

describe('clear actions stay distinct', () => {
  it('화면 지우기 is local only; 미완결 줄 지우기 is the explicit server uart.input.clear', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([data('rx', '41424344')]);
    tid('uart-clear-view').click();
    h.frame();
    expect(h.net.calls).toEqual([]);
    tid('uart-view-hex').click();
    h.frame();
    expect([...h.host.querySelectorAll<HTMLElement>('.uart-bytes .vt-row')].filter((r) => !r.hidden)).toHaveLength(0);
    expect(tid('uart-counts').textContent).toContain('RX 4 B'); // observed totals are not a view buffer
    const serverClear = tid<HTMLButtonElement>('uart-input-clear');
    expect(serverClear.textContent).toBe('미완결 줄 지우기');
    expect(serverClear.title).toContain('uart.input.clear');
    expect(tid('uart-clear-view').textContent).not.toContain('미완결');
  });
});

describe('Send String (Text/HEX + explicit line ending) via the existing gated input path', () => {
  const openDrawer = () => {
    tid('uart-send-string-toggle').click();
    h!.frame();
    return {
      area: tid<HTMLTextAreaElement>('uart-ss-input') as HTMLTextAreaElement,
      send: tid<HTMLButtonElement>('uart-ss-send') as HTMLButtonElement,
      ending: tid<HTMLSelectElement>('uart-ss-ending') as HTMLSelectElement,
    };
  };

  it('text + CR sends exactly the previewed bytes through uart.input with the shared input_seq', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    [...h.host.querySelectorAll('button')].find((b) => b.textContent === 'Ctrl-C')!.click(); // input_seq 1 (xterm path)
    const { area, send, ending } = openDrawer();
    expect(tid('uart-send-string').getAttribute('aria-label')).toBe('Send String (dut)');
    expect(ending.value).toBe('cr');
    typeInto(area, 'ls');
    expect(tid('uart-ss-preview').textContent).toContain('3 B · 6C 73 0D');
    send.click();
    await flush();
    expect(inputCalls().at(-1)).toEqual({ method: 'uart.input', params: { channel: 'dut', data_hex: '6c730d', input_seq: 2 } });
  });

  it('HEX mode defaults to no ending; Shift+Enter sends; invalid HEX disables Send', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    const { area, send, ending } = openDrawer();
    const hexRadio = tid<HTMLInputElement>('uart-ss-mode-hex') as HTMLInputElement;
    hexRadio.checked = true;
    hexRadio.dispatchEvent(new Event('change'));
    expect(ending.value).toBe('none');
    typeInto(area, '54 6');
    expect(send.disabled).toBe(true);
    expect(tid('uart-ss-preview').dataset.state).toBe('error');
    typeInto(area, '54 68 00 FF');
    area.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true }));
    await flush();
    expect(inputCalls().map((c) => (c.params as { data_hex: string }).data_hex)).toEqual(['546800ff']);
  });

  it('> 4 KiB goes through the inputLarge confirmation (preview says so)', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    const { area, send } = openDrawer();
    typeInto(area, 'x'.repeat(5000));
    expect(tid('uart-ss-preview').textContent).toContain('확인 카드');
    send.click();
    await flush();
    await flush();
    const last = inputCalls().at(-1)!.params as { data_hex: string };
    expect(last.data_hex).toHaveLength((5000 + 1) * 2);
  });

  it('is blocked when the port is not open and under watch rights — nothing is sent', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([ev({ kind: 'uart.state', channel: 'dut', payload: { state: 'closed' } })]);
    let d = openDrawer();
    typeInto(d.area, 'ls');
    expect(d.send.disabled).toBe(true);
    expect(tid('uart-ss-preview').textContent).toContain('전송 불가: 포트 closed');
    d.send.click();
    h.dispose();

    h = mountPanel('uart.terminal', { channel: 'dut' }, { hello: { rights: 'watch' } });
    openPort();
    d = openDrawer();
    typeInto(d.area, 'ls');
    expect(d.send.disabled).toBe(true);
    d.area.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true }));
    await flush();
    expect(inputCalls()).toHaveLength(0);
  });
});
