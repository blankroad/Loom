// Mock event hub: seq assignment, bounded ring, per-connection subscriptions
// with adaptive batching, the automatic control subscription, snapshot and
// after_seq resume. Speaks the provisional contract v1 (src/proto/provisional).
// Deliberately small: CORE's `loomd --fixture-replay` replaces this.
//
// Bounds (C2):
// - ring: 20,000 events; overflow drops the oldest (older resume -> gap + snapshot)
// - per subscription pending: flushed at 200 events / 60 KB, so never larger
// - slow client (ws.bufferedAmount > 4 MiB): DATA batches are dropped and the
//   next delivered batch carries display.gap{cause:"slow_client"} (never silent)
// - control path (bufferedAmount > 16 MiB): core.closed{slow_consumer} + drop
// - subscriptions per viewer connection: 8 (contract 01 §1.4)

import type { WebSocket } from 'ws';
import type { EventsBatch, GapData, HelloResult, LoomEvent, Right, Role, SubscribeFilter, U64 } from '../../src/proto/provisional';
import { CONTROL_SUB, isControlEvent } from '../../src/proto/provisional';
import { ethFilterMatch } from './eth';

export const RING_CAP = 20_000;
export const MAX_SUBS = 8;
const BATCH_EVENTS = 200;
const BATCH_BYTES = 60_000; // < 64 KiB frame incl. envelope
const SNAPSHOT_BACKLOG = 2_000;
const SLOW_DATA_BYTES = 4 * 1024 * 1024;
const SLOW_CONTROL_BYTES = 16 * 1024 * 1024;

export interface Clock {
  mono(): bigint;
  wall(): bigint;
}

export const realClock: Clock = {
  mono: () => process.hrtime.bigint(),
  wall: () => BigInt(Date.now()) * 1_000_000n,
};

/** Fields a publisher provides; the hub fills seq/epoch/ts/producer. */
export interface Emit {
  domain: string;
  channel?: string;
  kind: string;
  dir?: 'rx' | 'tx';
  origin: LoomEvent['origin'];
  client_id?: string;
  actor_id?: string;
  request_id?: string;
  risk?: LoomEvent['risk'];
  purpose?: string;
  test_session?: string;
  test_step?: string;
  metrics?: LoomEvent['metrics'];
  payload: Record<string, unknown>;
}

interface Stored {
  ev: LoomEvent;
  size: number;
}

interface Gap {
  from: U64;
  to: U64;
  count: number;
}

export class Sub {
  pending: Stored[] = [];
  pendingBytes = 0;
  timer: NodeJS.Timeout | null = null;
  lastFlush = 0;
  gap: Gap | null = null;
  constructor(
    readonly id: string,
    readonly conn: Conn,
    readonly filter: SubscribeFilter,
    readonly maxBatchMs: number,
  ) {}
}

export class Conn {
  readonly subs = new Map<string, Sub>();
  hello: HelloResult | null = null;
  role: Role = 'viewer';
  rights: Right = 'operate';
  clientId = '';
  visible = true;
  hbTimer: NodeJS.Timeout | null = null;
  closed = false;
  /** RPC calls received per method (E2E assertions: "no unintended TX"). C2: ≤ 64 methods, others counted under '*' */
  readonly calls = new Map<string, number>();
  /** last core.metrics.report params of this connection (C2: ring of 64) — UI-T03 assertions */
  readonly reports: unknown[] = [];
  /** last core.viewer.presence params */
  presence: unknown = null;
  constructor(
    readonly id: string,
    readonly ws: WebSocket,
    /** rights requested via ?rights= on the WS URL (mock only) */
    readonly queryRights: Right,
  ) {}

  send(msg: unknown): void {
    if (this.closed || this.ws.readyState !== this.ws.OPEN) return;
    this.ws.send(JSON.stringify(msg));
  }
  notify(method: string, params: unknown): void {
    this.send({ jsonrpc: '2.0', method, params });
  }
  countCall(method: string): void {
    const k = this.calls.has(method) || this.calls.size < 64 ? method : '*';
    this.calls.set(k, (this.calls.get(k) ?? 0) + 1);
  }
  get origin(): 'agent' | 'user' | 'script' {
    return this.role === 'agent' ? 'agent' : this.role === 'script' ? 'script' : 'user';
  }
}

