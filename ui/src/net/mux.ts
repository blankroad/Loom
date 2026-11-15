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
//
// Visibility leases (TASK-CAN-LIVE): a handle is ACTIVE while its panel is visible
// (setActive; new handles start active, shell-owned subscriptions never go inactive).
// Hiding tears nothing down. Only when an ACTIVE handle cannot be placed does the mux
// make room, bounded and without timers or retries:
//   a. SUSPEND the least-recently-visible entry whose handles are all inactive
//      (core.unsubscribe; its handles get onResync("suspended_hidden"), keep their
//      panel/mirror state, and are re-placed with a FRESH snapshot when shown again);
//   b. else MERGE the two compatible entries whose union is cheapest (same options,
//      ext and origins; an exact union such as uart/[a,b] first) into one server
//      subscription: its handles get onResync("merged"), then the union's fresh
//      snapshot, filtered per handle on the main thread (narrow bind);
//   c. else the handle stays parked with onResync("subscription_cap") — genuine
//      capacity, placed as soon as a slot frees or a panel is hidden.
// Freed slots go to merge groups first, then active handles (most recent first), then
// inactive ones. Slots still draining count as freed (no second relief for them).

import type { EventsBatch, LoomEvent, SubscribeFilter, U64 } from '../proto/provisional';
import type { SubscribeOptions } from './api';
import type { DeliveryItem, DeliveryQueue } from './budget';
import type { NetError, Timers } from './rpc';
import type { Inbound, LocalCtx } from './wire';
import { eventSeq, filterCovers, filterKey, isControlEvent, readSubscribeResult, rebatch, subscribeParams, subscriptionKey, unsubscribeParams } from './wire';

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
  /** placement filter: the handle's own, or the union of a merge group */
  filter: SubscribeFilter;
  opts: SubscribeOptions;
  /** member of a merge group (placed before any other parked handle) */
  merge?: boolean;
}

/** one main-thread handle: its own filter (narrow binding) and visibility lease */
interface HandleInfo {
  filter: SubscribeFilter;
  opts: SubscribeOptions;
  active: boolean;
  /** lease clock at the last visibility change (LRU for suspension) */
  touched: number;
  state: 'bound' | 'parked' | 'suspended';
}

/** Capacity as the mux sees it (exposed for diagnostics and tests). */
export interface MuxCapacity {
  maxSubs: number;
  entries: number;
  draining: number;
  handles: number;
  active: number;
  parked: number;
  suspended: number;
  /** entries carrying more than one distinct handle filter (shared/merged) */
  shared: number;
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
  private readonly handles = new Map<number, HandleInfo>();
  private leaseClock = 0;
  readonly stats = { orphansDropped: 0, unknownSub: 0, resyncs: 0, suspended: 0, merged: 0 };

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

  capacity(): MuxCapacity {
    let active = 0;
    let suspended = 0;
    for (const h of this.handles.values()) {
      if (h.active) active++;
      if (h.state === 'suspended') suspended++;
    }
    let shared = 0;
    for (const e of this.entries.values()) {
      const keys = new Set([...e.hids].map((h) => filterKey(this.handles.get(h)?.filter ?? e.filter)));
      if (keys.size > 1 || (keys.size === 1 && !keys.has(filterKey(e.filter)))) shared++;
    }
    return { maxSubs: this.l.maxSubs, entries: this.entries.size, draining: this.draining, handles: this.handles.size, active, parked: this.parked.length, suspended, shared };
  }

  subscribe(hid: number, filter: SubscribeFilter, options: SubscribeOptions): void {
    // the initial lease travels with the subscribe (a hidden lazy open never relieves pressure); it is
    // handle state only, never part of the entry's options, key or wire params
    const { active: initial, ...opts } = options;
    const active = initial !== false || opts.pinned === true;
    this.handles.set(hid, { filter, opts, active, touched: ++this.leaseClock, state: 'parked' });
    if (this.place(hid, filter, opts)) return;
    this.park(hid, filter, opts);
  }

