// CAN trace time column (CAN-1): wall clock | relative to a reference | delta to the previous shown row.
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B1-TIME.
//
// Rules (COMMON.md "Clock displays"):
//   · wall  = the event's own ts.wall_ns (UTC ns) shown with the shared fmtWall (UART parity);
//             missing/0 → "--:--:--.---", never derived from mono.
//   · rel / delta = differences of ts.mono_ns, only between rows of the SAME ts.clock_id;
//             a clock change (daemon restart, other producer) shows "≠clock" instead of a number.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-1 work.

import { fmtWall, type LoomEvent } from '../../sdk';
import type { KV } from './kv';

export type TimeMode = 'wall' | 'rel' | 'delta';
export const TIME_MODES: readonly TimeMode[] = ['wall', 'rel', 'delta'];
/** time-column header per mode */
export const TIME_HEAD: Readonly<Record<TimeMode, string>> = { wall: '시각', rel: '상대(s)', delta: 'Δ(ms)' };
/** select option labels per mode */
export const TIME_LABEL: Readonly<Record<TimeMode, string>> = { wall: '벽시계', rel: '상대 (기준점)', delta: 'Δ 직전 행' };
/** per-viewer remembered mode (localStorage) */
export const TIME_MODE_KEY = 'loom.can.timeMode';
/** clock index for "unknown / dictionary full" — never equal to any other row's clock */
export const NO_CLOCK = 0xffff;
/** distinct ts.clock_id values one panel interns (beyond → NO_CLOCK) */
export const CLOCK_IDS_MAX = 64;
/** delta mode: how many display entries to walk back over gap rows to find the previous frame row */
export const DELTA_LOOKBACK = 16;

/** Captured reference point (values, not a ring position: eviction never moves it). */
export interface TimeRef {
  /** ts.mono_ns of the reference row, in ms (f64) */
  readonly monoMs: number;
  /** clock index (ClockDict) of the reference row */
  readonly clock: number;
  /** stream seq of the reference row (shown in the label) */
  readonly seq: number;
  /** 'start' = first frame row this panel ingested; 'row' = user-selected row */
  readonly kind: 'start' | 'row';
}

/** One row's time facts as the model holds them. */
export interface TimeInputs {
  readonly monoMs: number;
  /** ms since Unix epoch (UTC), 0 = unknown */
  readonly wallMs: number;
  readonly clock: number;
  readonly clockId: string | null;
  readonly seq: number;
}

export interface TimeCell {
  text: string;
  title: string;
  /** '' | 'ct-t-unknown' | 'ct-t-xclock' | 'ct-t-gap' */
  cls: string;
}

/** ts.wall_ns (decimal u64 string, UTC ns) → ms since epoch (f64, sub-ms kept); 0 when missing/'0'/malformed. */
export function wallMsOf(ev: Pick<LoomEvent, 'ts'>): number {
  const s = ev.ts?.wall_ns;
  if (typeof s !== 'string' || !/^\d{1,20}$/.test(s)) return 0;
  if (/^0+$/.test(s)) return 0;
  if (s.length <= 6) return Number(s) / 1e6;
  return Number(s.slice(0, -6)) + Number(s.slice(-6)) / 1e6;
}

/** Bounded clock_id interning (≤ CLOCK_IDS_MAX ids; first seen = index 0). */
export class ClockDict {
  /** interned ids, index = position (≤ CLOCK_IDS_MAX) */
  private readonly ids: string[] = [];
  /** id → index */
  private readonly map = new Map<string, number>();
  /** index for a clock id; null/undefined/'' or dictionary full → NO_CLOCK */
  index(id: string | null | undefined): number {
    if (id === null || id === undefined || id === '') return NO_CLOCK;
    const known = this.map.get(id);
    if (known !== undefined) return known;
    if (this.ids.length >= CLOCK_IDS_MAX) return NO_CLOCK;
    const next = this.ids.length;
    this.ids.push(id);
    this.map.set(id, next);
    return next;
  }
  /** clock id for an index; NO_CLOCK / unknown → null */
  idOf(index: number): string | null {
    if (index === NO_CLOCK) return null;
    return this.ids[index] ?? null;
  }
  get size(): number {
    return this.ids.length;
  }
}

