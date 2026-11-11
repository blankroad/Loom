// Mock Ethernet capture (sim tap) for eth.packets — p1-eth.md §5 items 1–7.
// Shapes follow the panel's provisional readers (src/panels/eth/ethModel.ts head comment):
//   eth.capture_state {state, iface, bpf, snaplen, ts_precision, drop_stats, sim, started_mono_ns}
//   eth.pkt   {src, dst, proto, len, info, attributed_origin?, layers, link?, tx_request_id?}
//   eth.batch {covered_first_seq, covered_last_seq, total, shown}
//   eth.tx    write rows (request_id + write_state) {kind, summary, len}
//   source.gap{producer_range, count, cause:"kernel_drop"} on domain eth / channel ch
// RPCs: eth.capture.start/stop, eth.filter.check, eth.query (list + detail), eth.send
// (dry_run preview without rows), eth.ifaces, eth.export (job handle only).
//
// Bounds (C2): the "journal" keeps ≤ JOURNAL_CAP packets (oldest dropped → older eth.query
// ranges come back shorter, flagged `truncated`); query results ≤ 2,000 rows; one flood at a
// time; per-tick packets ≤ 10,000; filter expressions ≤ 256 chars.

import type { LoomEvent } from '../../src/proto/provisional';
import type { Hub } from './hub';
import { MockRpcError, type Actor, type Sim } from './sim';

export const JOURNAL_CAP = 200_000;
const QUERY_MAX = 2_000;
const FLOOD_TICK_MS = 10;
const MAX_PER_TICK = 10_000;

/** 69-byte DoIP diagnostic request (UDS 22 F1 90) with exact {off,len} tree — same bytes as src/panels/eth/fixtures.ts */
export const DIAG_REQ_HEX =
  '001b21aabbcc' + '001b21112233' + '0800' +
  '45000037' + '00004000' + '40060000' + 'c0a80002' + 'c0a8000a' +
  'c3c03458' + '00000091' + '00000001' + '5018ffff' + '00000000' +
  '02fd8001' + '00000007' +
  '0e801001' +
  '22f190';

interface TreeNode {
  label: string;
  off: number;
  len: number;
  children?: TreeNode[];
}

export const DIAG_REQ_TREE: TreeNode[] = [
  { label: 'Ethernet II, 00:1b:21:11:22:33 → 00:1b:21:aa:bb:cc', off: 0, len: 14, children: [{ label: 'Type: IPv4 (0x0800)', off: 12, len: 2 }] },
  { label: 'IPv4, 192.168.0.2 → 192.168.0.10', off: 14, len: 20, children: [{ label: 'Protocol: TCP (6)', off: 23, len: 1 }] },
  { label: 'TCP, 50112 → 13400, len 15', off: 34, len: 20, children: [{ label: 'Destination port: 13400', off: 36, len: 2 }] },
  {
    label: 'DoIP v2 Diagnostic message (0x8001) 0x0E80 → 0x1001',
    off: 54,
    len: 15,
    children: [
      { label: 'Payload type: Diagnostic message (0x8001)', off: 56, len: 2 },
      { label: 'Source address: 0x0E80', off: 62, len: 2 },
      { label: 'Target address: 0x1001', off: 64, len: 2 },
      {
        label: 'UDS 0x22 ReadDataByIdentifier',
        off: 66,
        len: 3,
        children: [
          { label: 'SID: 0x22 ReadDataByIdentifier', off: 66, len: 1 },
          { label: 'DID: 0xF190 VIN', off: 67, len: 2 },
        ],
      },
    ],
  },
];

/** A journal packet (also the eth.pkt payload without the record fields). */
export interface Pkt {
  seq: bigint;
  monoNs: bigint;
  wallNs: bigint;
  dir: 'rx' | 'tx';
  origin: LoomEvent['origin'];
  payload: Record<string, unknown>;
}

// ---- display filter (tiny subset: `proto`, `field == value`, `field != value`) ---------------

const PROTOS = ['doip', 'uds', 'arp', 'tcp', 'udp', 'someip', 'someipsd'] as const;
const FILTER_RE = /^\s*([a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)?)\s*(?:(==|!=)\s*(\S+)?)?\s*$/i;

/** `eth.filter.check` result shape the filter bar reads: {ok} | {ok:false, pos, expected, message} */
export function checkFilter(expr: string): { ok: true } | { ok: false; pos: number; expected: string; message: string } {
  if (expr.length > 256) return { ok: false, pos: 256, expected: '더 짧은 식', message: '식이 너무 김 (≤ 256자)' }; // the bar prefixes "위치 N:" itself
  const m = FILTER_RE.exec(expr);
  if (!m) return { ok: false, pos: 0, expected: '필드', message: '알 수 없는 식' };
  const [, field, op, value] = m;
  const root = field!.split('.')[0]!.toLowerCase();
  if (!(PROTOS as readonly string[]).includes(root)) return { ok: false, pos: expr.indexOf(field!), expected: '프로토콜 이름', message: `알 수 없는 필드 ${field}` };
  if (op && value === undefined) {
    const pos = expr.trimEnd().length; // position right after the operator
    return { ok: false, pos, expected: '값', message: '값 필요' };
  }
  return { ok: true };
}

