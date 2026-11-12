// CAN receive summary — one row per (channel, frame format, CAN-ID), PCAN-View "Receive" list
// analogue (research/fidelity/can.md row 2). Panel-local and derived ONLY from the same
// `can.frame` events that feed the chronological ring (traceModel.ts); no new RPC.
//
// Semantics (shown to the user, never hidden):
//   · count      = rx frames THIS panel aggregated since the window start (mount / reset) —
//                  not a bus total. Device rx only: Loom's own tx echoes are not "received".
//   · window     = aggregation window: start time, frames aggregated, ids tracked.
//   · gap        = loss marks seen while aggregating (display/record/source gap, resync).
//                  A row that existed when a gap happened shows its count as a lower bound
//                  ("≥"), and its cycle time is cleared until two frames arrive gap-free.
//   · bound      = at most `maxIds` rows; frames of further ids are counted as
//                  "untracked" (frames only — the id set itself is not kept, so not unbounded).
//   · ring eviction / render pause do not touch the summary (separate structure).
// Unsupported here (no daemon source): bus load, bus state, controller overruns, bitrate.

import type { LoomEvent } from '../../sdk';
import { readFrame } from './traceModel';
import { fmtId } from './sendLine';

export const RX_SUMMARY_MAX_IDS = 1_024;

export interface RxRow {
  readonly key: string;
  readonly channel: string;
  readonly id: number;
  readonly ext: boolean;
  fd: boolean;
  brs: boolean;
  esi: boolean;
  rtr: boolean;
  /** byte count of the latest frame */
  len: number;
  /** DLC code if the event carried it */
  dlc: number | null;
  /** latest bytes, lower-case hex without separators; null = bytes not inline (raw_ref / malformed) */
  data: string | null;
  /** frames aggregated in this window */
  count: number;
  firstMonoMs: number;
  lastMonoMs: number;
  /** last inter-arrival time (ms) of this id; null until 2 gap-free frames */
  cycleMs: number | null;
  /** gap marks seen since this row was created → count is a lower bound */
  gapsSince: number;
  /** stream seq of the latest frame (trace cross-check) */
  lastSeq: string;
  origin: string;
  /** bytes changed vs. the previous frame of this id */
  changed: boolean;
  /** window gap counter at this row's last frame (cycle-time validity) */
  gapEpoch: number;
}

export interface RxWindow {
  /** monotonic ms (hub) of the first aggregated frame, null before any */
  startMonoMs: number | null;
  /** panel clock (ms) when the window was (re)started */
  startedAtMs: number;
  frames: number;
  ids: number;
  maxIds: number;
  /** frames of ids beyond the bound (not aggregated) */
  untrackedFrames: number;
  /** gap marks seen in this window */
  gaps: number;
  /** sum of known lost/omitted counts from those marks */
  gapFramesKnown: number;
  /** marks whose count is unknown (resync, count: "unknown") */
  gapsUnknown: number;
}

const toHexFromB64 = (b64: string): string | null => {
  try {
    const s = atob(b64);
    let out = '';
    for (let i = 0; i < s.length; i++) out += (s.charCodeAt(i) & 0xff).toString(16).padStart(2, '0');
    return out;
  } catch {
    return null;
  }
};
const monoOf = (ev: LoomEvent): number => {
  const s = ev.ts?.mono_ns;
  if (typeof s !== 'string' || s.length === 0) return 0;
  if (s.length <= 6) return Number(s) / 1e6;
  return Number(s.slice(0, -6)) + Number(s.slice(-6)) / 1e6;
};

export const rxKey = (channel: string, ext: boolean, id: number) => `${channel}|${ext ? 'x' : 's'}|${id}`;

export class RxSummary {
  readonly maxIds: number;
  private rows = new Map<string, RxRow>();
  private sorted: RxRow[] | null = [];
  private win: RxWindow;
  /** bumps on every change (render diffing) */
  version = 0;

  constructor(opts: { maxIds?: number; nowMs?: number } = {}) {
    this.maxIds = Math.max(1, Math.min(65_536, opts.maxIds ?? RX_SUMMARY_MAX_IDS));
    this.win = this.freshWindow(opts.nowMs ?? 0);
  }

  private freshWindow(nowMs: number): RxWindow {
    return { startMonoMs: null, startedAtMs: nowMs, frames: 0, ids: 0, maxIds: this.maxIds, untrackedFrames: 0, gaps: 0, gapFramesKnown: 0, gapsUnknown: 0 };
  }

