// eth.packets model — pure readers over provisional payloads (eth-ui, eth/plan §4–6).
// Interpretation strings (info, tree labels, NRC names) are the DOMAIN's text; the UI
// never decodes packets.
//
// Provisional shapes (ask ETH — report p1-eth.md §4):
//   eth.pkt payload  {ref?, src?, dst?, proto, len, info, attributed_origin?, layers?,
//                     link?{domain:'uds', request_id, role, confidence, target?},
//                     tx_request_id?}   (seq/ts/dir/origin/channel from the record)
//   eth.query rows   = CLI --json rows {ref, seq, ts, dir, origin, attributed_origin, len,
//                     proto, info, layers, link}
//   eth.batch        {covered_first_seq, covered_last_seq, total, shown}  (UI ask ②)
//                    or {first_src, last_src, count, by_proto}             (eth/plan §6)
//   eth.link         {eth_ref, request_id, role, confidence, domain?, target?}
//   eth.capture_state / eth.stats: canonical in crates/loom-eth/src/model.rs (see status
//                     section below; legacy mock shapes still accepted)

import { parseEvidenceRef, type LoomEvent, type Origin } from '../../sdk';

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const s = (v: unknown): string | null => (typeof v === 'string' ? v : typeof v === 'number' ? String(v) : null);
const n = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : typeof v === 'string' && /^\d+$/.test(v) ? Number(v) : null);
const big = (v: unknown): bigint | null => (typeof v === 'string' && /^\d{1,20}$/.test(v) ? BigInt(v) : typeof v === 'number' ? BigInt(Math.trunc(v)) : null);
const ORIGINS: readonly Origin[] = ['agent', 'user', 'device', 'script'];

export interface PktLink {
  requestId: string;
  role: string;
  confidence: 'exact' | 'probable';
  target: string | null;
}

export interface PktRow {
  key: string;
  kind: 'pkt' | 'gap';
  seq: bigint;
  ref: string;
  wallNs: bigint;
  monoNs: bigint;
  tsSource: string;
  dir: 'rx' | 'tx' | null;
  /** badge origin = attributed_origin when present (eth/plan §3.6) */
  badge: Origin;
  attributed: boolean;
  src: string;
  dst: string;
  proto: string;
  len: number;
  info: string;
  color: string;
  link: PktLink | null;
  txRequestId: string | null;
  /** daemon conversation index (record payload `stream`) — key for eth.follow */
  stream: number | null;
  /** (B4a) index space of `stream` (payload `stream_proto`); null = absent (older records: tcp) */
  streamProto: 'tcp' | 'udp' | null;
  /** (B4b) the daemon re-decoded this row with a Decode As rule (query row `decode_as_applied === true`) */
  decodeAsApplied: boolean;
  gap: { kind: string; count: number | bigint | 'unknown'; cause: string; firstSeq: bigint | null; lastSeq: bigint | null } | null;
  /** rough retained size for the 4 MiB panel budget */
  bytesEst: number;
}

function readLink(v: unknown): PktLink | null {
  const l = obj(v);
  const rid = s(l.request_id);
  if (!rid || (l.domain !== undefined && l.domain !== 'uds')) return null;
  return {
    requestId: rid,
    role: s(l.role) ?? '',
    confidence: l.confidence === 'exact' ? 'exact' : 'probable',
    target: s(l.target),
  };
}

/** Colour rule (eth-ui defaults): gap > UDS NRC > TCP RST/retrans > SOME/IP-SD > DoIP > ARP. */
export function colorOf(proto: string, layers: Record<string, unknown>): string {
  const uds = obj(layers.uds);
  const tcp = obj(layers.tcp);
  const flags = obj(tcp.flags);
  const p = proto.toLowerCase();
  if (uds.nrc !== undefined || s(uds.sid)?.toLowerCase() === '0x7f') return 'c-nrc';
  if (tcp.flags_rst === true || flags.rst === true || tcp.retransmission === true || obj(tcp.analysis).retransmission === true) return 'c-tcpwarn';
  if (p.includes('someip-sd') || p.includes('someip/sd') || layers.someipsd !== undefined) return 'c-sd';
  if (p.includes('doip')) return 'c-doip';
  if (p === 'arp' || layers.arp !== undefined) return 'c-arp';
  return '';
}

