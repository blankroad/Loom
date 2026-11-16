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
  // ---- operator upgrade B4 (U1/U5/U6/U8); null = not reported by this producer ----
  /** link generation of the last uds.state */
  generation: number | null;
  /** running TesterPresent job interval (ms) */
  tpIntervalMs: number | null;
  /** 3E 80 (true) / 3E 00 (false) */
  tpSuppress: boolean | null;
  /** owner_origin of the running TesterPresent job */
  tpOwner: string | null;
  /** why the last TesterPresent job stopped */
  tpStopCause: string | null;
  /** attached diagnostic DB (uds.state.db) */
  dbHash: string | null;
  dbEcu: string | null;
  dbDids: number | null;
}

export function emptyTarget(target: string): TargetView {
  return {
    target, link: '?', linkCause: null, backend: '?', addr: null, session: '?', sessionConfidence: null,
    security: '?', securityConfidence: null, tpOn: false, tpOk: 0, p2: null, p2Star: null,
    leaseOrigin: null, leaseActor: null, leaseClient: null,
    generation: null, tpIntervalMs: null, tpSuppress: null, tpOwner: null, tpStopCause: null, dbHash: null, dbEcu: null, dbDids: null,
  };
}

/** U8: how a row's route is shown. Unstamped rows stay unknown (never filled from current state). */
export interface RowRoute {
  text: string;
  title: string;
  known: boolean;
  generation: number | null;
}
export const ROUTE_UNRECORDED_TITLE = '요청 이벤트에 경로 미기록';

/** FUNCTIONS.md B4-F1 */
export function rowRoute(p: Readonly<Record<string, unknown>>): RowRoute {
  const g = p.generation;
  if (typeof g === 'number' && Number.isInteger(g) && g >= 0) {
    const backend = s(p.backend) ?? '?';
    const addr = s(p.addr);
    return {
      text: `${backend}${addr ? ' ' + addr : ''} · g${g}`,
      title: `요청 시점에 daemon이 기록한 경로 (generation ${g})`,
      known: true,
      generation: g,
    };
  }
  return { text: '?', title: ROUTE_UNRECORDED_TITLE, known: false, generation: null };
}

/** FUNCTIONS.md B4-F2: final state of a target removed with uds.forget. */
export function isForgotten(payload: unknown): boolean {
  return obj(payload).forgotten === true;
}

/** FUNCTIONS.md B4-F3a: successful keep-alives to show. Daemon jobs (B2: `suppress` reported) count each exchange both
 *  in `ok_count` and as a folded row → max; legacy producers (`tp{on,ok_count}` without rows for the same exchanges) → sum. */
export function tpOkCount(t: TargetView, tpFolded: number): number {
  return t.tpSuppress !== null ? Math.max(t.tpOk, tpFolded) : t.tpOk + tpFolded;
}

/** FUNCTIONS.md B4-F3: TesterPresent cell text (no double counting of state ok_count and folded rows). */
export function tpCellText(t: TargetView, tpFolded: number): string {
  if (t.tpOn) {
    const ok = tpOkCount(t, tpFolded);
    return t.tpIntervalMs !== null ? `on ${(t.tpIntervalMs / 1000).toFixed(1)}s ✓×${ok}` : `on ✓×${ok}`;
  }
  if (t.tpStopCause) return `off (${t.tpStopCause})${tpFolded ? ` ×${tpFolded}` : ''}`;
  return tpFolded > 0 ? `off ×${tpFolded}` : 'off';
}

/** U2 minimum: a confirmed `27 xx` (odd) row answered `67 xx seed…`. */
export interface SeedInfo {
  level: number;
  seedHex: string;
  /** all-zero seed = already unlocked (ISO 14229-1) */
  zero: boolean;
}

