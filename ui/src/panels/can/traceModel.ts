// CAN Trace model (plan §5.1.3, §6.5, can-ui §5.3 / U-CAN-1). Pure — no DOM.
//
// CAN schema v1 (docs/impl/requests/can-ui.md §1–2):
//   can.frame payload {id:number, ext, fd, brs, esi, rtr, dlc: DLC CODE 0..15, len: BYTES
//   0..64, bytes{encoding:"hex", data}, device_ts?, isotp?, frame_index?}. The len column
//   shows bytes (`len`); the DLC code is kept for the tooltip (FD 12 B = DLC 9).
//   One send = one row: the write row `can.tx` (top request_id, payload.write_state,
//   request in payload.echo_of / payload.frame) and the bus echo `can.frame{dir:"tx"}`
//   with the SAME top request_id merge into one row whose time and bytes come from the
//   echo. A multi-frame send (count > 1) echoes `payload.frame_index` 0..n-1: frame 0
//   merges into the request row, each further index adds a row; the write state is per
//   request and shown on all its rows.
//
// Storage = column ring of typed arrays (no row objects on the main thread):
//   seq f64 · mono_ms f64 · id u32 · flags u16 (dir/ext/fd/brs/origin/hidden/echo/
//   no-data/rtr/no-id) · kind u8 (frame | tx request | gap) · len u8 · dlc u8 ·
//   fidx u16 (frame_index) · wstate u8 · risk u8 · aux u32 (request slot / gap slot) ·
//   data 8 B inline (> 8 B → FD slot ring).
// Bounds (C2), all fixed at construction:
//   rows        : `capacity` (default 100k, max 2M). Full → oldest row overwritten,
//                 `evicted` counts it ("보관 범위 밖 → 질의").
//   FD payloads : rows with > 8 data bytes keep them in an FD slot ring
//                 (max(1024, capacity/16) × 64 B). Overwritten slot → the row shows
//                 "(FD 데이터 보관 범위 밖)" instead of wrong bytes.
//   request ids : REQ_SLOTS (8192) most recent request_ids (oldest slot reused; a later
//                 stage update for a forgotten id starts a new row). Per request ≤
//                 REQ_ROWS_MAX rows get later state updates and ≤ REQ_ROWS_MAX frame
//                 indexes are de-duplicated (rows beyond keep the state they had).
//   gap details : GAP_SLOTS (4096) most recent gap rows (oldest reused; its row then
//                 shows "세부 정보 만료").
//   display idx : ring of `capacity` absolute row numbers (≤ 1 entry per stored row).
//   agent ranges: AGENT_RANGES (16) most recent `core.activity` read ranges.
//
// Honesty rules: gap rows (display / record / source / resync / local omission) are
// never filtered out. Rows hidden by the display budget stay in the ring and are
// counted on one synthetic omission row per run.
//
// Precision: seq is held as f64 — exact below 2^53 (u64 seqs beyond that would round;
// not reachable by a session counter). mono is kept in ms (f64 keeps µs precision).

import type { LoomEvent } from '../../sdk';

export const ROWS_DEFAULT = 100_000;
export const ROWS_MIN = 1_000;
export const ROWS_MAX = 2_000_000;
export const REQ_SLOTS = 8_192;
export const GAP_SLOTS = 4_096;
export const AGENT_RANGES = 16;
export const REQ_ROWS_MAX = 1_024;
export const NO_FIDX = 0xffff;
const NO_DLC = 255;
export const DISPLAY_BUDGET_ROWS_PER_S = 2_000;
const BUDGET_BURST = 500;
const FD_SLOT_BYTES = 64;

/** bytes this model allocates for `capacity` rows (manifest memory share) */
export function bytesForRows(capacity: number): number {
  const perRow = 8 + 8 + 4 + 2 + 1 + 1 + 1 + 2 + 1 + 1 + 4 + 8 + 8; // columns + display index
  return capacity * perRow + fdSlotsFor(capacity) * (FD_SLOT_BYTES + 8);
}
const fdSlotsFor = (cap: number) => Math.max(1_024, Math.ceil(cap / 16));

// kind
export const K_FRAME = 0;
export const K_TXREQ = 1;
export const K_GAP = 2;
// flags
export const F_TX = 1;
export const F_EXT = 2;
export const F_FD = 4;
export const F_BRS = 8;
const F_ORIGIN_SHIFT = 4; // 2 bits
export const F_HIDDEN = 64;
export const F_ECHO = 128;
/** bytes not inline (raw_ref / unknown encoding) — shown as "?" */
export const F_NODATA = 256;
/** Classic remote frame (no data; len = requested length) */
export const F_RTR = 512;
/** row belongs to a request (aux = request slot) */
export const F_REQ = 1024;
/** write row whose request carried no readable id (shown as "?") */
export const F_NOID = 2048;

