// Wire types for the UI. Since main 761870a these are ALIASES of types generated from
// CORE's schema/v1 (src/proto/gen, `pnpm gen:schema`, drift-checked by `pnpm check:schema`).
// Kept here: UI helpers (ranks, control classification) and the few shapes not yet in
// schema/v1 (marked [not in schema/v1]). The module name stays `provisional` to avoid a
// churn rename; read wire fields only through adapters (src/net/wire.ts, store/adapter.ts).
import type { PublishedRecord } from './gen/published_record';
import type { GapData as GenGapData, ProducerRange as GenProducerRange, SeqRange as GenSeqRange } from './gen/gap_data';
import type { GapSummary as GenGapSummary, EventsBatch as GenEventsBatch } from './gen/events_batch';
import type { Hello } from './gen/hello';
import type { HelloResult as GenHelloResult, Viewers as GenViewers } from './gen/hello_result';
import type { Subscribe, SubFilter } from './gen/subscribe';
import type { SubscribeResult as GenSubscribeResult } from './gen/subscribe_result';
import type { SnapshotPart as GenSnapshotPart } from './gen/snapshot_part';
import type { SnapshotEnd as GenSnapshotEnd } from './gen/snapshot_end';
import type { Heartbeat as GenHeartbeat } from './gen/heartbeat';
import type { Closed as GenClosed } from './gen/closed';
import type { TimeResult as GenTimeResult } from './gen/time_result';

/** u64 travels as a canonical JSON decimal string. Compute with BigInt. */
export type U64 = string;

export type Origin = PublishedRecord['origin'];
export type Dir = NonNullable<PublishedRecord['dir']>;
export type Risk = NonNullable<PublishedRecord['risk']>;
export type Env = NonNullable<PublishedRecord['env']>;
/** Single ordered level: watch < operate < approve (loom-proto Rights). */
export type Right = NonNullable<GenHelloResult['rights']>;
export const RIGHT_RANK: Record<Right, number> = { watch: 0, operate: 1, approve: 2 };
export type Role = GenHelloResult['role'];
export type RecordHealth = GenHeartbeat['record_health'];

/** D4/A14 write stages. Monotonic per request_id. [not in schema/v1 as a named type] */
export type WriteState =
  | 'requested'
  | 'accepted'
  | 'sent'
  | 'confirmed'
  | 'effective_verified'
  | 'effect_unverified'
  | 'failed'
  | 'outcome_unknown';

export const WRITE_STATE_RANK: Record<WriteState, number> = {
  requested: 0,
  accepted: 1,
  sent: 2,
  confirmed: 3,
  effective_verified: 4,
  effect_unverified: 4,
  failed: 5,
  outcome_unknown: 5,
};

export type Ts = PublishedRecord['ts'];
export type TsSource = Ts['source'];

/** Raw bytes inside payloads: payload.bytes */
export interface PayloadBytes {
  encoding: 'hex' | 'base64' | 'raw_ref';
  data?: string;
  ref?: string;
}

/** PublishedRecord (contract 02 §2.2) with a typed payload. Optional fields may be omitted or null. */
export type LoomEvent<P = Record<string, unknown>> = Omit<PublishedRecord, 'payload'> & { payload: P };

/** Write rows: any kind carrying request_id + payload.write_state (E8 / R-6). */
export interface WritePayload {
  write_state: WriteState;
  [k: string]: unknown;
}

/** A13 loss layers. */
export type GapKind = 'display.gap' | 'record.gap' | 'source.gap';
export type SeqRange = GenSeqRange;
export type ProducerRange = GenProducerRange;
/** GapCount: canonical decimal string or "unknown". */
export type GapCount = string;
export type GapData = GenGapData;
export type GapSummary = GenGapSummary;

// ---- JSON-RPC envelope (contract 01 §1.2) ----
export interface RpcRequest {
  jsonrpc: '2.0';
  id: number | string;
  method: string;
  params?: unknown;
}
export interface RpcError {
  code: number;
  message: string;
  data?: { code?: string; hint?: string; retryable?: boolean; request_id?: string; detail?: unknown };
}
export interface RpcResponse {
  jsonrpc: '2.0';
  id: number | string | null;
  result?: unknown;
  error?: RpcError;
}
export interface RpcNotification {
  jsonrpc: '2.0';
  method: string;
  params: unknown;
}

// ---- core.hello / subscribe / stream (generated) ----
export const PROTO = 'loom/1.0';
export type HelloParams = Hello;
export type Viewers = GenViewers;
export type HelloResult = GenHelloResult;
/** SubFilter + domain extension `ext` (eth display filter) [ext not in schema/v1 yet] */
export type SubscribeFilter = SubFilter & { ext?: Record<string, unknown> };
export type SubscribeParams = Omit<Subscribe, 'filter'> & { filter: SubscribeFilter };
export type SubscribeResult = GenSubscribeResult;
export type EventsBatch = Omit<GenEventsBatch, 'events'> & { events: LoomEvent[] };
export type SnapshotPart = Omit<GenSnapshotPart, 'events'> & { events?: LoomEvent[] };
export type SnapshotEnd = GenSnapshotEnd;
export type Heartbeat = GenHeartbeat;
export type Closed = GenClosed;
export type TimeResult = GenTimeResult;

/**
 * Control (priority) events. Per contract 01 §1.5 every viewer connection has an
 * automatic shell-wide control subscription; how it is addressed on the wire is not
 * fixed yet (asked CORE in docs/impl/requests/ui-core.md). Provisionally it arrives as
 * `core.events` with `sub === CONTROL_SUB`.
 */
export const CONTROL_SUB = 'control';

/**
 * Priority (control-path) classification — mirrors loom-proto `is_priority_kind`
 * (crates/loom-proto/src/core_events.rs): fixed core kinds, the `test`/`run` domains,
 * ANY event carrying payload.write_state, and state-like events
 * (`state|error|alert|timeout|late`, `*_state`, `*_error`). Keep in sync with CORE.
 */
export const PRIORITY_CORE_KINDS: ReadonlySet<string> = new Set([
  'core.latch',
  'core.approval_requested',
  'core.approval_resolved',
  'core.confirm_issued',
  'core.policy_denied',
  'core.policy_changed',
  'core.worker_state',
  'core.job_state',
  'core.job_progress',
  'core.activity',
  'core.agent_cursor',
  'core.mark',
  'core.epoch_changed',
  'core.write_recovered',
  'core.session_closed',
  'core.env',
  'display.gap',
  'record.gap',
  'record.health',
  'source.gap',
  'ui.focus_request',
]);
const PRIORITY_DOMAINS: ReadonlySet<string> = new Set(['test', 'run']);

function isStateLike(event: string): boolean {
  return (
    event === 'state' ||
    event === 'error' ||
    event === 'alert' ||
    event === 'timeout' ||
    event === 'late' ||
    event.endsWith('_state') ||
    event.endsWith('_error')
  );
}

export function isControlEvent(ev: Pick<LoomEvent, 'kind' | 'payload'>): boolean {
  if (PRIORITY_CORE_KINDS.has(ev.kind)) return true;
  const dot = ev.kind.indexOf('.');
  const domain = dot < 0 ? ev.kind : ev.kind.slice(0, dot);
  const event = dot < 0 ? '' : ev.kind.slice(dot + 1);
  if (PRIORITY_DOMAINS.has(domain)) return true;
  const p = ev.payload as { write_state?: unknown } | null | undefined;
  if (p && typeof p === 'object' && 'write_state' in p) return true; // valid or malformed write_state
  return isStateLike(event);
}