/** FUNCTIONS.md B4-F4 */
export function seedFromRow(r: RowLike): SeedInfo | null {
  if (!isFinal(r.state) || r.state === 'failed') return null;
  const req = reqHexOf(r);
  const m = req ? /^27([0-9A-F]{2})$/.exec(req) : null;
  if (!m) return null;
  const level = parseInt(m[1]!, 16);
  if (level % 2 === 0 || level < 0x01 || level > 0x7d) return null;
  const resp = respHexOf(r);
  if (!resp || !resp.startsWith(`67${m[1]}`) || resp.length < 6) return null;
  const seedHex = resp.slice(4);
  return { level, seedHex, zero: /^(00)+$/.test(seedHex) };
}

/** FUNCTIONS.md B4-F5: composer text for the matching sendKey; the `<key>` placeholder keeps it unsendable until typed. */
export function keyTemplate(level: number): string {
  return `27 ${((level + 1) & 0xff).toString(16).toUpperCase().padStart(2, '0')} <key>`;
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
  let tpIntervalMs = prev?.tpIntervalMs ?? null;
  let tpSuppress = prev?.tpSuppress ?? null;
  let tpOwner = prev?.tpOwner ?? null;
  let tpStopCause = prev?.tpStopCause ?? null;
  let dbHash = prev?.dbHash ?? null;
  let dbEcu = prev?.dbEcu ?? null;
  let dbDids = prev?.dbDids ?? null;
  if ('tester_present' in p) {
    const tp = obj(p.tester_present);
    tpOn = tp.active === true;
    tpOk = n(tp.ok_count) ?? 0;
    tpIntervalMs = tpOn ? n(tp.interval_ms) : null;
    tpSuppress = tpOn && typeof tp.suppress === 'boolean' ? tp.suppress : null;
    tpOwner = tpOn ? s(obj(tp.owner).owner_origin) : null;
    tpStopCause = s(p.tp_stop_cause);
    const db = 'db' in p && typeof p.db === 'object' && p.db !== null ? obj(p.db) : null;
    dbHash = db ? s(db.hash) : null;
    dbEcu = db ? s(db.ecu) : null;
    dbDids = db ? n(db.dids) : null;
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
    generation: n(p.generation) ?? prev?.generation ?? null,
    tpIntervalMs,
    tpSuppress,
    tpOwner,
    tpStopCause,
    dbHash,
    dbEcu,
    dbDids,
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
  const tp = t.tpOn ? ` TP ✓×${tpOkCount(t, tpFolded)}` : tpFolded ? ` TP ×${tpFolded}` : ' TP off';
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
  /** provenance: the transaction whose daemon-decoded response set this value */
  requestId: string;
  wallMs: number;
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
      requestId: r.requestId,
      wallMs: r.firstWallMs,
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
    testerPresent: tpCellText(t, tpFolded),
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

// ---- DTC pane (benchmark: Quick Test "Errors" / DTC Inspector). Codes stay raw (never SAE re-mapped);
// the status byte is additionally decoded into the 8 ISO 14229-1 DTCStatusMask bits (U3). ----
export interface DtcEntry {
  dtc: string;
  /** "0x09" or "—" (unchanged legacy text) */
  status: string;
  /** numeric status byte 0..255, null when the payload carried none / an unparsable value */
  statusByte: number | null;
  /** abbreviations of the SET bits in bit order 0→7 (e.g. ['TF','CDTC']); [] when statusByte is null */
  flags: string[];
}
export interface DtcView {
  requestId: string;
  target: string;
  entries: DtcEntry[];
  /** DTCStatusAvailabilityMask = 3rd byte of a `59 02 …` response; null when absent / not a 59 02 response */
  availabilityMask: number | null;
}

/** One ISO 14229-1 DTCStatusMask bit. */
export interface DtcStatusBit {
  bit: number;
  abbr: string;
  name: string;
}
/** Bit 0..7 in order. Abbreviations are fixed UI labels (contract-tested). */
export const DTC_STATUS_BITS: readonly DtcStatusBit[] = [
  { bit: 0, abbr: 'TF', name: 'testFailed' },
  { bit: 1, abbr: 'TFTOC', name: 'testFailedThisOperationCycle' },
  { bit: 2, abbr: 'PDTC', name: 'pendingDTC' },
  { bit: 3, abbr: 'CDTC', name: 'confirmedDTC' },
  { bit: 4, abbr: 'TNCSLC', name: 'testNotCompletedSinceLastClear' },
  { bit: 5, abbr: 'TFSLC', name: 'testFailedSinceLastClear' },
  { bit: 6, abbr: 'TNCTOC', name: 'testNotCompletedThisOperationCycle' },
  { bit: 7, abbr: 'WIR', name: 'warningIndicatorRequested' },
];

/** Per-bit decode of a status byte. `supported` = availability bit (null when the mask is unknown). */
export interface DtcFlag {
  bit: number;
  abbr: string;
  name: string;
  set: boolean;
  supported: boolean | null;
}

/** DTC pane filter (U3). 'all' keeps response order and every entry. */
export type DtcFilter = 'all' | 'confirmed' | 'testFailed' | 'pending';

/** FUNCTIONS.md B1-F1 */
export function dtcStatusFlags(status: number, availability: number | null): DtcFlag[] {
  return DTC_STATUS_BITS.map((b) => ({
    bit: b.bit,
    abbr: b.abbr,
    name: b.name,
    set: ((status >> b.bit) & 1) === 1,
    supported: availability === null ? null : ((availability >> b.bit) & 1) === 1,
  }));
}

/** FUNCTIONS.md B1-F2 */
export function dtcAvailabilityMask(r: RowLike): number | null {
  const hex = respHexOf(r);
  if (hex === null || hex.length < 6 || hex.slice(0, 2) !== '59') return null;
  if ((parseInt(hex.slice(2, 4), 16) & 0x7f) !== 0x02) return null;
  return parseInt(hex.slice(4, 6), 16);
}

/** FUNCTIONS.md B1-F3 */
export function filterDtcs(entries: readonly DtcEntry[], filter: DtcFilter): DtcEntry[] {
  if (filter === 'all') return entries.slice();
  const bit = filter === 'confirmed' ? 3 : filter === 'testFailed' ? 0 : 2;
  return entries.filter((e) => e.statusByte !== null && ((e.statusByte >> bit) & 1) === 1);
}

/** FUNCTIONS.md B1-F4: tooltip text of one DTC status cell (all 8 bits, one per line). */
export function dtcStatusTitle(status: number | null, availability: number | null): string {
  if (status === null) return 'status 미보고';
  const lines = [`status 0x${status.toString(16).toUpperCase().padStart(2, '0')} = 0b${status.toString(2).padStart(8, '0')}`];
  for (const f of dtcStatusFlags(status, availability)) {
    lines.push(`bit${f.bit} ${f.abbr} ${f.name}: ${f.set ? 1 : 0}${f.supported === false ? ' (ECU 미지원)' : ''}`);
  }
  lines.push(availability === null ? 'availability mask 미상' : `availability mask 0x${availability.toString(16).toUpperCase().padStart(2, '0')}`);
  return lines.join('\n');
}

// ---- U4 / U7 helpers ----

/** FUNCTIONS.md B1-F6: bytes per hex-dump row for a pane width in CSS px (0/unknown → 16). */
export function hexRowWidth(paneWidthPx: number): 8 | 16 {
  if (!Number.isFinite(paneWidthPx) || paneWidthPx <= 0) return 16;
  return paneWidthPx < 520 ? 8 : 16;
}

/** FUNCTIONS.md B1-F7: ECU logical address text from a daemon `addr` ("DoIP 0x1001" → "0x1001"), else null. */
export function laFromAddr(addr: string | null): string | null {
  if (!addr) return null;
  const m = /(?:^|\s)0x([0-9a-f]{1,4})$/i.exec(addr.trim());
  return m ? `0x${m[1]!.toUpperCase().padStart(4, '0')}` : null;
}

/** FUNCTIONS.md B1-F8: request-composer warning when the selected target's link is reported and not up. */
export function linkDownMessage(link: string, hasProfile: boolean): string {
  return hasProfile
    ? `링크 ${link} — 읽기 전용 (기록은 유지). [연결]로 다시 연결`
    : `링크 ${link} — 읽기 전용 (기록은 유지). 이 대상의 프로필 없음 — [이 대상으로 프로필 만들기] 후 [연결]`;
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
        const statusByte = st !== null && /^[0-9A-F]{2}$/.test(st) ? parseInt(st, 16) : null;
        const flags = statusByte === null ? [] : dtcStatusFlags(statusByte, null).filter((f) => f.set).map((f) => f.abbr);
        return { dtc: s(o.dtc) ?? '?', status: st ? `0x${st}` : '—', statusByte, flags };
      }),
      availabilityMask: dtcAvailabilityMask(r),
    };
  }
  return null;
}


