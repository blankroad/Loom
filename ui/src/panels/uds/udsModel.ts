// UDS console model — pure functions over provisional payloads (uds-ui U1–U5).
//
// Provisional payloads (p0-common; confirm with UDS):
//   uds.request {target, sid, service, summary, req_hex, resp_hex?, nrc?, nrc_name?,
//                pending_count, rtt_ms?, p2_ms?, p2_star_ms?, decoded?[{did,name,value,unit}],
//                late?, ok_suppressed?, guard?:'deny', timeout?}  + request_id + write_state
//   uds.state   {target, link, backend, addr?, session{value,confidence}, security{level},
//                tp{on, ok_count}, p2_ms, p2_star_ms, lease{owner_origin, actor}}

import { isFinal, type LoomEvent, type RowLike } from '../../sdk';

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const s = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const n = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);

export function hexByte(v: unknown): string | null {
  if (typeof v === 'number') return v.toString(16).toUpperCase().padStart(2, '0');
  if (typeof v === 'string') return v.replace(/^0x/i, '').toUpperCase().padStart(2, '0');
  return null;
}

/** TesterPresent (0x3E) rows are folded into the target bar counter (uds-ui U1). */
export function isTesterPresent(r: { payload: Readonly<Record<string, unknown>> }): boolean {
  const p = r.payload;
  return hexByte(p.sid) === '3E' || p.service === 'TesterPresent' || p.purpose === 'tester_present';
}

/** Result symbol column (✓ ✗NRC ⏳ ⌛ ⌛? ⛔ late ok_suppressed). Stage chip is separate. */
export function resultSymbol(r: RowLike): { text: string; cls: string } {
  const p = r.payload;
  if (p.guard === 'deny' || p.denied === true) return { text: '⛔ 거부', cls: 'res-deny' };
  if (p.late === true) return { text: 'late', cls: 'res-late' };
  if (p.ok_suppressed === true) return { text: '✓ suppressed', cls: 'res-ok' };
  const nrc = hexByte(p.nrc);
  // Never paint a negative response as success: a final row that carries an NRC other
  // than 0x78 (responsePending) is shown as ✗ even if write_state says confirmed.
  if (nrc && nrc !== '78' && isFinal(r.state) && r.state !== 'outcome_unknown') {
    return { text: `✗ 0x${nrc} ${s(p.nrc_name) ?? ''}`.trim(), cls: 'res-nrc' };
  }
  switch (r.state) {
    case 'failed':
      if (nrc) return { text: `✗ 0x${nrc} ${s(p.nrc_name) ?? ''}`.trim(), cls: 'res-nrc' };
      return p.timeout === true ? { text: '⌛ timeout', cls: 'res-timeout' } : { text: '✗', cls: 'res-fail' };
    case 'outcome_unknown':
      return { text: '⌛? 불확정', cls: 'res-unknown' };
    case 'confirmed':
    case 'effective_verified':
      return { text: '✓', cls: 'res-ok' };
    case 'effect_unverified':
      return { text: '✓?', cls: 'res-ok' };
    case 'sent': {
      const pc = n(p.pending_count) ?? 0;
      return { text: pc > 0 ? `⏳ pending ×${pc}` : '⏳', cls: 'res-pending' };
    }
    default:
      return { text: '', cls: '' };
  }
}

/** p2/rtt column: final → "p2/rtt ms"; in flight → "elapsed/limit s" (limit = P2*). */
export function timingText(r: RowLike, nowMs: number): string {
  const p = r.payload;
  if (isFinal(r.state)) {
    const rtt = n(p.rtt_ms);
    const p2 = n(p.p2_ms);
    if (rtt === null) return '';
    return p2 !== null && p2 !== rtt ? `${p2}/${rtt}ms` : `${rtt}ms`;
  }
  const el = Math.max(0, (nowMs - r.stageSeenMs) / 1000);
  const lim = n(p.p2_star_ms) ?? n(p.p2_ms);
  return lim !== null ? `${el.toFixed(1)}s/${(lim / 1000).toFixed(1)}s` : `${el.toFixed(1)}s`;
}

export interface TargetView {
  target: string;
  link: string;
  backend: string;
  addr: string | null;
  session: string;
  sessionConfidence: string | null;
  security: string;
  tpOn: boolean;
  tpOk: number;
  p2: number | null;
  p2Star: number | null;
  leaseOrigin: string | null;
  leaseActor: string | null;
}

