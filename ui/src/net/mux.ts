// SubscriptionMux — server subscriptions shared by refcount; ordering
// snapshot_part* -> snapshot_end{stream_seq:H} -> live(> H); resume after
// reconnect with after_seq (same epoch), else resync. Pure: talks to the daemon
// through `call` and to the main thread through the DeliveryQueue.
//
// Handles (hid) are main-thread handlers; entries (eid) are server subs.
//
// Sharing (contract 01 §1.4: ≤ 8 subscriptions per viewer connection):
// 1. identical (filter, options) -> same entry.
// 2. at the cap, a handle may join an entry whose filter COVERS its own (same
//    encoding/coalesce, and the entry has a snapshot if the handle wants one);
//    the main thread then filters events for that handle ("narrow" bind).
// 3. otherwise the handle is PARKED and told onResync("subscription_cap"); it
//    is placed (and snapshotted) as soon as an entry closes. Parked list is
//    bounded (overflow -> onResync("subscription_cap_full"), handle dropped).
//
// A handle joining an entry that already delivered items gets a REPLAY of the
// entry's items since its last fresh subscribe (snapshot parts, end, live
// batches) — the same view as a fresh subscription without a second server sub.
// The replay log is bounded; once it overflows the entry is no longer joinable
// by snapshot handles (they get a new entry or park).

import type { EventsBatch, LoomEvent, SubscribeFilter, U64 } from '../proto/provisional';
import type { SubscribeOptions } from './api';
import type { DeliveryItem, DeliveryQueue } from './budget';
import type { NetError, Timers } from './rpc';
import type { Inbound, LocalCtx } from './wire';
import { eventSeq, filterCovers, isControlEvent, readSubscribeResult, rebatch, subscribeParams, subscriptionKey, unsubscribeParams } from './wire';

type Phase = 'idle' | 'subscribing' | 'snapshot' | 'live' | 'failed';

interface Logged {
  item: DeliveryItem;
  bytes: number;
}

interface Entry {
  eid: number;
  key: string;
  filter: SubscribeFilter;
  opts: SubscribeOptions;
  hids: Set<number>;
  phase: Phase;
  closed: boolean;
  subId: string | null;
  epoch: U64 | null;
  h: bigint | null;
  cursor: bigint | null;
  cursorRaw: U64 | null;
  /** bumps on every (re)subscribe; stale responses compare against it */
  gen: number;
  /** something reached handlers since the last fresh subscribe */
  delivered: boolean;
  /** set at disconnect: live with a cursor, so after_seq resume is possible */
  resumable: boolean;
  /** live batches that arrived before snapshot_end */
  preEnd: { batch: EventsBatch; bytes: number; recvMs?: number; decodeMs?: number }[];
  preEndBytes: number;
  /** items since the last fresh subscribe, for late joiners; null = overflowed */
  replay: Logged[] | null;
  replayBytes: number;
  retryTimer: unknown;
}

interface Parked {
  hid: number;
  filter: SubscribeFilter;
  opts: SubscribeOptions;
}

export interface MuxLimits {
  /** contract 01 §1.4: subscriptions per viewer connection */
  maxSubs: number;
  /** handles waiting for a free subscription slot. Overflow -> handle rejected via onResync. */
  parked: number;
  /** live batches held while the snapshot is still arriving. Overflow -> resync. */
  preEndBatches: number;
  preEndBytes: number;
  /** replay log for late joiners. Overflow -> log dropped, entry not joinable by snapshot handles. */
  replayItems: number;
  replayBytes: number;
  /** notifications for a sub whose subscribe response has not arrived. Overflow -> drop oldest, affected subs resync. */
  orphans: number;
  orphanBytes: number;
  retryMs: number;
}

export const DEFAULT_MUX_LIMITS: MuxLimits = {
  maxSubs: 8,
  parked: 256,
  preEndBatches: 64,
  preEndBytes: 8 * 1024 * 1024,
  replayItems: 256,
  replayBytes: 4 * 1024 * 1024,
  orphans: 256,
  orphanBytes: 8 * 1024 * 1024,
  retryMs: 5_000,
};

