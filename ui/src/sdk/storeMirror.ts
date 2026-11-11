// StoreMirror (plan §4.2, UD-24): the main-thread view of one subscription.
// P0: the net layer has no Worker-side ring yet, so the mirror keeps the most
// recent `capacity` events itself and project() serves from it. When the Worker
// ring lands, project() forwards to it and the mirror stays as the recent tail.
//
// C2: fixed ring of `capacity` rows (oldest overwritten, `overwritten` counts);
// gap marks ring of MAX_GAP_MARKS (oldest dropped).

import type { EventsBatch, LoomEvent, SnapshotPart } from '../proto/provisional';
import { readGap, readGapSummary, seqOf, u64 } from '../store/adapter';

export interface GapMark {
  kind: 'display.gap' | 'record.gap' | 'source.gap' | 'resync';
  firstSeq: bigint | null;
  lastSeq: bigint | null;
  count: number | 'unknown';
  cause: string;
  /** came from a batch-level display_gap summary (not a gap event in the stream) */
  fromBatch?: boolean;
}

export interface Projection<T> {
  rows: T[];
  fromSeq: bigint | null;
  toSeq: bigint | null;
  /** older rows exist outside the mirror → query the journal */
  truncated: boolean;
}

export interface StoreMirror<T = LoomEvent> {
  readonly snapshotDone: boolean;
  readonly headSeq: bigint;
  readonly gaps: readonly GapMark[];
  tail(n: number): T[];
  /** events with seq > seq, oldest → newest */
  after(seq: bigint): T[];
  /** rows lost from this mirror ring before the panel read them */
  readonly overwritten: number;
  project(view: { fromSeq?: bigint; rows: number }): Promise<Projection<T>>;
  onAppend(cb: (from: bigint, to: bigint) => void): () => void;
}

const MAX_GAP_MARKS = 100;

export class EventMirror implements StoreMirror<LoomEvent> {
  snapshotDone = false;
  headSeq = 0n;
  gaps: GapMark[] = [];
  overwritten = 0;
  private ring: (LoomEvent | undefined)[];
  private head = 0;
  private count = 0;
  private listeners = new Set<(from: bigint, to: bigint) => void>();

  constructor(readonly capacity: number) {
    this.ring = new Array<LoomEvent | undefined>(capacity);
  }

  // ---- fed by SubscriptionHandlers ----
  snapshotPart(part: SnapshotPart): void {
    this.pushMany(part.events ?? []);
  }
  snapshotEnd(h: string): void {
    this.snapshotDone = true;
    const hs = u64(h);
    if (hs !== null && hs > this.headSeq) this.headSeq = hs;
    this.notify(this.headSeq, this.headSeq);
  }
  batch(b: EventsBatch): void {
    const g = readGapSummary(b.display_gap);
    if (g) this.addGap({ kind: g.kind, firstSeq: g.firstSeq, lastSeq: g.lastSeq, count: g.count, cause: g.cause, fromBatch: true });
    this.pushMany(b.events);
  }
  resync(reason: string): void {
    this.snapshotDone = false;
    this.addGap({ kind: 'resync', firstSeq: null, lastSeq: null, count: 'unknown', cause: reason });
    this.notify(this.headSeq, this.headSeq);
  }

  // ---- StoreMirror ----
  tail(n: number): LoomEvent[] {
    const out: LoomEvent[] = [];
    const m = Math.min(n, this.count);
    for (let i = m; i >= 1; i--) {
      const e = this.ring[(this.head - i + this.capacity) % this.capacity];
      if (e) out.push(e);
    }
    return out; // oldest → newest
  }

  /**
   * Events with seq > `seq`, oldest → newest (scans back from the newest; O(new)).
   * Filtered streams skip seqs normally — a jump is NOT loss. Loss inside the mirror
   * shows as `overwritten` growing between calls.
   */
  after(seq: bigint): LoomEvent[] {
    const out: LoomEvent[] = [];
    for (let i = 1; i <= this.count; i++) {
      const e = this.ring[(this.head - i + this.capacity) % this.capacity];
      if (!e || seqOf(e) <= seq) break;
      out.push(e);
    }
    return out.reverse();
  }

  async project(view: { fromSeq?: bigint; rows: number }): Promise<Projection<LoomEvent>> {
    let rows = this.tail(this.count);
    if (view.fromSeq !== undefined) {
      const from = view.fromSeq;
      rows = rows.filter((e) => seqOf(e) >= from);
    }
    rows = rows.slice(0, view.rows);
    const first = rows[0];
    const last = rows[rows.length - 1];
    return {
      rows,
      fromSeq: first ? seqOf(first) : null,
      toSeq: last ? seqOf(last) : null,
      truncated: this.overwritten > 0,
    };
  }

  onAppend(cb: (from: bigint, to: bigint) => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  private pushMany(evs: readonly LoomEvent[]): void {
    if (evs.length === 0) return;
    let from: bigint | null = null;
    for (const e of evs) {
      const g = readGap(e);
      if (g) this.addGap({ ...g });
      if (this.count === this.capacity) this.overwritten++;
      else this.count++;
      this.ring[this.head] = e;
      this.head = (this.head + 1) % this.capacity;
      const s = seqOf(e);
      if (from === null) from = s;
      if (s > this.headSeq) this.headSeq = s;
    }
    this.notify(from ?? this.headSeq, this.headSeq);
  }

  private addGap(g: GapMark): void {
    this.gaps.push(g);
    if (this.gaps.length > MAX_GAP_MARKS) this.gaps.shift();
  }

  private notify(from: bigint, to: bigint): void {
    for (const cb of this.listeners) cb(from, to);
  }
}
