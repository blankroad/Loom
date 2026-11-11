import { describe, expect, it, vi } from 'vitest';
import type { EventsBatch, LoomEvent } from '../proto/provisional';
import type { ConnState, NetClient, SubscriptionHandlers } from './api';
import type { DeliveryTiming } from './client';
import { backoffMs, type CoreDeps, type SocketEvents } from './core';
import { createNetClient, devWsUrl, stopUrlFor } from './index';
import { parseInbound, type PresenceExtra } from './wire';
import { ev, ManualTimers, range, tick } from './testing/fixtures';

interface Req {
  id: number;
  method: string;
  params?: Record<string, unknown>;
}

const HELLO = { role: 'viewer', rights: 'operate', conn_id: 'c1', stream_epoch: '1', server: { version: 'mock', build: 'x' }, env: 'sim', latch: false, viewers: { subscribed: 1, visible: 1 }, capabilities: [] };

function harness() {
  const timers = new ManualTimers();
  const sockets: { ev: SocketEvents; sent: Req[]; closed: boolean }[] = [];
  let clock = 1_762_000_000_000;
  const deps: CoreDeps = {
    openSocket: (_url, e) => {
      const s = { ev: e, sent: [] as Req[], closed: false };
      sockets.push(s);
      return { send: (t) => s.sent.push(JSON.parse(t) as Req), close: () => void (s.closed = true) };
    },
    now: () => (clock += 0.25),
    timers,
  };
  const frames: (() => void)[] = [];
  const fetchMock = vi.fn((...args: [string | URL | Request, RequestInit?]) => Promise.resolve(new Response(args.length ? '{}' : '', { status: 200 })));
  const net = createNetClient('ws://localhost:7421/rpc', {
    inline: true,
    deps,
    frames: { request: (cb) => frames.push(cb) },
    fetch: fetchMock as unknown as typeof fetch,
  });
  const states: ConnState['status'][] = [];
  net.onState((s) => states.push(s.status));
  const control: LoomEvent[] = [];
  net.onControl((e) => control.push(e));
  const sock = () => sockets[sockets.length - 1]!;
  const send = (msg: unknown) => sock().ev.onText(JSON.stringify(msg));
  const reply = (id: number, result: unknown) => send({ jsonrpc: '2.0', id, result });
  const pending = (method: string) => sock().sent.filter((r) => r.method === method);
  /** answer every unanswered request of `method` (core.time gets d1/d2) */
  const answered = new Set<string>();
  const answerAll = async (method: string, result: (r: Req) => unknown) => {
    for (let i = 0; i < 20; i++) {
      const open = pending(method).filter((r) => !answered.has(`${sockets.length}:${r.id}`));
      if (!open.length) return;
      for (const r of open) {
        answered.add(`${sockets.length}:${r.id}`);
        reply(r.id, result(r));
      }
      await tick();
    }
  };
  const open = async () => {
    await tick();
    sock().ev.onOpen();
    await tick();
    await answerAll('core.hello', () => HELLO);
    await tick();
  };
  const frame = async () => {
    const f = frames.splice(0);
    f.forEach((cb) => cb());
    await tick();
  };
  return { net, timers, sockets, sock, send, reply, pending, answerAll, open, frame, states, control, fetchMock };
}

async function until(fn: () => boolean) {
  for (let i = 0; i < 50 && !fn(); i++) await tick();
  expect(fn()).toBe(true);
}

