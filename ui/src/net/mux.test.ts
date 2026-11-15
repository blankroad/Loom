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

  it('at the cap an active handle makes room by folding a compatible stream into an exact union (no park); the slot is reused only after the unsubscribe reply', async () => {
    const { mux, subs, drain, calls } = setup({ maxSubs: 2 });
    mux.onOpen();
    mux.subscribe(1, { domains: ['can'] }, SNAP);
    mux.subscribe(2, { domains: ['uart'], channels: ['u0'] }, SNAP);
    mux.subscribe(3, { domains: ['can'], channels: ['ch1'] }, SNAP); // covered by #1
    mux.subscribe(4, { domains: ['uds'] }, SNAP); // nothing covers it: absorb into can → {domains:[can,uds]} (exact union)
    expect(subs()).toHaveLength(2);
    expect(mux.capacity()).toMatchObject({ entries: 1, parked: 3 }); // union group (1, 3, 4) waits for the released slot
    // the can entry's queued binds were purged with it; its handles learn the stream is being re-made
    expect(kinds(drain())).toEqual(['bind:2', 'resync:merged', 'resync:merged']);
    // the can subscribe is still in flight: its slot stays counted until answered and released
    for (const c of calls.filter((x) => x.method === 'core.subscribe')) c.resolve({ sub_id: `s-${(c.params.filter as { domains: string[] }).domains[0]}`, stream_epoch: '1' });
    await tick();
    const unsubs = calls.filter((c) => c.method === 'core.unsubscribe');
    expect(unsubs.map((c) => c.params.sub_id)).toEqual(['s-can']);
    expect(subs()).toHaveLength(2); // not before the server confirmed the release
    for (const u of unsubs) u.resolve({});
    await tick();
    expect(subs().slice(2).map((c) => c.params.filter)).toEqual([{ domains: ['can', 'uds'] }]);
    expect(mux.parkedCount).toBe(0);
    // every member of the union is bound narrowly (filtered to its own filter on the main thread)
    expect(kinds(drain()).sort()).toEqual(['bind:1n', 'bind:3n', 'bind:4n']);
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

  // -------------------------------------------------------------------------
  // visibility leases + bounded capacity (TASK-CAN-LIVE)

  describe('visibility leases (8 server subscriptions per connection)', () => {
    /** a server that enforces the cap like loomd (hub MAX_SUBS_PER_CONN) and answers every call */
    function server(t: ReturnType<typeof setup>, max: number) {
      let held = 0;
      let maxHeld = 0;
      let busy = 0;
      let n = 0;
      const done = new Set<Call>();
      const ids = new Map<Call, string>();
      const settle = async () => {
        for (let round = 0; round < 50; round++) {
          const open = t.calls.filter((c) => !done.has(c));
          if (!open.length) return;
          for (const c of open) {
            done.add(c);
            if (c.method === 'core.unsubscribe') {
              held--;
              c.resolve({});
            } else if (c.method === 'core.subscribe') {
              if (held >= max) {
                busy++;
                c.reject(new NetError({ code: 'rpc', message: 'subscription limit', dataCode: 'E_BUSY', maybeSent: true }));
              } else {
                held++;
                maxHeld = Math.max(maxHeld, held);
                ids.set(c, `s${++n}`);
                c.resolve({ sub_id: ids.get(c), stream_epoch: '1' });
              }
            }
          }
          await tick();
        }
      };
      /** the socket drops: every open call fails as rpc.failAll does, the server forgets the connection's subs */
      const drop = () => {
        for (const c of t.calls.filter((x) => !done.has(x))) {
          done.add(c);
          c.reject(new NetError({ code: 'disconnected', message: 'socket closed', maybeSent: true }));
        }
        held = 0;
      };
      return { settle, drop, subId: (c: Call) => ids.get(c), held: () => held, maxHeld: () => maxHeld, busy: () => busy };
    }
    const filterOf = (c: Call) => c.params.filter;
    const SHELL = { domains: ['core', 'test'] };
    const PIN = { snapshot: true, pinned: true };

    it('retained-dock tour (real filters): CAN agent-read activity rides on the shell stream; bound CAN gets a slot', async () => {
      const t = setup();
      const srv = server(t, 8);
      t.mux.onOpen();
      let hid = 0;
      t.mux.subscribe(++hid, SHELL, PIN); // shell (always active, pinned)
      const tour = [
        { domains: ['uart'], channels: ['qa-uart'] },
        { domains: ['eth'], channels: ['qa-eth'] },
        { domains: ['uds'] },
        { domains: ['video'], kinds: ['video.state', 'video.frame'] }, // unbound video: whole domain
        { domains: ['logic'], kinds: ['logic.state', 'logic.capture'] }, // unbound logic
        { domains: ['psu'], channels: ['qa-psu'] },
      ];
      for (const f of tour) {
        t.mux.subscribe(++hid, f, SNAP); // module shown …
        await srv.settle();
        t.mux.setActive(hid, false); // … then the user moves on: the dock is retained (hidden)
      }
      const can = hid + 1;
      const act = hid + 2;
      t.mux.subscribe(can, { domains: ['can'], channels: ['qa-can'] }, SNAP);
      t.mux.subscribe(act, { domains: ['core'], kinds: ['core.activity'] }, { snapshot: false });
      await srv.settle();
      expect(t.subs().map(filterOf)).not.toContainEqual({ domains: ['core'], kinds: ['core.activity'] }); // no own slot
      expect(t.subs().at(-1)!.params).toMatchObject({ filter: { domains: ['can'], channels: ['qa-can'] }, snapshot: true });
      expect(t.mux.capacity()).toMatchObject({ entries: 8, parked: 0, suspended: 0 });
      expect(kinds(t.drain()).filter((k) => k.startsWith('resync'))).toEqual([]);
      expect(srv.busy()).toBe(0);
      expect(srv.maxHeld()).toBeLessThanOrEqual(8);
    });

    it('over the cap, the visible bound CAN suspends the least-recently-visible hidden dock (ACK handoff), then gets snapshot + live', async () => {
      const t = setup();
      const srv = server(t, 8);
      t.mux.onOpen();
      t.mux.subscribe(1, SHELL, PIN);
      // seven hidden module docks with mutually distinct domains/channels (2 UART ports, 2 ETH, …)
      const hidden = [
        { domains: ['uart'], channels: ['qa-uart'] },
        { domains: ['eth'], channels: ['qa-eth'] },
        { domains: ['uds'] },
        { domains: ['video'], kinds: ['video.state'] },
        { domains: ['logic'], kinds: ['logic.state'] },
        { domains: ['psu'], channels: ['qa-psu'] },
        { domains: ['eth'], channels: ['qa-eth2'], ext: { eth_filter: 'udp' } },
      ];
      for (let i = 0; i < hidden.length; i++) {
        t.mux.subscribe(10 + i, hidden[i]!, SNAP);
        await srv.settle();
        t.mux.setActive(10 + i, false);
      }
      t.drain();
      expect(t.mux.capacity()).toMatchObject({ entries: 8, active: 1 });
      t.mux.subscribe(99, { domains: ['can'], channels: ['qa-can'] }, SNAP);
      // the oldest hidden dock (UART, hid 10) lends its slot; CAN is not told "subscription_cap"
      expect(kinds(t.drain())).toEqual(['resync:suspended_hidden']);
      const unsub = t.calls.filter((c) => c.method === 'core.unsubscribe');
      expect(unsub).toHaveLength(1);
      expect(t.subs().map(filterOf)).not.toContainEqual({ domains: ['can'], channels: ['qa-can'] }); // waits for the ACK
      await srv.settle();
      const canCall = t.subs().at(-1)!;
      expect(canCall.params).toMatchObject({ filter: { domains: ['can'], channels: ['qa-can'] }, snapshot: true });
      expect(srv.busy()).toBe(0);
      expect(srv.maxHeld()).toBe(8);
      // native order: snapshot → end → live 0x301 / 0x310 reach the CAN handle
      const sub = `s${t.subs().length}`;
      t.mux.onNotification(partNote(sub, range(1966, 1966)));
      t.mux.onNotification(endNote(sub, 1966));
      t.mux.onNotification(evNote(sub, range(1972, 1975)));
      expect(kinds(t.drain())).toEqual(['bind:99', 'part:1', 'end:1966', 'batch:1972,1973,1974,1975']);
    });

    it('hidden → shown: a suspended handle is re-placed with a FRESH snapshot (no after_seq), suspending the next LRU hidden one', async () => {
      const t = setup({ maxSubs: 3 });
      const srv = server(t, 3);
      t.mux.onOpen();
      t.mux.subscribe(1, SHELL, PIN);
      t.mux.subscribe(2, { domains: ['psu'], channels: ['qa-psu'] }, SNAP);
      t.mux.subscribe(3, { domains: ['uds'] }, SNAP);
      await srv.settle();
      t.mux.setActive(2, false); // PSU hidden first (LRU)
      t.mux.setActive(3, false);
      t.mux.subscribe(4, { domains: ['can'], channels: ['qa-can'] }, SNAP);
      await srv.settle();
      t.drain();
      expect(t.mux.capacity()).toMatchObject({ entries: 3, suspended: 1 });
      t.mux.setActive(4, false); // CAN hidden, PSU shown again
      t.mux.setActive(2, true);
      expect(kinds(t.drain())).toEqual(['resync:suspended_hidden']); // UDS (hidden longest now) lends its slot
      await srv.settle();
      const psu = t.subs().at(-1)!;
      expect(psu.params.filter).toEqual({ domains: ['psu'], channels: ['qa-psu'] });
      expect(psu.params.snapshot).toBe(true);
      expect(psu.params).not.toHaveProperty('after_seq');
      expect(srv.busy()).toBe(0);
      expect(srv.maxHeld()).toBeLessThanOrEqual(3);
    });

    it('under the cap, hiding/showing docks costs no server calls (no churn)', async () => {
      const t = setup();
      const srv = server(t, 8);
      t.mux.onOpen();
      for (let h = 1; h <= 5; h++) t.mux.subscribe(h, { domains: [`d${h}`] }, SNAP);
      await srv.settle();
      const before = t.calls.length;
      for (let i = 0; i < 200; i++) t.mux.setActive(1 + (i % 5), i % 2 === 0);
      for (let h = 1; h <= 5; h++) t.mux.setActive(h, true);
      await srv.settle();
      expect(t.calls.length).toBe(before);
      expect(kinds(t.drain()).filter((k) => k.startsWith('resync'))).toEqual([]);
    });

    it('Integrated: shell + all seven modules visible at once fit in 8; many visible UART ports share one exact union', async () => {
      const t = setup();
      const srv = server(t, 8);
      t.mux.onOpen();
      t.mux.subscribe(1, SHELL, PIN);
      const seven = [
        { domains: ['uart'], channels: ['u0'] },
        { domains: ['eth'], channels: ['qa-eth'] },
        { domains: ['uds'] },
        { domains: ['can'], channels: ['qa-can'] },
        { domains: ['psu'], channels: ['qa-psu'] },
        { domains: ['logic'], channels: ['la0'], kinds: ['logic.capture'] },
        { domains: ['video'], channels: ['cam0'], kinds: ['video.frame'] },
      ];
      seven.forEach((f, i) => t.mux.subscribe(10 + i, f, SNAP));
      t.mux.subscribe(30, { domains: ['core'], kinds: ['core.activity'] }, { snapshot: false }); // CAN agent-read
      await srv.settle();
      expect(t.mux.capacity()).toMatchObject({ entries: 8, parked: 0 });
      expect(kinds(t.drain()).filter((k) => k.startsWith('resync'))).toEqual([]);
      // five more UART ports shown side by side: no park, UART streams fold into one exact union
      for (let p = 1; p <= 5; p++) t.mux.subscribe(40 + p, { domains: ['uart'], channels: [`u${p}`] }, SNAP);
      await srv.settle();
      const items = t.drain();
      expect(kinds(items).filter((k) => k === 'resync:subscription_cap')).toEqual([]);
      expect(t.mux.capacity()).toMatchObject({ entries: 8, parked: 0 });
      const uartUnion = t.subs().map(filterOf).filter((f) => f.domains?.includes('uart') && (f.channels?.length ?? 0) === 6);
      expect(uartUnion).toEqual([{ domains: ['uart'], channels: ['u0', 'u1', 'u2', 'u3', 'u4', 'u5'] }]);
      // the other six modules and the shell were never merged or resynced
      const others = t.subs().map(filterOf).filter((f) => !f.domains?.includes('uart'));
      expect(others).toEqual([SHELL, ...seven.filter((f) => !f.domains.includes('uart'))]); // each subscribed exactly once
      expect(srv.busy()).toBe(0);
      expect(srv.maxHeld()).toBeLessThanOrEqual(8);
    });

    it('genuine capacity: mutually unmergeable visible handles park with "subscription_cap" and are placed as soon as one is hidden', async () => {
      const t = setup({ maxSubs: 2 });
      const srv = server(t, 2);
      t.mux.onOpen();
      const eth = (x: string) => ({ domains: ['eth'], channels: ['qa-eth'], ext: { eth_filter: x } });
      t.mux.subscribe(1, eth('tcp'), SNAP);
      t.mux.subscribe(2, eth('udp'), SNAP);
      await srv.settle();
      t.drain();
      t.mux.subscribe(3, eth('arp'), SNAP);
      expect(kinds(t.drain())).toEqual(['resync:subscription_cap']);
      expect(t.mux.capacity()).toMatchObject({ entries: 2, parked: 1, active: 3 });
      t.mux.setActive(1, false); // the user hides one view: its slot goes to the waiting one
      expect(kinds(t.drain())).toEqual(['resync:suspended_hidden']);
      await srv.settle();
      expect(t.subs().at(-1)!.params.filter).toEqual(eth('arp'));
      expect(t.mux.capacity()).toMatchObject({ entries: 2, parked: 0, suspended: 1 });
      t.mux.setActive(1, true); // shown again while both others are visible: honest park, no eviction of visible ones
      expect(kinds(t.drain())).toEqual(['bind:3', 'resync:subscription_cap']);
      expect(srv.busy()).toBe(0);
    });

    it('F3: a disconnect while a suspended entry\'s subscribe is in flight; reopen places the waiting visible handle (no E_BUSY)', async () => {
      const t = setup({ maxSubs: 2 });
      const srv = server(t, 2);
      const x = (n: number) => ({ domains: ['eth'], ext: { eth_filter: `f${n}` } }); // mutually unmergeable
      t.mux.onOpen();
      t.mux.subscribe(1, x(1), SNAP);
      t.mux.subscribe(2, x(2), SNAP);
      await srv.settle();
      t.mux.setActive(2, false); // retained hidden dock
      srv.drop();
      t.mux.onDisconnect();
      await tick();
      t.mux.onOpen(); // reconnect: both entries re-subscribe (in flight)
      t.drain();
      t.mux.subscribe(3, x(3), SNAP); // the user shows a third view
      // the hidden, still-subscribing entry is suspended; its slot drains until that subscribe is answered
      expect(kinds(t.drain())).toEqual(['resync:suspended_hidden']); // no "subscription_cap": counted as freed
      expect(t.mux.capacity()).toMatchObject({ entries: 1, draining: 1, parked: 1 });
      srv.drop(); // the socket drops before the answer: the rejected subscribe sees connected=false
      t.mux.onDisconnect();
      await tick();
      expect(t.subs().map((c) => c.params.filter)).not.toContainEqual(x(3));
      t.mux.onOpen(); // reopen alone must place the parked visible handle (old: parked forever, no signal)
      await srv.settle();
      expect(t.subs().at(-1)!.params).toMatchObject({ filter: x(3), snapshot: true });
      expect(t.mux.capacity()).toMatchObject({ entries: 2, parked: 0, suspended: 1, draining: 0 });
      const sub = srv.subId(t.subs().at(-1)!)!;
      t.mux.onNotification(partNote(sub, range(1, 1)));
      t.mux.onNotification(endNote(sub, 1));
      expect(kinds(t.drain())).toEqual(['bind:3', 'part:1', 'end:1']); // actual placement: snapshot reaches it
      expect(srv.busy()).toBe(0);
      expect(srv.maxHeld()).toBeLessThanOrEqual(2);
    });

    it('F4: at the cap a topic opened lazily while hidden (active:false in subscribe) merges/resyncs nothing visible', async () => {
      const t = setup({ maxSubs: 3 });
      const srv = server(t, 3);
      t.mux.onOpen();
      t.mux.subscribe(1, SHELL, { ...PIN, active: false }); // pinned: always active
      t.mux.subscribe(2, { domains: ['uart'], channels: ['u0'] }, SNAP); // two visible, mergeable streams
      t.mux.subscribe(3, { domains: ['uart'], channels: ['u1'] }, SNAP);
      await srv.settle();
      t.drain();
      const calls = t.calls.length;
      t.mux.subscribe(4, { domains: ['psu'], channels: ['p0'] }, { snapshot: true, active: false });
      await srv.settle();
      // honest park of the hidden handle only; no merge, no suspend, no visible resync, no server call
      expect(kinds(t.drain())).toEqual(['resync:subscription_cap']);
      expect(t.calls.length).toBe(calls);
      expect(t.mux.stats).toMatchObject({ merged: 0, suspended: 0 });
      expect(t.mux.capacity()).toMatchObject({ entries: 3, parked: 1, active: 3 });
      // the lease is not part of the subscription key: a hidden open of an identical filter shares the entry
      t.mux.subscribe(5, { domains: ['uart'], channels: ['u0'] }, { snapshot: true, active: false });
      expect(t.mux.refcount(5)).toBe(2);
      expect(t.calls.length).toBe(calls);
      // … and never reaches the wire
      for (const c of t.subs()) expect(c.params).not.toHaveProperty('active');
      // shown: now it relieves pressure (folds the visible UART pair) and gets its slot
      t.mux.setActive(4, true);
      await srv.settle();
      expect(t.subs().at(-1)!.params.filter).toEqual({ domains: ['psu'], channels: ['p0'] });
      expect(srv.busy()).toBe(0);
      expect(srv.maxHeld()).toBeLessThanOrEqual(3);
    });

    it('a parked or suspended handle that is closed leaves no state behind', async () => {
      const t = setup({ maxSubs: 1 });
      const srv = server(t, 1);
      t.mux.onOpen();
      t.mux.subscribe(1, { domains: ['a'], ext: { x: 1 } }, SNAP);
      await srv.settle();
      t.mux.setActive(1, false);
      t.mux.subscribe(2, { domains: ['b'], ext: { x: 2 } }, SNAP);
      await srv.settle();
      t.mux.unsubscribe(1); // suspended
      t.mux.unsubscribe(2);
      await srv.settle();
      expect(t.mux.capacity()).toMatchObject({ entries: 0, handles: 0, parked: 0, suspended: 0 });
      expect(srv.held()).toBe(0);
    });
  });
});
