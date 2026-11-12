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
  return reduceTargetState(prev, ev.payload, ev.channel);
}

/** State projection shared by native snapshots and live events; creates no history. */
export function reduceTargetState(prev: TargetView | undefined, payload: unknown, channel?: string | null): TargetView | null {
  const p = obj(payload);
  const target = s(p.target) ?? channel;
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

// ---- Diagnostic Services catalog (benchmark: DTS.monaco "Diagnostic Services" tree) ----
// MAIN D1: static ISO 14229-1 request *templates* for the composer only. Picking or editing a
// template only rewrites the canonical hex line; it never sends and never labels a
// transaction (rows keep the daemon-supplied `service`). Only SIDs the daemon risk table
// classifies (crates/loom-uds/src/risk.rs) and that are not R4 transfer/flash, immutable-deny
// (0x2A/0x86) or TesterPresent (0x3E, owned by the target TesterPresent cycle) are listed.

export interface ParamChoice {
  value: string;
  label: string;
}
export interface TemplateParam {
  key: string;
  /** ISO 14229-1 parameter name */
  name: string;
  /** fixed byte length; null = variable-length tail (rest of the PDU) */
  bytes: number | null;
  /** default hex ('' = must be filled by the user) */
  def: string;
  choices?: ParamChoice[];
  optional?: boolean;
}
export interface ServiceTemplate {
  sid: string;
  name: string;
  group: string;
  params: TemplateParam[];
}

export const SERVICE_GROUPS = ['Diagnostic and Communication Management', 'Data Transmission', 'Stored Data Transmission', 'Routine'] as const;

export const SERVICE_CATALOG: readonly ServiceTemplate[] = [
  {
    sid: '10', name: 'DiagnosticSessionControl', group: SERVICE_GROUPS[0],
    params: [{ key: 'type', name: 'diagnosticSessionType', bytes: 1, def: '03', choices: [
      { value: '01', label: 'defaultSession' }, { value: '02', label: 'programmingSession' }, { value: '03', label: 'extendedDiagnosticSession' }] }],
  },
  {
    sid: '11', name: 'ECUReset', group: SERVICE_GROUPS[0],
    params: [{ key: 'type', name: 'resetType', bytes: 1, def: '01', choices: [
      { value: '01', label: 'hardReset' }, { value: '02', label: 'keyOffOnReset' }, { value: '03', label: 'softReset' }] }],
  },
  {
    sid: '27', name: 'SecurityAccess', group: SERVICE_GROUPS[0],
    params: [
      { key: 'type', name: 'securityAccessType', bytes: 1, def: '01' },
      { key: 'key', name: 'securityKey', bytes: null, def: '', optional: true },
    ],
  },
  {
    sid: '28', name: 'CommunicationControl', group: SERVICE_GROUPS[0],
    params: [
      { key: 'ctl', name: 'controlType', bytes: 1, def: '00', choices: [
        { value: '00', label: 'enableRxAndTx' }, { value: '01', label: 'enableRxAndDisableTx' },
        { value: '02', label: 'disableRxAndEnableTx' }, { value: '03', label: 'disableRxAndTx' }] },
      { key: 'comm', name: 'communicationType', bytes: 1, def: '01' },
    ],
  },
  {
    sid: '85', name: 'ControlDTCSetting', group: SERVICE_GROUPS[0],
    params: [{ key: 'type', name: 'DTCSettingType', bytes: 1, def: '01', choices: [{ value: '01', label: 'on' }, { value: '02', label: 'off' }] }],
  },
  { sid: '22', name: 'ReadDataByIdentifier', group: SERVICE_GROUPS[1], params: [{ key: 'did', name: 'dataIdentifier', bytes: 2, def: 'F190' }] },
  {
    sid: '2E', name: 'WriteDataByIdentifier', group: SERVICE_GROUPS[1],
    params: [{ key: 'did', name: 'dataIdentifier', bytes: 2, def: '' }, { key: 'rec', name: 'dataRecord', bytes: null, def: '' }],
  },
  { sid: '14', name: 'ClearDiagnosticInformation', group: SERVICE_GROUPS[2], params: [{ key: 'group', name: 'groupOfDTC', bytes: 3, def: 'FFFFFF' }] },
  {
    sid: '19', name: 'ReadDTCInformation', group: SERVICE_GROUPS[2],
    params: [
      { key: 'type', name: 'reportType', bytes: 1, def: '02', choices: [
        { value: '01', label: 'reportNumberOfDTCByStatusMask' }, { value: '02', label: 'reportDTCByStatusMask' }] },
      { key: 'mask', name: 'DTCStatusMask', bytes: 1, def: 'FF' },
    ],
  },
  {
    sid: '31', name: 'RoutineControl', group: SERVICE_GROUPS[3],
    params: [
      { key: 'type', name: 'routineControlType', bytes: 1, def: '01', choices: [
        { value: '01', label: 'startRoutine' }, { value: '02', label: 'stopRoutine' }, { value: '03', label: 'requestRoutineResults' }] },
      { key: 'rid', name: 'routineIdentifier', bytes: 2, def: '' },
      { key: 'opt', name: 'routineControlOptionRecord', bytes: null, def: '', optional: true },
    ],
  },
];

export const templateLabel = (t: ServiceTemplate): string => `[${t.sid}] ${t.name}`;

export function findTemplate(sid: string | null): ServiceTemplate | null {
  if (!sid) return null;
  const u = sid.toUpperCase();
  return SERVICE_CATALOG.find((t) => t.sid === u) ?? null;
}

/** Catalog search: SID ("22", "0x22") or case-insensitive name substring. */
export function filterCatalog(q: string): ServiceTemplate[] {
  const s0 = q.trim().toLowerCase().replace(/^0x/, '');
  if (!s0) return [...SERVICE_CATALOG];
  return SERVICE_CATALOG.filter((t) => t.sid.toLowerCase() === s0 || t.name.toLowerCase().includes(s0));
}

export type ParamValues = Record<string, string>;

export const defaultValues = (t: ServiceTemplate): ParamValues => Object.fromEntries(t.params.map((p) => [p.key, p.def]));

const cleanHex = (v: string): string => v.replace(/\s+/g, '').replace(/^0x/i, '').toUpperCase();

/** Template + values → canonical hex. Bytes are written in order and stop at the first
 *  empty parameter (the request stays honestly incomplete; the daemon dry-run reports it). */
export function buildPdu(t: ServiceTemplate, values: ParamValues): string {
  let out = t.sid;
  for (const p of t.params) {
    const v = cleanHex(values[p.key] ?? '');
    if (!v) break;
    out += v;
  }
  return out.match(/../g)?.join(' ') ?? out;
}

/** Hex line → template values (composer editing only — never used to label a transaction). */
export function parsePdu(t: ServiceTemplate, hex: string): ParamValues {
  const h = cleanHex(hex);
  const out: ParamValues = Object.fromEntries(t.params.map((p) => [p.key, '']));
  let i = 2;
  for (const p of t.params) {
    if (i >= h.length) break;
    const take = p.bytes === null ? h.length - i : p.bytes * 2;
    out[p.key] = h.slice(i, i + take);
    i += take;
  }
  return out;
}

/** Parameters that are required but still empty or of the wrong length. */
export function paramIssues(t: ServiceTemplate, values: ParamValues): string[] {
  const out: string[] = [];
  for (const p of t.params) {
    const v = cleanHex(values[p.key] ?? '');
    if (!v) {
      if (!p.optional) out.push(`${p.name} 비어 있음`);
    } else if (!/^[0-9A-F]*$/.test(v) || v.length % 2 !== 0) out.push(`${p.name} hex 아님`);
    else if (p.bytes !== null && v.length !== p.bytes * 2) out.push(`${p.name} ${p.bytes}바이트 필요`);
  }
  return out;
}

// ---- Logical-link grid (benchmark: Logical Link List / Quick Test "ECUs") ----
export interface TargetCells {
  link: { text: string; cls: string; title: string };
  backend: string;
  addr: string;
  session: string;
  security: string;
  /** TesterPresent (0x3E) keep-alive — not an ISO-TP / CAN-TP transport state */
  testerPresent: string;
  p2: string;
  env: string;
}

const confMark = (c: string | null): string => (c ? (CONF[c] ?? '') : '');

export function targetCells(t: TargetView, tpFolded: number, env: string): TargetCells {
  const linkText = t.link === 'up' ? '● up' : t.link === '?' ? '? 미보고' : `${LINK[t.link] ?? '?'} ${t.link}${t.linkCause ? ` (${t.linkCause})` : ''}`;
  return {
    link: { text: linkText, cls: `uds-ds-link-${t.link === '?' ? 'unknown' : t.link}`, title: t.linkCause ?? '' },
    backend: t.backend,
    addr: t.addr ?? '—',
    session: `${t.session}${confMark(t.sessionConfidence)}`,
    security: `${t.security}${confMark(t.securityConfidence)}`,
    testerPresent: t.tpOn ? `on ✓×${t.tpOk + tpFolded}` : tpFolded ? `off ×${tpFolded}` : 'off',
    p2: t.p2 !== null ? `${t.p2}/${t.p2Star ?? '?'}ms` : '—',
    env: env.toUpperCase(),
  };
}

// ---- transaction block (benchmark: Diagnostic Services "Process Values" block order) ----
// MAIN D4: rendered through the existing RowTable detail pairs. Every value comes from the
// row/target payload; nothing is decoded from a UI table.
const STAGE_TEXT: Record<string, string> = {
  requested: '요청됨', accepted: '수락됨', sent: '보냄 (응답 대기)', confirmed: '확정',
  effective_verified: '검증됨', effect_unverified: '미검증', failed: '실패', outcome_unknown: '불확정',
};

export function detailPairs(r: RowLike, target: TargetView | undefined): [string, string][] {
  const p = r.payload;
  const t = s(p.target) ?? r.channel ?? '?';
  const svc = s(p.service);
  const sid = p.sid === undefined || p.sid === null ? null : hexByte(p.sid);
  const res = resultSymbol(r).text;
  const pc = n(p.pending_count) ?? 0;
  const state = [STAGE_TEXT[r.state] ?? r.state, pc > 0 ? `pending(0x78) ×${pc}` : '', p.timeout === true ? 'timeout' : '', p.late === true ? 'late' : '']
    .filter(Boolean)
    .join(' · ');
  const out: [string, string][] = [
    ['테스터', `${r.origin}${r.clientId ? ` ${r.clientId}` : ''}`],
    ['서비스', `${svc ?? (sid ? `SID 0x${sid}` : '—')}${res ? ` — ${res}` : ''}`],
    ['요청', bytesText(p.bytes) ?? bytesText(p.req_hex) ?? '—'],
    ['논리 링크', target ? `${t} · ${target.backend}${target.addr ? ` ${target.addr}` : ''}` : `${t} · ?`],
    ['응답 상태', state],
    ['응답', bytesText(p.resp_bytes) ?? bytesText(p.resp_hex) ?? '—'],
    ['NRC', nrcText(p) ?? '—'],
  ];
  if (Array.isArray(p.decoded)) {
    for (const d of p.decoded) {
      const o = obj(d);
      const did = s(o.did);
      if (!did) continue;
      const v = o.value === undefined ? '' : typeof o.value === 'string' ? o.value : JSON.stringify(o.value);
      out.push([`  DID ${did}${s(o.name) ? ` ${s(o.name)}` : ''}`, `${v}${s(o.unit) ? ` ${s(o.unit)}` : ''}`]);
    }
  }
  const dtcs = dtcText(p);
  if (dtcs !== null) out.push(['DTC', dtcs]);
  const block = p.block && typeof p.block === 'object' ? obj(p.block) : null;
  if (block) out.push(['차단', `${s(block.class) ?? ''} ${s(block.reason) ?? ''}`.trim()]);
  out.push(
    ['p2/rtt', isFinal(r.state) ? timingText(r, 0) || '—' : '진행 중'],
    ['등급', r.risk ?? (typeof p.risk === 'string' ? p.risk : 'R?')],
    ['request_id', r.requestId],
    ['갱신', `${r.updates}회 · ${r.state}`],
  );
  return out;
}

// ---- DTC pane (benchmark: Quick Test "Errors"; MAIN D2: raw code + status byte only) ----
export interface DtcEntry {
  dtc: string;
  status: string;
}
export interface DtcView {
  requestId: string;
  target: string;
  entries: DtcEntry[];
}

/** Latest final, non-failed row carrying `payload.dtcs` for a target (raw, never re-mapped). */
export function latestDtcs(rows: readonly RowLike[], target: string | null): DtcView | null {
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]!;
    const t = s(r.payload.target) ?? r.channel ?? null;
    if (target !== null && t !== target) continue;
    if (!isFinal(r.state) || r.state === 'failed' || !Array.isArray(r.payload.dtcs)) continue;
    return {
      requestId: r.requestId,
      target: t ?? '?',
      entries: (r.payload.dtcs as unknown[]).map((d) => {
        const o = obj(d);
        const st = hexByte(o.status);
        return { dtc: s(o.dtc) ?? '?', status: st ? `0x${st}` : '—' };
      }),
    };
  }
  return null;
}
