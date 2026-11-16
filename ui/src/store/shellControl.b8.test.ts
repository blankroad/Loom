// UART B8 (operator-upgrade U8, ROOT R4): "re-open" means this tab saw the channel open before, and opens this tab
// asked for itself are flagged `local`. Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B8 (lead-written).
import { describe, expect, it } from 'vitest';
import { LOCAL_OPEN_TTL_MS, MAX_LOCAL_OPENS, ShellControlStore, type ChannelOpened } from './shellControl';
import { ev } from './testing/fakeNet';

function store() {
  const s = new ShellControlStore();
  const seen: ChannelOpened[] = [];
  s.onChannelOpened((o) => seen.push(o));
  return { s, seen };
}
const open = (channel: string, extra: Record<string, unknown> = {}) =>
  ev({ domain: 'uart', channel, kind: 'uart.state', origin: 'device', payload: { state: 'open' }, ...extra });
const closed = (channel: string) => ev({ domain: 'uart', channel, kind: 'uart.state', origin: 'device', payload: { state: 'closed' } });

describe('UART B8: new vs re-open', () => {
  it('a channel known before its first open is still new', () => {
    const { s, seen } = store();
    s.applyControl(ev({ domain: 'uart', channel: 'dut', kind: 'uart.config', origin: 'device', payload: { baud: 115200 } }), 10_000);
    s.applyControl(open('dut'), 10_001);
    expect(seen.map((o) => o.isNew)).toEqual([true]);
    expect(s.channels.get('uart/dut')?.everOpen).toBe(true);
  });

  it('open → closed → open is a re-open', () => {
    const { s, seen } = store();
    s.applyControl(open('dut'), 10_000);
    s.applyControl(closed('dut'), 10_001);
    s.applyControl(open('dut'), 10_002);
    expect(seen.map((o) => o.isNew)).toEqual([true, false]);
  });
});

describe('UART B8: noteLocalOpen', () => {
  it('flags the next open of that channel once', () => {
    const { s, seen } = store();
    s.noteLocalOpen('uart', 'dut', 10_000);
    s.applyControl(open('dut'), 10_100);
    s.applyControl(closed('dut'), 10_200);
    s.applyControl(open('dut'), 10_300);
    expect(seen.map((o) => o.local)).toEqual([true, false]);
  });

  it('is per domain and channel', () => {
    const { s, seen } = store();
    s.noteLocalOpen('uart', 'dut', 10_000);
    s.noteLocalOpen('can', 'other', 10_000);
    s.applyControl(open('other'), 10_100);
    s.applyControl(open('dut'), 10_100);
    expect(seen.map((o) => [o.entry.channel, o.local])).toEqual([['other', false], ['dut', true]]);
  });

  it('expires after LOCAL_OPEN_TTL_MS', () => {
    const { s, seen } = store();
    s.noteLocalOpen('uart', 'dut', 10_000);
    s.applyControl(open('dut'), 10_000 + LOCAL_OPEN_TTL_MS + 1);
    expect(seen.map((o) => o.local)).toEqual([false]);
  });

  it('keeps at most MAX_LOCAL_OPENS pending notes (oldest dropped)', () => {
    const { s, seen } = store();
    for (let i = 0; i <= MAX_LOCAL_OPENS; i++) s.noteLocalOpen('uart', `c${i}`, 10_000 + i);
    s.applyControl(open('c0'), 11_000);
    s.applyControl(open(`c${MAX_LOCAL_OPENS}`), 11_000);
    expect(seen.map((o) => o.local)).toEqual([false, true]);
  });
});