/** DLC code for a byte count (ISO 11898-1 / CAN FD table) */
export function dlcForLen(len: number): number {
  if (len <= 8) return len;
  if (len <= 12) return 9;
  if (len <= 16) return 10;
  if (len <= 20) return 11;
  if (len <= 24) return 12;
  if (len <= 32) return 13;
  if (len <= 48) return 14;
  return 15;
}
const DLC_LEN = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];
/** byte count for a DLC code (Classic codes 9–15 still mean 8 bytes) */
export function lenForDlc(dlc: number, fd: boolean): number {
  return fd ? (DLC_LEN[dlc] ?? 64) : Math.min(8, dlc);
}

/** frame fields read from a can.frame payload or a can.tx request (echo_of / frame) */
export interface FrameFields {
  id: number | null;
  ext: boolean;
  fd: boolean;
  brs: boolean;
  rtr: boolean;
  /** byte count (payload.len) if given */
  len: number | null;
  /** DLC code (payload.dlc) if given */
  dlc: number | null;
  hex: string | null;
  b64: string | null;
}

export type Origin = 'device' | 'agent' | 'user' | 'script';
const ORIGINS: readonly Origin[] = ['device', 'agent', 'user', 'script'];
const originCode = (o: string): number => Math.max(0, ORIGINS.indexOf(o as Origin));

export const WSTATES = [
  'requested',
  'accepted',
  'sent',
  'confirmed',
  'effective_verified',
  'effect_unverified',
  'failed',
  'outcome_unknown',
] as const;
export type WState = (typeof WSTATES)[number];
/** D4/A14 stage rank (monotonic per request_id); equal-rank finals keep the first */
const RANK = [0, 1, 2, 3, 4, 4, 5, 5];
export const NO_STATE = 255;
const RISKS = ['R0', 'R1', 'R2', 'R3', 'R4'] as const;
export const NO_RISK = 255;

export type GapKindX = 'display.gap' | 'record.gap' | 'source.gap' | 'resync' | 'omit';

export interface GapInfo {
  kind: GapKindX;
  first: number | null;
  last: number | null;
  count: number | 'unknown';
  cause: string;
  /** record health at the time the gap was seen (only 'ok' allows "기록 정상") */
  recordOk: boolean;
  /** loss known only as a producer range (no stream seqs — never invented) */
  producerOnly: boolean;
  local: boolean;
}

export interface ReqInfo {
  requestId: string;
  clientId: string | null;
  error: string | null;
  mine: boolean;
  /** abs rows of this request (≤ REQ_ROWS_MAX) — state/risk updates go to all of them */
  rows: number[];
  /** echo frame indexes already shown (duplicates ignored, ≤ REQ_ROWS_MAX) */
  echoed: Set<number>;
  /** the can.tx row still waiting for its frame-0 echo (abs), or -1 */
  txRow: number;
  state: number;
  risk: number;
}

export interface AgentRange {
  a: number;
  b: number;
  actor: string;
  atMs: number;
}

export interface TraceFilter {
  /** CAN ids to show (null = all) */
  ids: ReadonlySet<number> | null;
  dir: 'all' | 'rx' | 'tx';
  /** origins to show (null = all) */
  origins: ReadonlySet<Origin> | null;
}

export const NO_FILTER: TraceFilter = { ids: null, dir: 'all', origins: null };

export interface ModelOptions {
  capacity?: number;
  /** ms clock for the display budget (performance.now in the panel) */
  now?: () => number;
  /** current record health (gap rows say "기록 정상" only when ok) */
  recordOk?: () => boolean;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null;
const str = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const numOf = (v: unknown): number | null => {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string' && /^\d{1,20}$/.test(v)) return Number(v);
  if (typeof v === 'string' && /^0x[0-9a-f]+$/i.test(v)) return parseInt(v.slice(2), 16);
  return null;
};

export class CanTraceModel {
  readonly capacity: number;
  // ---- row columns ----
  private seq: Float64Array;
  private mono: Float64Array;
  private ids: Uint32Array;
  private flags: Uint16Array;
  private kinds: Uint8Array;
  private lens: Uint8Array;
  private dlcs: Uint8Array;
  private fidx: Uint16Array;
  private wstate: Uint8Array;
  private risk: Uint8Array;
  private aux: Uint32Array;
  private data: Uint8Array; // 8 B per row
  /** absolute number of rows ever appended; row abs ∈ [total - count, total) */
  total = 0;
  count = 0;
  evicted = 0;
  // ---- FD payload ring ----
  private fdData: Uint8Array;
  private fdOwner: Float64Array;
  private fdNext = 0;
  // ---- request_id dictionary ----
  private reqInfo: (ReqInfo | undefined)[] = new Array<ReqInfo | undefined>(REQ_SLOTS);
  private reqRow = new Float64Array(REQ_SLOTS).fill(-1);
  private reqMap = new Map<string, number>();
  private reqNext = 0;
  // ---- gap table ----
  private gapInfo: (GapInfo | undefined)[] = new Array<GapInfo | undefined>(GAP_SLOTS);
  private gapRow = new Float64Array(GAP_SLOTS).fill(-1);
  private gapNext = 0;
  /** gap slot of the open omission run (display budget), -1 = none */
  private omitSlot = -1;
  // ---- display index (abs row numbers, ascending) ----
  private disp: Float64Array;
  private dispHead = 0; // index of oldest entry
  dispCount = 0;
  /** display rows ever appended (for "새 행 N") */
  dispAppended = 0;
  // ---- filter / budget / stats ----
  private filter: TraceFilter = NO_FILTER;
  degraded = false;
  private tokens = BUDGET_BURST;
  private tokensAt = 0;
  /** rows hidden by the display budget (cumulative) */
  omitted = 0;
  /** frames (incl. tx echo merges) received (cumulative) */
  framesIn = 0;
  /** echo frames for an index already shown (ignored, cumulative) */
  dupEchoes = 0;
  /** bumps on any change a renderer must reflect */
  version = 0;
  readonly agentRanges: AgentRange[] = [];
  private readonly now: () => number;
  private readonly recordOk: () => boolean;