export function matches(f: SubscribeFilter, ev: LoomEvent): boolean {
  if (f.domains && !f.domains.includes(ev.domain)) return false;
  // filter.ext{eth_filter} (CORE d5 ext): pushdown of the eth display filter onto eth.pkt rows
  const ethFilter = (f.ext as { eth_filter?: unknown } | null | undefined)?.eth_filter;
  if (typeof ethFilter === 'string' && ev.kind === 'eth.pkt' && !ethFilterMatch(ethFilter, ev.payload)) return false;
  if (f.channels && (!ev.channel || !f.channels.includes(ev.channel))) return false;
  if (f.kinds && !f.kinds.includes(ev.kind)) return false;
  if (f.origins && !f.origins.includes(ev.origin)) return false;
  return true;
}

export class Hub {
  epoch = 1n;
  seq = 0n;
  readonly sessionId: string;
  ring: Stored[] = [];
  readonly conns = new Set<Conn>();
  latch = false;
  /** heartbeats are suppressed until this mono-ms (stall_heartbeat injection) */
  hbStalledUntil = 0;
  private subSeq = 0;
  /** per-domain reducer state for snapshots (provider -> state) */
  readonly state = new Map<string, Record<string, unknown>>();

  constructor(
    readonly clock: Clock = realClock,
    sessionId = 'mock-session-1',
  ) {
    this.sessionId = sessionId;
  }

  get epochStr(): U64 {
    return String(this.epoch);
  }

  /**
   * Reserve `n` stream seqs without publishing events for them (mock eth journal: packets
   * that exist in the "journal" but are summarised by eth.batch on the live stream).
   * Returns the first reserved seq. The next publish() takes seq + 1.
   */
  reserve(n: number): bigint {
    const first = this.seq + 1n;
    this.seq += BigInt(Math.max(0, Math.trunc(n)));
    return first;
  }

  /** `atSeq`: publish under an already reserved seq (≤ this.seq, increasing); default = next seq */
  publish(e: Emit, atSeq?: bigint): LoomEvent {
    if (atSeq === undefined) this.seq += 1n;
    const seqStr = String(atSeq ?? this.seq);
    const ev: LoomEvent = {
      schema_version: 1,
      session_id: this.sessionId,
      domain: e.domain,
      kind: e.kind,
      epoch: this.epochStr,
      seq: seqStr,
      producer_id: `${e.domain}-sim`,
      producer_epoch: '1',
      source_seq: seqStr,
      ts: { mono_ns: String(this.clock.mono()), wall_ns: String(this.clock.wall()), clock_id: 'mock-mono', source: e.origin === 'device' ? 'host_read_complete' : 'gateway' },
      origin: e.origin,
      env: 'sim',
      payload: e.payload,
    };
    if (e.channel !== undefined) ev.channel = e.channel;
    if (e.dir) ev.dir = e.dir;
    if (e.client_id) ev.client_id = e.client_id;
    if (e.actor_id) ev.actor_id = e.actor_id;
    if (e.request_id) ev.request_id = e.request_id;
    if (e.risk) ev.risk = e.risk;
    if (e.purpose) ev.purpose = e.purpose;
    if (e.test_session) ev.test_session = e.test_session;
    if (e.test_step) ev.test_step = e.test_step;
    if (e.metrics?.length) ev.metrics = e.metrics;
    const stored: Stored = { ev, size: JSON.stringify(ev).length };
    this.ring.push(stored);
    if (this.ring.length > RING_CAP) this.ring.splice(0, this.ring.length - RING_CAP);
    const control = isControlEvent(ev);
    for (const c of this.conns) {
      if (!c.hello) continue;
      if (control) this.sendControl(c, ev);
      for (const s of c.subs.values()) if (matches(s.filter, ev)) this.push(s, stored, control);
    }
    return ev;
  }

  /**
   * publish-before-reply (contract 01 §1.6): every event published so far must
   * be on the wire before an RPC reply, so pending data batches go out first.
   */
  flushAll(): void {
    for (const c of this.conns) for (const s of c.subs.values()) if (s.pending.length) this.flush(s);
  }

  /** Daemon restart: new epoch, empty ring. */
  newEpoch(): void {
    this.epoch += 1n;
    this.seq = 0n;
    this.ring = [];
  }