// ---- Bench Light workbench helpers (selected web: work/reports/uds-read-did/app) ----
// Everything below projects the daemon row only. Rows carry no per-0x78 timestamps and no
// transport frames, so the response sequence is Tx · 0x78 ×N · final, and bytes are UDS PDUs.

/** compact uppercase hex of a row byte field (canonical `{encoding,data}` or legacy string), null if not clean hex */
export function rowHex(v: unknown): string | null {
  const t = bytesText(v);
  if (t === null) return null;
  const h = t.replace(/\s+/g, '');
  return /^([0-9A-F]{2})+$/.test(h) ? h : null;
}

export const spacedHex = (hex: string): string => hex.match(/../g)?.join(' ') ?? hex;

export const reqHexOf = (r: RowLike): string | null => rowHex(r.payload.bytes) ?? rowHex(r.payload.req_hex);
export const respHexOf = (r: RowLike): string | null => rowHex(r.payload.resp_bytes) ?? rowHex(r.payload.resp_hex);

/** requested DID of a single-DID ReadDataByIdentifier (22 xxxx), else null */
export function rowDid(r: RowLike): string | null {
  const h = reqHexOf(r);
  return h && /^22[0-9A-F]{4}$/.test(h) ? h.slice(2) : null;
}

/** "DID · SID" column: `F190 VIN` for a single-DID read, else `0x2E WriteDataByIdentifier`. */
export function didSidText(r: RowLike, didName: (did: string) => string | null): string {
  const did = rowDid(r);
  const p = r.payload;
  if (did) {
    const dec = Array.isArray(p.decoded) ? (p.decoded as unknown[]).map(obj).find((d) => s(d.did) === did) : undefined;
    const name = (dec && s(dec.name)) || didName(did) || '';
    return `${did} ${name}`.trim();
  }
  const sid = p.sid === undefined || p.sid === null ? (reqHexOf(r)?.slice(0, 2) ?? null) : hexByte(p.sid);
  return sid ? `0x${sid} ${s(p.service) ?? findTemplate(sid)?.name ?? ''}`.trim() : '—';
}

