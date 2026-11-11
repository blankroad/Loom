// Wire adapter — the ONE place in src/net that reads or writes wire field
// names (p0-common). Aligned with CORE contract v1 via src/proto/provisional.ts;
// when generated types replace it, edits should stay inside this file.
//
// Everything that crosses the socket goes through here:
//   text frame -> parseInbound() -> Inbound (normalised: bigint seqs, byte size)
//   method+params -> encodeRequest()
// Helpers that build wire-shaped objects (subscribe params, re-batched
// EventsBatch, synthetic local events) also live here.

import type {
  EventsBatch,
  GapData,
  GapSummary,
  Heartbeat,
  HelloParams,
  HelloResult,
  LoomEvent,
  RpcError,
  SnapshotEnd,
  SnapshotPart,
  SubscribeFilter,
  SubscribeParams,
  SubscribeResult,
  TimeResult,
  U64,
} from '../proto/provisional';
import { CONTROL_SUB, PROTO, isControlEvent } from '../proto/provisional';
import type { SubscribeOptions } from './api';

/** contract 01 §1.2: a request and a core.events batch are each ≤ 64 KiB. */
export const MAX_FRAME_BYTES = 64 * 1024;
export const HELLO_METHOD = 'core.hello';
export const CLIENT_VERSION = '0.0.0-p0';

// ---------------------------------------------------------------------------
// Inbound
// ---------------------------------------------------------------------------

export type Inbound =
  | { t: 'response'; id: number; result: unknown; error: RpcError | null }
  | { t: 'part'; subId: string; part: SnapshotPart; first: boolean; bytes: number }
  | { t: 'end'; subId: string; h: bigint; hRaw: U64; bytes: number }
  /** recvMs/decodeMs: set by the core after parsing (Worker clock, performance.timeOrigin + now) */
  | { t: 'events'; subId: string; batch: EventsBatch; bytes: number; recvMs?: number; decodeMs?: number }
  /** the connection-wide priority subscription (contract 01 §1.5) */
  | { t: 'control'; events: LoomEvent[] }
  | { t: 'heartbeat'; hb: Heartbeat }
  /** server is about to drop us (e.g. slow_consumer): reconnect + snapshot */
  | { t: 'closed'; reason: string }
  | { t: 'other'; method: string }
  | { t: 'invalid'; reason: string };

const isObj = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
/** canonical decimal u64: no leading zeros except "0" (contract 02 §2.2) */
const U64_RE = /^(0|[1-9]\d{0,19})$/;
const isU64 = (v: unknown): v is U64 => typeof v === 'string' && U64_RE.test(v);

/** u64 decimal string -> bigint, or null when malformed (never throws on wire data). */
export function u64(v: unknown): bigint | null {
  return isU64(v) ? BigInt(v) : null;
}

function isEvent(v: unknown): v is LoomEvent {
  return isObj(v) && typeof v.kind === 'string' && isU64(v.seq) && isU64(v.epoch) && isObj(v.payload);
}

function eventsOk(v: unknown): v is LoomEvent[] {
  return Array.isArray(v) && v.every(isEvent);
}

/**
 * Parse one text frame. Malformed frames become `{t:'invalid'}` — the caller
 * counts them; a bad frame never throws out of the socket handler.
 * Shape checks are minimal on purpose; Ajv validation of handshake/control
 * frames lands with the generated schema (plan §6.3).
 */