  constructor(o: ModelOptions = {}) {
    const cap = Math.round(o.capacity ?? ROWS_DEFAULT);
    this.capacity = Math.min(ROWS_MAX, Math.max(ROWS_MIN, Number.isFinite(cap) ? cap : ROWS_DEFAULT));
    const c = this.capacity;
    this.seq = new Float64Array(c);
    this.mono = new Float64Array(c);
    this.ids = new Uint32Array(c);
    this.flags = new Uint16Array(c);
    this.kinds = new Uint8Array(c);
    this.lens = new Uint8Array(c);
    this.dlcs = new Uint8Array(c);
    this.fidx = new Uint16Array(c);
    this.wstate = new Uint8Array(c);
    this.risk = new Uint8Array(c);
    this.aux = new Uint32Array(c);
    this.data = new Uint8Array(c * 8);
    const fd = fdSlotsFor(c);
    this.fdData = new Uint8Array(fd * FD_SLOT_BYTES);
    this.fdOwner = new Float64Array(fd).fill(-1);
    this.disp = new Float64Array(c);
    this.now = o.now ?? (() => 0);
    this.recordOk = o.recordOk ?? (() => false);
    this.tokensAt = this.now();
  }

  get oldestAbs(): number {
    return this.total - this.count;
  }

  // ======================= ingest =======================

  /** Apply one event of this panel's subscription (can domain + its channel). */
  apply(ev: LoomEvent, mine?: (requestId: string) => boolean): void {
    switch (ev.kind) {
      case 'can.frame':
        this.applyFrame(ev, mine);
        return;
      case 'can.tx':
        this.applyTx(ev, mine);
        return;
      case 'display.gap':
      case 'record.gap':
      case 'source.gap':
        this.applyGapEvent(ev);
        return;
      default:
        return; // can.stats, can.channel_state … are not Trace rows (M1)
    }
  }

  private applyFrame(ev: LoomEvent, mine?: (r: string) => boolean): void {
    this.framesIn++;
    const rid = ev.request_id ?? undefined;
    const p = isObj(ev.payload) ? ev.payload : {};
    const fr = readFrame(p);
    if (rid === undefined) {
      this.appendRow(ev, K_FRAME, fr);
      return;
    }
    // Loom-sent frame (bus echo): one row per frame index, state per request
    const index = numOf(p.frame_index) ?? 0;
    const fi = Math.min(NO_FIDX - 1, Math.max(0, Math.round(index)));
    const slot = this.reqSlot(rid);
    const info = slot === null ? undefined : this.reqInfo[slot];
    if (info) {
      if (info.echoed.has(fi)) {
        this.dupEchoes++;
        this.version++;
        return;
      }
      if (info.echoed.size < REQ_ROWS_MAX) info.echoed.add(fi);
      if (fi === 0 && info.txRow >= this.oldestAbs) {
        // merge into the request row: time and bytes are the echo's
        const i = info.txRow % this.capacity;
        this.mono[i] = monoMs(ev);
        const keep = (this.flags[i] as number) & ((3 << F_ORIGIN_SHIFT) | F_HIDDEN | F_REQ);
        this.flags[i] = keep | F_TX | F_ECHO | frameFlags(fr);
        if (fr.id !== null) this.ids[i] = fr.id >>> 0;
        this.writeData(i, info.txRow, fr);
        this.fidx[i] = fi;
        info.txRow = -1;
        this.version++;
        return;
      }
      const abs = this.appendRow(ev, K_FRAME, fr, true);
      if (abs === null) return;
      const i = abs % this.capacity;
      this.flags[i] = (this.flags[i] as number) | F_ECHO | F_REQ;
      this.fidx[i] = fi;
      this.aux[i] = slot as number;
      this.wstate[i] = info.state;
      this.risk[i] = info.risk;
      if (info.rows.length < REQ_ROWS_MAX) info.rows.push(abs);
      return;
    }
    // echo before (or without) its write row: the echo row becomes the request's row
    const abs = this.appendRow(ev, K_FRAME, fr, true);
    if (abs === null) return;
    const i = abs % this.capacity;
    this.flags[i] = (this.flags[i] as number) | F_ECHO;
    this.fidx[i] = fi;
    const ni = this.bindRequest(rid, abs, ev, mine);
    ni.echoed.add(fi);
  }