type SubNote = Extract<Inbound, { t: 'part' | 'end' | 'events' }>;

export interface MuxDeps {
  call(method: string, params: unknown): Promise<unknown>;
  queue: DeliveryQueue;
  /** control path sink (dedup happens behind it) */
  control(ev: LoomEvent): void;
  ctx(): LocalCtx;
  timers: Timers;
  limits?: Partial<MuxLimits>;
}

export class SubscriptionMux {
  private readonly entries = new Map<number, Entry>();
  private readonly byHid = new Map<number, Entry>();
  private readonly bySub = new Map<string, Entry>();
  private parked: Parked[] = [];
  private orphans: SubNote[] = [];
  private orphanBytes = 0;
  private orphanLost = false;
  private nextEid = 1;
  private connected = false;
  private readonly l: MuxLimits;
  readonly stats = { orphansDropped: 0, unknownSub: 0, resyncs: 0 };

  constructor(private readonly d: MuxDeps) {
    this.l = { ...DEFAULT_MUX_LIMITS, ...d.limits };
  }

  /** server subscriptions currently wanted */
  get entryCount(): number {
    return this.entries.size;
  }
  get parkedCount(): number {
    return this.parked.length;
  }

  refcount(hid: number): number {
    return this.byHid.get(hid)?.hids.size ?? 0;
  }

  subscribe(hid: number, filter: SubscribeFilter, opts: SubscribeOptions): void {
    if (this.place(hid, filter, opts)) return;
    if (this.parked.length >= this.l.parked) {
      this.d.queue.pushExempt({ k: 'resync', eid: -1, hid, reason: 'subscription_cap_full' }, 0);
      return;
    }
    this.parked.push({ hid, filter, opts });
    this.d.queue.pushExempt({ k: 'resync', eid: -1, hid, reason: 'subscription_cap' }, 0);
  }

  /**
   * Server slots still held by subscriptions this client already closed: a subscribe still in
   * flight, or a core.unsubscribe not yet answered. They count against the per-connection cap
   * (contract 01 §1.4: 8) so a burst of panel open/close never makes the server answer E_BUSY
   * (seen against loomd --fixture-replay, UI-T10); handles over the cap are parked instead and
   * placed when a slot really frees. Bounded by maxSubs + the number of in-flight calls.
   */
  private draining = 0;
  /** sub ids this client released: their late notifications are dropped, not orphaned (C2: FIFO 64) */
  private released: string[] = [];

  private markReleased(subId: string): void {
    this.released.push(subId);
    if (this.released.length > 64) this.released.shift();
  }

  private releaseLater(subId: string | null): void {
    if (!this.connected) return;
    this.draining++;
    const done = () => {
      this.draining = Math.max(0, this.draining - 1);
      this.placeParked();
    };
    if (subId) {
      this.markReleased(subId);
      this.d.call('core.unsubscribe', unsubscribeParams(subId)).then(done, done);
    }
    // subId null: the subscribe is still in flight; start() releases it when the reply arrives
  }

  private placeParked(): void {
    const waiting = this.parked;
    this.parked = [];
    for (const p of waiting) if (!this.place(p.hid, p.filter, p.opts)) this.parked.push(p);
  }

  unsubscribe(hid: number): void {
    this.parked = this.parked.filter((p) => p.hid !== hid);
    const e = this.byHid.get(hid);
    if (!e) return;
    this.byHid.delete(hid);
    e.hids.delete(hid);
    if (e.hids.size > 0) return;
    e.closed = true;
    e.gen++;
    this.d.timers.clearTimeout(e.retryTimer);
    this.entries.delete(e.eid);
    this.d.queue.purge(e.eid);
    if (e.subId) {
      this.bySub.delete(e.subId);
      this.releaseLater(e.subId);
    } else if (e.phase === 'subscribing') {
      this.releaseLater(null); // held until the in-flight subscribe is answered and released
    }
    // a slot may be free: place parked handles in order
    this.placeParked();
  }

