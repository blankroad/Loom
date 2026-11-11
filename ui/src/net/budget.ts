// DeliveryQueue — what the net core has decoded but the main thread has not
// taken yet (the "decode in-flight" budget, plan §6.5). Pure.
//
// The main thread pulls once per frame (backpressure): a busy or hidden main
// thread stops pulling, this queue grows, and the budget decides what to drop.
//
// Ordering: one FIFO for all subscriptions. Consecutive data batches of the
// same subscription are merged in place, so each pull carries at most one
// batch per subscription between snapshot/resync markers ("≤ 1 per frame").
//
// Bounds (C2):
// - DATA budget 8 MiB or 128 server batches, whichever first. Overflow: the
//   DATA events of the incoming batch are dropped and a local
//   `display.gap{cause:"client_decode_budget"}` is put in that subscription's
//   stream at the hole (consecutive drops widen the same gap). Never silent.
//   Control events (isControlEvent) are outside this budget and never dropped
//   here.
// - HARD cap 32 MiB / 4096 items for everything (snapshot parts, replays,
//   control rows). Overflow: that subscription's pending items are purged and
//   `onOverflow(eid)` asks the mux to resync it (re-snapshot), which the
//   handlers see as onResync("client_queue_overflow").

import type { EventsBatch, LoomEvent, SnapshotPart, SubscribeFilter, U64 } from '../proto/provisional';
import type { LocalCtx } from './wire';
import { copyBatch, eventMonoNs, isControlEvent, localDisplayGap, mergeBatchInto, rebatch, setGapCount } from './wire';

export type DeliveryItem =
  | { k: 'part'; eid: number; hid?: number; part: SnapshotPart }
  | { k: 'end'; eid: number; hid?: number; h: U64 }
  /**
   * recvMs = earliest Worker receive time of the server batches merged into this item,
   * decodeMs = their summed parse time (both ms, performance.timeOrigin + now basis) —
   * the store measures the UI segment t_recv → t_draw from them (plan §0.2).
   */
  | { k: 'batch'; eid: number; hid?: number; batch: EventsBatch; recvMs?: number; decodeMs?: number }
  | { k: 'resync'; eid: number; hid?: number; reason: string }
  /**
   * From here on, untargeted items of `eid` also go to `hid`. narrow = the
   * handle shares a broader server filter; the main thread filters its events.
   */
  | { k: 'bind'; eid: number; hid: number; narrow: boolean };

export interface BatchTiming {
  recvMs: number;
  decodeMs: number;
}

export interface BudgetLimits {
  dataBytes: number;
  dataBatches: number;
  totalBytes: number;
  totalItems: number;
}

export const DEFAULT_LIMITS: BudgetLimits = {
  dataBytes: 8 * 1024 * 1024,
  dataBatches: 128,
  totalBytes: 32 * 1024 * 1024,
  totalItems: 4096,
};

interface Slot {
  item: DeliveryItem;
  bytes: number;
  dataBytes: number;
  dataBatches: number;
}

interface OpenGap {
  ev: LoomEvent;
  count: number;
}

export interface Drained {
  items: DeliveryItem[];
  /** local gaps created since the last drain — also go to onControl */
  gaps: LoomEvent[];
}

export class DeliveryQueue {
  private slots: Slot[] = [];
  /** eid -> slot of its mergeable (untargeted, last-for-eid) batch */
  private mergeable = new Map<number, Slot>();
  private openGaps = new Map<number, OpenGap>();
  private newGaps: LoomEvent[] = [];
  private dataBytes = 0;
  private dataBatches = 0;
  private totalBytes = 0;
  /** lifetime counters (diagnostics) */
  readonly stats = { droppedEvents: 0, droppedBatches: 0, overflows: 0 };

  constructor(
    private readonly onOverflow: (eid: number) => void,
    private readonly limits: BudgetLimits = DEFAULT_LIMITS,
  ) {}

  get size(): number {
    return this.slots.length;
  }
  get pendingDataBytes(): number {
    return this.dataBytes;
  }

  /** Snapshot part / end / resync / bind / replayed batch — exempt from the data budget. */
  pushExempt(item: DeliveryItem, bytes: number): void {
    if (!this.fits(item.eid, bytes)) return;
    // anything exempt ends the merge run and the open gap (keeps per-sub order exact)
    this.mergeable.delete(item.eid);
    this.openGaps.delete(item.eid);
    this.add({ item, bytes, dataBytes: 0, dataBatches: 0 });
  }

  /** A live server batch for subscription `eid`. */
  pushLive(eid: number, filter: SubscribeFilter, batch: EventsBatch, bytes: number, ctx: LocalCtx, timing?: BatchTiming): void {
    this.timing = timing;
    try {
      this.pushLiveInner(eid, filter, batch, bytes, ctx);
    } finally {
      this.timing = undefined;
    }
  }

