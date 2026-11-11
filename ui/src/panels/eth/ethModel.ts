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
//   eth.capture_state{state, iface, bpf, snaplen, buffer_bytes, ts_precision, drop_stats,
//                     sim?, started_mono_ns?}
//   eth.stats        {pps, bps, decode_lag_ms?}

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
  gap: { kind: string; count: number | 'unknown'; cause: string; firstSeq: bigint | null; lastSeq: bigint | null } | null;
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
    txRequestId: s(row.tx_request_id),
    gap: null,
    bytesEst: 200 + 2 * (src.length + dst.length + proto.length + info.length),
  };
}

/** eth.pkt event → row (record fields + payload). */
export function fromEvent(ev: LoomEvent): PktRow | null {
  return readPkt({ ...obj(ev.payload), seq: ev.seq, ts: ev.ts, dir: ev.dir, origin: ev.origin }, ev.channel ?? '?');
}

let gapN = 0;
export function gapRow(kind: string, count: number | 'unknown', cause: string, firstSeq: bigint | null, lastSeq: bigint | null, afterSeq: bigint): PktRow {
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
    gap: { kind, count, cause, firstSeq, lastSeq },
    bytesEst: 160,
  };
}

export function gapRowText(g: NonNullable<PktRow['gap']>): string {
  const c = g.count === 'unknown' ? '?' : g.count.toLocaleString('ko-KR');
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

export interface CaptureState {
  state: string;
  iface: string | null;
  bpf: string | null;
  snaplen: number | null;
  bufferBytes: number | null;
  tsPrecision: string | null;
  dropStats: 'available' | 'partial' | 'unavailable';
  sim: boolean;
  startMonoNs: bigint | null;
}

export function readCaptureState(p: unknown, prev: CaptureState | null): CaptureState {
  const o = obj(p);
  const ds = o.drop_stats;
  return {
    state: s(o.state) ?? prev?.state ?? '?',
    iface: s(o.iface) ?? prev?.iface ?? null,
    bpf: s(o.bpf) ?? prev?.bpf ?? null,
    snaplen: n(o.snaplen) ?? prev?.snaplen ?? null,
    bufferBytes: n(o.buffer_bytes) ?? prev?.bufferBytes ?? null,
    tsPrecision: s(o.ts_precision) ?? prev?.tsPrecision ?? null,
    dropStats: ds === 'available' || ds === 'partial' || ds === 'unavailable' ? ds : (prev?.dropStats ?? 'unavailable'),
    sim: typeof o.sim === 'boolean' ? o.sim : (prev?.sim ?? false),
    startMonoNs: big(o.started_mono_ns) ?? prev?.startMonoNs ?? null,
  };
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
