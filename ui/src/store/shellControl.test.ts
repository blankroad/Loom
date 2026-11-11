import { describe, expect, it } from 'vitest';
import { ActivityStore } from './activityStore';
import { computeLiveStatus, hasRight, HEARTBEAT_STALL_MS, HEARTBEAT_WARN_MS, ShellControlStore } from './shellControl';
import { ev, TEST_HELLO } from './testing/fakeNet';

const base = { connOpen: true, lastHeartbeatMs: 0, panels: [] as never[] };

it('does not carry active devices into a new daemon epoch, while preserving reconnect history', () => {
  const store = new ShellControlStore();
  store.setConn({ status: 'open', hello: TEST_HELLO }, 0);
  store.noteSnapshotItems([{ domain: 'uart', channel: 'only-uart', state: { state: 'open', generation: 2 } }]);
  store.setConn({ status: 'closed', reason: 'network' }, 1);
  store.setConn({ status: 'open', hello: { ...TEST_HELLO, conn_id: 'same-stream-new-viewer' } }, 2);
  expect(store.channels.get('uart/only-uart')?.state).toBe('open');
  store.setConn({ status: 'open', hello: { ...TEST_HELLO, stream_epoch: String(BigInt(TEST_HELLO.stream_epoch) + 1n) } }, 3);
  expect(store.channels.get('uart/only-uart')).toMatchObject({ state: 'unknown', generation: null });
  store.noteSnapshotItems([{ domain: 'eth', channel: 'only-eth', state: { state: 'live' } }]);
  expect([...store.channels.values()].filter((channel) => channel.state === 'open').map((channel) => channel.domain)).toEqual(['eth']);
});

it('keeps unseen interface connectivity unknown until explicit evidence, without auto-opening', () => {
  const store = new ShellControlStore();
  const opened: string[] = [];
  store.onChannelOpened(({ entry }) => opened.push(entry.channel));
  store.applyControl(ev({ domain: 'psu', channel: 'supply-a', kind: 'psu.state', payload: { out: false, voltage: 0 } }), 10_000);
  expect(store.channels.get('psu/supply-a')?.state).toBe('unknown');
  expect(opened).toEqual([]);
  store.applyControl(ev({ domain: 'psu', channel: 'supply-a', kind: 'psu.state', payload: { state: 'open', out: false } }), 10_001);
  expect(store.channels.get('psu/supply-a')?.state).toBe('open');
  expect(opened).toEqual(['supply-a']);
  store.applyControl(ev({ domain: 'psu', channel: 'supply-a', kind: 'psu.state', payload: { state: 'closed' } }), 10_002);
  store.applyControl(ev({ domain: 'psu', channel: 'supply-a', kind: 'psu.state', payload: { state: 'open' } }), 10_003);
  expect(opened).toEqual(['supply-a', 'supply-a']);
});

describe('computeLiveStatus thresholds', () => {
  it('LIVE right after a heartbeat, no warning dot', () => {
    const s = computeLiveStatus({ ...base, nowMs: 1_000 });
    expect(s).toMatchObject({ badge: 'LIVE', warnDot: false, writeLocked: false });
  });
  it('grey warning dot from 1.5 s, still writable', () => {
    expect(computeLiveStatus({ ...base, nowMs: HEARTBEAT_WARN_MS - 1 }).warnDot).toBe(false);
    const s = computeLiveStatus({ ...base, nowMs: HEARTBEAT_WARN_MS });
    expect(s).toMatchObject({ badge: 'LIVE', warnDot: true, writeLocked: false });
  });
  it('STALLED from 3 s and locks writes', () => {
    expect(computeLiveStatus({ ...base, nowMs: HEARTBEAT_STALL_MS - 1 }).badge).toBe('LIVE');
    const s = computeLiveStatus({ ...base, nowMs: HEARTBEAT_STALL_MS });
    expect(s).toMatchObject({ badge: 'STALLED', writeLocked: true });
  });
  it('STALLED when not connected, regardless of panels', () => {
    const s = computeLiveStatus({
      connOpen: false,
      lastHeartbeatMs: 0,
      nowMs: 10,
      panels: [{ lagMs: 0, targetMs: 100, degraded: false, visible: true }],
    });
    expect(s).toMatchObject({ badge: 'STALLED', writeLocked: true });
  });
  it('LAG when a visible panel exceeds its target; severe over 2 s; hidden panels ignored', () => {
    const p = (lagMs: number, visible = true) => ({ lagMs, targetMs: 150, degraded: false, visible });
    expect(computeLiveStatus({ ...base, nowMs: 10, panels: [p(100)] }).badge).toBe('LIVE');
    expect(computeLiveStatus({ ...base, nowMs: 10, panels: [p(400)] })).toMatchObject({ badge: 'LAG', lagSevere: false });
    expect(computeLiveStatus({ ...base, nowMs: 10, panels: [p(2_500)] })).toMatchObject({ badge: 'LAG', lagSevere: true });
    expect(computeLiveStatus({ ...base, nowMs: 10, panels: [p(9_000, false)] }).badge).toBe('LIVE');
  });
  it('LIVE-DEGRADED when a panel is degraded; STALLED outranks everything', () => {
    const deg = [{ lagMs: 0, targetMs: 150, degraded: true, visible: true }];
    expect(computeLiveStatus({ ...base, nowMs: 10, panels: deg }).badge).toBe('LIVE-DEGRADED');
    expect(computeLiveStatus({ ...base, nowMs: 5_000, panels: deg }).badge).toBe('STALLED');
  });
});