  private timing: BatchTiming | undefined;

  private pushLiveInner(eid: number, filter: SubscribeFilter, batch: EventsBatch, bytes: number, ctx: LocalCtx): void {
    const data: LoomEvent[] = [];
    const ctrl: LoomEvent[] = [];
    for (const ev of batch.events) (isControlEvent(ev) ? ctrl : data).push(ev);
    if (data.length === 0) {
      if (ctrl.length) this.pushBatch(eid, batch, bytes, 0, 0);
      return;
    }
    const dataShare = Math.ceil((bytes * data.length) / batch.events.length);
    const over = this.dataBytes + dataShare > this.limits.dataBytes || this.dataBatches + 1 > this.limits.dataBatches;
    if (!over) {
      this.openGaps.delete(eid);
      this.pushBatch(eid, batch, bytes, dataShare, 1);
      return;
    }
    // Drop the data, keep control rows, widen or open the local gap.
    this.stats.droppedEvents += data.length;
    this.stats.droppedBatches += 1;
    const firstData = data[0]!;
    const lastData = data[data.length - 1]!;
    const gap = this.openGaps.get(eid);
    if (gap) {
      gap.count += data.length;
      setGapCount(gap.ev, lastData.seq, gap.count);
      if (ctrl.length) this.pushBatch(eid, rebatch(batch, ctrl), bytes - dataShare, 0, 0);
      return;
    }
    const ev = localDisplayGap(ctx, filter, firstData.seq, lastData.seq, data.length, eventMonoNs(firstData));
    const events = [ev, ...ctrl];
    // a refused push (hard cap) already purged + resynced this subscription
    if (this.pushBatch(eid, rebatch(batch, events), bytes - dataShare + 256, 0, 0)) {
      this.openGaps.set(eid, { ev, count: data.length });
      this.newGaps.push(ev);
    }
  }

  /** Remove everything pending for a subscription (closed or being resynced). */
  purge(eid: number): void {
    this.slots = this.slots.filter((s) => s.item.eid !== eid);
    this.mergeable.delete(eid);
    this.openGaps.delete(eid);
    this.recount();
  }

  drain(): Drained {
    const out: Drained = { items: this.slots.map((s) => s.item), gaps: this.newGaps };
    this.slots = [];
    this.mergeable.clear();
    this.openGaps.clear();
    this.newGaps = [];
    this.dataBytes = 0;
    this.dataBatches = 0;
    this.totalBytes = 0;
    return out;
  }

  // -------------------------------------------------------------------------

  private pushBatch(eid: number, batch: EventsBatch, bytes: number, dataBytes: number, dataBatches: number): boolean {
    if (!this.fits(eid, bytes)) return false;
    const slot = this.mergeable.get(eid);
    const t = this.timing;
    if (slot && slot.item.k === 'batch') {
      mergeBatchInto(slot.item.batch, batch);
      if (t) {
        slot.item.recvMs = slot.item.recvMs === undefined ? t.recvMs : Math.min(slot.item.recvMs, t.recvMs);
        slot.item.decodeMs = (slot.item.decodeMs ?? 0) + t.decodeMs;
      }
      slot.bytes += bytes;
      slot.dataBytes += dataBytes;
      slot.dataBatches += dataBatches;
      this.totalBytes += bytes;
      this.dataBytes += dataBytes;
      this.dataBatches += dataBatches;
      return true;
    }
    // own copy of the events array: merging appends into it
    const s: Slot = { item: { k: 'batch', eid, batch: copyBatch(batch), ...(t ? { recvMs: t.recvMs, decodeMs: t.decodeMs } : {}) }, bytes, dataBytes, dataBatches };
    this.add(s);
    this.mergeable.set(eid, s);
    return true;
  }

  private add(s: Slot): void {
    this.slots.push(s);
    this.totalBytes += s.bytes;
    this.dataBytes += s.dataBytes;
    this.dataBatches += s.dataBatches;
  }

  private fits(eid: number, bytes: number): boolean {
    if (this.slots.length + 1 <= this.limits.totalItems && this.totalBytes + bytes <= this.limits.totalBytes) return true;
    this.stats.overflows += 1;
    this.purge(eid);
    this.onOverflow(eid);
    return false;
  }

  private recount(): void {
    this.dataBytes = 0;
    this.dataBatches = 0;
    this.totalBytes = 0;
    for (const s of this.slots) {
      this.dataBytes += s.dataBytes;
      this.dataBatches += s.dataBatches;
      this.totalBytes += s.bytes;
    }
  }
}
