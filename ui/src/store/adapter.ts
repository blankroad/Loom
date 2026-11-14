// The ONE place that reads provisional wire fields (p0-common: do not scatter
// wire names). When CORE's generated types replace src/proto/provisional.ts,
// only this file should need edits.

import type {
  GapData,
  GapKind,
  GapSummary,
  LoomEvent,
  Origin,
  Risk,
  U64,
  WriteState,
} from '../proto/provisional';
import { WRITE_STATE_RANK } from '../proto/provisional';

export type RecordHealth = 'ok' | 'degraded' | 'failed' | 'disabled';

export interface OutputHeld {
  domain: string;
  channel: string;
  /** canonical `OutputHeld.description` (loom-proto core_events): what is held, as last reported */
  description: string;
  /** legacy/provisional fields (older fixtures); empty when absent */
  label: string;
  state: string;
}

export interface LatchView {
  on: boolean;
  by: string | null;
  origin: Origin | null;
  /** true once every worker acknowledged the latch (v1: `missing` present and empty) */
  acked: boolean;
  /** workers that have not acknowledged yet */
  missing: string[];
  /** latch written to disk (survives a daemon restart) */
  persisted: boolean;
  jobsStopped: string[];
  approvalsCancelled: number;
  uncancellable: string[];
  outputsHeld: OutputHeld[];
}

export interface ActivityEntry {
  seq: bigint;
  monoNs: bigint;
  wallMs: number;
  origin: Origin;
  actor: string;
  clientId: string | null;
  verb: string;
  /** schema/v1 `method` (e.g. "can.trace.query"); '' when the payload has none */
  method: string;
  domain: string;
  channel: string | null;
  range: { seqA: bigint; seqB: bigint } | null;
  summary: string;
  /** schema/v1 `waiting` (predicate, deadline) as short text; null when not waiting */
  waiting: string | null;
}

export interface ApprovalView {
  approvalId: string;
  risk: Risk | null;
  requester: string;
  origin: Origin;
  domain: string;
  channel: string | null;
  requestId: string | null;
  /** domain-provided display object (approval.display) — rendered generically */
  display: unknown;
  expiresAtWallMs: number | null;
  /** policy asked for presence verification (WebAuthn, M2) */
  presenceRequired: boolean;
  /** last chars of the request code the agent told the user (optional) */
  codeSuffix: string | null;
}

export interface WriteFields {
  requestId: string;
  state: WriteState;
}

export interface WorkerStateView {
  worker: string;
  state: string;
  restarts: number;
}

export interface AgentCursorView {
  clientId: string;
  fromSeq: bigint;
  toSeq: bigint;
  othersCount: number;
}

export interface GapView {
  kind: GapKind;
  firstSeq: bigint | null;
  lastSeq: bigint | null;
  count: number | 'unknown';
  cause: string;
  domain: string;
  channel: string | null;
  /** loss known only as a producer (worker) range — no stream seqs */
  producerOnly: boolean;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null;
const str = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const num = (v: unknown, d = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : d);
const strArr = (v: unknown): string[] =>
  Array.isArray(v) ? v.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))) : [];

/** u64 decimal string → bigint; malformed → null (never throws on wire data). */
export function u64(v: U64 | null | undefined): bigint | null {
  if (typeof v !== 'string' || !/^\d{1,20}$/.test(v)) return null;
  return BigInt(v);
}

export function seqOf(ev: LoomEvent): bigint {
  return u64(ev.seq) ?? 0n;
}

export function writeFields(ev: LoomEvent): WriteFields | null {
  if (!ev.request_id || !isObj(ev.payload)) return null;
  const ws = ev.payload.write_state;
  if (typeof ws !== 'string' || !(ws in WRITE_STATE_RANK)) return null;
  return { requestId: ev.request_id, state: ws as WriteState };
}