  /** hello done: resume or (re)subscribe everything. */
  onOpen(): void {
    this.connected = true;
    for (const e of this.entries.values()) {
      if (e.resumable) {
        this.start(e, true);
      } else {
        if (e.delivered) this.emitResync(e, 'disconnected');
        this.start(e, false);
      }
    }
  }

  onDisconnect(): void {
    this.connected = false;
    this.released = [];
    this.draining = 0; // the server dropped every subscription of the old connection
    this.bySub.clear();
    this.orphans = [];
    this.orphanBytes = 0;
    this.orphanLost = false;
    for (const e of this.entries.values()) {
      e.resumable = e.phase === 'live' && e.cursorRaw !== null && e.epoch !== null;
      e.gen++;
      e.subId = null;
      e.phase = 'idle';
      e.preEnd = [];
      e.preEndBytes = 0;
      this.d.timers.clearTimeout(e.retryTimer);
    }
  }

  /** Called by the queue when its hard cap forced a purge of `eid`. */
  resync(eid: number, reason: string): void {
    const e = this.entries.get(eid);
    if (!e || e.closed) return;
    this.emitResync(e, reason);
    this.restartFresh(e);
  }

  onNotification(n: SubNote): void {
    const e = this.bySub.get(n.subId);
    if (!e) {
      this.orphan(n);
      return;
    }
    switch (n.t) {
      case 'part':
        if (e.phase === 'live' && n.first) {
          // server-initiated snapshot (resume out of ring, epoch change, server retry)
          this.emitResync(e, 'server_resnapshot');
          this.resetForSnapshot(e);
        }
        if (e.phase !== 'snapshot') return this.protocolResync(e, 'part_outside_snapshot');
        this.emit(e, { k: 'part', eid: e.eid, part: n.part }, n.bytes);
        return;
      case 'end': {
        if (e.phase !== 'snapshot') return this.protocolResync(e, 'end_outside_snapshot');
        e.h = n.h;
        e.cursor = n.h;
        e.cursorRaw = n.hRaw;
        e.phase = 'live';
        this.emit(e, { k: 'end', eid: e.eid, h: n.hRaw }, n.bytes);
        const held = e.preEnd;
        e.preEnd = [];
        e.preEndBytes = 0;
        for (const p of held) this.live(e, p.batch, p.bytes, p);
        return;
      }
      case 'events':
        // control kinds on a data sub also go to the control path at once (deduped downstream)
        for (const ev of n.batch.events) if (isControlEvent(ev)) this.d.control(ev);
        if (e.phase === 'snapshot') {
          if (e.preEnd.length + 1 > this.l.preEndBatches || e.preEndBytes + n.bytes > this.l.preEndBytes) {
            return this.protocolResync(e, 'live_before_snapshot_end_overflow');
          }
          e.preEnd.push({ batch: n.batch, bytes: n.bytes, ...(n.recvMs !== undefined ? { recvMs: n.recvMs, decodeMs: n.decodeMs ?? 0 } : {}) });
          e.preEndBytes += n.bytes;
          return;
        }
        if (e.phase === 'live') this.live(e, n.batch, n.bytes, n);
        return;
    }
  }

  // -------------------------------------------------------------------------

  /** Attach `hid` to an existing or new entry. false = no slot (cap reached). */
  private place(hid: number, filter: SubscribeFilter, opts: SubscribeOptions): boolean {
    const key = subscriptionKey(filter, opts);
    const joinable = (e: Entry) => !e.closed && e.phase !== 'failed' && (!opts.snapshot || (e.opts.snapshot && e.replay !== null));
    let exact: Entry | null = null;
    for (const e of this.entries.values()) if (e.key === key && joinable(e)) exact = e;
    if (exact) {
      this.join(exact, hid, opts, false);
      return true;
    }
    if (this.entries.size + this.draining < this.l.maxSubs) {
      const e = this.newEntry(this.nextEid++, filter, opts);
      this.entries.set(e.eid, e);
      e.hids.add(hid);
      this.byHid.set(hid, e);
      this.d.queue.pushExempt({ k: 'bind', eid: e.eid, hid, narrow: false }, 0);
      if (this.connected) this.start(e, false);
      return true;
    }
    for (const e of this.entries.values()) {
      if (
        joinable(e) &&
        (e.opts.encoding ?? 'json') === (opts.encoding ?? 'json') &&
        e.opts.coalesceBy === opts.coalesceBy &&
        filterCovers(e.filter, filter)
      ) {
        this.join(e, hid, opts, true);
        return true;
      }
    }
    return false;
  }