  /**
   * Visibility lease. Hiding changes nothing at once (no churn under the cap); a hidden
   * handle's entry only becomes suspendable. Showing re-places a suspended handle (fresh
   * snapshot) and makes room for a parked one.
   */
  setActive(hid: number, active: boolean): void {
    const h = this.handles.get(hid);
    if (!h || h.active === active) return;
    h.active = active;
    h.touched = ++this.leaseClock;
    if (!active) {
      // a visible handle waiting for a slot can take this one's now (genuine-capacity park)
      if (this.parked.some((p) => p.merge || this.handles.get(p.hid)?.active) && this.relieve()) this.placeParked();
      return;
    }
    if (h.state === 'suspended') {
      if (!this.place(hid, h.filter, h.opts)) this.park(hid, h.filter, h.opts);
    } else if (h.state === 'parked') {
      this.parked.sort(parkOrder(this.handles));
      if (this.relieve()) this.placeParked();
    }
  }

  private park(hid: number, filter: SubscribeFilter, opts: SubscribeOptions): void {
    const h = this.handles.get(hid);
    if (this.parked.length >= this.l.parked) {
      this.handles.delete(hid);
      this.d.queue.pushExempt({ k: 'resync', eid: -1, hid, reason: 'subscription_cap_full' }, 0);
      return;
    }
    if (h) h.state = 'parked';
    this.parked.push({ hid, filter, opts });
    this.parked.sort(parkOrder(this.handles));
    // an active handle makes room (suspend hidden / merge); only genuine excess is reported
    const covered = h?.active === true && this.relieve();
    if (!covered) this.d.queue.pushExempt({ k: 'resync', eid: -1, hid, reason: 'subscription_cap' }, 0);
    else this.placeParked();
  }

  /** distinct server subscriptions the waiting active handles (and merge groups) need */
  private demand(): number {
    const keys = new Set<string>();
    for (const p of this.parked) if (p.merge || this.handles.get(p.hid)?.active) keys.add(subscriptionKey(p.filter, p.opts));
    return keys.size;
  }

  /**
   * Make room for every waiting active handle: suspend hidden-only entries (LRU), else merge.
   * Slots still draining count as free. Each step lowers entries.size by ≥ 1 net, so this
   * ends after ≤ maxSubs steps. true = the waiting demand fits.
   */
  private relieve(): boolean {
    for (let guard = 0; guard <= this.l.maxSubs * 2; guard++) {
      if (this.demand() <= this.l.maxSubs - this.entries.size) return true;
      if (!this.suspendOne() && !this.mergeOne()) return false;
    }
    return this.demand() <= this.l.maxSubs - this.entries.size;
  }

  private suspendOne(): boolean {
    let best: Entry | null = null;
    let bestAt = Infinity;
    for (const e of this.entries.values()) {
      if (e.closed || e.hids.size === 0 || e.opts.pinned) continue;
      let at = -Infinity;
      let anyActive = false;
      for (const hid of e.hids) {
        const h = this.handles.get(hid);
        if (!h || h.active) anyActive = true;
        else at = Math.max(at, h.touched);
      }
      if (!anyActive && at < bestAt) {
        best = e;
        bestAt = at;
      }
    }
    if (!best) return false;
    const hids = [...best.hids];
    this.closeEntry(best);
    for (const hid of hids) {
      const h = this.handles.get(hid);
      if (h) h.state = 'suspended';
      this.d.queue.pushExempt({ k: 'resync', eid: -1, hid, reason: 'suspended_hidden' }, 0);
    }
    this.stats.suspended++;
    return true;
  }