  private applyTx(ev: LoomEvent, mine?: (r: string) => boolean): void {
    const rid = ev.request_id ?? undefined;
    const p = isObj(ev.payload) ? ev.payload : {};
    const ws = WSTATES.indexOf(str(p.write_state) as WState);
    if (rid === undefined) return;
    const slot = this.reqSlot(rid);
    const info = slot === null ? undefined : this.reqInfo[slot];
    if (info) {
      if (ws >= 0 && (info.state === NO_STATE || (RANK[ws] as number) > (RANK[info.state] as number))) this.setReqState(info, ws);
      if (ev.risk) this.setReqRisk(info, Math.max(0, RISKS.indexOf(ev.risk)));
      const err = str(p.error);
      if (err) info.error = err;
      this.version++;
      return;
    }
    // first sight of this request: the write row (frame read from the request params)
    const fr = txFrame(p);
    const nabs = this.appendRow(ev, K_TXREQ, fr);
    if (nabs === null) return;
    const ni = this.bindRequest(rid, nabs, ev, mine);
    ni.txRow = nabs;
    if (ws >= 0) this.setReqState(ni, ws);
  }

  private setReqState(info: ReqInfo, ws: number): void {
    info.state = ws;
    for (const abs of info.rows) if (abs >= this.oldestAbs) this.wstate[abs % this.capacity] = ws;
  }
  private setReqRisk(info: ReqInfo, r: number): void {
    info.risk = r;
    for (const abs of info.rows) if (abs >= this.oldestAbs) this.risk[abs % this.capacity] = r;
  }

  private bindRequest(rid: string, abs: number, ev: LoomEvent, mine?: (r: string) => boolean): ReqInfo {
    const slot = this.reqNext;
    this.reqNext = (this.reqNext + 1) % REQ_SLOTS;
    const old = this.reqInfo[slot];
    if (old) this.reqMap.delete(old.requestId);
    const p = isObj(ev.payload) ? ev.payload : {};
    const i = abs % this.capacity;
    const info: ReqInfo = {
      requestId: rid,
      clientId: ev.client_id ?? null,
      error: str(p.error),
      mine: mine?.(rid) ?? false,
      rows: [abs],
      echoed: new Set<number>(),
      txRow: -1,
      state: this.wstate[i] as number,
      risk: this.risk[i] as number,
    };
    this.reqInfo[slot] = info;
    this.reqRow[slot] = abs;
    this.reqMap.set(rid, slot);
    this.aux[i] = slot;
    this.flags[i] = (this.flags[i] as number) | F_REQ;
    return info;
  }

  /** request slot whose first row is still in the ring, else null */
  private reqSlot(rid: string): number | null {
    const slot = this.reqMap.get(rid);
    if (slot === undefined) return null;
    return (this.reqRow[slot] as number) >= this.oldestAbs ? slot : null;
  }

  /** abs row of a request id still in the ring, else null */
  private reqAbs(rid: string): number | null {
    const slot = this.reqSlot(rid);
    return slot === null ? null : (this.reqRow[slot] as number);
  }

  /** Mark a request id as sent by this viewer (USR(나)). */
  markMine(rid: string): void {
    const slot = this.reqMap.get(rid);
    const info = slot === undefined ? undefined : this.reqInfo[slot];
    if (info) {
      info.mine = true;
      this.version++;
    }
  }

  /**
   * Append a frame / write row. `write` rows (requests and their echoes) are never hidden
   * by the display budget. A write row without a readable id is kept (id "?").
   */
  private appendRow(ev: LoomEvent, kind: number, fr: FrameFields, write = false): number | null {
    const isWrite = write || kind === K_TXREQ;
    if (fr.id === null && !isWrite) return null;
    const id = fr.id ?? 0;
    let fl = (ev.dir === 'tx' || kind === K_TXREQ ? F_TX : 0) | frameFlags(fr) | (fr.id === null ? F_NOID : 0);
    fl |= originCode(ev.origin) << F_ORIGIN_SHIFT;
    const visible = this.passes(fl, id);
    let hidden = false;
    if (visible && this.degraded && !isWrite && !this.takeToken()) hidden = true;
    const seq = numOf(ev.seq) ?? 0;
    if (hidden) {
      this.omitRun(seq);
      fl |= F_HIDDEN;
      this.omitted++;
    } else if (visible) {
      this.omitSlot = -1; // a shown row ends the omission run
    }
    const abs = this.push(kind, seq, monoMs(ev), fl);
    const i = abs % this.capacity;
    this.ids[i] = id >>> 0;
    this.wstate[i] = NO_STATE;
    this.risk[i] = ev.risk ? Math.max(0, RISKS.indexOf(ev.risk)) : NO_RISK;
    this.writeData(i, abs, fr);
    if (visible && !hidden) this.dispPush(abs);
    return abs;
  }