function layerValue(layers: Record<string, unknown>, path: string): unknown {
  let cur: unknown = layers;
  for (const k of path.toLowerCase().split('.')) cur = typeof cur === 'object' && cur !== null ? (cur as Record<string, unknown>)[k] : undefined;
  return cur;
}

/** Evaluate a checked expression against an eth.pkt payload. Unknown/invalid expressions match nothing. */
export function ethFilterMatch(expr: string, payload: Record<string, unknown>): boolean {
  if (!checkFilter(expr).ok) return false;
  const [, field, op, value] = FILTER_RE.exec(expr)!;
  const layers = (payload.layers ?? {}) as Record<string, unknown>;
  const v = layerValue(layers, field!);
  if (!op) return v !== undefined;
  const eq = typeof v === 'string' || typeof v === 'number' ? String(v).toLowerCase() === value!.toLowerCase() : false;
  return op === '==' ? eq : !eq;
}

// ---- the sim tap ---------------------------------------------------------------------------

interface ChannelState {
  state: 'LIVE' | 'STOPPED';
  iface: string;
  bpf: string | null;
  dropStats: 'available' | 'unavailable';
  startedMonoNs: bigint;
}

export class EthSim {
  /** journal (ring): every captured packet, including the ones the live stream only summarised */
  private journal: Pkt[] = [];
  private journalDropped = 0;
  private readonly chans = new Map<string, ChannelState>();
  private flood: NodeJS.Timeout | null = null;
  private bg: NodeJS.Timeout | null = null;
  private bgN = 0;
  /** packets produced by the current/last flood (test accounting) */
  floodStats = { total: 0, shown: 0 };

  constructor(
    private readonly hub: Hub,
    private readonly sim: Sim,
  ) {}

  private chan(ch: string): ChannelState | undefined {
    return this.chans.get(ch);
  }

  checkFilterRpc(expr: string): ReturnType<typeof checkFilter> {
    return checkFilter(expr);
  }

  live(ch: string): boolean {
    return this.chan(ch)?.state === 'LIVE';
  }

  private publishState(a: Actor, ch: string): void {
    const c = this.chans.get(ch)!;
    this.hub.publish({
      domain: 'eth',
      channel: ch,
      kind: 'eth.capture_state',
      origin: a.origin,
      client_id: a.clientId,
      payload: { state: c.state, iface: c.iface, bpf: c.bpf ?? undefined, snaplen: 262_144, ts_precision: 'us', drop_stats: c.dropStats, sim: true, started_mono_ns: String(c.startedMonoNs) },
    });
  }

  captureStart(a: Actor, p: { ch?: string; iface?: string; bpf?: string; drop_stats?: 'available' | 'unavailable' }): Record<string, unknown> {
    const ch = p.ch ?? 'lan1';
    if (this.chans.size >= 8 && !this.chans.has(ch)) throw new MockRpcError(-32602, 'too many capture channels', { code: 'invalid_params' });
    this.chans.set(ch, { state: 'LIVE', iface: p.iface ?? 'lo', bpf: p.bpf ?? null, dropStats: p.drop_stats ?? 'available', startedMonoNs: this.hub.clock.mono() });
    this.publishState(a, ch);
    this.sim.activity(a, 'capture_start', 'eth', ch, `eth capture ${ch} (${p.iface ?? 'lo'})`);
    return { ch, state: 'LIVE' };
  }

  captureStop(a: Actor, p: { ch?: string }): Record<string, unknown> {
    const ch = p.ch ?? 'lan1';
    const c = this.chans.get(ch);
    if (c) {
      c.state = 'STOPPED';
      this.publishState(a, ch);
    }
    this.stopFlood();
    return { ch, state: 'STOPPED' };
  }

  /** Background traffic while a channel is LIVE: one SOME/IP-SD offer every `ms` (keeps rows coming during STOP). */
  startBackground(ms = 250): void {
    this.bg ??= setInterval(() => {
      for (const [ch, c] of this.chans) {
        if (c.state !== 'LIVE') continue;
        this.packet(ch, 'rx', 'device', { src: '192.168.0.20', dst: '224.224.224.245', proto: 'SOME/IP-SD', len: 110, info: `OfferService 0x1234 ttl 3 #${++this.bgN}`, layers: { someipsd: { entries: 1 }, udp: { dstport: 30490 } } });
      }
    }, ms);
  }