  /**
   * Fold two streams into one union subscription (frees one slot net). Candidates are pairs of
   * open entries, and an open entry with a waiting active handle ("absorb": only that entry is
   * re-subscribed). Cheapest union first; on a tie absorbing wins (fewer handles disturbed).
   */
  private mergeOne(): boolean {
    const open = [...this.entries.values()].filter((e) => !e.closed && e.hids.size > 0 && !e.opts.pinned);
    const waiting = this.parked.filter((p) => !p.merge && this.handles.get(p.hid)?.active);
    // 1. a waiting handle joins a union already being formed: no server call at all
    for (const w of waiting) {
      const g = this.parked.find((p) => p.merge && mergeCost(p.filter, p.opts, w.filter, w.opts) === 0);
      if (!g) continue;
      const key = subscriptionKey(g.filter, g.opts);
      const union = unionFilter(g.filter, w.filter);
      for (const p of this.parked) if (p.merge && subscriptionKey(p.filter, p.opts) === key) p.filter = union;
      w.filter = union;
      w.opts = g.opts;
      w.merge = true;
      this.parked.sort(parkOrder(this.handles));
      return true;
    }
    // 2. fold an open entry with a waiting handle ("absorb") or with another entry
    let best: { a: Entry; b: Entry | null; w: Parked | null; cost: number } | null = null;
    const consider = (a: Entry, b: Entry | null, w: Parked | null, cost: number | null) => {
      if (cost === null) return;
      if (!best || cost < best.cost || (cost === best.cost && w && !best.w)) best = { a, b, w, cost };
    };
    for (let i = 0; i < open.length; i++) {
      const a = open[i]!;
      for (const w of waiting) consider(a, null, w, mergeCost(a.filter, a.opts, w.filter, w.opts));
      for (let j = i + 1; j < open.length; j++) consider(a, open[j]!, null, mergeCost(a.filter, a.opts, open[j]!.filter, open[j]!.opts));
    }
    if (!best) return false;
    const { a, b, w } = best as { a: Entry; b: Entry | null; w: Parked | null };
    const union = unionFilter(a.filter, b ? b.filter : w!.filter);
    const opts = a.opts;
    const hids = [...a.hids, ...(b ? b.hids : [])];
    this.closeEntry(a);
    if (b) this.closeEntry(b);
    const group: Parked[] = [];
    for (const hid of hids) {
      const h = this.handles.get(hid);
      if (h) h.state = 'parked';
      group.push({ hid, filter: union, opts, merge: true });
      this.d.queue.pushExempt({ k: 'resync', eid: -1, hid, reason: 'merged' }, 0);
    }
    if (w) group.push({ hid: w.hid, filter: union, opts, merge: true }); // never had a stream: no resync
    const moved = new Set([...hids, ...(w ? [w.hid] : [])]);
    this.parked = [...group, ...this.parked.filter((p) => !moved.has(p.hid))];
    this.stats.merged++;
    return true;
  }

  /** Close a server subscription without touching its handles' bookkeeping beyond the entry. */
  private closeEntry(e: Entry): void {
    for (const hid of e.hids) this.byHid.delete(hid);
    e.hids.clear();
    e.closed = true;
    e.gen++;
    this.d.timers.clearTimeout(e.retryTimer);
    this.entries.delete(e.eid);
    this.d.queue.purge(e.eid);
    if (e.subId) {
      this.bySub.delete(e.subId);
      this.releaseLater(e.subId);
    } else if (e.phase === 'subscribing') {
      this.releaseLater(null);
    }
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
    const waiting = this.parked.sort(parkOrder(this.handles));
    this.parked = [];
    for (const p of waiting) if (!this.place(p.hid, p.filter, p.opts)) this.parked.push(p);
    // a waiting active handle that a freed slot did not reach: make room again (bounded)
    if (this.parked.some((p) => p.merge || this.handles.get(p.hid)?.active) && this.relieve()) {
      const again = this.parked;
      this.parked = [];
      for (const p of again) if (!this.place(p.hid, p.filter, p.opts)) this.parked.push(p);
    }
  }