/** canonical history row class `s-<status>` */
export function statusClass(r: RowLike): string {
  const p = r.payload;
  if (isDenied(p)) return 's-nrc';
  if (!isFinal(r.state)) return (n(p.pending_count) ?? 0) > 0 ? 's-pending' : 's-waiting';
  const res = resultSymbol(r).cls;
  if (res === 'res-nrc' || res === 'res-fail') return 's-nrc';
  if (res === 'res-timeout') return 's-timeout';
  if (res === 'res-unknown') return 's-link-lost';
  if (p.ok_suppressed === true) return 's-suppressed';
  return 's-positive';
}

export interface SeqEvent {
  kind: 'tx' | 'pending' | 'positive' | 'nrc' | 'timeout' | 'link' | 'cancel' | 'late' | 'wait';
  dt: string;
  dir: string;
  label: string;
  /** compact hex of the UDS PDU, null when nothing was transmitted/received */
  hex: string | null;
  note?: string;
}

/** 응답 순서: Tx, then 0x78 ×N (count only — no per-event time on the row), then the final event. */
export function seqEvents(r: RowLike, nowMs: number): SeqEvent[] {
  const p = r.payload;
  const req = reqHexOf(r);
  const sid = req?.slice(0, 2) ?? hexByte(p.sid);
  const svc = s(p.service) ?? (sid ? (findTemplate(sid)?.name ?? `SID 0x${sid}`) : '요청');
  const out: SeqEvent[] = [{ kind: 'tx', dt: '+0', dir: 'Tx', label: svc, hex: req }];
  if (isDenied(p)) {
    const b = obj(p.block);
    out[0] = { ...out[0]!, note: '전송 안 됨' };
    out.push({ kind: 'nrc', dt: '', dir: '⛔', label: '거부 — 정책/차단, ECU로 보내지 않음', hex: null, note: `${s(b.class) ?? s(p.policy_action) ?? ''} ${s(b.reason) ?? ''}`.trim() || undefined });
    return out;
  }
  const pc = n(p.pending_count) ?? 0;
  if (pc > 0) out.push({ kind: 'pending', dt: '…', dir: 'Rx', label: `0x78 responsePending ×${pc}`, hex: sid ? `7F${sid}78` : null, note: '개별 수신 시각은 행에 기록되지 않음 — 횟수만 표시' });
  const rtt = n(p.rtt_ms);
  const dt = rtt !== null ? `+${Math.round(rtt * 10) / 10}ms` : '';
  const resp = respHexOf(r);
  if (!isFinal(r.state)) {
    out.push({ kind: 'wait', dt: `+${Math.max(0, Math.round(nowMs - r.firstSeenMs))}ms`, dir: '…', label: '최종 응답 대기 중', hex: null });
  } else if (p.ok_suppressed === true) {
    out.push({ kind: 'positive', dt, dir: '—', label: '응답 억제 (SPRMIB) — 긍정 응답 바이트 없음', hex: null });
  } else if (resp && resp.startsWith('7F') && resp.slice(4, 6) !== '78') {
    out.push({ kind: 'nrc', dt, dir: 'Rx', label: `NRC ${nrcText(p) ?? `0x${resp.slice(4, 6)}`}`, hex: resp });
  } else if (resp) {
    out.push({ kind: r.state === 'failed' ? 'nrc' : 'positive', dt, dir: 'Rx', label: `긍정 응답 0x${resp.slice(0, 2)}`, hex: resp });
  } else if (p.timeout === true) {
    const lim = n(p.p2_star_ms) ?? n(p.p2_ms);
    out.push({ kind: 'timeout', dt: lim !== null ? `+${lim}ms` : '', dir: '⌛', label: '타임아웃 — 응답 바이트 없음', hex: null });
  } else if (r.state === 'outcome_unknown') {
    out.push({ kind: 'link', dt: '', dir: '⌛?', label: '불확정 — 전송 후 결과 확인 불가 (응답 없음)', hex: null, note: s(p.reason) ?? undefined });
  } else if (r.state === 'failed') {
    out.push({ kind: 'nrc', dt, dir: '✗', label: nrcText(p) ? `NRC ${nrcText(p)}` : `실패${s(p.reason) ? ` — ${s(p.reason)}` : ''}`, hex: null });
  } else {
    out.push({ kind: 'positive', dt, dir: '✓', label: '확정 — 응답 바이트 미보고', hex: null });
  }
  if (p.late === true) out.push({ kind: 'late', dt: '', dir: 'Rx', label: 'late — 종료 후 도착, 무시됨', hex: null });
  return out;
}