/** Format one trace row's time cell for `mode` (pure; see FUNCTIONS.md §B1-TIME for exact text). */
export function timeCell(mode: TimeMode, row: TimeInputs, ref: TimeRef | null, prev: TimeInputs | null, prevAcrossGap: boolean): TimeCell {
  const FULL = `wall ${row.wallMs > 0 ? new Date(row.wallMs).toISOString() : '없음'} · mono ${(row.monoMs / 1000).toFixed(6)} s · clock ${row.clockId ?? '?'} · seq ${row.seq}`;
  const signed = (x: number, d: number): string => (x < 0 ? '-' : '+') + Math.abs(x).toFixed(d);
  const xclock = (a: number, b: number): boolean => a === NO_CLOCK || b === NO_CLOCK || a !== b;
  if (mode === 'wall') {
    if (row.wallMs > 0) return { text: fmtWall(row.wallMs), cls: '', title: FULL };
    return { text: '--:--:--.---', cls: 'ct-t-unknown', title: `벽시계 없음 (ts.wall_ns 없음) — 단조 시각으로 추정하지 않음 · ${FULL}` };
  }
  if (mode === 'rel') {
    if (ref === null) return { text: '—', cls: 'ct-t-unknown', title: `기준점 없음 · ${FULL}` };
    if (xclock(row.clock, ref.clock)) {
      return { text: '≠clock', cls: 'ct-t-xclock', title: `시계가 다름 (행 clock ${row.clockId ?? '?'}) — 기준점과 다른 시계라 상대 시각을 계산하지 않음 · ${FULL}` };
    }
    return { text: signed((row.monoMs - ref.monoMs) / 1000, 6), cls: '', title: `${refLabel(ref)} · ${FULL}` };
  }
  if (prev === null) return { text: '—', cls: 'ct-t-unknown', title: `직전 표시 행 없음 · ${FULL}` };
  if (xclock(row.clock, prev.clock)) {
    return { text: '≠clock', cls: 'ct-t-xclock', title: `시계가 다름 (직전 행 seq ${prev.seq}) — Δ를 계산하지 않음 · ${FULL}` };
  }
  return {
    text: signed(row.monoMs - prev.monoMs, 3),
    cls: prevAcrossGap ? 'ct-t-gap' : '',
    title: `직전 표시 행(seq ${prev.seq})과의 간격 (필터 적용 후)${prevAcrossGap ? ' · 사이에 생략·손실 행 있음' : ''} · ${FULL}`,
  };
}

/** Reference label for the toolbar: 'start' → "기준: 시작 seq N", 'row' → "기준: seq N (선택 행)", null → "기준 없음". */
export function refLabel(ref: TimeRef | null): string {
  if (ref === null) return '기준 없음';
  if (ref.kind === 'start') return `기준: 시작 seq ${ref.seq}`;
  return `기준: seq ${ref.seq} (선택 행)`;
}

/** remembered mode; missing / invalid / storage error → 'wall' */
export function readTimeMode(kv: KV | null): TimeMode {
  try {
    const v = kv?.getItem(TIME_MODE_KEY);
    if (typeof v === 'string' && (TIME_MODES as readonly string[]).includes(v)) return v as TimeMode;
  } catch {
    /* storage unreadable → default */
  }
  return 'wall';
}

/** remember mode; storage errors are swallowed (setting lasts for this page only) */
export function writeTimeMode(kv: KV | null, mode: TimeMode): void {
  try {
    kv?.setItem(TIME_MODE_KEY, mode);
  } catch {
    /* swallowed: setting lasts for this page only */
  }
}