describe('NetCore + MainNetClient (inline)', () => {
  it('backoff is 0.5 -> 8 s', () => {
    expect([1, 2, 3, 4, 5, 6, 9].map(backoffMs)).toEqual([500, 1000, 2000, 4000, 8000, 8000, 8000]);
  });

  it('core.hello -> open, and window-less DEV exposure is skipped safely', async () => {
    const h = harness();
    await h.open();
    const hello = h.sockets[0]!.sent[0]!;
    expect(hello.method).toBe('core.hello');
    expect(hello.params).toMatchObject({ proto: 'loom/1.0', want_role: 'viewer', client: { kind: 'web', tty: false } });
    expect(hello.params).not.toHaveProperty('token');
    expect(h.net.state).toMatchObject({ status: 'open', hello: { rights: 'operate', env: 'sim' } });
  });

  it('rejects calls before open (no queueing) and owns core.subscribe', async () => {
    const h = harness();
    await expect(h.net.call('core.stop_all')).rejects.toMatchObject({ code: 'not_connected' });
    await h.open();
    await expect(h.net.call('core.subscribe', {})).rejects.toMatchObject({ code: 'rpc', rpcCode: -32601 });
  });

  it('control sub + heartbeat reach onControl immediately; duplicates on a data sub are deduped', async () => {
    const h = harness();
    await h.open();
    const latch = ev(5, 'core.latch', { payload: { on: true } });
    h.send({ jsonrpc: '2.0', method: 'core.events', params: { sub: 'control', first: '5', last: '5', lag_ms: 0, events: [latch] } });
    h.send({ jsonrpc: '2.0', method: 'core.heartbeat', params: { stream_seq_hw: '5', daemon_mono_ns: '1', daemon_wall_ns: '1', record_health: 'ok', latch: true } });
    await tick();
    expect(h.control.map((e) => e.kind)).toEqual(['core.latch', 'core.heartbeat']);
    // same latch also arrives on a data subscription
    const batches: EventsBatch[] = [];
    h.net.subscribe({ domains: ['core'] }, { snapshot: false }, { onBatch: (b) => batches.push(b) });
    await tick();
    await h.answerAll('core.subscribe', () => ({ sub_id: 'd1', stream_epoch: '1' }));
    h.send({ jsonrpc: '2.0', method: 'core.events', params: { sub: 'd1', first: '5', last: '5', lag_ms: 0, events: [latch] } });
    await h.frame();
    expect(h.control.filter((e) => e.kind === 'core.latch')).toHaveLength(1);
    expect(batches[0]!.events[0]!.kind).toBe('core.latch'); // still in-band for the domain store
  });

  it('data is pull-paced: ≤ 1 delivery per frame, merged per subscription', async () => {
    const h = harness();
    await h.open();
    const batches: EventsBatch[] = [];
    h.net.subscribe({ domains: ['can'] }, { snapshot: false }, { onBatch: (b) => batches.push(b) });
    await tick();
    await h.answerAll('core.subscribe', () => ({ sub_id: 'd1', stream_epoch: '1' }));
    await h.frame(); // first pull answered by the bind
    const note = (a: number, b: number) => ({ jsonrpc: '2.0', method: 'core.events', params: { sub: 'd1', first: String(a), last: String(b), lag_ms: 0, events: range(a, b) } });
    h.send(note(1, 2)); // idle pull waiting -> immediate delivery
    await tick();
    h.send(note(3, 4));
    h.send(note(5, 6));
    await tick();
    expect(batches.map((b) => b.events.length)).toEqual([2]);
    await h.frame();
    expect(batches.map((b) => `${b.first}-${b.last}`)).toEqual(['1-2', '3-6']);
  });

  it('disconnect: in-flight mutation fails as disconnected and is NOT re-sent; reconnect backs off and resumes', async () => {
    const h = harness();
    await h.open();
    const sub = h.net.subscribe({ domains: ['uart'] }, { snapshot: true }, { onBatch: () => undefined });
    await tick();
    await h.answerAll('core.subscribe', () => ({ sub_id: 'u1', stream_epoch: '1' }));
    h.send({ jsonrpc: '2.0', method: 'core.snapshot_part', params: { sub: 'u1', n: 1, total: 1, events: [] } });
    h.send({ jsonrpc: '2.0', method: 'core.snapshot_end', params: { sub: 'u1', stream_seq: '40' } });
    await tick();
    const write = h.net.call('can.tx.send', { channel: 'ch0', id: '0x300' });
    await tick();
    expect(h.pending('can.tx.send')).toHaveLength(1);
    h.sock().ev.onClose('ws_close_1006');
    await expect(write).rejects.toMatchObject({ code: 'disconnected', maybeSent: true });
    await tick();
    expect(h.net.state).toMatchObject({ status: 'reconnecting', retryInMs: 500 });
    h.timers.advance(500);
    await h.open();
    expect(h.sockets).toHaveLength(2);
    expect(h.pending('can.tx.send')).toHaveLength(0); // never re-sent
    expect(h.pending('core.subscribe')[0]!.params).toMatchObject({ after_seq: '40', snapshot: false });
    sub.close();
  });

  it('second failure doubles the backoff; core.closed{slow_consumer} triggers reconnect', async () => {
    const h = harness();
    await h.open();
    h.send({ jsonrpc: '2.0', method: 'core.closed', params: { reason: 'slow_consumer' } });
    await tick();
    expect(h.net.state).toMatchObject({ status: 'reconnecting', reason: 'server_closed:slow_consumer', retryInMs: 500 });
    h.timers.advance(500);
    await tick();
    h.sock().ev.onClose('refused');
    await tick();
    expect(h.net.state).toMatchObject({ status: 'reconnecting', retryInMs: 1000 });
  });

  it('runs a core.time burst and publishes a synced clock', async () => {
    const h = harness();
    await h.open();
    await h.answerAll('core.time', () => ({ recv_mono_ns: '5000000000', send_mono_ns: '5000100000', wall_ns: '1', clock_id: 'mono-1' }));
    await tick();
    expect(h.pending('core.time').length).toBeGreaterThanOrEqual(8);
    await until(() => h.net.clock().synced);
    expect(h.net.clock().clockId).toBe('mono-1');
  });

  it('setVisible -> core.viewer.presence; stopViaHttp POSTs /api/v1/stop', async () => {
    const h = harness();
    await h.open();
    h.net.setVisible(false);
    await tick();
    expect(h.pending('core.viewer.presence')[0]!.params).toEqual({ visible: false });
    await h.net.stopViaHttp();
    expect(h.fetchMock).toHaveBeenCalledWith('http://localhost:7421/api/v1/stop', expect.objectContaining({ method: 'POST' }));
    expect(stopUrlFor('wss://h:1/rpc?x=1')).toBe('https://h:1/api/v1/stop');
  });

  it('batches carry Worker receive/decode timing (recv_ms = earliest merged, decode_ms summed)', async () => {
    const h = harness();
    await h.open();
    const got: { b: EventsBatch; t?: DeliveryTiming }[] = [];
    const onBatch = (b: EventsBatch, t?: DeliveryTiming) => void got.push({ b, ...(t ? { t } : {}) });
    h.net.subscribe({ domains: ['can'] }, { snapshot: false }, { onBatch } as unknown as SubscriptionHandlers);
    await tick();
    await h.answerAll('core.subscribe', () => ({ sub_id: 'd1', stream_epoch: '1' }));
    await h.frame();
    const note = (a: number, b: number) => ({ jsonrpc: '2.0', method: 'core.events', params: { sub: 'd1', first: String(a), last: String(b), lag_ms: 0, events: range(a, b) } });
    h.send(note(1, 2));
    await tick();
    h.send(note(3, 4));
    h.send(note(5, 6));
    await tick();
    await h.frame();
    expect(got).toHaveLength(2);
    for (const g of got) {
      expect(g.t?.recv_ms).toBeGreaterThan(1_762_000_000_000);
      expect(g.t?.decode_ms).toBeGreaterThan(0); // fake clock advances 0.25 ms per reading
    }
    // second delivery merged two server batches: decode summed, recv = the earlier one
    expect(got[1]!.t!.decode_ms).toBeCloseTo(0.5, 5);
    expect(got[1]!.t!.recv_ms).toBeLessThan(got[1]!.t!.recv_ms + got[1]!.t!.decode_ms);
  });

  it('setVisible(v, extra) keeps the latest presence fields and re-sends them after reconnect', async () => {
    const h = harness();
    await h.open();
    const client = h.net as unknown as { setVisible(v: boolean, extra?: PresenceExtra): void };
    client.setVisible(true, { rendered_through_seq: '120', oldest_pending_age_ms: 14, channel: null });
    await tick();
    expect(h.pending('core.viewer.presence').at(-1)!.params).toEqual({ visible: true, rendered_through_seq: '120', oldest_pending_age_ms: 14 });
    client.setVisible(true); // no change, no extra -> nothing sent
    await tick();
    expect(h.pending('core.viewer.presence')).toHaveLength(1);
    client.setVisible(false, { rendered_through_seq: 'bad' as unknown as string, oldest_pending_age_ms: 30 });
    await tick();
    expect(h.pending('core.viewer.presence').at(-1)!.params).toEqual({ visible: false, oldest_pending_age_ms: 30 });
    h.sock().ev.onClose('ws_close_1006');
    await tick();
    h.timers.advance(500);
    await h.open();
    expect(h.pending('core.viewer.presence')).toEqual([expect.objectContaining({ params: { visible: false, oldest_pending_age_ms: 30 } })]);
  });

  it('a refused handshake that the HTTP probe calls 401/403 ends in closed{unauthorized}, no reconnect loop', async () => {
    const timers = new ManualTimers();
    const sockets: SocketEvents[] = [];
    const probe = vi.fn(() => Promise.resolve(401 as number | null));
    const net = createNetClient('ws://localhost:7421/rpc', {
      inline: true,
      deps: { openSocket: (_u, e) => (sockets.push(e), { send: () => undefined, close: () => undefined }), now: () => 0, timers, probeAuth: probe },
      frames: { request: () => undefined },
    });
    await tick();
    sockets[0]!.onClose('ws_close_1006');
    await tick();
    await tick();
    expect(probe).toHaveBeenCalledWith('ws://localhost:7421/rpc');
    expect(net.state).toEqual({ status: 'closed', reason: 'unauthorized' });
    timers.advance(10_000);
    await tick();
    expect(sockets).toHaveLength(1); // no reconnect
    // other statuses (e.g. 400 from an upgrade-only route) keep the normal backoff
    const sockets2: SocketEvents[] = [];
    const net2 = createNetClient('ws://localhost:7421/rpc', {
      inline: true,
      deps: { openSocket: (_u, e) => (sockets2.push(e), { send: () => undefined, close: () => undefined }), now: () => 0, timers, probeAuth: () => Promise.resolve(400) },
      frames: { request: () => undefined },
    });
    await tick();
    sockets2[0]!.onClose('ws_close_1006');
    await tick();
    await tick();
    expect(net2.state).toMatchObject({ status: 'reconnecting' });
  });

  it('DEV hooks: ?loom_daemon (loopback ws only) and ?loom_rights', () => {
    expect(devWsUrl('ws://localhost:7420/rpc', '')).toBe('ws://localhost:7420/rpc');
    expect(devWsUrl('ws://localhost:7420/rpc', '?loom_rights=watch')).toBe('ws://localhost:7420/rpc?rights=watch');
    expect(devWsUrl('ws://localhost:7420/rpc', '?loom_daemon=ws://127.0.0.1:4791/rpc&loom_rights=approve')).toBe('ws://127.0.0.1:4791/rpc?rights=approve');
    expect(devWsUrl('ws://localhost:7420/rpc', '?loom_daemon=ws://evil.example/rpc')).toBe('ws://localhost:7420/rpc');
    expect(devWsUrl('ws://localhost:7420/rpc', '?loom_daemon=http://localhost:1/x&loom_rights=root')).toBe('ws://localhost:7420/rpc');
  });

  it('snapshot parts are 1-based (schema/v1): n === 1 starts a snapshot', () => {
    const part = (n: number) => parseInbound(JSON.stringify({ jsonrpc: '2.0', method: 'core.snapshot_part', params: { sub: 's1', n, total: 2, items: [], events: [] } }));
    expect(part(1)).toMatchObject({ t: 'part', first: true });
    expect(part(2)).toMatchObject({ t: 'part', first: false });
  });

  it('close() fails pending calls and ends in closed', async () => {
    const h = harness();
    await h.open();
    const p = (h.net as NetClient).call('core.safe_state.list');
    await tick();
    h.net.close();
    await expect(p).rejects.toMatchObject({ code: 'closed' });
    await until(() => h.net.state.status === 'closed');
  });
});

describe('stale sockets', () => {
  it('a late close from an abandoned socket does not tear down the new connection', async () => {
    const timers = new ManualTimers();
    const sockets: SocketEvents[] = [];
    const net = createNetClient('ws://localhost:7421/rpc', {
      inline: true,
      deps: { openSocket: (_u, e) => (sockets.push(e), { send: () => undefined, close: () => undefined }), now: () => 0, timers },
      frames: { request: () => undefined },
    });
    await tick();
    sockets[0]!.onClose('refused');
    timers.advance(500);
    await tick();
    expect(sockets).toHaveLength(2);
    sockets[0]!.onClose('late duplicate'); // stale
    await tick();
    expect(net.state).toMatchObject({ status: 'connecting', attempt: 2 });
  });
});
