// Waveform geometry + canvas painter for logic.analyzer.
// Geometry is pure (unit-tested): lane runs are computed in pixel space from retained chunks.
//   level 0/1  known steady level
//   dense      ≥ 2 transitions inside one pixel column — drawn as a filled block, labelled, never a fake level
//   unknown    no raw data (edge cap, chunk gap, stream loss, eviction, before first chunk) — hatched, never bridged
// Decoded annotations are a separate track (boxes over [start,end]) and never drawn as raw levels.

import type { Annotation, CaptureData, UnknownRange } from './logicModel';
import { annEnd, fmtSampleTime, tickStep } from './logicModel';

export interface View {
  /** first visible sample (may be fractional) */
  s0: number;
  /** samples per pixel */
  spp: number;
  /** drawable width in px (excluding the lane label gutter) */
  width: number;
}

export type RunKind = 0 | 1 | 'dense' | 'unknown';
export interface Run {
  x0: number;
  x1: number;
  kind: RunKind;
  /** transitions counted inside a dense run */
  n?: number;
}

const xOf = (v: View, s: number) => (s - v.s0) / v.spp;

function pushRun(runs: Run[], x0: number, x1: number, kind: RunKind, n = 0): void {
  if (x1 <= x0) return;
  const last = runs[runs.length - 1];
  if (last && last.kind === kind && Math.abs(last.x1 - x0) < 1e-6) {
    last.x1 = x1;
    if (kind === 'dense') last.n = (last.n ?? 0) + n;
    return;
  }
  runs.push(kind === 'dense' ? { x0, x1, kind, n } : { x0, x1, kind });
}

/** unknown ranges sorted, clipped to [a,b) */
function unknownIn(u: readonly UnknownRange[], a: number, b: number): UnknownRange[] {
  return u.filter((r) => r.end > a && r.start < b);
}

/** sample ranges with raw data inside the view: chunks minus unknown/evicted ranges, ordered */
export function knownPieces(cap: CaptureData, v: View): { a: number; b: number; ci: number }[] {
  const s1 = v.s0 + v.spp * v.width;
  const uk = unknownIn(cap.unknown, v.s0, s1);
  const out: { a: number; b: number; ci: number }[] = [];
  for (let ci = cap.chunkAt(v.s0); ci < cap.chunks.length; ci++) {
    const c = cap.chunks[ci]!;
    if (c.start >= s1) break;
    let p0 = Math.max(c.start, v.s0, cap.evictedTo);
    const end = Math.min(c.end, s1);
    for (const u of uk) {
      if (u.end <= p0 || u.start >= end) continue;
      if (u.start > p0) out.push({ a: p0, b: u.start, ci });
      p0 = Math.max(p0, u.end);
    }
    if (p0 < end) out.push({ a: p0, b: end, ci });
  }
  return out;
}

/**
 * Lane runs for channel `bit` over the view. Pixel columns with ≥ 2 transitions of this bit become `dense`.
 * Sample ranges without data are `unknown`; the level is NOT carried across them.
 */