export function readLatch(ev: LoomEvent): LatchView {
  const p = isObj(ev.payload) ? ev.payload : {};
  // Contract v1 has no `acked` field: acked = `missing` present and empty. If
  // `missing` is omitted we do NOT assume success (STOP is never painted as
  // locked without proof — the button falls to "확인 안 됨" after 2 s).
  const missing = Array.isArray(p.missing) ? strArr(p.missing) : null;
  const du = p.driver_uncancellable;
  return {
    on: p.on === true,
    by: str(p.by),
    origin: (str(p.origin) as Origin | null) ?? null,
    acked: p.on === true && missing !== null && missing.length === 0,
    missing: missing ?? [],
    persisted: p.persisted === true,
    jobsStopped: strArr(p.jobs_stopped),
    approvalsCancelled: Array.isArray(p.approvals_cancelled) ? p.approvals_cancelled.length : num(p.approvals_cancelled),
    // v1 `driver_uncancellable`: already handed to drivers; scalar count or list
    uncancellable:
      typeof du === 'number' || typeof du === 'string'
        ? du === 0 || du === '0'
          ? []
          : [`드라이버에 넘어간 ${String(du)}건`]
        : strArr(du),
    outputsHeld: Array.isArray(p.outputs_held)
      ? p.outputs_held.filter(isObj).map((o) => ({
          domain: str(o.domain) ?? '?',
          channel: str(o.channel) ?? '?',
          // Rust emits {domain, channel, description}; older fixtures used label/state
          description: str(o.description) ?? [str(o.label), str(o.state)].filter(Boolean).join(' '),
          label: str(o.label) ?? '',
          state: str(o.state) ?? '',
        }))
      : [],
  };
}

/** u64 as decimal string or a safe JSON number (CAN answer uses numbers) */
const u64any = (v: unknown): bigint | null =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? BigInt(v) : u64(str(v));

/**
 * `core.activity` — schema/v1 is canonical (`src/proto/gen/activity_payload.ts`):
 *   {client_id, method, summary, range?{from, to}, waiting?}
 * Also accepted (lower priority): CAN answer `{action:"read", domain, channel, range{seq_a, seq_b}}`
 * and the earlier UI shape `{actor, verb, domain, channel}`.
 */
export function readActivity(ev: LoomEvent): ActivityEntry {
  const p = isObj(ev.payload) ? ev.payload : {};
  const r = isObj(p.range) ? p.range : null;
  const a = r ? (u64any(r.from) ?? u64any(r.seq_a)) : null;
  const b = r ? (u64any(r.to) ?? u64any(r.seq_b)) : null;
  const wall = u64(ev.ts?.wall_ns);
  const method = str(p.method) ?? '';
  const clientId = str(p.client_id) ?? ev.client_id ?? null;
  const w = isObj(p.waiting) ? p.waiting : null;
  const waiting = w
    ? [str(w.predicate) ?? (w.predicate !== undefined ? JSON.stringify(w.predicate) : null), str(w.deadline) ?? (typeof w.deadline_ms === 'number' ? `≤ ${w.deadline_ms} ms` : null)]
        .filter((x): x is string => !!x)
        .join(' · ') || '대기 중'
    : null;
  return {
    seq: seqOf(ev),
    monoNs: u64(ev.ts?.mono_ns) ?? 0n,
    wallMs: wall === null ? 0 : Number(wall / 1_000_000n),
    origin: ev.origin,
    actor: str(p.actor) ?? ev.actor_id ?? clientId ?? '?',
    clientId,
    verb: method || (str(p.action) ?? str(p.verb) ?? ''),
    method,
    domain: str(p.domain) ?? (method.includes('.') ? (method.split('.')[0] as string) : ev.domain),
    channel: str(p.channel) ?? ev.channel ?? null,
    range: a !== null && b !== null ? { seqA: a <= b ? a : b, seqB: a <= b ? b : a } : null,
    summary: str(p.summary) ?? '',
    waiting,
  };
}