describe('ShellControlStore', () => {
  it('opens with hello env/rights and tracks heartbeat freshness via tick', () => {
    const s = new ShellControlStore();
    s.setConn({ status: 'open', hello: TEST_HELLO }, 1_000);
    expect(s.env).toBe('sim');
    expect(s.can('operate')).toBe(true);
    expect(s.can('approve')).toBe(false);
    s.tick(4_100);
    expect(s.status.badge).toBe('STALLED');
    s.applyControl(ev({ kind: 'core.heartbeat', payload: { record_health: 'failed', latch: false } }), 4_200);
    expect(s.status.badge).toBe('LIVE');
    expect(s.recordHealth).toBe('failed');
  });
  it('tracks latch, approvals (only when sent) and gaps', () => {
    const s = new ShellControlStore();
    expect(s.approvals.size).toBe(0);
    s.applyControl(ev({ kind: 'core.latch', payload: { on: true, missing: [], outputs_held: [{ domain: 'psu', channel: 'bench', label: '13.5 V', state: 'ON' }] } }), 0);
    expect(s.latch).toMatchObject({ on: true, acked: true });
    expect(s.latch.outputsHeld).toHaveLength(1);
    s.applyControl(ev({ kind: 'core.approval_requested', payload: { approval_id: 'a1', risk: 'R2' } }), 0);
    expect(s.approvals.has('a1')).toBe(true);
    s.applyControl(ev({ kind: 'core.approval_resolved', payload: { approval_id: 'a1' } }), 0);
    expect(s.approvals.size).toBe(0);
    s.applyControl(ev({ kind: 'display.gap', payload: { seq_range: { from: '1', to: '9' }, count: '9', cause: 'slow_client' } }), 0);
    expect(s.gaps[0]).toMatchObject({ kind: 'display.gap', count: 9, firstSeq: 1n });
    s.applyControl(ev({ kind: 'core.latch', payload: { on: false } }), 0);
    expect(s.latch.on).toBe(false);
  });
  it('rights are hierarchical', () => {
    expect(hasRight(new Set(['approve']), 'operate')).toBe(true);
    expect(hasRight(new Set(['watch']), 'operate')).toBe(false);
    expect(hasRight(new Set(), 'watch')).toBe(false);
  });
});

describe('ActivityStore', () => {
  it('is a bounded ring and marks user entries delivered by agent cursor', () => {
    const a = new ActivityStore(3);
    for (let i = 1; i <= 5; i++) a.applyEvent(ev({ kind: 'core.activity', seq: String(i), origin: i === 5 ? 'user' : 'agent', payload: { actor: 'claude-1', verb: 'read' } }));
    expect(a.size).toBe(3);
    expect(a.dropped).toBe(2);
    const latest = a.latest();
    expect(latest?.seq).toBe(5n);
    expect(a.isDelivered(latest!)).toBe(false);
    a.applyEvent(ev({ kind: 'core.agent_cursor', payload: { client_id: 'claude-1', from_seq: '1', to_seq: '5', others_count: 1 } }));
    expect(a.isDelivered(latest!)).toBe(true);
  });
});

describe('contract v1 alignment', () => {
  it('latch without `missing` is NOT treated as acked; missing:[] is', () => {
    const s = new ShellControlStore();
    s.applyControl(ev({ kind: 'core.latch', payload: { on: true } }), 0);
    expect(s.latch).toMatchObject({ on: true, acked: false });
    s.applyControl(ev({ kind: 'core.latch', payload: { on: true, missing: ['uart-worker'] } }), 0);
    expect(s.latch).toMatchObject({ acked: false, missing: ['uart-worker'] });
    s.applyControl(ev({ kind: 'core.latch', payload: { on: true, missing: [], approvals_cancelled: ['a1', 'a2'], driver_uncancellable: 2 } }), 0);
    expect(s.latch).toMatchObject({ acked: true, approvalsCancelled: 2 });
    expect(s.latch.uncancellable).toHaveLength(1);
  });
  it('hello latch:true shows an established (acked) latch; viewers from hello; single rights level', () => {
    const s = new ShellControlStore();
    s.setConn({ status: 'open', hello: { ...TEST_HELLO, latch: true, rights: 'watch', viewers: { subscribed: 3, visible: 2 } } }, 0);
    expect(s.latch).toMatchObject({ on: true, acked: true });
    expect(s.viewers).toEqual({ subscribed: 3, visible: 2 });
    expect(s.can('watch')).toBe(true);
    expect(s.can('operate')).toBe(false);
  });
  it('producer-only gap keeps stream seqs null (never invented), count "unknown" preserved', () => {
    const s = new ShellControlStore();
    s.applyControl(ev({ kind: 'source.gap', payload: { producer_range: { producer_id: 'can', producer_epoch: '1', from: '5', to: '9' }, count: 'unknown', cause: 'driver_overrun' } }), 0);
    expect(s.gaps[0]).toMatchObject({ kind: 'source.gap', firstSeq: null, lastSeq: null, count: 'unknown', producerOnly: true });
  });
});

