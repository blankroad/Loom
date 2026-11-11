import { describe, expect, it, vi } from 'vitest';
import type { SubscribeParams } from '../proto/provisional';
import { DeliveryQueue, type DeliveryItem } from './budget';
import { SubscriptionMux, type MuxLimits } from './mux';
import { NetError } from './rpc';
import { endNote, ev, evNote, ManualTimers, partNote, range, tick } from './testing/fixtures';

interface Call {
  method: string;
  params: SubscribeParams & { sub_id?: string };
  resolve(v: unknown): void;
  reject(e: unknown): void;
}

function setup(limits: Partial<MuxLimits> = {}) {
  const calls: Call[] = [];
  const control = vi.fn();
  const timers = new ManualTimers();
  const queue = new DeliveryQueue((eid) => mux.resync(eid, 'client_queue_overflow'));
  const mux: SubscriptionMux = new SubscriptionMux({
    call: (method, params) =>
      new Promise((resolve, reject) => calls.push({ method, params: params as Call['params'], resolve, reject })),
    queue,
    control,
    ctx: () => ({ sessionId: 's1', epoch: '1', clientId: 'ui-t' }),
    timers,
    limits,
  });
  let n = 0;
  /** answer the oldest pending core.subscribe */
  const ack = async (epoch = '1') => {
    const c = calls.find((x) => x.method === 'core.subscribe' && !(x as { done?: boolean }).done)!;
    (c as { done?: boolean }).done = true;
    const sub = `s${++n}`;
    c.resolve({ sub_id: sub, stream_epoch: epoch });
    await tick();
    return { sub, params: c.params };
  };
  const drain = () => queue.drain().items;
  const subs = () => calls.filter((c) => c.method === 'core.subscribe');
  return { mux, queue, calls, control, timers, ack, drain, subs };
}

const kinds = (items: DeliveryItem[]) =>
  items.map((i) =>
    i.k === 'batch' ? `batch:${i.batch.events.map((e) => e.seq).join(',')}` : i.k === 'end' ? `end:${i.h}` : i.k === 'part' ? `part:${i.part.n}` : i.k === 'bind' ? `bind:${i.hid}${i.narrow ? 'n' : ''}` : `resync:${i.reason}`,
  );

const uart = { domains: ['uart'], channels: ['dut'] };
const SNAP = { snapshot: true };