  unsubscribe(hid: number): void {
    this.handles.delete(hid);
    this.parked = this.parked.filter((p) => p.hid !== hid);
    const e = this.byHid.get(hid);
    if (!e) return;
    this.byHid.delete(hid);
    e.hids.delete(hid);
    if (e.hids.size > 0) return;
    // an in-flight subscribe stays counted (draining) until it is answered and released
    this.closeEntry(e);
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
    // a handle parked while the old connection dropped an in-flight release has no other trigger
    // (its slot's catch saw connected=false): place it now. Idempotent, gated by the cap as always.
    this.placeParked();
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
      this.join(exact, hid, opts);
      return true;
    }
    const covering = () => {
      for (const e of this.entries.values()) {
        if (
          joinable(e) &&
          (e.opts.encoding ?? 'json') === (opts.encoding ?? 'json') &&
          e.opts.coalesceBy === opts.coalesceBy &&
          filterCovers(e.filter, filter)
        ) {
          this.join(e, hid, opts);
          return true;
        }
      }
      return false;
    };
    // a live-only handle never needs a replay: join a covering stream before spending a slot
    // (CAN's agent-read activity topic rides on the shell's core stream)
    if (!opts.snapshot && covering()) return true;
    if (this.entries.size + this.draining < this.l.maxSubs) {
      const e = this.newEntry(this.nextEid++, filter, opts);
      this.entries.set(e.eid, e);
      this.join(e, hid, opts);
      if (this.connected) this.start(e, false);
      return true;
    }
    return covering();
  }

  private join(e: Entry, hid: number, opts: SubscribeOptions): void {
    const h = this.handles.get(hid);
    // narrow = the handle's own filter is not the entry's (covering join or merge group)
    const narrow = filterKey(h?.filter ?? e.filter) !== filterKey(e.filter);
    // bind first: untargeted items already queued are covered by the replay
    this.d.queue.pushExempt({ k: 'bind', eid: e.eid, hid, narrow }, 0);
    if (opts.snapshot && e.replay) {
      for (const r of e.replay) this.d.queue.pushExempt({ ...r.item, hid } as DeliveryItem, r.bytes);
    }
    e.hids.add(hid);
    this.byHid.set(hid, e);
    if (h) h.state = 'bound';
    if (opts.pinned && !e.opts.pinned) e.opts = { ...e.opts, pinned: true }; // a shell stream stays pinned
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

// ---------------------------------------------------------------------------
// lease helpers (pure)

/** merge groups first, then active handles (most recently shown first), then hidden ones */
function parkOrder(handles: ReadonlyMap<number, HandleInfo>): (a: Parked, b: Parked) => number {
  const rank = (p: Parked) => (p.merge ? 0 : handles.get(p.hid)?.active ? 1 : 2);
  return (a, b) => rank(a) - rank(b) || (handles.get(b.hid)?.touched ?? 0) - (handles.get(a.hid)?.touched ?? 0);
}

const dimUnion = (a?: readonly string[], b?: readonly string[]) => (!a || !b ? undefined : [...new Set([...a, ...b])].sort());
const sameDim = (a?: readonly string[], b?: readonly string[]) => JSON.stringify(a ? [...a].sort() : null) === JSON.stringify(b ? [...b].sort() : null);

/** smallest filter selecting everything either selects (absent dimension = all; never an empty list) */
export function unionFilter(a: SubscribeFilter, b: SubscribeFilter): SubscribeFilter {
  const u: SubscribeFilter = {};
  const d = dimUnion(a.domains, b.domains);
  const c = dimUnion(a.channels, b.channels);
  const k = dimUnion(a.kinds, b.kinds);
  if (d) u.domains = d;
  if (c) u.channels = c;
  if (k) u.kinds = k;
  if (a.origins) u.origins = [...a.origins];
  if (a.ext !== undefined) u.ext = a.ext;
  return u;
}

/**
 * Cost of serving both entries from one union subscription; null = not mergeable (different
 * options, push-down ext or origins). 0 = exact union (one covers the other, or they differ in
 * one dimension only, e.g. uart/[a] + uart/[b]); otherwise the number of differing dimensions
 * (the cross product may carry extra rows, filtered per handle on the main thread).
 */
function mergeCost(af: SubscribeFilter, ao: SubscribeOptions, bf: SubscribeFilter, bo: SubscribeOptions): number | null {
  const opt = (o: SubscribeOptions) => JSON.stringify([o.snapshot, o.maxBatchMs ?? null, o.encoding ?? 'json', o.coalesceBy ?? null]);
  if (opt(ao) !== opt(bo)) return null;
  if (JSON.stringify(af.ext ?? null) !== JSON.stringify(bf.ext ?? null)) return null;
  if (!sameDim(af.origins, bf.origins)) return null;
  if (filterCovers(af, bf) || filterCovers(bf, af)) return 0;
  const diff = [sameDim(af.domains, bf.domains), sameDim(af.channels, bf.channels), sameDim(af.kinds, bf.kinds)].filter((same) => !same).length;
  return diff <= 1 ? 0 : diff;
}