describe('F1 automation rights', () => {
  it('uses granted operate only with an explicit current Auto policy declaration', () => {
    const s = new ShellControlStore();
    s.setAutomation(true);
    s.setConn({ status: 'open', hello: TEST_HELLO }, 0);
    expect(s.can('operate')).toBe(false);
    s.setConn({ status: 'open', hello: { ...TEST_HELLO, conn_id: 'auto', capabilities: ['core/1', 'policy/auto'] } }, 1);
    expect(s.can('operate')).toBe(true);
    s.applyControl(ev({ kind: 'core.policy_changed' }), 2);
    expect(s.can('operate')).toBe(false);
    s.setConn({ status: 'open', hello: { ...TEST_HELLO, conn_id: 'auto', capabilities: ['policy/auto'] } }, 3);
    expect(s.can('operate')).toBe(false);
    s.setConn({ status: 'open', hello: { ...TEST_HELLO, conn_id: 'fresh', capabilities: ['policy/auto'] } }, 4);
    expect(s.can('operate')).toBe(true);
  });
  it('never upgrades an Auto-policy watch viewer', () => {
    const s = new ShellControlStore();
    s.setAutomation(true);
    s.setConn({ status: 'open', hello: { ...TEST_HELLO, rights: 'watch', capabilities: ['policy/auto'] } }, 0);
    expect(s.can('operate')).toBe(false);
  });
});


describe('native late-join discovery without open panels', () => {
  it('hydrates core + reducer SnapshotItems silently and refreshes on reconnect', () => {
    const store = new ShellControlStore();
    const opened: string[] = [];
    store.onChannelOpened(({ entry }) => opened.push(entry.channel));
    store.setConn({ status: 'open', hello: TEST_HELLO }, 0);
    // Hub always includes core.channels, including string states, in core.activity's snapshot.
    const core = { domain: 'core', kind: 'core.channels', state: { channels: [
      { domain: 'uart', channel: 'dut', state: 'open', generation: 1, seq: '1', kind: 'uart.state' },
      { domain: 'uart', channel: 'aux', state: 'open', generation: 1, seq: '2', kind: 'uart.state' },
      { domain: 'can', channel: 'ch0', state: 'open', generation: 1, seq: '3', kind: 'can.channel_state' },
    ] } };
    const uds = (state: string, generation = 1) => ({ domain: 'uds', kind: 'uds.snapshot', state: {
      kind: 'uds.state', payload: { target: 'ecu', generation, link: { state }, backend: 'sim' },
    } });
    store.noteSnapshotItems([core, uds('up'), { domain: 'eth', kind: 'eth.snapshot', state: {
      kind: 'eth.capture_state', ch: 'cap', state: 'live', backend: 'sim', iface: 'lo', generation: 1,
    } }]);
    expect([...store.channels].map(([key, c]) => [key, c.state, c.generation, c.addedLiveMs])).toEqual([
      ['uart/dut', 'open', 1, null], ['uart/aux', 'open', 1, null], ['can/ch0', 'open', 1, null],
      ['uds/ecu', 'open', 1, null], ['eth/cap', 'open', 1, null],
    ]);
    store.noteSnapshotItems([core, uds('up')]);
    store.applyControl(ev({ domain: 'uds', channel: 'ecu', kind: 'uds.state', payload: { target: 'ecu', link: { state: 'up' } }, generation: 1 }), 10_000);
    expect(opened).toEqual([]);
    store.setConn({ status: 'connecting', attempt: 1 }, 11_000);
    store.setConn({ status: 'open', hello: { ...TEST_HELLO, conn_id: 'rejoined' } }, 12_000);
    store.noteSnapshotItems([uds('down', 2)]);
    expect(store.channels.get('uds/ecu')).toMatchObject({ state: 'closed', generation: 2, addedLiveMs: null });
    store.noteSnapshotItems([uds('up', 3)]);
    expect(store.channels.get('uds/ecu')).toMatchObject({ state: 'open', generation: 3 });
    expect(store.channels.size).toBe(5);
    expect(opened).toEqual([]);
  });
});
