// UDS console model — pure functions over uds.* payloads (uds-ui U1–U5).
//
// Canonical payloads: crates/loom-uds/src/payload.rs (fixtures: crates/loom-uds/fixtures/*.json)
//   rows (uds.tx/pending/result) {target, write_state, bytes{encoding,data}, sid, service?,
//        summary, risk, policy_action, block?{class,reason}, pending_count, p2_ms, p2_star_ms,
//        resp_bytes?, nrc?(number), nrc_name?, timeout, ok_suppressed, rtt_ms?, decoded?, dtcs?}
//   uds.state {target, generation, backend|null, addr|null, link{state,cause?},
//        session{id,name,confidence}, security{level:number|null,confidence},
//        tester_present{active,interval_ms,ok_count}|null, p2_ms, p2_star_ms,
//        lease{owner_origin,actor_id,client_id}|null}
// Canonical fields win; the older mock/replay shape (req_hex/resp_hex, link string,
// session.value, security.level string, tp{on}, lease.actor, guard:'deny') is still read.

import { isFinal, type LoomEvent, type RowLike } from '../../sdk';

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const s = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const n = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);

export function hexByte(v: unknown): string | null {
  if (typeof v === 'number') return v.toString(16).toUpperCase().padStart(2, '0');
  if (typeof v === 'string') return v.replace(/^0x/i, '').toUpperCase().padStart(2, '0');
  return null;
}

/** `{encoding:'hex', data}` (canonical) or a bare hex string (legacy) → "22 F1 90". */
export function bytesText(v: unknown): string | null {
  let hex: string;
  if (typeof v === 'string') hex = v;
  else {
    const o = obj(v);
    const data = s(o.data);
    if (data === null) return null;
    if (o.encoding !== undefined && o.encoding !== 'hex') return `[${String(o.encoding)}] ${data}`; // shown as-is, never re-decoded
    hex = data;
  }
  const h = hex.replace(/\s+/g, '').toUpperCase();
  if (h.length === 0) return null;
  if (h.length % 2 !== 0 || !/^[0-9A-F]+$/.test(h)) return hex; // not clean hex: show the raw text honestly
  return h.match(/../g)!.join(' ');
}

/** "0x31 requestOutOfRange" from a numeric (canonical) or string (legacy) NRC. */
export function nrcText(p: Readonly<Record<string, unknown>>): string | null {
  const nrc = p.nrc === null || p.nrc === undefined ? null : hexByte(p.nrc);
  if (nrc === null) return null;
  return `0x${nrc} ${s(p.nrc_name) ?? ''}`.trim();
}

/** `19 02` DTC list: "923400 st=0x09 · C10000 st=0x2F" (raw 24-bit codes, never re-mapped). */
export function dtcText(p: Readonly<Record<string, unknown>>): string | null {
  if (!Array.isArray(p.dtcs)) return null;
  if (p.dtcs.length === 0) return '없음';
  return p.dtcs
    .map((d) => {
      const o = obj(d);
      const st = hexByte(o.status);
      return `${s(o.dtc) ?? '?'}${st ? ` st=0x${st}` : ''}`;
    })
    .join(' · ');
}

/** One-line dry-run preview from `uds.request{dry_run:true}` (DryRunResult; bare or `{data}`). */
export function previewText(res: unknown): { text: string; risk: unknown } {
  let r = obj(res);
  if (!('dry_run' in r) && typeof r.data === 'object' && r.data !== null) r = obj(r.data);
  const pe = obj(r.parse_error);
  if (r.parse_error) {
    const at = n(pe.at);
    return { text: `구문 오류: ${s(pe.message) ?? '?'}${at !== null ? ` (위치 ${at})` : ''}`, risk: null };
  }
  const denied = isDenied(r);
  const risk = r.risk ?? null;
  const text = [
    denied ? '⛔ 거부' : '',
    s(r.service) ?? (r.sid !== undefined && r.sid !== null ? `SID 0x${hexByte(r.sid) ?? '?'}` : ''),
    s(r.did_name) ?? '',
    typeof risk === 'string' ? `· ${risk}` : '',
    s(r.reason) ?? '',
  ]
    .filter(Boolean)
    .join(' ');
  return { text, risk };
}

/** Refused before sending: canonical `block`/`policy_action:'deny'`, legacy `guard:'deny'`/`denied`. */
export function isDenied(p: Readonly<Record<string, unknown>>): boolean {
  return p.guard === 'deny' || p.denied === true || p.policy_action === 'deny' || (typeof p.block === 'object' && p.block !== null);
}

/** TesterPresent (0x3E) rows are folded into the target bar counter (uds-ui U1). */
export function isTesterPresent(r: { payload: Readonly<Record<string, unknown>> }): boolean {
  const p = r.payload;
  return hexByte(p.sid) === '3E' || p.service === 'TesterPresent' || p.purpose === 'tester_present';
}

/** Result symbol column (✓ ✗NRC ⏳ ⌛ ⌛? ⛔ late ok_suppressed). Stage chip is separate. */
export function resultSymbol(r: RowLike): { text: string; cls: string } {
  const p = r.payload;
  if (isDenied(p)) return { text: '⛔ 거부', cls: 'res-deny' };
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
    const rawRtt = n(p.rtt_ms);
    const p2 = n(p.p2_ms);
    if (rawRtt === null) return '';
    const rtt = Math.round(rawRtt * 10) / 10; // measured ms arrive as floats (0.9, 2010.0)
    return p2 !== null && p2 !== rtt ? `${p2}/${rtt}ms` : `${rtt}ms`;
  }
  const el = Math.max(0, (nowMs - r.stageSeenMs) / 1000);
  const lim = n(p.p2_star_ms) ?? n(p.p2_ms);
  return lim !== null ? `${el.toFixed(1)}s/${(lim / 1000).toFixed(1)}s` : `${el.toFixed(1)}s`;
}