  /** feed any event; only device-received can.frame rows are aggregated. Returns true if aggregated. */
  apply(ev: LoomEvent): boolean {
    if (ev.kind !== 'can.frame' || ev.dir === 'tx' || ev.request_id) return false;
    const p = typeof ev.payload === 'object' && ev.payload !== null ? (ev.payload as Record<string, unknown>) : {};
    const fr = readFrame(p);
    if (fr.id === null) return false;
    const channel = typeof ev.channel === 'string' ? ev.channel : '';
    const key = rxKey(channel, fr.ext, fr.id);
    const t = monoOf(ev);
    let r = this.rows.get(key);
    if (!r) {
      if (this.rows.size >= this.maxIds) {
        this.win.untrackedFrames++;
        this.version++;
        return false;
      }
      r = {
        key, channel, id: fr.id, ext: fr.ext, fd: false, brs: false, esi: false, rtr: false, len: 0, dlc: null, data: null,
        count: 0, firstMonoMs: t, lastMonoMs: t, cycleMs: null, gapsSince: 0, lastSeq: '', origin: '', changed: false, gapEpoch: this.win.gaps,
      };
      this.rows.set(key, r);
      this.sorted = null;
      this.win.ids = this.rows.size;
    } else {
      // a gap between the previous frame and this one makes the interval meaningless
      r.cycleMs = r.gapEpoch === this.win.gaps && t >= r.lastMonoMs ? t - r.lastMonoMs : null;
    }
    const data = fr.rtr ? '' : fr.hex !== null ? (/^([0-9a-fA-F]{2})*$/.test(fr.hex) ? fr.hex.toLowerCase() : null) : fr.b64 !== null ? toHexFromB64(fr.b64) : null;
    r.changed = r.count > 0 && data !== r.data;
    r.fd = fr.fd;
    r.brs = fr.brs;
    r.esi = p.esi === true;
    r.rtr = fr.rtr;
    r.data = data;
    r.len = fr.len ?? (data !== null ? data.length / 2 : 0);
    r.dlc = fr.dlc;
    r.count++;
    r.lastMonoMs = t;
    r.lastSeq = String(ev.seq);
    r.origin = ev.origin ?? '';
    r.gapEpoch = this.win.gaps;
    if (this.win.startMonoMs === null) this.win.startMonoMs = t;
    this.win.frames++;
    this.version++;
    return true;
  }

  /** a loss mark (gap event / gap mark / resync) was seen by the panel */
  noteGap(count: number | 'unknown'): void {
    this.win.gaps++;
    if (count === 'unknown') this.win.gapsUnknown++;
    else this.win.gapFramesKnown += Math.max(0, count);
    for (const r of this.rows.values()) r.gapsSince++;
    this.version++;
  }

  /** new aggregation window (user "reset"); the trace ring is untouched */
  reset(nowMs: number): void {
    this.rows.clear();
    this.sorted = [];
    this.win = this.freshWindow(nowMs);
    this.version++;
  }

  get window(): Readonly<RxWindow> {
    return this.win;
  }
  get size(): number {
    return this.rows.size;
  }
  get(key: string): Readonly<RxRow> | undefined {
    return this.rows.get(key);
  }
  /** rows ordered like the reference Receive list: 11-bit before 29-bit, then by id */
  list(): readonly Readonly<RxRow>[] {
    if (!this.sorted) {
      this.sorted = [...this.rows.values()].sort((a, b) => (a.channel < b.channel ? -1 : a.channel > b.channel ? 1 : 0) || Number(a.ext) - Number(b.ext) || a.id - b.id);
    }
    return this.sorted;
  }
}

/** "FD BRS" / "EXT" / "RTR" — compact type cell like the reference Type column */
export function typeFlags(r: Pick<RxRow, 'ext' | 'fd' | 'brs' | 'esi' | 'rtr'>): string[] {
  const out: string[] = [r.ext ? 'EXT' : 'STD'];
  if (r.fd) out.push('FD');
  if (r.brs) out.push('BRS');
  if (r.esi) out.push('ESI');
  if (r.rtr) out.push('RTR');
  return out;
}

export function fmtData(hex: string | null, rtr: boolean): string {
  if (rtr) return 'RTR';
  if (hex === null) return '?';
  return (hex.match(/../g) ?? []).join(' ').toUpperCase();
}

export const fmtRxId = (r: Pick<RxRow, 'id' | 'ext'>) => fmtId(r.id, r.ext);