function firstAddr(layers: Record<string, unknown>, key: 'src' | 'dst'): string {
  for (const l of ['ip', 'ipv6', 'arp', 'eth']) {
    const v = s(obj(layers[l])[key]);
    if (v) return v;
  }
  return '';
}

/**
 * (C1, ETH-RT-03) Request id of Loom's own `eth.send` copy: `link` with `domain === 'eth'` and a
 * non-empty string `request_id`; anything else → null. UDS links stay with {@link readLink}.
 * FUNCTIONS.md FC1.7.
 */
export function readEthLinkRequestId(v: unknown): string | null {
  const l = obj(v);
  return l.domain === 'eth' && typeof l.request_id === 'string' && l.request_id !== '' ? l.request_id : null;
}

/** Row-shaped object (query row, or event flattened by fromEvent) → PktRow. */
export function readPkt(row: Record<string, unknown>, channel: string): PktRow | null {
  const seq = big(row.seq);
  if (seq === null) return null;
  const ts = obj(row.ts);
  const layers = obj(row.layers);
  const att = s(row.attributed_origin);
  const origin = (ORIGINS as readonly string[]).includes(s(row.origin) ?? '') ? (row.origin as Origin) : 'device';
  const badge = att && (ORIGINS as readonly string[]).includes(att) ? (att as Origin) : origin;
  const proto = s(row.proto) ?? '?';
  const info = s(row.info) ?? '';
  const src = s(row.src) ?? firstAddr(layers, 'src');
  const dst = s(row.dst) ?? firstAddr(layers, 'dst');
  return {
    key: `p-${seq}`,
    kind: 'pkt',
    seq,
    ref: s(row.ref) ?? `eth:${channel}@seq:${seq}`,
    wallNs: big(ts.wall_ns) ?? 0n,
    monoNs: big(ts.mono_ns) ?? 0n,
    tsSource: s(ts.source) ?? '?',
    dir: row.dir === 'rx' || row.dir === 'tx' ? row.dir : null,
    badge,
    attributed: !!att,
    src,
    dst,
    proto,
    len: n(row.len) ?? 0,
    info,
    color: colorOf(proto, layers),
    link: readLink(row.link),
    txRequestId: s(row.tx_request_id) ?? readEthLinkRequestId(row.link),
    stream: n(row.stream) ?? null,
    streamProto: n(row.stream) !== null && (row.stream_proto === 'tcp' || row.stream_proto === 'udp') ? row.stream_proto : null,
    decodeAsApplied: row.decode_as_applied === true,
    gap: null,
    bytesEst: 200 + 2 * (src.length + dst.length + proto.length + info.length),
  };
}

/** eth.pkt event → row (record fields + payload). */
export function fromEvent(ev: LoomEvent): PktRow | null {
  return readPkt({ ...obj(ev.payload), seq: ev.seq, ts: ev.ts, dir: ev.dir, origin: ev.origin }, ev.channel ?? '?');
}

let gapN = 0;
export function gapRow(kind: string, count: number | bigint | 'unknown', cause: string, firstSeq: bigint | null, lastSeq: bigint | null, afterSeq: bigint): PktRow {
  return {
    key: `g-${++gapN}`,
    kind: 'gap',
    seq: firstSeq ?? afterSeq,
    ref: '',
    wallNs: 0n,
    monoNs: 0n,
    tsSource: '',
    dir: null,
    badge: 'device',
    attributed: false,
    src: '',
    dst: '',
    proto: '',
    len: 0,
    info: '',
    color: 'c-gap',
    link: null,
    txRequestId: null,
    stream: null,
    streamProto: null,
    decodeAsApplied: false,
    gap: { kind, count, cause, firstSeq, lastSeq },
    bytesEst: 160,
  };
}