  private writeData(i: number, abs: number, fr: FrameFields): void {
    let fl = this.flags[i] as number;
    fl &= ~F_NODATA;
    if (fr.rtr) {
      // remote frame: no data, len = requested length
      const n = fr.len ?? (fr.dlc !== null ? lenForDlc(fr.dlc, false) : 0);
      this.lens[i] = Math.min(8, Math.max(0, n));
      this.dlcs[i] = fr.dlc ?? dlcForLen(this.lens[i] as number);
      this.flags[i] = fl;
      return;
    }
    const s = fr.hex;
    if (s !== null && s.length % 2 === 0 && s.length <= 128 && /^[0-9a-fA-F]*$/.test(s)) {
      const len = s.length / 2;
      const dst = this.dataSlot(i, abs, len);
      for (let k = 0; k < len; k++) dst.buf[dst.base + k] = parseInt(s.substr(k * 2, 2), 16);
      this.lens[i] = len;
      this.dlcs[i] = fr.dlc ?? dlcForLen(len);
      this.flags[i] = fl;
      return;
    }
    let bytes: string | null = null; // latin1 string of the bytes
    if (fr.b64 !== null) {
      try {
        bytes = atob(fr.b64);
      } catch {
        bytes = null;
      }
    }
    if (bytes !== null && bytes.length <= 64) {
      const dst = this.dataSlot(i, abs, bytes.length);
      for (let k = 0; k < bytes.length; k++) dst.buf[dst.base + k] = bytes.charCodeAt(k) & 0xff;
      this.lens[i] = bytes.length;
      this.dlcs[i] = fr.dlc ?? dlcForLen(bytes.length);
      this.flags[i] = fl;
      return;
    }
    // bytes not inline (raw_ref, malformed): keep the length (len, else from the DLC code), show "?"
    const n = fr.len ?? (fr.dlc !== null ? lenForDlc(fr.dlc, fr.fd) : 0);
    this.lens[i] = Math.min(64, Math.max(0, n));
    this.dlcs[i] = fr.dlc ?? NO_DLC;
    this.flags[i] = fl | F_NODATA;
  }

  /** FD slot of rows with > 8 data bytes (bounded: one entry per FD slot, removed on reuse) */
  private fdOf = new Map<number, number>();

  private dataSlot(i: number, abs: number, len: number): { buf: Uint8Array; base: number } {
    if (len <= 8) return { buf: this.data, base: i * 8 };
    const slot = this.fdNext;
    this.fdNext = (this.fdNext + 1) % this.fdOwner.length;
    const prevOwner = this.fdOwner[slot] as number;
    if (prevOwner >= 0) this.fdOf.delete(prevOwner);
    this.fdOwner[slot] = abs;
    this.fdOf.set(abs, slot);
    return { buf: this.fdData, base: slot * FD_SLOT_BYTES };
  }

  private applyGapEvent(ev: LoomEvent): void {
    const p = isObj(ev.payload) ? ev.payload : {};
    const r = isObj(p.seq_range) ? p.seq_range : null;
    const first = r ? numOf(r.from) : null;
    const last = r ? numOf(r.to) : null;
    const c = p.count;
    const count = typeof c === 'number' ? c : typeof c === 'string' && /^\d{1,20}$/.test(c) ? Number(c) : 'unknown';
    const detail = isObj(p.detail) ? p.detail : {};
    this.addGap(
      {
        kind: ev.kind as GapKindX,
        first,
        last,
        count,
        cause: str(p.cause) ?? '?',
        recordOk: this.recordOk(),
        producerOnly: !r && isObj(p.producer_range),
        local: detail.local === true,
      },
      numOf(ev.seq) ?? 0,
      monoMs(ev),
    );
  }

  /** Gap rows from outside the event stream (batch display_gap summary, resync). */
  addGap(g: GapInfo, seq = 0, mono = 0): void {
    this.omitSlot = -1;
    const abs = this.push(K_GAP, seq, mono, 0);
    this.bindGap(abs, g);
    this.dispPush(abs); // gaps are never filtered
  }