export function readApproval(ev: LoomEvent): ApprovalView | null {
  const p = isObj(ev.payload) ? ev.payload : {};
  const id = str(p.approval_id);
  if (!id) return null;
  const exp = u64(str(p.expires_wall_ns));
  return {
    approvalId: id,
    risk: (str(p.risk) as Risk | null) ?? ev.risk ?? null,
    requester: str(p.requester) ?? ev.actor_id ?? ev.client_id ?? '?',
    origin: ev.origin,
    domain: str(p.domain) ?? ev.domain,
    channel: str(p.channel) ?? ev.channel ?? null,
    requestId: ev.request_id ?? null,
    display: p.display ?? null,
    expiresAtWallMs: exp === null ? null : Number(exp / 1_000_000n),
    presenceRequired: p.presence_required === true,
    codeSuffix: str(p.code_suffix),
  };
}

export function readApprovalResolvedId(ev: LoomEvent): string | null {
  return isObj(ev.payload) ? str(ev.payload.approval_id) : null;
}

export function readWorkerState(ev: LoomEvent): WorkerStateView {
  const p = isObj(ev.payload) ? ev.payload : {};
  return {
    worker: str(p.worker) ?? ev.channel ?? ev.domain,
    state: str(p.state) ?? '?',
    restarts: num(p.restarts),
  };
}

export function readAgentCursor(ev: LoomEvent): AgentCursorView | null {
  const p = isObj(ev.payload) ? ev.payload : {};
  const to = u64(str(p.to_seq));
  if (to === null) return null;
  return {
    clientId: str(p.client_id) ?? ev.client_id ?? '?',
    fromSeq: u64(str(p.from_seq)) ?? 0n,
    toSeq: to,
    othersCount: num(p.others_count),
  };
}

/** GapCount: canonical decimal string or "unknown" (v1). Tolerates a number. */
export function gapCount(v: unknown): number | 'unknown' {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string' && /^\d{1,20}$/.test(v)) return Number(v);
  return 'unknown';
}

/**
 * v1 GapData: `seq_range{from,to}` (stream seqs) and/or `producer_range` (loss before
 * the hub). Producer-only losses keep firstSeq/lastSeq null — never invent stream seqs.
 */
export function readGap(ev: LoomEvent): GapView | null {
  if (ev.kind !== 'display.gap' && ev.kind !== 'record.gap' && ev.kind !== 'source.gap') return null;
  const p = (isObj(ev.payload) ? ev.payload : {}) as Partial<GapData>;
  const r = isObj(p.seq_range) ? p.seq_range : null;
  return {
    kind: ev.kind,
    firstSeq: r ? u64(r.from) : null,
    lastSeq: r ? u64(r.to) : null,
    count: gapCount(p.count),
    cause: typeof p.cause === 'string' ? p.cause : '?',
    domain: ev.domain,
    channel: ev.channel ?? null,
    producerOnly: !r && isObj(p.producer_range),
  };
}

/** Batch-level `display_gap` summary (EventsBatch.display_gap). */
export function readGapSummary(g: GapSummary | null | undefined): Omit<GapView, 'domain' | 'channel'> | null {
  if (!g) return null;
  return {
    kind: 'display.gap',
    firstSeq: g.seq_range ? u64(g.seq_range.from) : null,
    lastSeq: g.seq_range ? u64(g.seq_range.to) : null,
    count: gapCount(g.count),
    cause: g.causes?.join(',') ?? 'display',
    producerOnly: false,
  };
}

export function readRecordHealth(v: unknown): RecordHealth | null {
  return v === 'ok' || v === 'degraded' || v === 'failed' || v === 'disabled' ? v : null;
}

/** Heartbeat arrives on the control path; provisional: payload = Heartbeat fields. */
export function readHeartbeat(ev: LoomEvent): { recordHealth: RecordHealth | null; latch: boolean | null } {
  const p = isObj(ev.payload) ? ev.payload : {};
  return {
    recordHealth: readRecordHealth(p.record_health),
    latch: typeof p.latch === 'boolean' ? p.latch : null,
  };
}

/**
 * v1 `Viewers{subscribed, visible}` comes in the hello result. A live update event
 * is not in the contract yet; if one arrives as `core.viewers` its payload is read
 * with the same shape.
 */
export function readViewers(v: unknown): { subscribed: number; visible: number } | null {
  if (!isObj(v)) return null;
  const subscribed = v.subscribed;
  const visible = v.visible;
  if (typeof subscribed !== 'number') return null;
  return { subscribed, visible: typeof visible === 'number' ? visible : subscribed };
}

