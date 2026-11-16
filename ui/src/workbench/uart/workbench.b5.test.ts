// U4 batch 5 — closing the LAST view of an open channel tells the user the port stays open (FUNCTIONS.md §B5.3).
// Opus-owned specification; Sonnet must not edit.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ShellServices } from '../../shell/services';
import { LAST_VIEW_TOAST_MS, UartBench, type TerminalHandle } from './workbench';

function services(open: string[]) {
  const channels = new Map(open.map((c) => [`uart/${c}`, { state: 'open', generation: 1 }]));
  return {
    shell: { latch: { on: false }, status: { writeLocked: false }, channels, subscribe: () => () => {}, noteLocalOpen: vi.fn() },
    stop: { phase: 'idle', subscribe: () => () => {} },
    gate: { run: vi.fn(async () => ({})) },
    toasts: { push: vi.fn() },
    showSlot: vi.fn(() => 'shown'),
    now: () => 1,
  };
}
const view = (channel: string): TerminalHandle =>
  ({
    channel,
    blocked: () => null,
    sendRaw: () => true,
    path: () => '/dev/x',
    frame: () => '',
    status: () => 'open',
    counters: () => ({ rx: 0, tx: 0 }),
    focus: () => {},
    activate: () => {},
    paneId: () => '1',
    isActive: () => false,
    visible: () => true,
    invalidate: () => {},
  }) as TerminalHandle;

let bench: UartBench;
let off: () => void;
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  off?.();
  vi.useRealTimers();
});
function setup(open: string[]) {
  const s = services(open);
  bench = new UartBench(null);
  off = bench.attach(s as unknown as ShellServices);
  return s;
}

describe('U4 last-view toast', () => {
  it('toasts after LAST_VIEW_TOAST_MS with [포트 닫기] and [다시 보기]', async () => {
    const s = setup(['dut']);
    const unregister = bench.registerTerminal(view('dut'));
    unregister();
    expect(s.toasts.push).not.toHaveBeenCalled();
    vi.advanceTimersByTime(LAST_VIEW_TOAST_MS);
    expect(s.toasts.push).toHaveBeenCalledTimes(1);
    const [text, o] = s.toasts.push.mock.calls[0] as unknown as [string, { key: string; actions: { label: string; run(): void }[] }];
    expect(text).toBe('dut 포트는 아직 열려 있습니다 (다른 도구가 쓰려면 닫으세요)');
    expect(o.key).toBe('uart-last-view-dut');
    expect(o.actions.map((a) => a.label)).toEqual(['포트 닫기', '다시 보기']);
    o.actions[1]!.run();
    expect(s.showSlot).toHaveBeenCalledWith({ panel: 'uart.terminal', props: { channel: 'dut' } });
    o.actions[0]!.run();
    await vi.runAllTimersAsync();
    expect(s.gate.run).toHaveBeenCalledWith(expect.objectContaining({ rpc: 'uart.close' }), { channel: 'dut' });
  });

  it('no toast while another view of the channel remains', () => {
    const s = setup(['dut']);
    const a = bench.registerTerminal(view('dut'));
    bench.registerTerminal(view('dut'));
    a();
    vi.advanceTimersByTime(LAST_VIEW_TOAST_MS * 2);
    expect(s.toasts.push).not.toHaveBeenCalled();
  });

  it('a view re-registered within the delay (moved / re-mounted) cancels it', () => {
    const s = setup(['dut']);
    bench.registerTerminal(view('dut'))();
    vi.advanceTimersByTime(LAST_VIEW_TOAST_MS - 50);
    bench.registerTerminal(view('dut'));
    vi.advanceTimersByTime(LAST_VIEW_TOAST_MS * 2);
    expect(s.toasts.push).not.toHaveBeenCalled();
  });

  it('no toast when the port is not open (closed first, or never open)', () => {
    const s = setup([]);
    bench.registerTerminal(view('dut'))();
    vi.advanceTimersByTime(LAST_VIEW_TOAST_MS * 2);
    expect(s.toasts.push).not.toHaveBeenCalled();
  });

  it('[포트 닫기] failure is reported, not swallowed', async () => {
    const s = setup(['dut']);
    s.gate.run.mockRejectedValueOnce(new Error('rights'));
    bench.registerTerminal(view('dut'))();
    vi.advanceTimersByTime(LAST_VIEW_TOAST_MS);
    const o = s.toasts.push.mock.calls[0]![1] as unknown as { actions: { run(): void }[] };
    o.actions[0]!.run();
    await vi.runAllTimersAsync();
    expect(s.toasts.push).toHaveBeenLastCalledWith('포트 닫기 실패: rights');
  });
});
