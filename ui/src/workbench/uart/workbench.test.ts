// @vitest-environment jsdom
// Focused regression: macro execution boundaries (ROOT review) — STOP/abort/generation re-check right before the
// bytes leave, pre-registered waits consumed, REV6.1 recipient rules (fixed-unresolved blocks the whole macro,
// ineligible recipients excluded with reasons), no retries.
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ShellServices } from '../../shell/services';
import { TerminalRegistry, UartBench, type Macro, type TerminalHandle } from './workbench';

function fakeServices() {
  const listeners = new Set<() => void>();
  const stopListeners = new Set<() => void>();
  const channels = new Map<string, { state: string; generation: number | null }>();
  const calls: { method: string; params: Record<string, unknown> }[] = [];
  const expectResolvers: ((v: unknown) => void)[] = [];
  const s = {
    shell: {
      latch: { on: false },
      status: { writeLocked: false },
      channels,
      subscribe: (cb: () => void) => { listeners.add(cb); return () => listeners.delete(cb); },
    },
    stop: { phase: 'idle', subscribe: (cb: () => void) => { stopListeners.add(cb); return () => stopListeners.delete(cb); } },
    net: {
      call: vi.fn((method: string, params: Record<string, unknown>, opts?: { signal?: AbortSignal }) => {
        calls.push({ method, params });
        return new Promise((resolve, reject) => {
          expectResolvers.push(resolve);
          opts?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
        });
      }),
    },
    gate: { run: vi.fn(async () => ({})) },
    toasts: { push: vi.fn() },
  };
  return {
    s,
    calls,
    expectResolvers,
    emit: () => { for (const l of [...listeners]) l(); },
    emitStop: () => { for (const l of [...stopListeners]) l(); },
    open: (ch: string, generation = 1) => channels.set(`uart/${ch}`, { state: 'open', generation }),
  };
}

function term(channel: string, sent: string[], blocked: () => string | null = () => null): TerminalHandle {
  return {
    channel,
    blocked,
    sendRaw: (b) => { if (blocked()) return false; sent.push(`${channel}:${new TextDecoder().decode(b)}`); return true; },
    path: () => `/dev/${channel}`,
    frame: () => '115200 8N1',
    status: () => 'open',
    counters: () => ({ rx: 0, tx: 0 }),
    focus: () => {},
    activate: () => {},
    paneId: () => '1',
    isActive: () => channel === 'mcu',
    visible: () => true,
    invalidate: () => {},
  };
}
const macro = (steps: Macro['steps'], fixed: string[] = []): Macro => ({ id: 'm1', name: 'M', hotkey: '', pinned: false, vars: [], steps, fixed });
const flush = () => new Promise((r) => setTimeout(r, 5));
const unhandled: unknown[] = [];
const onUnhandled = (e: unknown) => unhandled.push(e);
process.on('unhandledRejection', onUnhandled);
afterEach(() => { const u = unhandled.splice(0).map((e) => String(e instanceof Error ? e.stack?.split("\n").slice(0, 4).join(" | ") : e)); expect(u).toEqual([]); });

