// RowStore — one row per request_id, progressively updated through the D4/A14
// write stages (plan §5.2.3). Shared by UDS console, CAN TX rows, DLT CTRL rows,
// TEST steps.
//
// Rules:
//  1. Key = request_id. A row is created by the first event carrying
//     request_id + payload.write_state (kind is free, E8/R-6).
//  2. Monotonic: an event whose stage rank is LOWER than the row's is ignored
//     (late/out-of-order). Same rank is accepted when its seq is not older —
//     UDS `pending ×N` updates keep stage `sent` but refresh the payload.
//  3. Row order is fixed by the FIRST request time (ts.mono_ns, then seq).
//     Updates never move a row.
//  4. C2 bounds:
//     - completed rows ≤ maxCompleted (default 20,000). Over the cap, the
//       OLDEST completed rows are evicted down to 95% of the cap (hysteresis,
//       so eviction is not O(n) per row); `evicted` counts them. The UI offers
//       "older rows → query the journal".
//     - in-flight rows are never evicted, but are capped at maxInFlight
//       (default 10,000). Beyond it, NEW request_ids are rejected and counted in
//       `rejected` — shown as a loss banner, never silently.

import type { LoomEvent, Origin, Risk, WriteState } from '../proto/provisional';
import { WRITE_STATE_RANK } from '../proto/provisional';
import { seqOf, u64, writeFields } from './adapter';
import { Emitter } from './emitter';

export const FINAL_STATES: ReadonlySet<WriteState> = new Set<WriteState>([
  'confirmed',
  'effective_verified',
  'effect_unverified',
  'failed',
  'outcome_unknown',
]);

export interface Row {
  readonly requestId: string;
  /** sort key: first event's mono ns, then seq */
  readonly firstMonoNs: bigint;
  readonly firstSeq: bigint;
  state: WriteState;
  lastSeq: bigint;
  origin: Origin;
  risk: Risk | null;
  domain: string;
  channel: string | null;
  kind: string;
  clientId: string | null;
  /** latest accepted payload (domain fields: summary, nrc, pending_count …) */
  payload: Record<string, unknown>;
  /** number of accepted events for this row */
  updates: number;
  /** wall clock of the first event (display time), ms; 0 if unknown */
  readonly firstWallMs: number;
  /** client clock (now()) when the row was first seen / last changed stage — for "elapsed" counters */
  readonly firstSeenMs: number;
  stageSeenMs: number;
}

export type ApplyResult = 'created' | 'updated' | 'ignored' | 'rejected';

export interface RowStoreOptions {
  maxCompleted?: number;
  maxInFlight?: number;
  /** client clock for elapsed counters (default performance.now) */
  now?: () => number;
}

export class RowStore extends Emitter {
  readonly maxCompleted: number;
  readonly maxInFlight: number;
  private byId = new Map<string, Row>();
  /** ordered by (firstMonoNs, firstSeq) */
  private ordered: Row[] = [];
  private completed = 0;
  private _evicted = 0;
  private _rejected = 0;
  private readonly now: () => number;

  constructor(opts: RowStoreOptions = {}) {
    super();
    this.now = opts.now ?? (() => performance.now());
    this.maxCompleted = opts.maxCompleted ?? 20_000;
    this.maxInFlight = opts.maxInFlight ?? 10_000;
  }

  get evicted(): number {
    return this._evicted;
  }
  get rejected(): number {
    return this._rejected;
  }
  get size(): number {
    return this.ordered.length;
  }
  get inFlight(): number {
    return this.ordered.length - this.completed;
  }

  rows(): readonly Row[] {
    return this.ordered;
  }
  get(requestId: string): Row | undefined {
    return this.byId.get(requestId);
  }

  apply(ev: LoomEvent): ApplyResult {
    const r = this.applyQuiet(ev);
    if (r !== 'ignored') this.changed();
    return r;
  }

  /** Apply a batch and notify once. */
  applyMany(evs: readonly LoomEvent[]): void {
    let dirty = false;
    for (const ev of evs) if (this.applyQuiet(ev) !== 'ignored') dirty = true;
    if (dirty) this.changed();
  }

  private applyQuiet(ev: LoomEvent): ApplyResult {
    const wf = writeFields(ev);
    if (!wf) return 'ignored';
    const seq = seqOf(ev);
    const row = this.byId.get(wf.requestId);
    if (!row) return this.create(ev, wf.state, seq);

    const curRank = WRITE_STATE_RANK[row.state];
    const newRank = WRITE_STATE_RANK[wf.state];
    if (newRank < curRank) return 'ignored'; // late lower stage
    if (seq < row.lastSeq) return 'ignored'; // older duplicate

    const wasFinal = FINAL_STATES.has(row.state);
    if (row.state !== wf.state) row.stageSeenMs = this.now();
    row.state = wf.state;
    row.lastSeq = seq;
    row.payload = ev.payload as Record<string, unknown>;
    if (ev.risk) row.risk = ev.risk;
    row.updates++;
    if (!wasFinal && FINAL_STATES.has(row.state)) {
      this.completed++;
      this.evictIfNeeded();
    }
    return 'updated';
  }

  private create(ev: LoomEvent, state: WriteState, seq: bigint): ApplyResult {
    const isFinal = FINAL_STATES.has(state);
    if (!isFinal && this.inFlight >= this.maxInFlight) {
      this._rejected++;
      return 'rejected';
    }
    const seen = this.now();
    const row: Row = {
      requestId: ev.request_id as string,
      firstMonoNs: u64(ev.ts?.mono_ns) ?? 0n,
      firstSeq: seq,
      state,
      lastSeq: seq,
      origin: ev.origin,
      risk: ev.risk ?? null,
      domain: ev.domain,
      channel: ev.channel ?? null,
      kind: ev.kind,
      clientId: ev.client_id ?? null,
      payload: ev.payload as Record<string, unknown>,
      updates: 1,
      firstWallMs: wallMs(ev),
      firstSeenMs: seen,
      stageSeenMs: seen,
    };
    this.byId.set(row.requestId, row);
    this.insertOrdered(row);
    if (isFinal) {
      this.completed++;
      this.evictIfNeeded();
    }
    return 'created';
  }

  private insertOrdered(row: Row): void {
    // Rows nearly always arrive in order → scan from the end.
    let i = this.ordered.length;
    while (i > 0 && cmp(this.ordered[i - 1] as Row, row) > 0) i--;
    this.ordered.splice(i, 0, row);
  }

  private evictIfNeeded(): void {
    if (this.completed <= this.maxCompleted) return;
    const target = Math.floor(this.maxCompleted * 0.95);
    let toEvict = this.completed - target;
    const keep: Row[] = [];
    for (const r of this.ordered) {
      if (toEvict > 0 && FINAL_STATES.has(r.state)) {
        this.byId.delete(r.requestId);
        toEvict--;
        this.completed--;
        this._evicted++;
      } else {
        keep.push(r);
      }
    }
    this.ordered = keep;
  }

  clear(): void {
    this.byId.clear();
    this.ordered = [];
    this.completed = 0;
    this.changed();
  }
}

function cmp(a: Row, b: Row): number {
  if (a.firstMonoNs !== b.firstMonoNs) return a.firstMonoNs < b.firstMonoNs ? -1 : 1;
  if (a.firstSeq !== b.firstSeq) return a.firstSeq < b.firstSeq ? -1 : 1;
  return 0;
}

function wallMs(ev: LoomEvent): number {
  const w = u64(ev.ts?.wall_ns);
  return w === null ? 0 : Number(w / 1_000_000n);
}