  stop(): void {
    this.stopFlood();
    if (this.bg) clearInterval(this.bg);
    this.bg = null;
  }

  private journalPush(p: Pkt): void {
    this.journal.push(p);
    if (this.journal.length > JOURNAL_CAP) {
      const n = this.journal.length - JOURNAL_CAP;
      this.journal.splice(0, n);
      this.journalDropped += n;
    }
  }

  /** capture one packet: journal + live eth.pkt event */
  packet(ch: string, dir: 'rx' | 'tx', origin: LoomEvent['origin'], payload: Record<string, unknown>): LoomEvent | null {
    if (!this.live(ch)) return null;
    const ev = this.hub.publish({ domain: 'eth', channel: ch, kind: 'eth.pkt', dir, origin, payload });
    this.journalPush({ seq: BigInt(ev.seq), monoNs: BigInt(ev.ts.mono_ns), wallNs: BigInt(ev.ts.wall_ns), dir, origin, payload });
    return ev;
  }

  /** a UDS request went over DoIP: request + response packets linked to the UDS row (request_id) */
  onUds(target: string, requestId: string, origin: LoomEvent['origin'], reqHex: string, respHex: string | null): void {
    for (const ch of this.chans.keys()) {
      if (!this.live(ch)) continue;
      const sp = (h: string) => h.replace(/(..)(?!$)/g, '$1 ').toUpperCase();
      this.packet(ch, 'tx', 'device', {
        src: '192.168.0.2', dst: '192.168.0.10', proto: 'DoIP/UDS', len: 54 + 12 + reqHex.length / 2,
        info: `Diag 0x0E80→0x1001 ${sp(reqHex)}`,
        attributed_origin: origin,
        layers: { doip: { type: '0x8001' }, uds: { sid: `0x${reqHex.slice(0, 2)}` }, tcp: { dstport: 13400 } },
        link: { domain: 'uds', request_id: requestId, role: 'request', confidence: 'exact', target },
      });
      if (respHex) {
        this.packet(ch, 'rx', 'device', {
          src: '192.168.0.10', dst: '192.168.0.2', proto: 'DoIP/UDS', len: 54 + 12 + respHex.length / 2,
          info: `Diag 0x1001→0x0E80 ${sp(respHex.slice(0, 16))}${respHex.length > 16 ? ' …' : ''}`,
          layers: { doip: { type: '0x8001' }, uds: { sid: `0x${respHex.slice(0, 2)}`, ...(respHex.startsWith('7f') ? { nrc: `0x${respHex.slice(4, 6)}` } : {}) }, tcp: { srcport: 13400 } },
          link: { domain: 'uds', request_id: requestId, role: 'response', confidence: 'probable', target },
        });
      }
    }
  }

  /** kernel drop on the capture → source.gap (no stream seqs: producer range only) */
  kernelDrop(ch: string, count: number): LoomEvent {
    return this.hub.publish({
      domain: 'eth',
      channel: ch,
      kind: 'source.gap',
      origin: 'device',
      payload: { producer_range: { producer_id: `eth-${ch}`, producer_epoch: '1', from: '1', to: String(count) }, count: String(count), cause: 'kernel_drop' },
    });
  }

  /**
   * High-rate replay: `pps` packets/s for `seconds`. Every packet is journalled under a real stream
   * seq (Hub.reserve), the live stream gets 1 of every `sample` plus one eth.batch per tick
   * {covered_first_seq, covered_last_seq, total, shown} — the UI shows "표시 1/N 샘플링".
   */
  startFlood(ch: string, pps: number, seconds: number, sample: number): void {
    this.stopFlood();
    const per = Math.max(1, Math.min(MAX_PER_TICK, Math.round((pps * FLOOD_TICK_MS) / 1000)));
    const every = Math.max(1, Math.trunc(sample));
    const end = Date.now() + Math.min(30, Math.max(0.1, seconds)) * 1000;
    this.floodStats = { total: 0, shown: 0 };
    let n = 0;
    this.flood = setInterval(() => {
      if (Date.now() >= end || !this.live(ch)) return this.stopFlood();
      const first = this.hub.reserve(per);
      const mono = this.hub.clock.mono();
      const wall = this.hub.clock.wall();
      let shown = 0;
      for (let i = 0; i < per; i++, n++) {
        const seq = first + BigInt(i);
        const payload = { src: '192.168.0.30', dst: '192.168.0.31', proto: 'SOME/IP', len: 64, info: `Notification 0x1234.0x8001 #${n}`, layers: { someip: { service: '0x1234' }, udp: { dstport: 30501 } } };
        this.journalPush({ seq, monoNs: mono, wallNs: wall, dir: 'rx', origin: 'device', payload });
        if (i % every === 0) {
          this.hub.publish({ domain: 'eth', channel: ch, kind: 'eth.pkt', dir: 'rx', origin: 'device', payload }, seq);
          shown++;
        }
      }
      this.hub.publish({ domain: 'eth', channel: ch, kind: 'eth.batch', origin: 'device', payload: { covered_first_seq: String(first), covered_last_seq: String(first + BigInt(per - 1)), total: per, shown } });
      this.floodStats.total += per;
      this.floodStats.shown += shown;
    }, FLOOD_TICK_MS);
  }