export interface PduField {
  name: string;
  off: number;
  len: number;
  value: string;
  note: string;
  /** daemon-decoded value row (no byte range of its own) */
  decoded?: boolean;
  bad?: boolean;
}

/** Byte-level split of one UDS PDU. Request parameters follow the ISO 14229-1 template table;
 *  response values come only from the daemon's `decoded` list (no UI-side decoding). */
export function pduFields(hex: string, dir: 'tx' | 'rx', p: Readonly<Record<string, unknown>>): PduField[] {
  const b = hex.match(/../g) ?? [];
  if (!b.length) return [];
  const span = (off: number, len: number) => b.slice(off, off + len).join(' ');
  const out: PduField[] = [];
  if (dir === 'rx' && b[0] === '7F') {
    out.push({ name: 'negativeResponse', off: 0, len: 1, value: '0x7F', note: 'NRC 응답' });
    if (b.length > 1) out.push({ name: 'requestServiceId', off: 1, len: 1, value: `0x${b[1]}`, note: findTemplate(b[1]!)?.name ?? '' });
    if (b.length > 2) out.push({ name: 'negativeResponseCode', off: 2, len: 1, value: `0x${b[2]}`, note: b[2] === '78' ? 'responsePending' : (s(p.nrc_name) ?? ''), bad: b[2] !== '78' });
    if (b.length > 3) out.push({ name: '추가 바이트', off: 3, len: b.length - 3, value: span(3, b.length - 3), note: '형식 밖', bad: true });
    return out;
  }
  if (dir === 'rx') {
    const reqSid = ((parseInt(b[0]!, 16) - 0x40) & 0xff).toString(16).toUpperCase().padStart(2, '0');
    out.push({ name: 'responseServiceId', off: 0, len: 1, value: `0x${b[0]}`, note: `긍정 응답 (요청 SID 0x${reqSid} + 0x40)` });
    if (b[0] === '62' && b.length >= 3) {
      out.push({ name: 'dataIdentifier', off: 1, len: 2, value: b[1]! + b[2]!, note: '' });
      if (b.length > 3) out.push({ name: 'dataRecord', off: 3, len: b.length - 3, value: span(3, b.length - 3), note: `${b.length - 3} B` });
    } else if (b.length > 1) out.push({ name: 'responseParameters', off: 1, len: b.length - 1, value: span(1, b.length - 1), note: `${b.length - 1} B` });
    if (Array.isArray(p.decoded)) {
      for (const d of p.decoded) {
        const o = obj(d);
        const did = s(o.did);
        if (!did) continue;
        const v = o.value === undefined ? '' : typeof o.value === 'string' ? o.value : JSON.stringify(o.value);
        out.push({ name: `${did} ${s(o.name) ?? ''}`.trim(), off: -1, len: 0, value: `${v}${s(o.unit) ? ` ${s(o.unit)}` : ''}`, note: 'daemon 해석', decoded: true });
      }
    }
    return out;
  }
  const t = findTemplate(b[0]!);
  out.push({ name: 'requestServiceId', off: 0, len: 1, value: `0x${b[0]}`, note: t?.name ?? '카탈로그 밖 SID' });
  let off = 1;
  for (const prm of t?.params ?? []) {
    if (off >= b.length) break;
    const len = prm.bytes === null ? b.length - off : Math.min(prm.bytes, b.length - off);
    const v = span(off, len);
    const choice = prm.choices?.find((c) => c.value === v.replace(/\s/g, ''));
    out.push({ name: prm.name, off, len, value: v, note: choice?.label ?? '', bad: prm.bytes !== null && len < prm.bytes });
    off += len;
  }
  if (off < b.length) out.push({ name: t ? '추가 바이트' : 'parameters', off, len: b.length - off, value: span(off, b.length - off), note: `${b.length - off} B` });
  return out;
}