  // ---- connections & subscriptions ------------------------------------------

  addConn(c: Conn): void {
    this.conns.add(c);
  }

  dropConn(c: Conn): void {
    c.closed = true;
    if (c.hbTimer) clearInterval(c.hbTimer);
    for (const s of c.subs.values()) if (s.timer) clearTimeout(s.timer);
    c.subs.clear();
    this.conns.delete(c);
  }

  startHeartbeat(c: Conn): void {
    c.hbTimer = setInterval(() => {
      if (Number(this.clock.mono() / 1_000_000n) < this.hbStalledUntil) return;
      c.notify('core.heartbeat', {
        stream_seq_hw: String(this.seq),
        daemon_mono_ns: String(this.clock.mono()),
        daemon_wall_ns: String(this.clock.wall()),
        record_health: 'ok',
        latch: this.latch,
      });
    }, 1_000);
  }

  /** Register a subscription. Returns the sub and a function to run AFTER the RPC reply. */
  subscribe(c: Conn, p: { filter?: SubscribeFilter; snapshot?: boolean; after_seq?: U64; max_batch_ms?: number }): { sub: Sub; after: () => void } {
    const sub = new Sub(`sub-${++this.subSeq}`, c, p.filter ?? {}, Math.max(0, Math.min(p.max_batch_ms ?? 40, 1000)));
    const after = () => {
      if (c.closed) return;
      if (p.after_seq !== undefined) {
        const after = BigInt(p.after_seq);
        const oldest = this.ring[0] ? BigInt(this.ring[0].ev.seq) : this.seq + 1n;
        if (after <= this.seq && after + 1n >= oldest) {
          // in ring: replay the missed events, then live
          const missed = this.ring.filter((s) => BigInt(s.ev.seq) > after && matches(sub.filter, s.ev));
          for (const chunk of chunks(missed)) this.sendBatch(sub, chunk, null);
          c.subs.set(sub.id, sub);
          return;
        }
        // out of ring (or another epoch): gap + snapshot (contract 01 §1.4)
        const gapEv = this.localGap(sub, { from: String(after + 1n), to: String(this.seq), count: -1 }, 'resume_out_of_ring');
        this.sendBatch(sub, [gapEv], null);
        this.sendSnapshot(sub);
      } else if (p.snapshot) {
        this.sendSnapshot(sub);
      }
      c.subs.set(sub.id, sub);
    };
    return { sub, after };
  }

  unsubscribe(c: Conn, subId: string): boolean {
    const s = c.subs.get(subId);
    if (!s) return false;
    if (s.timer) clearTimeout(s.timer);
    c.subs.delete(subId);
    return true;
  }

  // ---- delivery -------------------------------------------------------------

  private sendControl(c: Conn, ev: LoomEvent): void {
    if (c.ws.bufferedAmount > SLOW_CONTROL_BYTES) {
      c.notify('core.closed', { reason: 'slow_consumer' });
      c.ws.terminate();
      return;
    }
    // separate frame, before any data batch (contract 01 §1.5)
    c.notify('core.events', { sub: CONTROL_SUB, first: ev.seq, last: ev.seq, events: [ev], lag_ms: 0 } satisfies EventsBatch);
  }

  private push(s: Sub, st: Stored, control: boolean): void {
    s.pending.push(st);
    s.pendingBytes += st.size;
    const now = Date.now();
    // control rows on a data sub flush at once (no batch wait); so do full batches
    if (control || s.pending.length >= BATCH_EVENTS || s.pendingBytes >= BATCH_BYTES) return this.flush(s);
    if (s.timer) return;
    // adaptive: first event after a quiet period goes out immediately
    if (now - s.lastFlush >= s.maxBatchMs) return this.flush(s);
    s.timer = setTimeout(() => this.flush(s), s.maxBatchMs);
  }