  stopFlood(): void {
    if (this.flood) clearInterval(this.flood);
    this.flood = null;
  }

  private row(ch: string, p: Pkt): Record<string, unknown> {
    return {
      ref: `eth:${ch}@seq:${p.seq}`,
      seq: String(p.seq),
      ts: { mono_ns: String(p.monoNs), wall_ns: String(p.wallNs), source: 'host_read_complete' },
      dir: p.dir,
      origin: p.origin,
      ...p.payload,
    };
  }

  /** eth.query: detail for one ref, or the newest ≤ max rows with seq ≤ range.to (journal = complete). */
  query(p: { ch?: string; ref?: string; detail?: boolean; range?: { from?: string; to?: string }; filter?: string; max?: number }): Record<string, unknown> {
    if (p.detail && p.ref) {
      return { ref: p.ref, len: DIAG_REQ_HEX.length / 2, bytes: { encoding: 'hex', data: DIAG_REQ_HEX }, tree: DIAG_REQ_TREE };
    }
    const ch = p.ch ?? 'lan1';
    const to = p.range?.to !== undefined ? BigInt(p.range.to) : this.hub.seq;
    const from = p.range?.from !== undefined ? BigInt(p.range.from) : 0n;
    const max = Math.max(1, Math.min(QUERY_MAX, Math.trunc(p.max ?? QUERY_MAX)));
    if (p.filter && !checkFilter(p.filter).ok) throw new MockRpcError(-32602, `bad filter: ${p.filter}`, { code: 'invalid_params' });
    const out: Pkt[] = [];
    let more = false;
    for (let i = this.journal.length - 1; i >= 0; i--) {
      const x = this.journal[i]!;
      if (x.seq > to) continue;
      if (x.seq < from) break;
      if (p.filter && !ethFilterMatch(p.filter, x.payload)) continue;
      if (out.length >= max) {
        more = true;
        break;
      }
      out.push(x);
    }
    out.reverse();
    const rows = out.map((x) => this.row(ch, x));
    const first = out[0];
    const last = out[out.length - 1];
    return { rows, ...(first && last ? { range: { from: String(first.seq), to: String(last.seq) } } : {}), truncated: more || (this.journalDropped > 0 && !first) };
  }

  /** eth.send: dry_run = decode preview only (no rows, no audit); real send = write rows + captured copy */
  async send(a: Actor, p: { kind?: 'raw' | 'udp' | 'tcp'; ch?: string; hex?: string; to?: string; dry_run?: boolean }): Promise<Record<string, unknown>> {
    const ch = p.ch ?? 'lan1';
    const kind = p.kind ?? 'raw';
    const h = (p.hex ?? '').replace(/\s+/g, '').toLowerCase();
    if (!/^([0-9a-f]{2}){1,1514}$/.test(h)) throw new MockRpcError(-32602, 'hex: 1–1514 bytes', { code: 'invalid_params' });
    const isDiag = h.length >= 66 * 2 && h.slice(112, 116) === '8001';
    const info = isDiag ? `Diag ${h.slice(132).replace(/(..)(?!$)/g, '$1 ').toUpperCase()}` : `${kind} ${h.length / 2} B`;
    const proto = isDiag ? 'DoIP/UDS' : kind === 'raw' ? 'ETH' : kind.toUpperCase();
    if (p.dry_run) return { dry_run: true, proto, info, risk: 'R1', len: h.length / 2 };
    return this.sim.pipeline(a, { domain: 'eth', channel: ch, kind: 'eth.tx', dir: 'tx', payload: { kind, summary: `${proto} ${info}`, len: h.length / 2, ...(p.to ? { to: p.to } : {}) } }, async (_stage, requestId) => {
      // the captured copy of our own frame links back to the write row ("NIC 관찰 ✓")
      this.packet(ch, 'tx', 'device', { src: '192.168.0.2', dst: p.to ?? '192.168.0.10', proto, len: h.length / 2, info, attributed_origin: a.origin, layers: isDiag ? { doip: { type: '0x8001' } } : {}, tx_request_id: requestId });
      return { final: 'confirmed' };
    });
  }
}