describe('macro send boundaries', () => {
  it('STOP during the send→wait registration yield sends nothing and consumes the pre-registered wait', async () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    f.open('mcu');
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    b.requestRun(macro([{ type: 'send', text: 'status', eol: 'CR' }, { type: 'wait', text: 'OK', ms: 1000 }]));
    expect(f.calls.map((c) => c.method)).toEqual(['uart.expect']); // registered first
    f.s.stop.phase = 'requesting'; // STOP clicked inside the yield
    f.emitStop();
    await flush();
    expect(sent).toEqual([]);
    expect(b.runs.get('mcu')).toMatchObject({ status: 'stopped', reason: 'STOP으로 중단' });
  });

  it('a new connection generation of the same alias never continues the run', async () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    f.open('mcu', 3);
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    b.requestRun(macro([{ type: 'send', text: 'a', eol: 'none' }, { type: 'delay', ms: 20 }, { type: 'send', text: 'b', eol: 'none' }]));
    await flush();
    expect(sent).toEqual(['mcu:a']);
    f.open('mcu', 4); // closed and re-opened during the delay
    await new Promise((r) => setTimeout(r, 40));
    expect(sent).toEqual(['mcu:a']);
    expect(b.runs.get('mcu')).toMatchObject({ status: 'stopped', reason: '다시 연결됨 — 이어서 보내지 않음' });
  });

  it('a wait verdict comes only from the engine `matched` flag; a timeout fails without retry', async () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    f.open('mcu');
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    b.requestRun(macro([{ type: 'send', text: 'x', eol: 'none' }, { type: 'wait', text: 'OK', ms: 500 }, { type: 'send', text: 'y', eol: 'none' }]));
    await flush();
    f.expectResolvers[0]!({ matched: false });
    await flush();
    expect(sent).toEqual(['mcu:x']);
    expect(b.runs.get('mcu')).toMatchObject({ status: 'failed' });
    expect(f.calls.filter((c) => c.method === 'uart.expect')).toHaveLength(1);
    expect(f.calls[0]!.params).toMatchObject({ channel: 'mcu', from: 'now', timeout_ms: 500, patterns: [{ kind: 'literal', pattern: 'OK' }] });
  });

  it('wait sends the daemon/CLI pattern contract exactly: {kind:"literal"} (PatternKind = literal|regex|prompt)', async () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    f.open('mcu');
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    b.requestRun(macro([{ type: 'send', text: 'mping', eol: 'none' }, { type: 'wait', text: 'ack:mping', ms: 2000 }]));
    await flush();
    const ex = f.calls.filter((c) => c.method === 'uart.expect');
    expect(ex).toHaveLength(1);
    // whole-object equality: no extra/unknown fields; same shape as crates/loom-uart/src/cli.rs patterns_json
    expect(ex[0]!.params).toEqual({ channel: 'mcu', patterns: [{ kind: 'literal', pattern: 'ack:mping' }], from: 'now', timeout_ms: 2000 });
    expect(['literal', 'regex', 'prompt']).toContain((ex[0]!.params.patterns as { kind: string }[])[0]!.kind);
    f.expectResolvers[0]!({ matched: true });
    await flush();
    expect(b.runs.get('mcu')).toMatchObject({ status: 'done' });
    expect(sent).toEqual(['mcu:mping']);
  });

  it('an engine rejection of the wait fails the run once: error kept, no retry, no later sends, nothing left running', async () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    f.open('mcu');
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    let rejectExpect: (e: unknown) => void = () => {};
    f.s.net.call.mockImplementationOnce((method: string, params: Record<string, unknown>) => {
      f.calls.push({ method, params });
      return new Promise((_, rej) => { rejectExpect = rej; });
    });
    b.requestRun(macro([{ type: 'send', text: 'x', eol: 'none' }, { type: 'wait', text: 'OK', ms: 500 }, { type: 'send', text: 'y', eol: 'none' }]));
    await flush();
    expect(b.runningCount).toBe(1);
    rejectExpect(new Error('uart.expect: bad patterns: unknown variant'));
    await flush();
    const r = b.runs.get('mcu')!;
    expect(r.status).toBe('failed');
    expect(r.err).toContain('bad patterns');
    expect(r.steps.map((s) => s.st)).toEqual(['ok', 'fail', 'skip']);
    expect(b.runningCount).toBe(0);
    expect(sent).toEqual(['mcu:x']);
    expect(f.calls.filter((c) => c.method === 'uart.expect')).toHaveLength(1);
  });

  it('a refused send consumes the pending wait (no unhandled rejection, no leaked waiter)', async () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    f.open('mcu');
    const sent: string[] = [];
    let ro = false;
    b.registerTerminal(term('mcu', sent, () => (ro ? '읽기 전용' : null)));
    b.requestRun(macro([{ type: 'send', text: 'x', eol: 'none' }, { type: 'wait', text: 'OK', ms: 500 }]));
    ro = true;
    await flush();
    expect(sent).toEqual([]);
    expect(b.runs.get('mcu')?.status).toBe('stopped');
  });
});