export function parseInbound(text: string): Inbound {
  let msg: unknown;
  try {
    msg = JSON.parse(text);
  } catch {
    return { t: 'invalid', reason: 'json' };
  }
  if (!isObj(msg)) return { t: 'invalid', reason: 'not_object' };
  if (!('method' in msg)) {
    if (typeof msg.id !== 'number') return { t: 'invalid', reason: 'response_id' };
    const error = isObj(msg.error) ? (msg.error as unknown as RpcError) : null;
    return { t: 'response', id: msg.id, result: msg.result, error };
  }
  if (typeof msg.method !== 'string') return { t: 'invalid', reason: 'no_method' };
  const p = msg.params;
  const bytes = text.length;
  switch (msg.method) {
    case 'core.events': {
      if (!isObj(p) || typeof p.sub !== 'string' || !eventsOk(p.events)) return { t: 'invalid', reason: 'core.events' };
      if (p.sub === CONTROL_SUB) return { t: 'control', events: p.events };
      return { t: 'events', subId: p.sub, batch: p as unknown as EventsBatch, bytes };
    }
    case 'core.snapshot_part': {
      if (!isObj(p) || typeof p.sub !== 'string' || typeof p.n !== 'number') return { t: 'invalid', reason: 'core.snapshot_part' };
      if (p.events !== undefined && !eventsOk(p.events)) return { t: 'invalid', reason: 'core.snapshot_part.events' };
      const part = p as unknown as SnapshotPart;
      // parts are numbered from 1 (schema/v1 snapshot_part: "Part number (1-based)";
      // loomd --fixture-replay sends n = 1..total). n === 1 starts a snapshot.
      return { t: 'part', subId: part.sub, part, first: part.n === 1, bytes };
    }
    case 'core.snapshot_end': {
      if (!isObj(p) || typeof p.sub !== 'string' || !isU64(p.stream_seq)) return { t: 'invalid', reason: 'core.snapshot_end' };
      const end = p as unknown as SnapshotEnd;
      return { t: 'end', subId: end.sub, h: BigInt(end.stream_seq), hRaw: end.stream_seq, bytes };
    }
    case 'core.heartbeat': {
      if (!isObj(p) || !isU64(p.stream_seq_hw)) return { t: 'invalid', reason: 'core.heartbeat' };
      return { t: 'heartbeat', hb: p as unknown as Heartbeat };
    }
    case 'core.closed':
      return { t: 'closed', reason: isObj(p) && typeof p.reason === 'string' ? p.reason : 'unknown' };
    default:
      return { t: 'other', method: msg.method };
  }
}

// ---------------------------------------------------------------------------
// Outbound
// ---------------------------------------------------------------------------

export function encodeRequest(id: number, method: string, params: unknown): string {
  return JSON.stringify(params === undefined ? { jsonrpc: '2.0', id, method } : { jsonrpc: '2.0', id, method, params });
}

const enc = new TextEncoder();
/** UTF-8 byte length, skipping the encode when the UTF-16 length already proves it fits. */
export function utf8Bytes(text: string, limit = MAX_FRAME_BYTES): number {
  if (text.length * 3 <= limit) return text.length; // lower bound is enough to pass the check
  return enc.encode(text).length;
}

export function helloParams(clientId: string): HelloParams {
  // viewers authenticate with the cookie: no token
  return { proto: PROTO, client_id: clientId, want_role: 'viewer', client: { kind: 'web', version: CLIENT_VERSION, tty: false } };
}

export function readHello(v: unknown): HelloResult | null {
  if (!isObj(v) || typeof v.role !== 'string' || typeof v.env !== 'string' || !isU64(v.stream_epoch)) return null;
  return v as unknown as HelloResult;
}

export function helloEpoch(h: HelloResult): U64 {
  return h.stream_epoch;
}

export function readSubscribeResult(v: unknown): { subId: string; epoch: U64 } | null {
  if (!isObj(v) || typeof v.sub_id !== 'string' || !isU64(v.stream_epoch)) return null;
  const r = v as unknown as SubscribeResult;
  return { subId: r.sub_id, epoch: r.stream_epoch };
}

export function readTime(v: unknown): { d1: bigint; d2: bigint; clockId: string } | null {
  if (!isObj(v)) return null;
  const r = v as unknown as TimeResult;
  const d1 = u64(r.recv_mono_ns);
  const d2 = u64(r.send_mono_ns);
  if (d1 === null || d2 === null || typeof r.clock_id !== 'string') return null;
  return { d1, d2, clockId: r.clock_id };
}

/**
 * M1 always asks for JSON: the `loom.columns.v1` binary decoder is an M2 item
 * (contract 01 §1.4), so `opts.encoding` is not forwarded yet.
 * Resume (`afterSeq`) sends snapshot:false — the server adds a snapshot itself
 * when the cursor is out of ring or the epoch changed (contract 01 §1.4).
 */