export interface TargetView {
  target: string;
  link: string;
  linkCause: string | null;
  backend: string;
  addr: string | null;
  session: string;
  sessionConfidence: string | null;
  security: string;
  securityConfidence: string | null;
  tpOn: boolean;
  tpOk: number;
  p2: number | null;
  p2Star: number | null;
  leaseOrigin: string | null;
  leaseActor: string | null;
  /** conn_id of the lease holder (canonical `lease.client_id`) */
  leaseClient: string | null;
}

export function emptyTarget(target: string): TargetView {
  return {
    target, link: '?', linkCause: null, backend: '?', addr: null, session: '?', sessionConfidence: null,
    security: '?', securityConfidence: null, tpOn: false, tpOk: 0, p2: null, p2Star: null,
    leaseOrigin: null, leaseActor: null, leaseClient: null,
  };
}

/** Canonical numeric level (null = locked) or legacy display string. */
function securityText(sec: Record<string, unknown>, prev: string | undefined): string {
  if (!('level' in sec)) return prev ?? '?'; // not reported: unknown, never assume locked
  // 0x00 is ISO-reserved (no unlocked level 0); the mock uses 0 for locked
  if (sec.level === null || sec.level === 0) return '🔒';
  if (typeof sec.level === 'number') return `L${sec.level}🔓`;
  return s(sec.level) ?? prev ?? '?';
}

export function reduceTarget(prev: TargetView | undefined, ev: LoomEvent): TargetView | null {
  if (ev.kind !== 'uds.state') return null;
  const p = obj(ev.payload);
  const target = s(p.target) ?? ev.channel;
  if (!target) return null;
  const link = obj(p.link);
  const sess = obj(p.session);
  const sec = obj(p.security);
  const lease = obj(p.lease);
  // TesterPresent: canonical `tester_present` (null = off) wins over legacy `tp{on}`
  let tpOn = prev?.tpOn ?? false;
  let tpOk = prev?.tpOk ?? 0;
  if ('tester_present' in p) {
    const tp = obj(p.tester_present);
    tpOn = tp.active === true;
    tpOk = n(tp.ok_count) ?? 0;
  } else if ('tp' in p) {
    const tp = obj(p.tp);
    if (typeof tp.on === 'boolean') tpOn = tp.on;
    tpOk = n(tp.ok_count) ?? tpOk;
  }
  const linkState = s(link.state) ?? s(p.link);
  return {
    target,
    link: linkState ?? prev?.link ?? '?',
    linkCause: 'link' in p ? s(link.cause) : (prev?.linkCause ?? null),
    backend: s(p.backend) ?? prev?.backend ?? '?',
    addr: s(p.addr) ?? prev?.addr ?? null,
    session: s(sess.name) ?? s(sess.value) ?? prev?.session ?? '?',
    sessionConfidence: s(sess.confidence) ?? prev?.sessionConfidence ?? null,
    security: securityText(sec, prev?.security),
    securityConfidence: s(sec.confidence) ?? prev?.securityConfidence ?? null,
    tpOn,
    tpOk,
    p2: n(p.p2_ms) ?? prev?.p2 ?? null,
    p2Star: n(p.p2_star_ms) ?? prev?.p2Star ?? null,
    leaseOrigin: 'lease' in p ? s(lease.owner_origin) : (prev?.leaseOrigin ?? null),
    leaseActor: 'lease' in p ? (s(lease.actor_id) ?? s(lease.actor)) : (prev?.leaseActor ?? null),
    leaseClient: 'lease' in p ? s(lease.client_id) : (prev?.leaseClient ?? null),
  };
}

const CONF: Record<string, string> = { confirmed: '✓', assumed: '?', stale: '!' };
const ORG: Record<string, string> = { agent: 'AGT', user: 'USR', script: 'SCR', device: 'DEV' };
const LINK: Record<string, string> = { up: '●', connecting: '◌', down: '○' };

export function targetLine(t: TargetView, tpFolded: number, env: string): string {
  const conf = t.sessionConfidence ? (CONF[t.sessionConfidence] ?? '') : '';
  const secConf = t.securityConfidence ? (CONF[t.securityConfidence] ?? '') : '';
  // never paint an unreported link as up; down/connecting carry their cause
  const link = t.link === 'up' ? '●' : t.link === '?' ? '?' : `${LINK[t.link] ?? '?'} ${t.link}${t.linkCause ? `(${t.linkCause})` : ''}`;
  const p2 = t.p2 !== null ? ` P2 ${t.p2}/${t.p2Star ?? '?'}` : '';
  const tp = t.tpOn ? ` TP ✓×${t.tpOk + tpFolded}` : tpFolded ? ` TP ×${tpFolded}` : ' TP off';
  return `${t.target} ${link} ${t.backend}${t.addr ? ` ${t.addr}` : ''} sess=${t.session}${conf} sec=${t.security}${secConf}${tp}${p2} [${env.toUpperCase()}]`;
}

export function leaseText(t: TargetView, myConnId: string | null): string {
  if (!t.leaseOrigin) return '제어: 비어 있음';
  const who = ORG[t.leaseOrigin] ?? t.leaseOrigin;
  // canonical: lease.client_id is the holder's conn_id; legacy mocks put the conn_id in `actor`
  const holder = t.leaseClient ?? t.leaseActor;
  const me = t.leaseOrigin === 'user' && myConnId !== null && holder === myConnId;
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