describe('REV6.1 recipient rules', () => {
  it('a fixed macro with an unresolved session is blocked as a whole — nothing is sent, not even to known targets', () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    b.sessions = [{ id: 's-aux', alias: 'aux', group: '', port: '/dev/aux', baud: 9600, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', ts: true, color: '#0969da' }];
    f.open('aux');
    const sent: string[] = [];
    b.registerTerminal(term('aux', sent));
    const r = b.requestRun(macro([{ type: 'send', text: 'x', eol: 'none' }], ['s-aux', 's-gone']));
    expect(r.started).toEqual([]);
    expect(r.refused).toContain('실행 차단');
    expect(sent).toEqual([]);
  });

  it('연결된 모두 / 선택 exclude ineligible recipients with reasons and never widen', async () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    for (const c of ['mcu', 'aux', 'dbg']) f.open(c);
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    b.registerTerminal(term('aux', sent));
    b.registerTerminal(term('dbg', sent, () => '읽기 전용'));
    b.ms = { mode: 'all', sel: [] };
    const all = b.resolveScope({ fixed: [] });
    expect(all.ids).toEqual(['aux', 'mcu']);
    expect(all.excluded).toEqual([{ alias: 'dbg', why: '읽기 전용' }]);
    b.ms = { mode: 'selected', sel: ['aux', 'gone'] };
    const sel = b.resolveScope({ fixed: [] });
    expect(sel.ids).toEqual(['aux']);
    expect(sel.excluded).toEqual([{ alias: 'gone', why: '열려 있지 않음' }]);
    b.requestRun(macro([{ type: 'send', text: 'z', eol: 'none' }]));
    await flush();
    expect(sent).toEqual(['aux:z']);
  });

  it('refuses to start while STOP is latched', () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    f.open('mcu');
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    f.s.shell.latch.on = true;
    expect(b.requestRun(macro([{ type: 'send', text: 'x', eol: 'none' }])).refused).toContain('STOP');
    expect(sent).toEqual([]);
  });

  it('keyboard broadcast freezes recipients and maps Enter to each target EOL', () => {
    const f = fakeServices();
    const b = new UartBench(null);
    b.attach(f.s as unknown as ShellServices);
    const sent: string[] = [];
    b.registerTerminal(term('mcu', sent));
    b.registerTerminal(term('aux', sent));
    b.tab.aux = { eol: 'CRLF' };
    b.setBroadcast(true);
    b.registerTerminal(term('late', sent)); // opened after: never joins
    expect(b.routeKeys('mcu', new Uint8Array([0x0d]), (ch, x) => b.mapEnter(ch, x))).toBe(true);
    expect(sent.sort()).toEqual(['aux:\r\n', 'mcu:\r']);
  });
});

describe('terminal views across retained workspaces', () => {
  it('one channel shown in two docks resolves to one preferred view; only the last view closing unregisters it', () => {
    const reg = new TerminalRegistry();
    const sent: string[] = [];
    const hidden = { ...term('lnx', sent), visible: () => false, isActive: () => false };
    const front = { ...term('lnx', sent), visible: () => true, isActive: () => false };
    reg.add(front);
    reg.add(hidden); // registered later, but hidden (its dock is behind another tab)
    expect(reg.get('lnx')).toBe(front);
    expect(reg.values()).toEqual([front]); // macros/broadcast address the channel once
    expect(reg.all('lnx')).toHaveLength(2);
    expect(reg.remove(front)).toBe(false); // the other workspace still shows it
    expect(reg.get('lnx')).toBe(hidden);
    expect(reg.remove(hidden)).toBe(true);
    expect(reg.get('lnx')).toBeUndefined();
  });
});