  private bindGap(abs: number, g: GapInfo): number {
    const slot = this.gapNext;
    this.gapNext = (this.gapNext + 1) % GAP_SLOTS;
    this.gapInfo[slot] = g;
    this.gapRow[slot] = abs;
    this.aux[abs % this.capacity] = slot;
    if (slot === this.omitSlot) this.omitSlot = -1;
    return slot;
  }

  /** extend (or open) the omission row for a budget-hidden row */
  private omitRun(seq: number): void {
    const g = this.omitSlot >= 0 && (this.gapRow[this.omitSlot] as number) >= this.oldestAbs ? this.gapInfo[this.omitSlot] : undefined;
    if (g && typeof g.count === 'number') {
      g.count++;
      g.last = seq;
      g.recordOk = g.recordOk && this.recordOk();
      return;
    }
    const abs = this.push(K_GAP, seq, 0, 0);
    this.omitSlot = this.bindGap(abs, {
      kind: 'omit',
      first: seq,
      last: seq,
      count: 1,
      cause: 'display_budget',
      recordOk: this.recordOk(),
      producerOnly: false,
      local: true,
    });
    this.dispPush(abs);
  }

  private takeToken(): boolean {
    const t = this.now();
    this.tokens = Math.min(BUDGET_BURST, this.tokens + ((t - this.tokensAt) * DISPLAY_BUDGET_ROWS_PER_S) / 1000);
    this.tokensAt = t;
    if (this.tokens >= 1) {
      this.tokens -= 1;
      return true;
    }
    return false;
  }

  setDegraded(d: boolean): void {
    if (d === this.degraded) return;
    this.degraded = d;
    this.tokens = BUDGET_BURST;
    this.tokensAt = this.now();
    if (!d) this.omitSlot = -1;
    this.version++;
  }

  private push(kind: number, seq: number, mono: number, flags: number): number {
    const abs = this.total++;
    const i = abs % this.capacity;
    if (this.count === this.capacity) this.evicted++;
    else this.count++;
    this.kinds[i] = kind;
    this.seq[i] = seq;
    this.mono[i] = mono;
    this.flags[i] = flags;
    this.ids[i] = 0;
    this.lens[i] = 0;
    this.dlcs[i] = NO_DLC;
    this.fidx[i] = NO_FIDX;
    this.wstate[i] = NO_STATE;
    this.risk[i] = NO_RISK;
    this.aux[i] = 0;
    this.version++;
    return abs;
  }

  // ======================= agent read ranges =======================

  /** `core.activity` with range{seq_a, seq_b} for this domain/channel. Returns true if it applied. */
  applyActivity(ev: LoomEvent, channel: string, nowMs: number): boolean {
    if (ev.kind !== 'core.activity' || !isObj(ev.payload)) return false;
    const p = ev.payload;
    // schema/v1 {client_id, method, summary, range{from,to}}; also CAN answer {action, domain,
    // channel, range{seq_a,seq_b}} and the earlier {actor}
    const method = str(p.method) ?? '';
    const domain = str(p.domain) ?? (method.includes('.') ? method.split('.')[0] : null) ?? ev.domain;
    if (domain !== 'can') return false;
    const ch = str(p.channel) ?? ev.channel ?? null;
    if (ch !== null && ch !== channel) return false;
    const r = isObj(p.range) ? p.range : null;
    const a = r ? (numOf(r.from) ?? numOf(r.seq_a)) : null;
    const b = r ? (numOf(r.to) ?? numOf(r.seq_b)) : null;
    if (a === null || b === null) return false;
    this.agentRanges.push({ a: Math.min(a, b), b: Math.max(a, b), actor: str(p.actor) ?? str(p.client_id) ?? ev.client_id ?? 'agent', atMs: nowMs });
    if (this.agentRanges.length > AGENT_RANGES) this.agentRanges.shift();
    this.version++;
    return true;
  }

  agentReadAt(seq: number): AgentRange | null {
    for (let k = this.agentRanges.length - 1; k >= 0; k--) {
      const r = this.agentRanges[k]!;
      if (seq >= r.a && seq <= r.b) return r;
    }
    return null;
  }

  // ======================= display index =======================

  private dispPush(abs: number): void {
    this.dispTrim();
    if (this.dispCount === this.capacity) {
      // cannot happen (≤ 1 entry per stored row after trim) — keep the bound anyway
      this.dispHead = (this.dispHead + 1) % this.capacity;
      this.dispCount--;
    }
    this.disp[(this.dispHead + this.dispCount) % this.capacity] = abs;
    this.dispCount++;
    this.dispAppended++;
  }

  /** drop display entries whose rows were evicted from the ring */
  dispTrim(): void {
    const oldest = this.oldestAbs;
    while (this.dispCount > 0 && (this.disp[this.dispHead] as number) < oldest) {
      this.dispHead = (this.dispHead + 1) % this.capacity;
      this.dispCount--;
    }
  }

  /** abs row number of the k-th displayed row (0 = oldest shown) */
  dispAt(k: number): number {
    return this.disp[(this.dispHead + k) % this.capacity] as number;
  }