  flush(s: Sub): void {
    if (s.timer) clearTimeout(s.timer);
    s.timer = null;
    s.lastFlush = Date.now();
    const events = s.pending;
    s.pending = [];
    s.pendingBytes = 0;
    if (!events.length || s.conn.closed) return;
    if (s.conn.ws.bufferedAmount > SLOW_DATA_BYTES) {
      // slow client: drop this sub's data, keep control rows, remember the hole
      const data = events.filter((e) => !isControlEvent(e.ev));
      const ctrl = events.filter((e) => isControlEvent(e.ev));
      if (data.length) {
        const first = data[0]!.ev.seq;
        const last = data[data.length - 1]!.ev.seq;
        s.gap = s.gap ? { from: s.gap.from, to: last, count: s.gap.count + data.length } : { from: first, to: last, count: data.length };
      }
      for (const chunk of chunks(ctrl)) this.sendBatch(s, chunk, null);
      return;
    }
    let gap: Gap | null = null;
    if (s.gap) {
      gap = s.gap;
      s.gap = null;
      events.unshift(this.localGap(s, gap, 'slow_client'));
    }
    let firstChunk = true;
    for (const chunk of chunks(events)) {
      this.sendBatch(s, chunk, firstChunk ? gap : null, 'slow_client');
      firstChunk = false;
    }
  }

  private sendBatch(s: Sub, chunk: Stored[], gap: Gap | null, cause = 'slow_client'): void {
    if (!chunk.length) return;
    const b: EventsBatch = {
      sub: s.id,
      first: chunk[0]!.ev.seq,
      last: chunk[chunk.length - 1]!.ev.seq,
      events: chunk.map((x) => x.ev),
      lag_ms: Math.max(0, Number((this.clock.mono() - BigInt(chunk[0]!.ev.ts.mono_ns)) / 1_000_000n)),
    };
    if (gap) b.display_gap = { count: String(gap.count), seq_range: { from: gap.from, to: gap.to }, causes: [cause] };
    s.conn.notify('core.events', b);
  }

  /** Per-subscriber display.gap (not in the ring: only this viewer lost it). */
  private localGap(s: Sub, g: Gap, cause: string): Stored {
    const payload: GapData = { seq_range: { from: g.from, to: g.to }, count: g.count < 0 ? 'unknown' : String(g.count), cause };
    const ev: LoomEvent = {
      schema_version: 1,
      session_id: this.sessionId,
      domain: s.filter.domains?.length === 1 ? s.filter.domains[0]! : 'display',
      kind: 'display.gap',
      epoch: this.epochStr,
      seq: g.from,
      producer_id: 'gateway',
      producer_epoch: '1',
      source_seq: '0',
      ts: { mono_ns: String(this.clock.mono()), wall_ns: String(this.clock.wall()), clock_id: 'mock-mono', source: 'gateway' },
      origin: 'device',
      payload: { ...payload },
    };
    if (s.filter.channels?.length === 1) ev.channel = s.filter.channels[0]!;
    return { ev, size: JSON.stringify(ev).length };
  }

  private sendSnapshot(s: Sub): void {
    const backlog = this.ring.filter((x) => matches(s.filter, x.ev)).slice(-SNAPSHOT_BACKLOG);
    // contract v1 SnapshotPart{sub, n, total, items:[{domain, channel?, kind, state}], events?}
    const items: { domain: string; kind: string; state: unknown }[] = [];
    for (const [provider, st] of this.state) {
      const domain = provider.split('.')[0]!;
      if (!s.filter.domains || s.filter.domains.includes(domain)) items.push({ domain, kind: provider, state: st });
    }
    const parts = chunks(backlog);
    if (!parts.length) parts.push([]);
    const total = parts.length;
    parts.forEach((p, i) => {
      // n is 1-based (schema/v1 snapshot_part; loomd --fixture-replay does the same)
      const params: Record<string, unknown> = { sub: s.id, n: i + 1, total, events: p.map((x) => x.ev) };
      if (i === 0) params.items = items; // reducer state rides on part 1
      s.conn.notify('core.snapshot_part', params);
    });
    s.conn.notify('core.snapshot_end', { sub: s.id, stream_seq: String(this.seq) });
  }
}

/** Split into ≤ 200 events / ≤ 60 KB chunks. */
function chunks(xs: Stored[]): Stored[][] {
  const out: Stored[][] = [];
  let cur: Stored[] = [];
  let bytes = 0;
  for (const x of xs) {
    if (cur.length && (cur.length >= BATCH_EVENTS || bytes + x.size > BATCH_BYTES)) {
      out.push(cur);
      cur = [];
      bytes = 0;
    }
    cur.push(x);
    bytes += x.size;
  }
  if (cur.length) out.push(cur);
  return out;
}
