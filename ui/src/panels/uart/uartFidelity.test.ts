// @vitest-environment jsdom
// CoolTerm-fidelity behaviors of uart.terminal (docs/impl/main/research/fidelity/uart.md):
// toolbar/status bar, Terminal vs HEX (original bytes), Send String through the existing gated path.
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import type { LoomEvent } from '../../sdk';
import './terminalPanel';
import type { ShellServices } from '../../shell/services';
import { uartBench, type Macro } from '../../workbench/uart/workbench';

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

describe('pane header (REV6.1 .phead: alias · port · frame · state · RX/TX)', () => {
  it('formats the reported config and keeps the existing port text', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort({ parity: 'even', stop_bits: '2', data_bits: 7 });
    expect(q('.uart-port').textContent).toContain('open 115200 gen2');
    expect(q('.ph-alias').textContent).toBe('dut');
    expect(q('.ph-port').textContent).toBe('/dev/ttyUSB0');
    expect(q('.ph-frame').textContent).toBe('115200 7E2 · none · CR');
    expect(q('.ph-state').textContent).toBe('연결됨');
    expect(q('.ph-state').classList.contains('connected')).toBe(true);
  });

  it('never invents a frame format that was not reported', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([ev({ kind: 'uart.state', channel: 'dut', payload: { state: 'open', baud: 9600 } })]);
    expect(q('.ph-frame').textContent).toBe('9600 · CR');
    expect(h.host.textContent).not.toContain('8-N-1');
    expect(h.host.textContent).not.toContain('8N1');
  });

  it('labels TX/RX counts as observed by this view and flags gaps', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    h.batch([data('rx', '4c696e75780d0a'), data('tx', '6c730d')]);
    expect(tid('uart-counts').textContent).toBe('RX 7 B · TX 3 B');
    expect(tid('uart-counts').title).toContain('장치·데몬 누계 아님');
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
    expect(q('.term.xslot').hidden).toBe(true);
    expect(tid('uart-bytes').hidden).toBe(false);
    const rows = [...h.host.querySelectorAll<HTMLElement>('.uart-bytes .vt-row')].filter((r) => !r.hidden);
    expect(rows.map((r) => r.dataset.dir)).toEqual(['rx', 'tx']);
    expect(rows[0]!.querySelector('.uart-hex')!.textContent!.trim()).toBe('1B 5B FF 41');
    expect(rows[0]!.querySelector('.uart-ascii')!.textContent).toBe('.[.A');
    expect(rows[1]!.querySelector('.uart-hex')!.textContent!.trim()).toBe('6C 73 0D');
    expect(tid('uart-view-hex').getAttribute('aria-pressed')).toBe('true');
    // switching views never talks to the daemon
    tid('uart-view-terminal').click();
    expect(q('.term.xslot').hidden).toBe(false);
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

const benchServices = () => {
  let rejectExpect: (e: unknown) => void = () => {};
  const calls: string[] = [];
  const s = {
    shell: { latch: { on: false }, status: { writeLocked: false }, channels: new Map([['uart/dut', { state: 'open', generation: 2 }]]), subscribe: () => () => {} },
    stop: { phase: 'idle', subscribe: () => () => {} },
    net: {
      call: (method: string) => {
        calls.push(method);
        return new Promise((_, rej) => { rejectExpect = rej; });
      },
    },
    toasts: { push: () => {} },
  };
  return { s: s as unknown as ShellServices, calls, reject: (e: unknown) => rejectExpect(e) };
};
const m: Macro = { id: 'm-ping', name: 'mping', hotkey: '', pinned: false, vars: [], steps: [{ type: 'send', text: 'mping', eol: 'none' }, { type: 'wait', text: 'ack:mping', ms: 2000 }], fixed: [] };
describe('macro run strip (REV6.1 .runstrip) — one row per channel, final state only', () => {
  // QA FINDING-MACRO-EXPECT: after an expect rejection the pane kept a stale '실행 중 · 1/2 ■ 중지' row above the 실패 row
  // and rows accumulated per run, while runningCount was 0.
  let detach: (() => void) | null = null;
  afterEach(() => {
    uartBench.dismissRun('dut');
    detach?.();
    detach = null;
  });
  const strips = () => [...h!.host.querySelectorAll<HTMLElement>('.runstrip')];
  const tops = () => [...h!.host.querySelectorAll<HTMLElement>('.runstrip .run-top')];

  it('a rejected wait replaces the running row with the single failed row (error visible, no stop button, no resend)', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    const f = benchServices();
    detach = uartBench.attach(f.s);
    for (let n = 1; n <= 2; n++) {
      expect(uartBench.requestRun(m, 'dut').started).toEqual(['dut']);
      h.frame();
      expect(tops()).toHaveLength(1);
      expect(strips()[0]!.dataset.status).toBe('running');
      await flush();
      h.frame();
      expect(inputCalls()).toHaveLength(n); // the send left once
      f.reject(new Error('uart.expect: bad patterns: unknown variant'));
      await flush();
      h.frame();
      expect(uartBench.runningCount).toBe(0);
      expect(strips()).toHaveLength(1);
      expect(tops()).toHaveLength(1); // run n replaced, never stacked under run n-1 or its own running row
      expect(strips()[0]!.dataset.status).toBe('failed');
      expect(h.host.querySelectorAll('.runstrip .steps')).toHaveLength(1);
      const text = strips()[0]!.textContent!;
      expect(text).toContain('실패');
      expect(text).toContain('bad patterns');
      expect(text).not.toContain('실행 중');
      expect(h.host.querySelector('.runstrip [data-act="stop-macro"]')).toBeNull();
    }
    expect(f.calls.filter((c) => c === 'uart.expect')).toHaveLength(2); // one per run, no retry
    expect(inputCalls()).toHaveLength(2);
  });

  it('STOP mid-wait leaves only the stopped row', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    openPort();
    const f = benchServices();
    detach = uartBench.attach(f.s);
    uartBench.requestRun(m, 'dut');
    await flush();
    h.frame();
    expect(strips()[0]!.dataset.status).toBe('running');
    uartBench.stopRun('dut');
    await flush();
    h.frame();
    expect(tops()).toHaveLength(1);
    expect(strips()[0]!.dataset.status).toBe('stopped');
    expect(strips()[0]!.textContent).not.toContain('실행 중');
    expect(inputCalls()).toHaveLength(1);
  });
});