  private join(e: Entry, hid: number, opts: SubscribeOptions, narrow: boolean): void {
    // bind first: untargeted items already queued are covered by the replay
    this.d.queue.pushExempt({ k: 'bind', eid: e.eid, hid, narrow }, 0);
    if (opts.snapshot && e.replay) {
      for (const r of e.replay) this.d.queue.pushExempt({ ...r.item, hid } as DeliveryItem, r.bytes);
    }
    e.hids.add(hid);
    this.byHid.set(hid, e);
  }

  private newEntry(eid: number, filter: SubscribeFilter, opts: SubscribeOptions): Entry {
    return {
      eid,
      key: subscriptionKey(filter, opts),
      filter,
      opts,
      hids: new Set(),
      phase: 'idle',
      closed: false,
      subId: null,
      epoch: null,
      h: null,
      cursor: null,
      cursorRaw: null,
      gen: 0,
      delivered: false,
      resumable: false,
      preEnd: [],
      preEndBytes: 0,
      replay: opts.snapshot ? [] : null,
      replayBytes: 0,
      retryTimer: undefined,
    };
  }

  private live(e: Entry, batch: EventsBatch, bytes: number, t?: { recvMs?: number; decodeMs?: number }): void {
    let b = batch;
    const lastSeq = b.events.length ? eventSeq(b.events[b.events.length - 1]!) : null;
    if (e.h !== null && b.events.length && eventSeq(b.events[0]!) <= e.h) {
      const h = e.h;
      const kept = b.events.filter((ev) => eventSeq(ev) > h); // already in the snapshot
      if (!kept.length) return;
      b = rebatch(b, kept);
    }
    if (lastSeq !== null && (e.cursor === null || lastSeq > e.cursor)) {
      e.cursor = lastSeq;
      e.cursorRaw = b.events[b.events.length - 1]!.seq;
    }
    e.delivered = true;
    this.log(e, { k: 'batch', eid: e.eid, batch: b }, bytes);
    this.d.queue.pushLive(e.eid, e.filter, b, bytes, this.d.ctx(), t?.recvMs !== undefined ? { recvMs: t.recvMs, decodeMs: t.decodeMs ?? 0 } : undefined);
  }

  private emit(e: Entry, item: DeliveryItem, bytes: number): void {
    e.delivered = true;
    this.log(e, item, bytes);
    this.d.queue.pushExempt(item, bytes);
  }

  private log(e: Entry, item: DeliveryItem, bytes: number): void {
    if (!e.replay) return;
    if (e.replay.length + 1 > this.l.replayItems || e.replayBytes + bytes > this.l.replayBytes) {
      e.replay = null; // C2: stop logging; next snapshot joiner opens its own server sub
      return;
    }
    e.replay.push({ item, bytes });
    e.replayBytes += bytes;
  }

  private emitResync(e: Entry, reason: string): void {
    this.stats.resyncs++;
    this.d.queue.pushExempt({ k: 'resync', eid: e.eid, reason }, 0);
  }

  private protocolResync(e: Entry, reason: string): void {
    this.emitResync(e, `protocol:${reason}`);
    this.restartFresh(e);
  }

  private resetForSnapshot(e: Entry): void {
    e.phase = 'snapshot';
    e.h = null;
    e.delivered = false;
    e.replay = e.opts.snapshot ? [] : null;
    e.replayBytes = 0;
    e.preEnd = [];
    e.preEndBytes = 0;
  }

  private restartFresh(e: Entry): void {
    if (e.subId) {
      this.bySub.delete(e.subId);
      if (this.connected) this.d.call('core.unsubscribe', unsubscribeParams(e.subId)).catch(() => undefined);
    }
    e.subId = null;
    e.resumable = false;
    if (this.connected) this.start(e, false);
    else e.phase = 'idle';
  }