// ---- core.safe_state.list / run (A15 복구 송신, CR-UI-12) ----
// schema/v1 is canonical (`gen/safe_state_item.ts`, `gen/safe_state_run_result.ts`):
//   list → {items: [{id, domain, channel?, description, method, kind?, blocked?: bool}]} (CORE R1)
//   run  → {items: [{id, outcome: 'sent'|'skipped'|'failed', finally_blocked?, reason?, request_id?}]}
// Also accepted: the earlier replay bare array, provisional {items:[{label, kind, blocked: string}]} and
// {results:[{id, ok, blocked?, error?}]} (mock sends a superset).
// Explicit schema/v1 `kind` wins; for older items an id/method/description that
// says emergency/비상 is the emergency-OFF path; any other item is a recovery item. A held output
// with no emergency item shows "비상 OFF 경로 없음" (StopPanel) — never a guessed command.

export interface SafeStateItem {
  id: string;
  domain: string;
  channel: string | null;
  label: string;
  /** the RPC the daemon runs for it (schema/v1 `method`) */
  method: string | null;
  kind: 'recover' | 'emergency_off';
  /** interlock / deny reason when the item cannot run now */
  blocked: string | null;
}

export interface SafeStateResult {
  id: string;
  ok: boolean;
  outcome: 'sent' | 'skipped' | 'failed';
  blocked: string | null;
  error: string | null;
  finallyBlocked: boolean;
  requestId: string | null;
}

const EMERGENCY_RE = /emergency|비상/i;

export function readSafeStateList(res: unknown): SafeStateItem[] {
  const items = Array.isArray(res) ? res : isObj(res) && Array.isArray(res.items) ? res.items : [];
  return items.filter(isObj).flatMap((o) => {
    const id = str(o.id);
    if (!id) return [];
    const method = str(o.method);
    const label = str(o.description) ?? str(o.label) ?? id;
    const kind: SafeStateItem['kind'] =
      o.kind === 'emergency_off' || o.kind === 'recover'
        ? o.kind
        : EMERGENCY_RE.test(`${id} ${method ?? ''} ${label}`)
          ? 'emergency_off'
          : 'recover';
    const blocked = typeof o.blocked === 'boolean' ? (o.blocked ? (str(o.reason) ?? '인터록') : null) : str(o.blocked);
    return [
      {
        id,
        domain: str(o.domain) ?? (id.includes('.') ? (id.split('.')[0] as string) : '?'),
        channel: str(o.channel),
        label,
        method,
        kind,
        blocked,
      } satisfies SafeStateItem,
    ];
  });
}

export function readSafeStateRun(res: unknown): SafeStateResult[] {
  if (isObj(res) && Array.isArray(res.items)) {
    return res.items.filter(isObj).flatMap((o) => {
      const id = str(o.id);
      if (!id) return [];
      const outcome = o.outcome === 'sent' || o.outcome === 'skipped' || o.outcome === 'failed' ? o.outcome : 'failed';
      const reason = str(o.reason);
      const fb = o.finally_blocked === true;
      return [
        {
          id,
          ok: outcome === 'sent',
          outcome,
          blocked: outcome === 'skipped' ? (reason ?? '건너뜀') : null,
          error: outcome === 'failed' ? (reason ?? (str(o.outcome) ? null : '결과 형식 불명')) : null,
          finallyBlocked: fb,
          requestId: str(o.request_id),
        } satisfies SafeStateResult,
      ];
    });
  }
  const rs = isObj(res) && Array.isArray(res.results) ? res.results : [];
  return rs.filter(isObj).flatMap((o) => {
    const id = str(o.id);
    if (!id) return [];
    const ok = o.ok === true;
    const blocked = str(o.blocked);
    return [
      {
        id,
        ok,
        outcome: ok ? 'sent' : blocked ? 'skipped' : 'failed',
        blocked,
        error: str(o.error),
        finallyBlocked: false,
        requestId: str(o.request_id),
      } satisfies SafeStateResult,
    ];
  });
}