export function laneRuns(cap: CaptureData, bit: number, v: View): Run[] {
  const runs: Run[] = [];
  const s1 = v.s0 + v.spp * v.width;
  const m = 1 << bit;
  const lv = (x: number): 0 | 1 => ((x & m) !== 0 ? 1 : 0);
  // retained data ends at the head; beyond it nothing has been sampled yet → blank, not "unknown"
  const head = Math.min(cap.range?.end ?? v.s0, s1);
  let cursor = v.s0;
  for (const p of knownPieces(cap, v)) {
    if (p.a > cursor) pushRun(runs, xOf(v, cursor), xOf(v, p.a), 'unknown');
    const c = cap.chunks[p.ci]!;
    // level at p.a
    let i = 0;
    let level = lv(c.initial);
    while (i < c.samples.length && c.samples[i]! <= p.a) level = lv(c.masks[i++]!);
    let from = xOf(v, p.a);
    let col = Number.NaN;
    let colCount = 0;
    let colStartLevel: 0 | 1 = level;
    const pieceEnd = Math.min(xOf(v, p.b), v.width);
    const closeCol = () => {
      if (colCount >= 2) {
        // clipped to this piece: a dense pixel never claims samples past the chunk end / into unknown
        const x1 = Math.min(col + 1, pieceEnd);
        pushRun(runs, Math.max(from, col), x1, 'dense', colCount);
        from = Math.max(from, x1);
      }
      colCount = 0;
    };
    for (; i < c.samples.length && c.samples[i]! < p.b; i++) {
      const nl = lv(c.masks[i]!);
      if (nl === level) continue;
      const x = xOf(v, c.samples[i]!);
      const cx = Math.floor(x);
      if (cx !== col) {
        closeCol();
        col = cx;
        colStartLevel = level;
      }
      if (colCount === 0) {
        pushRun(runs, from, x, colStartLevel);
        from = x;
      }
      colCount++;
      level = nl;
    }
    closeCol();
    pushRun(runs, from, xOf(v, p.b), level);
    cursor = p.b;
  }
  if (head > cursor) pushRun(runs, xOf(v, cursor), xOf(v, head), 'unknown');
  return runs;
}

export const MAX_VISIBLE_ANNOTATIONS = 2000;

/** annotations overlapping the view, grouped by decoder id (string preserved, e.g. "uart-1").
 * `omitted` > 0 when the draw cap was hit (shown to the user, never silent). */
export function visibleAnnotations(cap: CaptureData, v: View, limit = MAX_VISIBLE_ANNOTATIONS): { byDecoder: Map<string, Annotation[]>; omitted: number } {
  const s1 = v.s0 + v.spp * v.width;
  const byDecoder = new Map<string, Annotation[]>();
  let n = 0;
  let omitted = 0;
  for (let i = cap.annotationAt(v.s0); i < cap.annotations.length; i++) {
    const a = cap.annotations[i]!;
    if (a.start >= s1) break;
    if (annEnd(a) <= v.s0) continue; // nested/long frames: start order ≠ end order
    if (n >= limit) {
      omitted++;
      continue;
    }
    let list = byDecoder.get(a.decoder);
    if (!list) byDecoder.set(a.decoder, (list = []));
    list.push(a);
    n++;
  }
  return { byDecoder, omitted };
}

export interface Tick {
  x: number;
  label: string;
  major: boolean;
}

/** Approximate label glyph width (11px UI font) used to keep axis labels from overlapping. */
export const AXIS_CHAR_PX = 6.5;

/** Tick label with enough decimals to resolve `step` (samples) in the label's own unit, so
 * neighbouring ticks never collapse to the same text (e.g. 2.7 s + 50 µs steps → 2.70005 s). */
export function tickLabel(sample: number, step: number, rate: number | null): string {
  if (!rate) return fmtSampleTime(sample, rate);
  const t = sample / rate;
  const a = Math.abs(t);
  const [scale, unit] = a >= 1 ? [1, 's'] : a >= 1e-3 ? [1e3, 'ms'] : a >= 1e-6 ? [1e6, 'µs'] : a === 0 ? [1e6, 'µs'] : [1e9, 'ns'];
  const stepU = (step / rate) * scale;
  const dec = Math.min(12, Math.max(0, Math.ceil(-Math.log10(stepU) - 1e-9)));
  return `${(t * scale).toFixed(dec)} ${unit}`;
}