export function gapRowText(g: NonNullable<PktRow['gap']>): string {
  const c = g.count === 'unknown' ? '?' : typeof g.count === 'bigint' ? fmtCount(g.count) : g.count.toLocaleString('ko-KR');
  const r = g.firstSeq !== null && g.lastSeq !== null ? ` (seq ${g.firstSeq}–${g.lastSeq})` : '';
  if (g.kind === 'source.gap') return `⚠ 수신 전 손실 ${c}${r} — ${g.cause}`;
  if (g.kind === 'record.gap') return `⚠ 기록 손실 ${c}${r} — ${g.cause}`;
  return `⋯ 화면 생략 ${c}${r} — 기록은 저널 (${g.cause})`;
}

// ---- time column ----

export type TimeMode = 'rel' | 'abs' | 'delta' | 'ref';

export function fmtTime(r: PktRow, mode: TimeMode, base: { startMonoNs: bigint | null; prevMonoNs: bigint | null; refMonoNs: bigint | null }): string {
  if (r.kind === 'gap') return '';
  const secs = (d: bigint) => {
    const neg = d < 0n;
    const a = neg ? -d : d;
    return `${neg ? '-' : ''}${a / 1_000_000_000n}.${String((a % 1_000_000_000n) / 1_000n).padStart(6, '0')}`;
  };
  switch (mode) {
    case 'abs': {
      const ms = Number(r.wallNs / 1_000_000n);
      if (!ms) return '--';
      const d = new Date(ms);
      const p = (x: number, w = 2) => String(x).padStart(w, '0');
      return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${String((r.wallNs % 1_000_000_000n) / 1_000n).padStart(6, '0')}`;
    }
    case 'delta':
      return base.prevMonoNs === null ? secs(0n) : secs(r.monoNs - base.prevMonoNs);
    case 'ref':
      return base.refMonoNs === null ? '?' : secs(r.monoNs - base.refMonoNs);
    default:
      return base.startMonoNs === null ? '?' : secs(r.monoNs - base.startMonoNs);
  }
}

// ---- status ----
// Canonical shapes: crates/loom-eth/src/model.rs (CaptureStatePayload, StatsPayload).
// 64-bit fields are decimal strings (U64); legacy numeric / `sim` / `buffer_bytes` /
// upper-case state shapes from the dev mock are still accepted.

/** U64 decimal string (canonical) or non-negative safe integer (legacy) → exact bigint. */
/** eth.query/eth.export range cursors use the proto cursor grammar (`seq:N`), never a bare number:
 * a bare "94" is parsed as a clock time and rejected (`bad HH:MM:SS`). Responses keep bare decimal strings. */
export function seqCursor(seq: bigint | null | undefined): string | undefined {
  return seq === null || seq === undefined || seq < 0n ? undefined : `seq:${seq}`;
}
/** Range with only the known ends; an unknown end is omitted (open), never sent as "" or a bare number. */
export function seqRange(from: bigint | null | undefined, to: bigint | null | undefined): { from?: string; to?: string } | undefined {
  const f = seqCursor(from);
  const t = seqCursor(to);
  return f || t ? { ...(f ? { from: f } : {}), ...(t ? { to: t } : {}) } : undefined;
}

export function u64(v: unknown): bigint | null {
  if (typeof v === 'string' && /^\d{1,20}$/.test(v)) {
    const b = BigInt(v);
    return b <= 0xffff_ffff_ffff_ffffn ? b : null;
  }
  if (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0) return BigInt(v);
  return null;
}

export interface CaptureState {
  /** normalised lower-case state (`permission_denied`, `live`, …) */
  state: string;
  backend: 'live' | 'replay' | 'sim' | null;
  iface: string | null;
  bpf: string | null;
  snaplen: number | null;
  bufferBytesRequested: bigint | null;
  /** OS-reported buffer; null = the OS did not report it (never assume = requested) */
  bufferBytesActual: bigint | null;
  tsPrecision: string | null;
  dropStats: 'available' | 'partial' | 'unavailable';
  sim: boolean;
  startMonoNs: bigint | null;
  error: string | null;
}

const LEGACY_STATE: Record<string, string> = { PERMISSION: 'permission_denied' };
function normState(v: unknown): string | null {
  const t = s(v);
  if (!t) return null;
  return LEGACY_STATE[t] ?? t.toLowerCase();
}

/** (C3) Capture states that end a generation; the worker never leaves them within the same generation. */
export const TERMINAL_CAPTURE_STATES: ReadonlySet<string> = new Set(['stopped', 'replay_done', 'error', 'permission_denied']);

/**
 * (operator-upgrade C3) Lifecycle merge rule for capture state from any source (control event,
 * backlog, snapshot item, or eth.capture.status reply, which has no event seq). Within one generation
 * the worker goes starting → running → terminal and never back, so an input that would move the state
 * backwards is stale and returns false; an older generation is stale, a newer one is accepted.
 * FUNCTIONS.md FC3.1.
 */
export function supersedesCapture(prevState: string | null, prevGen: number | null, nextState: unknown, nextGen: number | null): boolean {
  const n = normState(nextState);
  if (n === null) return true;
  if (prevState === null || prevState === '?') return true;
  if (prevGen !== null && nextGen !== null && nextGen !== prevGen) return nextGen > prevGen;
  const rank = (x: string): number => (x === 'starting' ? 0 : TERMINAL_CAPTURE_STATES.has(x) ? 2 : 1);
  return rank(n) >= rank(prevState);
}

export function readCaptureState(p: unknown, prev: CaptureState | null): CaptureState {
  const o = obj(p);
  const ds = o.drop_stats;
  const be = o.backend;
  const backend = be === 'live' || be === 'replay' || be === 'sim' ? be : (prev?.backend ?? null);
  const state = normState(o.state) ?? prev?.state ?? '?';
  return {
    state,
    backend,
    iface: s(o.iface) ?? prev?.iface ?? null,
    bpf: s(o.bpf) ?? prev?.bpf ?? null,
    snaplen: n(o.snaplen) ?? prev?.snaplen ?? null,
    bufferBytesRequested: u64(o.buffer_bytes_requested) ?? u64(o.buffer_bytes) ?? prev?.bufferBytesRequested ?? null,
    bufferBytesActual: 'buffer_bytes_requested' in o ? u64(o.buffer_bytes_actual) : (prev?.bufferBytesActual ?? null),
    tsPrecision: s(o.ts_precision) ?? prev?.tsPrecision ?? null,
    dropStats: ds === 'available' || ds === 'partial' || ds === 'unavailable' ? ds : (prev?.dropStats ?? 'unavailable'),
    sim: typeof be === 'string' ? be === 'sim' : typeof o.sim === 'boolean' ? o.sim : (prev?.sim ?? false),
    startMonoNs: big(o.started_mono_ns) ?? prev?.startMonoNs ?? null,
    // error belongs to the state it came with; a new state without one clears it
    error: o.state !== undefined ? s(o.error) : (s(o.error) ?? prev?.error ?? null),
  };
}

export interface CaptureStats {
  pps: bigint | null;
  bps: bigint | null;
  decodeLagMs: bigint | null;
  /** drops in the last second; null = unknown (field absent), never read as 0 */
  kernelDrop: bigint | null;
  ifDrop: bigint | null;
  shown: bigint | null;
  total: bigint | null;
}

export const EMPTY_STATS: CaptureStats = { pps: null, bps: null, decodeLagMs: null, kernelDrop: null, ifDrop: null, shown: null, total: null };

/** eth.stats (1 s window). Each message is a full snapshot: an absent drop field means unknown for that second. */
export function readStats(p: unknown, prev: CaptureStats): CaptureStats {
  const o = obj(p);
  return {
    pps: u64(o.pps) ?? prev.pps,
    bps: u64(o.bps) ?? prev.bps,
    decodeLagMs: u64(o.decode_lag_ms) ?? prev.decodeLagMs,
    kernelDrop: u64(o.kernel_drop),
    ifDrop: u64(o.if_drop),
    shown: u64(o.shown),
    total: u64(o.total),
  };
}

/** exact integer with ko-KR grouping, no float rounding (U64-safe) */
export function fmtCount(v: bigint): string {
  return v.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}

/** bits/s → short text; scaled values are approximate by design (one decimal) */
export function fmtRate(v: bigint | null): string {
  if (v === null) return '?';
  const scale = (d: bigint, u: string) => `${v / d}.${(v % d) / (d / 10n)}${u}`;
  if (v >= 1_000_000_000n) return scale(1_000_000_000n, 'G');
  if (v >= 1_000_000n) return scale(1_000_000n, 'M');
  if (v >= 1_000n) return scale(1_000n, 'k');
  return v.toString();
}

/** stats shown/total → coverage (same window as pps), when no batch summary has arrived */
export function statsCoverage(st: CaptureStats): BatchCoverage | null {
  if (st.total === null || st.shown === null) return null;
  const big2n = (b: bigint) => (b > BigInt(Number.MAX_SAFE_INTEGER) ? Number.MAX_SAFE_INTEGER : Number(b));
  return { total: big2n(st.total), shown: big2n(st.shown) };
}

export interface BatchCoverage {
  total: number;
  shown: number;
}

/** eth.batch → coverage; accepts the UI-requested and the eth/plan shapes. */
export function readBatch(p: unknown): BatchCoverage | null {
  const o = obj(p);
  const total = n(o.total) ?? n(o.count);
  if (total === null) return null;
  const shown = n(o.shown) ?? 0;
  return { total, shown };
}

export function samplingText(c: BatchCoverage | null, recordHealth: string): string {
  const rec = recordHealth === 'ok' ? '기록 100 %' : `기록 ${recordHealth}`;
  if (!c || c.total <= 0 || c.shown >= c.total) return `표시 1/1 · ${rec}`;
  const ratio = c.shown > 0 ? Math.round(c.total / c.shown) : c.total;
  return `표시 1/${ratio} 샘플링 · ${rec}`;
}

export function readLinkEvent(p: unknown): { seq: bigint | null; link: PktLink | null; ethTx: string | null } {
  const o = obj(p);
  const r = parseEvidenceRef(s(o.eth_ref) ?? '');
  const seq = r?.seqA ?? big(o.eth_seq);
  if (o.domain === 'eth') return { seq, link: null, ethTx: s(o.request_id) };
  return { seq, link: readLink(o), ethTx: null };
}

// ---- bounded packet store (C2: 20,000 rows / 4 MiB per panel) ----

export const MAX_ROWS = 20_000;
export const MAX_BYTES = 4 * 1024 * 1024;
const EVICT_CHUNK = 1_000;

export class PacketStore {
  rows: PktRow[] = [];
  bytes = 0;
  evicted = 0;
  private bySeq = new Map<string, PktRow>();

  push(r: PktRow): void {
    this.rows.push(r);
    this.bytes += r.bytesEst;
    if (r.kind === 'pkt') this.bySeq.set(String(r.seq), r);
    if (this.rows.length > MAX_ROWS || this.bytes > MAX_BYTES) this.evict();
  }

  /** drop the panel's retained rows (view only — the journal/recording keeps everything) */
  clear(): void {
    this.rows = [];
    this.bytes = 0;
    this.bySeq.clear();
  }

  get(seq: bigint): PktRow | undefined {
    return this.bySeq.get(String(seq));
  }

  /** drop the oldest chunk(s) until both caps hold; amortised (no per-row shift) */
  private evict(): void {
    let k = 0;
    while (k < this.rows.length && (this.rows.length - k > MAX_ROWS - EVICT_CHUNK || this.bytes > MAX_BYTES * 0.9)) {
      const r = this.rows[k++] as PktRow;
      this.bytes -= r.bytesEst;
      if (r.kind === 'pkt') this.bySeq.delete(String(r.seq));
    }
    this.rows.splice(0, k);
    this.evicted += k;
  }
}

// ---- range / read scope (operator-upgrade B2, finding ETH-01) ----
// Complete (journal) views always send an explicit `range.from`: the capture start anchor the daemon
// observed (`start_cursor`), else an explicit "recent N" range labelled as a fallback, or a typed seq range.
// The daemon's `partial_reasons` / `complete` decide the badge; `complete` is shown as 완전 ONLY when the
// daemon reported partial_reasons (B1+) and none of them (contract: work/reports/operator-upgrade/eth/FUNCTIONS.md B2).

/** rows per complete-mode page = the daemon's hard row cap (CONTRACT_MAX_ROWS) */
export const PAGE_ROWS = 1_000;
/** newer-page cursors remembered while paging back (oldest dropped beyond this) */
export const MAX_PAGE_STACK = 32;
/** explicit "recent" choices (seconds) */
export const RECENT_CHOICES = [600, 3_600, 21_600] as const;
export type RecentSecs = (typeof RECENT_CHOICES)[number];
/** `start_cursor` grammar the daemon emits (`e<E>:seq:<N>`, `seq:<N>` accepted) */
export const START_CURSOR_RE = /^(e\d{1,20}:)?seq:\d{1,20}$/;

export type RangeChoice = { kind: 'capture' } | { kind: 'recent'; secs: RecentSecs } | { kind: 'seq'; from: bigint; to: bigint | null };
export interface RangeAnchors {
  /** daemon `start_cursor` of this channel's current generation; null = unknown */
  startCursor: string | null;
  /** newest packet seq the panel knows (live head); null/≤0 = unknown → no `to` */
  headSeq: bigint | null;
}
/** eth.query/export/follow `range` — `from` is always present */
export interface RangeRequest {
  from: string;
  to?: string;
}
export interface ResolvedRange {
  req: RangeRequest;
  label: string;
  /** capture start unknown → explicit recent-10-minutes fallback */
  fallback: boolean;
}

export const PARTIAL_REASONS = ['default_lookback', 'scan_budget', 'scan_incomplete', 'decode_budget', 'loss', 'row_cap', 'output_cap'] as const;
export type PartialReason = (typeof PARTIAL_REASONS)[number];

export interface ReadScope {
  from: string | null;
  to: string | null;
  fromSource: 'explicit' | 'default_lookback' | null;
  lookbackSecs: number | null;
}
/** honesty fields of eth.query / eth.export / eth.follow / eth.stats results (daemon B1) */
export interface ReadMeta {
  /** the daemon sent `partial_reasons` (B1+). false = older daemon → completeness unknown, never 완전 */
  known: boolean;
  /** known && daemon `complete === true` && no reason (known or unknown) */
  complete: boolean;
  /** daemon `scan_complete` (coverage of the applied range only); null = absent */
  scanComplete: boolean | null;
  reasons: PartialReason[];
  /** reason strings this UI does not know (shown verbatim, still partial) */
  unknownReasons: string[];
  scope: ReadScope | null;
  scannedFirst: bigint | null;
  scannedLast: bigint | null;
  resumeCursor: string | null;
  /** listed gap entries + gaps_omitted */
  gaps: number;
}
export interface QueryPage {
  rows: PktRow[];
  rowsFrom: bigint | null;
  rowsTo: bigint | null;
  matched: number | null;
  truncated: boolean;
  prevCursor: string | null;
  nextCursor: string | null;
  meta: ReadMeta;
}
export interface CompleteBadge {
  text: string;
  state: 'complete' | 'partial' | 'unknown';
  title: string;
}
/** complete-mode paging state: `base` = resolved range, `current` = request on screen, `newer` = `to` values to go back to */
export interface PageNav {
  base: RangeRequest;
  current: RangeRequest;
  newer: (string | undefined)[];
}

/** F2.1 */
export function resolveRange(choice: RangeChoice, anchors: RangeAnchors): ResolvedRange {
  const head = anchors.headSeq !== null && anchors.headSeq > 0n ? `seq:${anchors.headSeq}` : undefined;
  const withTo = (from: string, to: string | undefined): RangeRequest => (to !== undefined ? { from, to } : { from });
  if (choice.kind === 'capture') {
    if (anchors.startCursor !== null) return { req: withTo(anchors.startCursor, head), label: '캡처 시작 이후', fallback: false };
    return { req: withTo('-600s', head), label: '최근 10분 (캡처 시작 위치 모름)', fallback: true };
  }
  if (choice.kind === 'recent') {
    const label = choice.secs === 600 ? '최근 10분' : choice.secs === 3_600 ? '최근 1시간' : '최근 6시간';
    return { req: withTo(`-${choice.secs}s`, head), label, fallback: false };
  }
  const to = choice.to !== null ? `seq:${choice.to}` : head;
  return { req: withTo(`seq:${choice.from}`, to), label: `seq ${choice.from}–${choice.to !== null ? choice.to : '최신'}`, fallback: false };
}
/** F2.2 — `<select>` value + seq inputs → choice, or a user-facing error */
export function parseRangeChoice(value: string, fromText: string, toText: string): RangeChoice | { error: string } {
  if (value === 'capture') return { kind: 'capture' };
  if (value === 'recent:600') return { kind: 'recent', secs: 600 };
  if (value === 'recent:3600') return { kind: 'recent', secs: 3_600 };
  if (value === 'recent:21600') return { kind: 'recent', secs: 21_600 };
  if (value !== 'seq') return { error: '알 수 없는 범위' };
  const u64Text = (t: string): bigint | null => {
    if (!/^\d{1,20}$/.test(t)) return null;
    const b = BigInt(t);
    return b <= 0xffff_ffff_ffff_ffffn ? b : null;
  };
  const from = u64Text(fromText.trim());
  if (from === null) return { error: '시작 seq를 숫자로 입력하세요' };
  const toT = toText.trim();
  const to = toT === '' ? null : u64Text(toT);
  if (toT !== '' && to === null) return { error: '끝 seq를 숫자로 입력하세요' };
  if (to !== null && from > to) return { error: '시작 seq가 끝 seq보다 큽니다' };
  return { kind: 'seq', from, to };
}
/** F2.3 — choice → `<select>` value */
export function rangeChoiceValue(choice: RangeChoice): string {
  return choice.kind === 'recent' ? `recent:${choice.secs}` : choice.kind;
}
/** F2.4 */
export function readReadMeta(v: unknown): ReadMeta {
  const o = obj(v);
  const known = Array.isArray(o.partial_reasons);
  const reasons: PartialReason[] = [];
  const unknownReasons: string[] = [];
  if (Array.isArray(o.partial_reasons)) {
    for (const r of o.partial_reasons as unknown[]) {
      if (typeof r !== 'string') continue;
      if ((PARTIAL_REASONS as readonly string[]).includes(r)) {
        if (!reasons.includes(r as PartialReason)) reasons.push(r as PartialReason);
      } else unknownReasons.push(r);
    }
  }
  const sc = o.scope;
  const scope: ReadScope | null =
    typeof sc === 'object' && sc !== null
      ? (() => {
          const so = obj(sc);
          return {
            from: typeof so.from === 'string' ? so.from : null,
            to: typeof so.to === 'string' ? so.to : null,
            fromSource: so.from_source === 'explicit' || so.from_source === 'default_lookback' ? so.from_source : null,
            lookbackSecs: n(so.lookback_secs),
          };
        })()
      : null;
  const scanned = obj(o.scanned);
  return {
    known,
    complete: known && o.complete === true && reasons.length === 0 && unknownReasons.length === 0,
    scanComplete: typeof o.scan_complete === 'boolean' ? o.scan_complete : null,
    reasons,
    unknownReasons,
    scope,
    scannedFirst: u64(scanned.first_seq),
    scannedLast: u64(scanned.last_seq),
    resumeCursor: typeof o.resume_cursor === 'string' && o.resume_cursor !== '' ? o.resume_cursor : null,
    gaps: (Array.isArray(o.gaps) ? o.gaps.length : 0) + (n(o.gaps_omitted) ?? 0),
  };
}
/** F2.5 */
export function readQueryPage(v: unknown, channel: string): QueryPage {
  const o = obj(v);
  const rows: PktRow[] = [];
  if (Array.isArray(o.rows)) {
    for (const r of o.rows as unknown[]) {
      const row = readPkt(obj(r), channel);
      if (row) rows.push(row);
    }
  }
  const range = obj(o.range);
  return {
    rows,
    rowsFrom: big(range.from) ?? rows[0]?.seq ?? null,
    rowsTo: big(range.to) ?? rows[rows.length - 1]?.seq ?? null,
    matched: n(o.matched),
    truncated: o.truncated === true,
    prevCursor: typeof o.prev_cursor === 'string' ? o.prev_cursor : null,
    nextCursor: typeof o.next_cursor === 'string' ? o.next_cursor : null,
    meta: readReadMeta(v),
  };
}
/** F2.6 */
export function reasonText(r: PartialReason, meta: ReadMeta): string {
  switch (r) {
    case 'default_lookback':
      return '기본 조회(최근 10분)만';
    case 'scan_budget':
      return '스캔 한도(64 MiB) 도달';
    case 'scan_incomplete':
      return '저널 스캔 불완전';
    case 'decode_budget':
      return '디코드 한도 도달';
    case 'loss':
      return `손실 구간 ${meta.gaps}건`;
    case 'row_cap':
      return `최대 ${PAGE_ROWS.toLocaleString('ko-KR')}행 초과`;
    case 'output_cap':
      return '출력 한도 도달';
  }
}
/** F2.7 */
export function scopeText(meta: ReadMeta): string {
  if (!meta.known) return '요청/스캔 범위 정보 없음 (이전 daemon)';
  const sc = meta.scope;
  return (
    `요청 범위 ${sc?.from ?? '?'} → ${sc?.to ?? '최신'}` +
    (sc?.fromSource === 'default_lookback' ? ' (기본 최근 10분)' : '') +
    ` · 스캔 seq ${meta.scannedFirst ?? '?'}–${meta.scannedLast ?? '?'}` +
    (meta.scanComplete === true ? ' · 스캔 범위 전체 확인' : meta.scanComplete === false ? ' · 스캔 중단' : '')
  );
}
/** F2.8 */
export function completeBadge(page: QueryPage, label: string): CompleteBadge {
  const meta = page.meta;
  const cnt = page.rows.length.toLocaleString('ko-KR');
  const seqs = page.rowsFrom !== null && page.rowsTo !== null ? ` · seq ${page.rowsFrom}–${page.rowsTo}` : '';
  const title = scopeText(meta);
  if (!meta.known) return { text: `완전 여부 미확인 · ${label} · ${cnt}행${seqs}`, state: 'unknown', title };
  if (meta.complete) return { text: `완전 · ${label} · ${cnt}행${seqs}`, state: 'complete', title };
  const m = page.matched !== null && page.matched > page.rows.length ? ` / 일치 ${page.matched.toLocaleString('ko-KR')}` : '';
  const list = [...meta.reasons.map((r) => reasonText(r, meta)), ...meta.unknownReasons.map((r) => `기타 사유 ${r}`)].join(' · ');
  return { text: `부분 · ${label} · ${cnt}행${m}${seqs} — ${list || '완전 아님(사유 미보고)'}`, state: 'partial', title };
}
/** F2.9 */
export function navStart(req: RangeRequest): PageNav {
  return { base: { ...req }, current: { ...req }, newer: [] };
}
/** F2.10 */
export function navOlder(nav: PageNav, prevCursor: string): PageNav {
  return {
    base: nav.base,
    current: { from: nav.current.from, to: prevCursor },
    newer: [...nav.newer, nav.current.to].slice(-MAX_PAGE_STACK),
  };
}
/** F2.11 */
export function navNewer(nav: PageNav): PageNav | null {
  if (nav.newer.length === 0) return null;
  const t = nav.newer[nav.newer.length - 1];
  return {
    base: nav.base,
    current: t !== undefined ? { from: nav.current.from, to: t } : { from: nav.current.from },
    newer: nav.newer.slice(0, -1),
  };
}
/** F2.12 */
export function navResume(nav: PageNav, resumeCursor: string): PageNav {
  return {
    base: nav.base,
    current: { from: resumeCursor, ...(nav.current.to !== undefined ? { to: nav.current.to } : {}) },
    newer: [],
  };
}

// ---- tree → display filter (operator-upgrade B4a, ETH-02) ----

/** F4.2 — "prepare as filter … and/or": combine the field's text with a daemon-made expression */
export function combineFilter(draft: string, expr: string, op: '&&' | '||'): string {
  const d = draft.trim();
  return d === '' ? expr : `(${d}) ${op} ${expr}`;
}