export function subscribeParams(filter: SubscribeFilter, opts: SubscribeOptions, afterSeq: U64 | null): SubscribeParams {
  const p: SubscribeParams = { filter, snapshot: afterSeq ? false : opts.snapshot, encoding: 'json' };
  if (afterSeq) p.after_seq = afterSeq;
  if (opts.maxBatchMs !== undefined) p.max_batch_ms = opts.maxBatchMs;
  if (opts.coalesceBy) p.coalesce_by = opts.coalesceBy;
  return p;
}

export function unsubscribeParams(subId: string): { sub_id: string } {
  return { sub_id: subId };
}

/** Optional ViewerPresence fields besides `visible` (schema/v1 viewer_presence). */
export interface PresenceExtra {
  channel?: string | null;
  rendered_through_seq?: U64 | null;
  oldest_pending_age_ms?: number | null;
}

/** core.viewer.presence params; only the known schema fields of `extra` are copied. */
export function presenceParams(visible: boolean, extra?: PresenceExtra | null): { visible: boolean } & PresenceExtra {
  const p: { visible: boolean } & PresenceExtra = { visible };
  if (extra?.channel != null) p.channel = extra.channel;
  if (extra?.rendered_through_seq != null && isU64(extra.rendered_through_seq)) p.rendered_through_seq = extra.rendered_through_seq;
  if (typeof extra?.oldest_pending_age_ms === 'number' && Number.isFinite(extra.oldest_pending_age_ms)) p.oldest_pending_age_ms = Math.max(0, extra.oldest_pending_age_ms);
  return p;
}

export function stopHttpBody(clientId: string): string {
  return JSON.stringify({ client_id: clientId, reason: 'ws_down' });
}

// ---------------------------------------------------------------------------
// Filters (subscription sharing under the 8-per-connection cap)
// ---------------------------------------------------------------------------

/** Stable key for (filter, options) — identical subscriptions share one server sub. */
export function subscriptionKey(filter: SubscribeFilter, opts: SubscribeOptions): string {
  return JSON.stringify([filterKey(filter), opts.snapshot, opts.maxBatchMs ?? null, opts.encoding ?? 'json', opts.coalesceBy ?? null]);
}

export function filterKey(f: SubscribeFilter): string {
  const sorted = (a?: readonly string[]) => (a ? [...a].sort() : null);
  return JSON.stringify([sorted(f.domains), sorted(f.channels), sorted(f.kinds), sorted(f.origins), f.ext ?? null]);
}

/** true when every event matching `inner` also matches `outer` (absent dimension = all). */
export function filterCovers(outer: SubscribeFilter, inner: SubscribeFilter): boolean {
  if (outer.ext !== undefined && JSON.stringify(outer.ext) !== JSON.stringify(inner.ext ?? null)) return false;
  const dim = (o?: readonly string[], i?: readonly string[]) => !o || (!!i && i.every((x) => o.includes(x)));
  return dim(outer.domains, inner.domains) && dim(outer.channels, inner.channels) && dim(outer.kinds, inner.kinds) && dim(outer.origins, inner.origins);
}

/** Client-side filter for a handle sharing a broader server subscription. `ext` is server-only. */
export function matchesFilter(f: SubscribeFilter, ev: LoomEvent): boolean {
  if (f.domains && !f.domains.includes(ev.domain)) return false;
  if (f.channels && (ev.channel == null || !f.channels.includes(ev.channel))) return false;
  if (f.kinds && !f.kinds.includes(ev.kind)) return false;
  if (f.origins && !f.origins.includes(ev.origin)) return false;
  return true;
}

// ---------------------------------------------------------------------------
// Event / batch helpers
// ---------------------------------------------------------------------------

export function eventSeq(ev: LoomEvent): bigint {
  return BigInt(ev.seq); // validated by parseInbound
}

export function eventKey(ev: LoomEvent): string {
  return `${ev.epoch}:${ev.seq}`;
}

export { isControlEvent };

export function partEvents(p: SnapshotPart): LoomEvent[] {
  return p.events ?? [];
}

export function withPartEvents(p: SnapshotPart, events: LoomEvent[]): SnapshotPart {
  return { ...p, events };
}

/** Same batch envelope, different event list (first/last recomputed). */
export function rebatch(batch: EventsBatch, events: LoomEvent[]): EventsBatch {
  const first = events[0];
  const last = events[events.length - 1];
  return { ...batch, events, first: first ? first.seq : batch.first, last: last ? last.seq : batch.last };
}