export function axisTicks(v: View, rate: number | null): Tick[] {
  const step = tickStep(v.spp, rate);
  const first = Math.ceil(v.s0 / step) * step;
  const out: Tick[] = [];
  for (let k = 0; k < 200; k++) {
    // Multiply instead of accumulating to avoid drift at large sample offsets.
    const s = first + k * step;
    if (s >= v.s0 + v.spp * v.width) break;
    out.push({ x: xOf(v, s), label: tickLabel(s, step, rate), major: true });
  }
  // Labels wider than the tick spacing: label every n-th tick (lines stay on every tick).
  const longest = out.reduce((m, t) => Math.max(m, t.label.length), 0);
  const stepPx = step / v.spp;
  const every = Math.max(1, Math.ceil((longest * AXIS_CHAR_PX + 8) / Math.max(stepPx, 1e-9)));
  if (every > 1) {
    out.forEach((t, i) => {
      const idx = Math.round((first + i * step) / step);
      if (idx % every !== 0) {
        t.label = '';
        t.major = false;
      }
    });
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// painter

export interface Palette {
  bg: string;
  grid: string;
  fg: string;
  muted: string;
  hi: string;
  lo: string;
  dense: string;
  unknown: string;
  ann: string;
  annText: string;
  cursor: string;
  sel: string;
}

export const LANE_H = 28;
export const ANN_H = 22;
export const AXIS_H = 22;
export const GUTTER = 96;

export interface PaintInput {
  cap: CaptureData | null;
  lanes: { bit: number; name: string }[];
  decoders: string[];
  view: View;
  rate: number | null;
  palette: Palette;
  hoverX: number | null;
  selected: Annotation | null;
}

function hatch(ctx: CanvasRenderingContext2D, x0: number, y0: number, w: number, h: number, color: string): void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(x0, y0, w, h);
  ctx.clip();
  ctx.strokeStyle = color;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let x = Math.floor(x0 / 6) * 6 - h; x < x0 + w; x += 6) {
    ctx.moveTo(x, y0 + h);
    ctx.lineTo(x + h, y0);
  }
  ctx.stroke();
  ctx.restore();
}