describe('hidden dock tab (not drawn) still knows the engine port state — FINDING-UART-SCOPE-LAYOUT item 1', () => {
  // The RenderScheduler never draws a hidden tab; here "hidden" = events delivered with NO frame() afterwards.
  // Recipient resolution / writability must come from the engine-fed port model, not from the last draw.
  let detach: (() => void) | null = null;
  afterEach(() => {
    uartBench.dismissRun('dut');
    uartBench.ms = { mode: 'active', sel: [] };
    detach?.();
    detach = null;
  });
  const sub = () => h!.net.subs[0]!;
  const hiddenSnapshot = (state: Record<string, unknown>, through = '10') => {
    sub().h.onSnapshotPart?.({ sub: sub().id, n: 1, total: 1, events: [], items: [{ domain: 'uart', kind: 'uart.snapshot', channel: 'dut', state: { channel: 'dut', generation: 2, state, config: { baud: 115200 } } }] } as never);
    sub().h.onSnapshotEnd?.(through); // no frame(): the tab is not drawn
  };
  const portText = () => q('.uart-port').textContent ?? '';

  it('a CONNECTED session in a background tab is a recipient of 연결된 모두 / 선택 and receives the macro send', async () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    hiddenSnapshot({ state: 'open', path: '/dev/ttys007' });
    expect(portText()).not.toContain('open'); // proof the panel was never drawn after the snapshot
    uartBench.ms = { mode: 'all', sel: [] };
    expect(uartBench.resolveScope({ fixed: [] })).toMatchObject({ ids: ['dut'], excluded: [] });
    uartBench.ms = { mode: 'selected', sel: ['dut'] };
    expect(uartBench.resolveScope({ fixed: [] })).toMatchObject({ ids: ['dut'], excluded: [] });
    const f = benchServices();
    detach = uartBench.attach(f.s);
    expect(uartBench.requestRun({ ...m, steps: [{ type: 'send', text: 'mping', eol: 'none' }] }).started).toEqual(['dut']);
    await flush();
    expect(inputCalls()).toHaveLength(1); // sent through the panel's own uart.input path, still undrawn
    expect(uartBench.runs.get('dut')?.status).toBe('done');
  });

  it('a hidden tab still refuses: unknown (no engine report), later closed, read-only rights', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    uartBench.ms = { mode: 'all', sel: [] };
    // unknown is never promoted to open
    expect(uartBench.resolveScope({ fixed: [] }).excluded).toEqual([{ alias: 'dut', why: '상태 확인 중' }]);
    hiddenSnapshot({ state: 'open' });
    expect(uartBench.resolveScope({ fixed: [] }).ids).toEqual(['dut']);
    // a newer backlog/live row closes it while hidden
    h.net.emitBatch(0, [ev({ seq: '11', kind: 'uart.state', channel: 'dut', generation: 2, payload: { state: 'closed' } })]);
    expect(uartBench.resolveScope({ fixed: [] }).excluded).toEqual([{ alias: 'dut', why: '닫힘' }]);
    // an older row delivered later never re-opens it (seq guard shared with the drawn path)
    h.net.emitBatch(0, [ev({ seq: '10', kind: 'uart.state', channel: 'dut', generation: 2, payload: { state: 'open' } })]);
    expect(uartBench.resolveScope({ fixed: [] }).excluded).toEqual([{ alias: 'dut', why: '닫힘' }]);
    h.dispose();

    h = mountPanel('uart.terminal', { channel: 'dut' }, { hello: { rights: 'watch' } });
    hiddenSnapshot({ state: 'open' });
    expect(uartBench.resolveScope({ fixed: [] }).excluded).toEqual([{ alias: 'dut', why: '읽기 전용' }]);
    expect(inputCalls()).toHaveLength(0);
  });

  it('when the hidden tab is shown later it draws the same state (no double application)', () => {
    h = mountPanel('uart.terminal', { channel: 'dut' });
    hiddenSnapshot({ state: 'open' });
    h.net.emitBatch(0, [ev({ seq: '12', kind: 'uart.config', channel: 'dut', generation: 2, payload: { baud: 9600, data_bits: 8, parity: 'none', stop_bits: '1', flow: 'none' } })]);
    h.frame();
    expect(portText()).toContain('open 9600 gen2');
    expect(q('.ph-state').textContent).toBe('연결됨');
  });
});