export function copyBatch(batch: EventsBatch): EventsBatch {
  return { ...batch, events: [...batch.events] };
}

function mergeGapSummary(a: GapSummary | null | undefined, b: GapSummary | null | undefined): GapSummary | undefined {
  if (!a) return b ?? undefined;
  if (!b) return a;
  const count = a.count === 'unknown' || b.count === 'unknown' ? 'unknown' : String(BigInt(a.count) + BigInt(b.count));
  const out: GapSummary = { count };
  const r = a.seq_range && b.seq_range ? { from: a.seq_range.from, to: b.seq_range.to } : (a.seq_range ?? b.seq_range);
  if (r) out.seq_range = r;
  const causes = [...new Set([...(a.causes ?? []), ...(b.causes ?? [])])];
  if (causes.length) out.causes = causes;
  return out;
}

/** Append `next` into `into` (same subscription, both pending, next is newer). */
export function mergeBatchInto(into: EventsBatch, next: EventsBatch): void {
  for (const ev of next.events) into.events.push(ev);
  if (next.events.length) into.last = next.last;
  into.lag_ms = Math.max(into.lag_ms, next.lag_ms);
  const g = mergeGapSummary(into.display_gap, next.display_gap);
  if (g) into.display_gap = g;
}

export function batchSubId(b: EventsBatch): string {
  return b.sub;
}

export interface LocalCtx {
  sessionId: string;
  epoch: U64;
  clientId: string;
}

function localEvent(ctx: LocalCtx, kind: string, domain: string, channel: string | null, seq: U64, monoNs: U64, payload: Record<string, unknown>): LoomEvent {
  const ev: LoomEvent = {
    schema_version: 1,
    session_id: ctx.sessionId,
    domain,
    kind,
    epoch: ctx.epoch,
    seq,
    producer_id: 'ui-net',
    producer_epoch: '0',
    source_seq: '0',
    ts: { mono_ns: monoNs, wall_ns: '0', clock_id: 'client', source: 'synthetic' },
    // provisional: there is no "client" origin; a local synthetic row is
    // attributed to the device stream it describes (asked of CORE, report §4).
    origin: 'device',
    client_id: ctx.clientId,
    actor_id: 'ui-net',
    payload,
  };
  if (channel !== null) ev.channel = channel;
  return ev;
}

/**
 * Local `display.gap` for data this tab dropped before the main thread took it.
 * `domain`/`channel` name the affected stream (same convention as server gaps);
 * `seq` = first dropped seq so it sorts where the hole starts.
 */
export function localDisplayGap(ctx: LocalCtx, filter: SubscribeFilter, from: U64, to: U64, count: number, monoNs: U64): LoomEvent {
  const domain = filter.domains?.length === 1 ? filter.domains[0]! : 'display';
  const channel = filter.channels?.length === 1 ? filter.channels[0]! : null;
  const payload: GapData = { seq_range: { from, to }, count: String(count), cause: 'client_decode_budget', detail: { local: true } };
  return localEvent(ctx, 'display.gap', domain, channel, from, monoNs, { ...payload });
}

/** Widen a local gap created by localDisplayGap. */
export function setGapCount(ev: LoomEvent, to: U64, count: number): void {
  const p = ev.payload as unknown as GapData;
  if (p.seq_range) p.seq_range.to = to;
  p.count = String(count);
}

export function eventMonoNs(ev: LoomEvent): U64 {
  return ev.ts.mono_ns;
}

/**
 * Heartbeat notification -> control-path event `core.heartbeat` (payload =
 * Heartbeat fields). Not a recorded event (contract 02 §2.5); synthesised so the
 * shell's STALLED logic has one input path.
 */
export function heartbeatEvent(ctx: LocalCtx, hb: Heartbeat): LoomEvent {
  return localEvent(ctx, 'core.heartbeat', 'core', null, hb.stream_seq_hw, hb.daemon_mono_ns, { ...hb });
}

export function rpcErrorFields(e: RpcError): { code: number; message: string; dataCode: string | null; data: unknown } {
  const dataCode = isObj(e.data) && typeof e.data.code === 'string' ? e.data.code : null;
  return { code: e.code, message: e.message, dataCode, data: e.data ?? null };
}