/** paints onto a 2D context sized (GUTTER + view.width) × height in CSS px */
export function paint(ctx: CanvasRenderingContext2D, inp: PaintInput, heightPx: number): void {
  const { cap, lanes, decoders, view: v, palette: pal } = inp;
  const W = GUTTER + v.width;
  ctx.fillStyle = pal.bg;
  ctx.fillRect(0, 0, W, heightPx);
  ctx.font = '11px ui-monospace, SFMono-Regular, Menlo, monospace';
  ctx.textBaseline = 'middle';

  // axis
  const ticks = axisTicks(v, inp.rate);
  ctx.strokeStyle = pal.grid;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (const t of ticks) {
    const x = Math.round(GUTTER + t.x) + 0.5;
    ctx.moveTo(x, AXIS_H - 6);
    ctx.lineTo(x, heightPx);
  }
  ctx.stroke();
  ctx.fillStyle = pal.muted;
  for (const t of ticks) ctx.fillText(t.label, GUTTER + t.x + 3, AXIS_H / 2);
  ctx.fillStyle = pal.muted;
  ctx.fillText(inp.rate ? '시간(sample clock)' : '샘플', 6, AXIS_H / 2);

  // raw lanes
  lanes.forEach((ln, li) => {
    const y = AXIS_H + li * LANE_H;
    const yHi = y + 6;
    const yLo = y + LANE_H - 6;
    ctx.fillStyle = pal.fg;
    ctx.fillText(ln.name, 8, y + LANE_H / 2);
    ctx.strokeStyle = pal.grid;
    ctx.beginPath();
    ctx.moveTo(0, y + LANE_H - 0.5);
    ctx.lineTo(W, y + LANE_H - 0.5);
    ctx.stroke();
    if (!cap) return;
    const runs = laneRuns(cap, ln.bit, v);
    let prevLevel: RunKind | null = null;
    let prevX1 = -1;
    ctx.lineWidth = 1.5;
    for (const r of runs) {
      const x0 = GUTTER + r.x0;
      const x1 = GUTTER + r.x1;
      if (r.kind === 'unknown') {
        hatch(ctx, x0, yHi, Math.max(1, x1 - x0), yLo - yHi, pal.unknown);
        prevLevel = null;
        continue;
      }
      if (r.kind === 'dense') {
        ctx.fillStyle = pal.dense;
        ctx.fillRect(x0, yHi, Math.max(1, x1 - x0), yLo - yHi);
        prevLevel = 'dense';
        prevX1 = x1;
        continue;
      }
      const yy = r.kind === 1 ? yHi : yLo;
      ctx.strokeStyle = r.kind === 1 ? pal.hi : pal.lo;
      ctx.beginPath();
      // vertical edge only between two KNOWN runs that touch (never across unknown)
      if ((prevLevel === 0 || prevLevel === 1) && prevLevel !== r.kind && Math.abs(prevX1 - x0) < 0.01) {
        ctx.moveTo(x0, prevLevel === 1 ? yHi : yLo);
        ctx.lineTo(x0, yy);
      } else {
        ctx.moveTo(x0, yy);
      }
      ctx.lineTo(x1, yy);
      ctx.stroke();
      prevLevel = r.kind;
      prevX1 = x1;
    }
  });

  // decoded tracks (interpretation layer, visually distinct: boxes, dashed separators)
  const annY0 = AXIS_H + lanes.length * LANE_H;
  if (cap) {
    const { byDecoder: vis, omitted } = visibleAnnotations(cap, v);
    if (omitted > 0) {
      ctx.fillStyle = pal.unknown;
      ctx.fillText(`디코드 ${omitted.toLocaleString('ko-KR')}개 표시 생략 — 확대하세요`, GUTTER + 6, annY0 - 8);
    }
    decoders.forEach((dec, di) => {
      const y = annY0 + di * ANN_H;
      ctx.fillStyle = pal.ann;
      ctx.fillText(`⟦${dec}⟧`, 8, y + ANN_H / 2);
      ctx.setLineDash([3, 3]);
      ctx.strokeStyle = pal.grid;
      ctx.beginPath();
      ctx.moveTo(0, y + 0.5);
      ctx.lineTo(W, y + 0.5);
      ctx.stroke();
      ctx.setLineDash([]);
      for (const a of vis.get(dec) ?? []) {
        const x0 = Math.max(GUTTER, GUTTER + xOf(v, a.start));
        const x1 = Math.min(W, GUTTER + xOf(v, a.end));
        const w = Math.max(2, x1 - x0);
        const sel = inp.selected?.key === a.key;
        ctx.fillStyle = sel ? pal.sel : pal.ann;
        ctx.globalAlpha = sel ? 0.55 : 0.22;
        ctx.fillRect(x0, y + 3, w, ANN_H - 6);
        ctx.globalAlpha = 1;
        ctx.strokeStyle = sel ? pal.sel : pal.ann;
        ctx.lineWidth = 1;
        ctx.strokeRect(x0 + 0.5, y + 3.5, w - 1, ANN_H - 7);
        if (w > 14) {
          ctx.save();
          ctx.beginPath();
          ctx.rect(x0 + 2, y, w - 4, ANN_H);
          ctx.clip();
          ctx.fillStyle = pal.annText;
          ctx.fillText(a.text, x0 + 4, y + ANN_H / 2);
          ctx.restore();
        }
      }
    });
  }

  // gutter separator
  ctx.strokeStyle = pal.grid;
  ctx.beginPath();
  ctx.moveTo(GUTTER - 0.5, AXIS_H);
  ctx.lineTo(GUTTER - 0.5, heightPx);
  ctx.stroke();

  // hover cursor
  if (inp.hoverX !== null && inp.hoverX >= 0 && inp.hoverX <= v.width) {
    ctx.strokeStyle = pal.cursor;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(GUTTER + inp.hoverX + 0.5, AXIS_H);
    ctx.lineTo(GUTTER + inp.hoverX + 0.5, heightPx);
    ctx.stroke();
  }
}