describe('SubscriptionMux', () => {
  it('shares one server subscription per identical (filter, options) with refcount', async () => {
    const { mux, subs, ack, calls } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    mux.subscribe(2, { channels: ['dut'], domains: ['uart'] }, SNAP); // same filter, other key order
    expect(subs()).toHaveLength(1);
    expect(mux.refcount(1)).toBe(2);
    const { sub } = await ack();
    mux.unsubscribe(1);
    expect(calls.some((c) => c.method === 'core.unsubscribe')).toBe(false);
    mux.unsubscribe(2);
    expect(calls.find((c) => c.method === 'core.unsubscribe')?.params).toEqual({ sub_id: sub });
    expect(mux.entryCount).toBe(0);
  });

  it('orders snapshot parts -> end -> live, holds early live, drops live ≤ H', async () => {
    const { mux, ack, drain } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub, params } = await ack();
    expect(params).toMatchObject({ snapshot: true, encoding: 'json' });
    mux.onNotification(partNote(sub, range(1, 3), 1, 2));
    mux.onNotification(evNote(sub, range(4, 6))); // live before end: held
    mux.onNotification(partNote(sub, range(4, 5), 2, 2));
    mux.onNotification(endNote(sub, 5));
    mux.onNotification(evNote(sub, range(7, 7)));
    expect(kinds(drain())).toEqual(['bind:1', 'part:1', 'part:2', 'end:5', 'batch:6,7']);
  });

  it('routes control kinds on a data sub to the control path immediately (even mid-snapshot)', async () => {
    const { mux, ack, control } = setup();
    mux.onOpen();
    mux.subscribe(1, { domains: ['uds'] }, SNAP);
    const { sub } = await ack();
    // every write stage is control-path (loom-proto is_priority_kind: payload.write_state present)
    const sent = ev(8, 'uds.request', { request_id: 'r1', payload: { write_state: 'sent' } });
    const fin = ev(9, 'uds.request', { request_id: 'r1', payload: { write_state: 'confirmed' } });
    mux.onNotification(evNote(sub, [ev(7, 'uds.data', { payload: {} }), sent, fin]));
    expect(control).toHaveBeenCalledTimes(2);
    expect(control).toHaveBeenNthCalledWith(1, sent);
    expect(control).toHaveBeenNthCalledWith(2, fin);
  });

  it('a late joiner gets a replay of snapshot + live, then shared live', async () => {
    const { mux, ack, drain, subs } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub } = await ack();
    mux.onNotification(partNote(sub, range(1, 2)));
    mux.onNotification(endNote(sub, 2));
    mux.onNotification(evNote(sub, range(3, 4)));
    drain();
    mux.subscribe(2, uart, SNAP);
    expect(subs()).toHaveLength(1);
    mux.onNotification(evNote(sub, range(5, 5)));
    const items = drain();
    expect(kinds(items)).toEqual(['bind:2', 'part:1', 'end:2', 'batch:3,4', 'batch:5']);
    expect(items.slice(1, 4).every((i) => i.hid === 2)).toBe(true);
    expect(items[4]!.hid).toBeUndefined(); // shared delivery
  });

  it('a late joiner on an entry whose replay overflowed gets its own server sub', async () => {
    const { mux, ack, subs } = setup({ replayItems: 2 });
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub } = await ack();
    mux.onNotification(partNote(sub, range(1, 1)));
    mux.onNotification(endNote(sub, 1));
    mux.onNotification(evNote(sub, range(2, 2)));
    mux.subscribe(2, uart, SNAP);
    expect(subs()).toHaveLength(2);
  });

  it('enforces ≤ 8 server subs: covering entry is shared (narrow), otherwise park + resync, placed when a slot frees', async () => {
    const { mux, subs, drain, calls } = setup({ maxSubs: 2 });
    mux.onOpen();
    mux.subscribe(1, { domains: ['can'] }, SNAP);
    mux.subscribe(2, { domains: ['uart'] }, SNAP);
    mux.subscribe(3, { domains: ['can'], channels: ['ch1'] }, SNAP); // covered by #1
    mux.subscribe(4, { domains: ['uds'] }, SNAP); // nothing covers it
    expect(subs()).toHaveLength(2);
    expect(mux.parkedCount).toBe(1);
    expect(kinds(drain())).toEqual(['bind:1', 'bind:2', 'bind:3n', 'resync:subscription_cap']);
    // #2's subscribe is still in flight: its server slot stays counted until it is answered and released
    mux.unsubscribe(2);
    expect(subs()).toHaveLength(2);
    expect(mux.parkedCount).toBe(1);
    const sub2 = calls.find((c) => c.method === 'core.subscribe' && (c.params.filter as { domains: string[] }).domains[0] === 'uart')!;
    sub2.resolve({ sub_id: 's-uart', stream_epoch: '1' });
    await tick();
    const unsub = calls.find((c) => c.method === 'core.unsubscribe')!;
    expect(unsub.params).toEqual({ sub_id: 's-uart' });
    expect(subs()).toHaveLength(2); // not before the server confirmed the release
    unsub.resolve({});
    await tick();
    expect(subs()).toHaveLength(3);
    expect(subs()[2]!.params.filter).toEqual({ domains: ['uds'] });
    expect(mux.parkedCount).toBe(0);
  });

  it('late snapshot frames of a subscription closed while subscribing are dropped, not orphaned', async () => {
    const { mux, calls, drain } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    mux.unsubscribe(1); // closed before the reply
    mux.subscribe(2, { domains: ['can'] }, SNAP); // another sub is now 'subscribing'
    const first = calls.find((c) => c.method === 'core.subscribe')!;
    first.resolve({ sub_id: 'gone', stream_epoch: '1' });
    await tick();
    for (let i = 0; i < 300; i++) mux.onNotification(partNote('gone', range(1, 200), i + 1, 300)); // would overflow the orphan buffer
    expect(kinds(drain()).filter((k) => k.startsWith('resync'))).toEqual([]);
    expect(mux.stats.unknownSub).toBe(300);
  });

  it('a burst of open/close never exceeds the server cap (no E_BUSY): released slots are reused only after the unsubscribe reply', async () => {
    const { mux, subs, calls } = setup({ maxSubs: 2 });
    mux.onOpen();
    for (let i = 1; i <= 6; i++) {
      mux.subscribe(i, { domains: [`d${i}`] }, SNAP);
      mux.unsubscribe(i);
    }
    mux.subscribe(99, { domains: ['can'] }, SNAP);
    const inFlight = () => subs().filter((c) => !(c as { settled?: boolean }).settled).length;
    expect(inFlight()).toBeLessThanOrEqual(2); // never more than the cap held at the server
    // answer everything in order until the last handle is placed
    for (let k = 0; k < 20 && !subs().some((c) => (c.params.filter as { domains: string[] }).domains[0] === 'can'); k++) {
      for (const c of calls.filter((x) => !(x as { settled?: boolean }).settled)) {
        (c as { settled?: boolean }).settled = true;
        c.resolve(c.method === 'core.subscribe' ? { sub_id: `s${k}-${calls.indexOf(c)}`, stream_epoch: '1' } : {});
      }
      await tick();
    }
    expect(subs().some((c) => (c.params.filter as { domains: string[] }).domains[0] === 'can')).toBe(true);
  });

  it('resumes after reconnect with after_seq (no resync when the epoch matches)', async () => {
    const { mux, ack, drain, subs } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub } = await ack();
    mux.onNotification(partNote(sub, []));
    mux.onNotification(endNote(sub, 10));
    mux.onNotification(evNote(sub, range(11, 12)));
    drain();
    mux.onDisconnect();
    mux.onOpen();
    expect(subs()[1]!.params).toMatchObject({ after_seq: '12', snapshot: false });
    const r = await ack('1');
    mux.onNotification(evNote(r.sub, range(13, 13)));
    expect(kinds(drain())).toEqual(['batch:13']);
  });

  it('epoch change on resume -> onResync + fresh snapshot subscribe', async () => {
    const { mux, ack, drain, subs } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub } = await ack();
    mux.onNotification(partNote(sub, []));
    mux.onNotification(endNote(sub, 10));
    drain();
    mux.onDisconnect();
    mux.onOpen();
    await ack('2'); // daemon restarted
    expect(kinds(drain())).toEqual(['resync:epoch_changed']);
    expect(subs()[2]!.params).toMatchObject({ snapshot: true });
    expect(subs()[2]!.params.after_seq).toBeUndefined();
  });

  it('resume rejected (cursor out of ring) -> onResync + fresh snapshot', async () => {
    const { mux, ack, drain, subs } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub } = await ack();
    mux.onNotification(partNote(sub, []));
    mux.onNotification(endNote(sub, 10));
    drain();
    mux.onDisconnect();
    mux.onOpen();
    subs()[1]!.reject(new NetError({ code: 'rpc', message: 'x', dataCode: 'resume_out_of_ring', maybeSent: true }));
    await tick();
    expect(kinds(drain())).toEqual(['resync:resume_failed:resume_out_of_ring']);
    expect(subs()[2]!.params.snapshot).toBe(true);
  });

  it('server-initiated snapshot on a live sub -> onResync then the new snapshot', async () => {
    const { mux, ack, drain } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub } = await ack();
    mux.onNotification(partNote(sub, []));
    mux.onNotification(endNote(sub, 10));
    drain();
    mux.onNotification(partNote(sub, range(20, 21)));
    mux.onNotification(endNote(sub, 21));
    expect(kinds(drain())).toEqual(['resync:server_resnapshot', 'part:1', 'end:21']);
  });

  it('disconnect mid-snapshot -> onResync + fresh snapshot on reopen', async () => {
    const { mux, ack, drain, subs } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    const { sub } = await ack();
    mux.onNotification(partNote(sub, range(1, 2), 1, 2));
    drain();
    mux.onDisconnect();
    mux.onOpen();
    expect(kinds(drain())).toEqual(['resync:disconnected']);
    expect(subs()[1]!.params.snapshot).toBe(true);
  });

  it('buffers notifications that beat the subscribe response (bounded orphans)', async () => {
    const { mux, subs, drain } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    mux.onNotification(partNote('s1', range(1, 1)));
    mux.onNotification(endNote('s1', 1));
    subs()[0]!.resolve({ sub_id: 's1', stream_epoch: '1' });
    await tick();
    expect(kinds(drain())).toEqual(['bind:1', 'part:1', 'end:1']);
  });

  it('subscribe failure is surfaced (onResync) and retried after 5 s', async () => {
    const { mux, subs, drain, timers } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    subs()[0]!.reject(new NetError({ code: 'rpc', message: 'too many', dataCode: 'sub_limit', maybeSent: true }));
    await tick();
    expect(kinds(drain())).toEqual(['bind:1', 'resync:subscribe_failed:sub_limit']);
    timers.advance(5_000);
    expect(subs()).toHaveLength(2);
  });

  it('queue hard-cap overflow -> resync + fresh subscribe', async () => {
    const { mux, ack, drain, subs } = setup();
    mux.onOpen();
    mux.subscribe(1, uart, SNAP);
    await ack();
    drain();
    mux.resync(1, 'client_queue_overflow');
    expect(kinds(drain())).toEqual(['resync:client_queue_overflow']);
    expect(subs()).toHaveLength(2);
  });
});