  /** position of the first display entry with abs ≥ target (binary search) */
  dispIndexOf(abs: number): number {
    let lo = 0;
    let hi = this.dispCount;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (this.dispAt(mid) < abs) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  setFilter(f: TraceFilter): void {
    this.filter = f;
    // rebuild once: O(rows in ring)
    this.dispHead = 0;
    this.dispCount = 0;
    for (let abs = this.oldestAbs; abs < this.total; abs++) {
      const i = abs % this.capacity;
      const k = this.kinds[i] as number;
      const fl = this.flags[i] as number;
      if (k === K_GAP || (!(fl & F_HIDDEN) && this.passes(fl, this.ids[i] as number))) {
        this.disp[this.dispCount++] = abs;
      }
    }
    this.version++;
  }

  getFilter(): TraceFilter {
    return this.filter;
  }

  private passes(fl: number, id: number): boolean {
    const f = this.filter;
    if (f.dir === 'tx' && !(fl & F_TX)) return false;
    if (f.dir === 'rx' && fl & F_TX) return false;
    if (f.ids && !f.ids.has(id)) return false;
    if (f.origins && !f.origins.has(ORIGINS[(fl >> F_ORIGIN_SHIFT) & 3] as Origin)) return false;
    return true;
  }

  // ======================= row readers (renderer) =======================

  has(abs: number): boolean {
    return abs >= this.oldestAbs && abs < this.total;
  }
  kindAt(abs: number): number {
    return this.kinds[abs % this.capacity] as number;
  }
  seqAt(abs: number): number {
    return this.seq[abs % this.capacity] as number;
  }
  monoAt(abs: number): number {
    return this.mono[abs % this.capacity] as number;
  }
  idAt(abs: number): number {
    return this.ids[abs % this.capacity] as number;
  }
  flagsAt(abs: number): number {
    return this.flags[abs % this.capacity] as number;
  }
  originAt(abs: number): Origin {
    return ORIGINS[((this.flags[abs % this.capacity] as number) >> F_ORIGIN_SHIFT) & 3] as Origin;
  }
  /** byte count (payload.len) */
  lenAt(abs: number): number {
    return this.lens[abs % this.capacity] as number;
  }
  /** DLC code (payload.dlc, else derived from len); null when unknown */
  dlcAt(abs: number): number | null {
    const d = this.dlcs[abs % this.capacity] as number;
    return d === NO_DLC ? null : d;
  }
  /** multi-frame send: frame_index of this echo row; null when not given */
  frameIndexAt(abs: number): number | null {
    const v = this.fidx[abs % this.capacity] as number;
    return v === NO_FIDX ? null : v;
  }
  stateAt(abs: number): WState | null {
    const w = this.wstate[abs % this.capacity] as number;
    return w === NO_STATE ? null : (WSTATES[w] as WState);
  }
  riskAt(abs: number): string | null {
    const r = this.risk[abs % this.capacity] as number;
    return r === NO_RISK ? null : (RISKS[r] ?? null);
  }
  reqAt(abs: number): ReqInfo | null {
    const i = abs % this.capacity;
    if (this.kinds[i] === K_GAP || !((this.flags[i] as number) & F_REQ)) return null;
    const slot = this.aux[i] as number;
    if (slot >= REQ_SLOTS) return null;
    // the slot still belongs to this row's request iff its first row is not newer than the row
    return (this.reqRow[slot] as number) <= abs ? (this.reqInfo[slot] ?? null) : null;
  }
  gapAt(abs: number): GapInfo | null {
    const i = abs % this.capacity;
    if (this.kinds[i] !== K_GAP) return null;
    const slot = this.aux[i] as number;
    return this.gapRow[slot] === abs ? (this.gapInfo[slot] ?? null) : null;
  }

  /** data hex "01 A5 …"; "?" when bytes are not inline; null when an FD slot was reused */
  dataHexAt(abs: number): string | null {
    const i = abs % this.capacity;
    const len = this.lens[i] as number;
    if ((this.flags[i] as number) & F_RTR) return '';
    if ((this.flags[i] as number) & F_NODATA) return len ? '?' : '';
    let src: Uint8Array = this.data;
    let base = i * 8;
    if (len > 8) {
      const slot = this.fdOf.get(abs);
      if (slot === undefined || this.fdOwner[slot] !== abs) return null;
      src = this.fdData;
      base = slot * FD_SLOT_BYTES;
    }
    let s = '';
    for (let k = 0; k < len; k++) s += (k ? ' ' : '') + HEX[src[base + k] as number];
    return s;
  }

  /** request id → its write state (if still known) */
  stateOf(rid: string): WState | null {
    const slot = this.reqSlot(rid);
    const info = slot === null ? undefined : this.reqInfo[slot];
    return info && info.state !== NO_STATE ? (WSTATES[info.state] as WState) : null;
  }

  /** rows of a request still in the ring (oldest first) */
  rowsOf(rid: string): number[] {
    const slot = this.reqSlot(rid);
    const info = slot === null ? undefined : this.reqInfo[slot];
    return info ? info.rows.filter((a) => a >= this.oldestAbs) : [];
  }

  /** raise a row's stage from another source (control path / RowStore), monotonic, request-wide */
  raiseState(abs: number, s: WState): boolean {
    const i = abs % this.capacity;
    const w = WSTATES.indexOf(s);
    const req = this.reqAt(abs);
    const cur = req ? req.state : (this.wstate[i] as number);
    if (w < 0 || (cur !== NO_STATE && (RANK[w] as number) <= (RANK[cur] as number))) return false;
    if (req) this.setReqState(req, w);
    else this.wstate[i] = w;
    this.version++;
    return true;
  }
}

const HEX: string[] = Array.from({ length: 256 }, (_, i) => i.toString(16).toUpperCase().padStart(2, '0'));

/** ts.mono_ns (u64 decimal string) → ms as f64 (µs precision kept) */
export function monoMs(ev: LoomEvent): number {
  const s = ev.ts?.mono_ns;
  if (typeof s !== 'string' || s.length === 0) return 0;
  // split to keep precision: ns = hi * 1e6 + lo
  if (s.length <= 6) return Number(s) / 1e6;
  return Number(s.slice(0, -6)) + Number(s.slice(-6)) / 1e6;
}

const bool = (v: unknown) => v === true;
const smallInt = (v: unknown, max: number): number | null => {
  const n = numOf(v);
  return n !== null && Number.isInteger(n) && n >= 0 && n <= max ? n : null;
};

export function frameFlags(fr: FrameFields): number {
  return (fr.ext ? F_EXT : 0) | (fr.fd ? F_FD : 0) | (fr.brs ? F_BRS : 0) | (fr.rtr ? F_RTR : 0);
}

/** can.frame payload (schema v1 §2) → frame fields */
export function readFrame(p: Record<string, unknown>): FrameFields {
  const id = numOf(p.id);
  const b = isObj(p.bytes) ? p.bytes : null;
  const enc = b ? b.encoding : null;
  const data = b ? str(b.data) : (str(p.data) ?? str(p.data_hex));
  return {
    id: id !== null && id <= 0x1fffffff ? id : null,
    ext: bool(p.ext) || (id !== null && id > 0x7ff),
    fd: bool(p.fd),
    brs: bool(p.brs),
    rtr: bool(p.rtr),
    len: smallInt(p.len, 64),
    dlc: smallInt(p.dlc, 15),
    hex: enc === 'base64' ? null : data,
    b64: enc === 'base64' ? data : null,
  };
}

/** cansend text "300#01" / "18DAF110#02 10 03" / "123##1 00 …" / "300#R" → frame fields (no checks beyond syntax) */
export function parseCansend(text: string): FrameFields | null {
  const m = /^\s*([0-9a-fA-F]{1,8})#(#([0-9a-fA-F]))?(.*)$/.exec(text);
  if (!m) return null;
  const digits = m[1] as string;
  const id = parseInt(digits, 16);
  if (id > 0x1fffffff) return null;
  const fd = m[2] !== undefined;
  const rest = (m[4] ?? '').trim();
  const rtr = !fd && /^r\d*$/i.test(rest);
  const hex = rtr ? '' : rest.replace(/[\s.]/g, '');
  if (!rtr && !/^([0-9a-fA-F]{2})*$/.test(hex)) return null;
  const rlen = rtr && rest.length > 1 ? Number(rest.slice(1)) : null;
  return {
    id,
    ext: digits.length === 8 || id > 0x7ff,
    fd,
    brs: fd && (parseInt(m[3] as string, 16) & 1) === 1,
    rtr,
    len: rtr ? rlen : hex.length / 2,
    dlc: null,
    hex: rtr ? null : hex.toLowerCase(),
    b64: null,
  };
}

/**
 * can.tx write row → the requested frame. The request params ride in payload.echo_of
 * (CORE fixture gateway), payload.frame (string or object), or the payload itself:
 * `{frame:"300#01"}` (cansend) or structured `{id, ext, fd, brs, esi, data}`.
 */
export function txFrame(p: Record<string, unknown>): FrameFields {
  const fromObj = (o: Record<string, unknown>): FrameFields | null => {
    if (typeof o.frame === 'string') return parseCansend(o.frame);
    if (isObj(o.frame)) return fromObj(o.frame);
    if (numOf(o.id) !== null) return readFrame(o);
    return null;
  };
  return (
    fromObj(p) ??
    (isObj(p.echo_of) ? fromObj(p.echo_of) : null) ?? { id: null, ext: false, fd: false, brs: false, rtr: false, len: null, dlc: null, hex: null, b64: null }
  );
}