export function reduceTarget(prev: TargetView | undefined, ev: LoomEvent): TargetView | null {
  if (ev.kind !== 'uds.state') return null;
  const p = obj(ev.payload);
  const target = s(p.target) ?? ev.channel;
  if (!target) return null;
  const sess = obj(p.session);
  const sec = obj(p.security);
  const tp = obj(p.tp);
  const lease = obj(p.lease);
  return {
    target,
    link: s(p.link) ?? prev?.link ?? '?',
    backend: s(p.backend) ?? prev?.backend ?? '?',
    addr: s(p.addr) ?? prev?.addr ?? null,
    session: s(sess.value) ?? prev?.session ?? '?',
    sessionConfidence: s(sess.confidence) ?? prev?.sessionConfidence ?? null,
    security: s(sec.level) ?? prev?.security ?? '🔒',
    tpOn: typeof tp.on === 'boolean' ? tp.on : (prev?.tpOn ?? false),
    tpOk: n(tp.ok_count) ?? prev?.tpOk ?? 0,
    p2: n(p.p2_ms) ?? prev?.p2 ?? null,
    p2Star: n(p.p2_star_ms) ?? prev?.p2Star ?? null,
    leaseOrigin: 'lease' in p ? s(lease.owner_origin) : (prev?.leaseOrigin ?? null),
    leaseActor: 'lease' in p ? s(lease.actor) : (prev?.leaseActor ?? null),
  };
}

const CONF: Record<string, string> = { confirmed: '✓', assumed: '?', stale: '!' };
const ORG: Record<string, string> = { agent: 'AGT', user: 'USR', script: 'SCR', device: 'DEV' };

export function targetLine(t: TargetView, tpFolded: number, env: string): string {
  const conf = t.sessionConfidence ? (CONF[t.sessionConfidence] ?? '') : '';
  const p2 = t.p2 !== null ? ` P2 ${t.p2}/${t.p2Star ?? '?'}` : '';
  const tp = t.tpOn ? ` TP ✓×${t.tpOk + tpFolded}` : tpFolded ? ` TP ×${tpFolded}` : ' TP off';
  return `${t.target} ● ${t.backend}${t.addr ? ` ${t.addr}` : ''} sess=${t.session}${conf} sec=${t.security}${tp}${p2} [${env.toUpperCase()}]`;
}

export function leaseText(t: TargetView, myConnId: string | null): string {
  if (!t.leaseOrigin) return '제어: 비어 있음';
  const who = ORG[t.leaseOrigin] ?? t.leaseOrigin;
  const me = t.leaseOrigin === 'user' && t.leaseActor !== null && t.leaseActor === myConnId;
  return me ? '제어: 나' : `제어: ${who}${t.leaseActor ? ` ${t.leaseActor}` : ''}`;
}

export interface LastValue {
  target: string;
  did: string;
  name: string;
  value: string;
  unit: string;
  origin: string;
  atMs: number;
}

export const MAX_LAST_VALUES = 256;

/** Last-value table (uds-ui U5 ①): every confirmed response's decoded fields. LRU-capped. */
export function updateLastValues(map: Map<string, LastValue>, r: RowLike, nowMs: number): void {
  if (!isFinal(r.state) || r.state === 'failed') return;
  const dec = r.payload.decoded;
  if (!Array.isArray(dec)) return;
  const target = s(r.payload.target) ?? r.channel ?? '?';
  for (const d of dec) {
    const o = obj(d);
    const did = s(o.did);
    if (!did) continue;
    const key = `${target}:${did}`;
    map.delete(key); // re-insert = most recent last
    map.set(key, {
      target,
      did,
      name: s(o.name) ?? '',
      value: o.value === undefined ? '' : typeof o.value === 'string' ? o.value : JSON.stringify(o.value),
      unit: s(o.unit) ?? '',
      origin: r.origin,
      atMs: nowMs,
    });
    if (map.size > MAX_LAST_VALUES) {
      const oldest = map.keys().next().value;
      if (oldest !== undefined) map.delete(oldest);
    }
  }
}

/** Command line: hex only in M1 ("22 F190" → "22F190"). */
export function normalizeHex(text: string): string | null {
  const h = text.replace(/\s+/g, '').toUpperCase();
  if (h.length === 0 || h.length % 2 !== 0 || !/^[0-9A-F]+$/.test(h)) return null;
  return h;
}

const RISK_RANK: Record<string, number> = { R0: 0, R1: 1, R2: 2, R3: 3, R4: 4 };
/** Mistake prevention (uds-ui U3): R2+ preview needs a second Enter. Policy stays server-side (F1). */
export function needsSecondEnter(risk: unknown): boolean {
  return typeof risk === 'string' && (RISK_RANK[risk] ?? 0) >= 2;
}