  private start(e: Entry, resume: boolean): void {
    const gen = ++e.gen;
    this.d.timers.clearTimeout(e.retryTimer);
    const afterSeq = resume && e.cursorRaw !== null && e.epoch !== null ? e.cursorRaw : null;
    const epochAtResume = e.epoch;
    if (!afterSeq) {
      e.h = null;
      e.cursor = null;
      e.cursorRaw = null;
      e.delivered = false;
      e.replay = e.opts.snapshot ? [] : null;
      e.replayBytes = 0;
    }
    e.phase = 'subscribing';
    e.preEnd = [];
    e.preEndBytes = 0;
    this.d
      .call('core.subscribe', subscribeParams(e.filter, e.opts, afterSeq))
      .then((raw) => {
        const r = readSubscribeResult(raw);
        if (e.closed || e.gen !== gen) {
          const done = () => {
            if (e.closed) this.draining = Math.max(0, this.draining - 1); // the slot counted at close
            this.placeParked();
          };
          if (r && this.connected) {
            this.markReleased(r.subId); // its snapshot may already be on the wire: drop, don't orphan
            this.d.call('core.unsubscribe', unsubscribeParams(r.subId)).then(done, done);
          } else done();
          return;
        }
        if (!r) throw new Error('bad core.subscribe result');
        e.subId = r.subId;
        this.bySub.set(r.subId, e);
        if (afterSeq && r.epoch !== epochAtResume) {
          this.emitResync(e, 'epoch_changed');
          this.restartFresh(e);
          return;
        }
        e.epoch = r.epoch;
        e.resumable = false;
        e.phase = !afterSeq && e.opts.snapshot ? 'snapshot' : 'live';
        this.drainOrphans(e);
      })
      .catch((err: NetError | Error) => {
        if (e.closed || e.gen !== gen) {
          if (e.closed && this.connected) {
            this.draining = Math.max(0, this.draining - 1); // refused: the server holds nothing
            this.placeParked();
          }
          return;
        }
        const code = 'code' in err ? err.code : 'error';
        if (code === 'disconnected' || code === 'not_connected' || code === 'closed') return; // onOpen restarts
        const why = 'dataCode' in err && err.dataCode ? err.dataCode : err.message;
        if (afterSeq) {
          this.emitResync(e, `resume_failed:${why}`);
          this.start(e, false);
          return;
        }
        e.phase = 'failed';
        this.emitResync(e, `subscribe_failed:${why}`);
        e.retryTimer = this.d.timers.setTimeout(() => {
          if (!e.closed && this.connected && e.phase === 'failed') this.start(e, false);
        }, this.l.retryMs);
      });
  }

  private orphan(n: SubNote): void {
    if (this.released.includes(n.subId)) {
      this.stats.unknownSub++; // late frames of a subscription we already let go
      return;
    }
    const waiting = [...this.entries.values()].some((e) => e.phase === 'subscribing');
    if (!waiting) {
      this.stats.unknownSub++; // late notification for a closed sub — normal
      return;
    }
    this.orphans.push(n);
    this.orphanBytes += n.bytes;
    while (this.orphans.length > this.l.orphans || this.orphanBytes > this.l.orphanBytes) {
      const old = this.orphans.shift()!;
      this.orphanBytes -= old.bytes;
      this.orphanLost = true;
      this.stats.orphansDropped++;
    }
  }

  private drainOrphans(e: Entry): void {
    if (!this.orphans.length && !this.orphanLost) return;
    const mine = this.orphans.filter((n) => n.subId === e.subId);
    this.orphans = this.orphans.filter((n) => n.subId !== e.subId);
    this.orphanBytes = this.orphans.reduce((a, n) => a + n.bytes, 0);
    if (this.orphanLost) {
      // cannot tell whose notifications were dropped: be honest, re-snapshot
      if (!this.orphans.length) this.orphanLost = false;
      this.protocolResync(e, 'orphan_overflow');
      return;
    }
    for (const n of mine) this.onNotification(n);
  }
}
