// psu.gauge model (_xteam/explore-ui.md §psu.gauge). Pure readers + 60 s series ring.
// Provisional (ask EXPLORE): psu.state payload {set_v, set_i, out, prot, mode?}, measured
// values in the record's `metrics[{name: voltage|current|power, value, unit}]` (on
// psu.state or psu.meas), loss = source.gap{cause:"scpi_no_response"}.
//
// Fidelity (docs/impl/main/research/fidelity/psu.md): the view mirrors the BenchVue
// Instrument View split — measured readback ≠ setpoints — but nothing here invents a value.
// Unknown stays null (rendered "—"); CV/CC and protection come only from `psu.state`.

import type { LoomEvent } from '../../sdk';

export const WINDOW_S = 60;
const MAX_POINTS = 4_096; // C2: ≥ 60 s at 50 Hz; oldest dropped

export interface PsuState {
  setV: number | null;
  setI: number | null;
  /** last confirmed output state (null = never reported / reported unknown) */
  out: boolean | null;
  prot: string | null;
  mode: string | null;
  /** provenance as reported by the worker (never the daemon env badge) */
  backend?: string | null;
  address?: string | null;
  idn?: string | null;
  /** device link: false = disconnected → `out` is the last confirmed value, current output unknown */
  connected?: boolean | null;
  link?: string | null;
  /** PSU-01: active profile limits as reported in psu.state/psu.status (`vmax` V, `imax` A; finite > 0). null = not reported */
  vmax?: number | null;
  imax?: number | null;
  /** PSU-04: a power cycle is in its OFF phase (`psu.state.cycling`). null = not reported */
  cycling?: boolean | null;
}

/** PSU-01: result of checking one setpoint field against the reported limit (FUNCTIONS.md B1-M4). */
export type SetpointCheck =
  | { ok: true; value: number | null }
  | { ok: false; reason: 'nan' | 'negative' | 'over'; message: string };

/** PSU-02: per-sample provenance kept beside the measured values (FUNCTIONS.md B1-M9). */
export interface SampleRef {
  /** record `ts.wall_ns` in seconds (null when the record has no parsable wall time) */
  wallS?: number | null;
  /** setpoints last reported before this sample (never the user's draft) */
  setV?: number | null;
  setI?: number | null;
}

/** PSU-02: one row of the 60 s window for the cursor/latest readout (FUNCTIONS.md B1-M10). */
export interface ReadoutRow {
  tS: number;
  wallS: number | null;
  v: number | null;
  i: number | null;
  p: number | null;
  setV: number | null;
  setI: number | null;
  /** every measured lane is null at this row (a loss break) */
  gap: boolean;
}

/** PSU-02: canvas colours resolved from theme tokens (FUNCTIONS.md B1-M14). */
export interface ChartColors {
  axis: string;
  grid: string;
  tick: string;
  /** `${axis}|${grid}|${tick}` — plots are rebuilt when it changes */
  key: string;
}

export interface Meas {
  v: number | null;
  i: number | null;
  p: number | null;
  /** true when P was computed here as V×I (the device did not report power) */
  pDerived: boolean;
  /** client clock of the last measurement (age display) */
  atMs: number | null;
}

export const EMPTY_MEAS: Meas = { v: null, i: null, p: null, pDerived: false, atMs: null };

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);

export function reduceState(prev: PsuState, ev: LoomEvent): PsuState {
  if (ev.kind !== 'psu.state') return prev;
  const p = obj(ev.payload);
  // `out_known:false` means the backend has no confirmed output state: unknown, never "OFF"
  const out = p.out_known === false
    ? (prev.out ?? null)
    : p.out === true || p.out === 'on' ? true : p.out === false || p.out === 'off' ? false : prev.out;
  const posNum = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : null);
  const str = (v: unknown, keep: string | null | undefined) => (typeof v === 'string' && v ? v : (keep ?? null));
  return {
    setV: num(p.set_v) ?? prev.setV,
    setI: num(p.set_i) ?? prev.setI,
    out,
    prot: typeof p.prot === 'string' ? p.prot : prev.prot,
    mode: typeof p.mode === 'string' ? p.mode : prev.mode,
    backend: str(p.backend, prev.backend),
    address: str(p.address, prev.address),
    idn: str(p.idn, prev.idn),
    connected: typeof p.connected === 'boolean' ? p.connected : (prev.connected ?? null),
    link: str(p.link, prev.link),
    vmax: posNum(p.vmax) ?? prev.vmax ?? null,
    imax: posNum(p.imax) ?? prev.imax ?? null,
    cycling: typeof p.cycling === 'boolean' ? p.cycling : (prev.cycling ?? null),
  };
}

/** metrics[] → v/i/p (names: voltage|v, current|i, power|p). Missing → null. */
export function readMetrics(ev: LoomEvent): { v: number | null; i: number | null; p: number | null; pDerived: boolean } | null {
  if (!ev.metrics || ev.metrics.length === 0) return null;
  const get = (...names: string[]) => num(ev.metrics?.find((m) => names.includes(m.name))?.value);
  const v = get('voltage', 'v');
  const i = get('current', 'i');
  const reported = get('power', 'p');
  const p = reported ?? (v !== null && i !== null ? v * i : null);
  return v === null && i === null && p === null ? null : { v, i, p, pDerived: reported === null && p !== null };
}

/** Readback label for the reported operating mode. Only what the device said; never inferred. */
export function modeLabel(mode: string | null): string | null {
  if (mode === null || mode.trim() === '') return null;
  const m = mode.trim();
  const u = m.toUpperCase();
  if (u === 'OFF') return null;
  if (u === 'UNKNOWN') return '모드 미보고';
  return u === 'CV' || u === 'CC' ? u : m;
}

/** Protection readback (`ok|ovp|ocp|otp|unknown`, gateway PROT_VALUES). */
export function protLabel(prot: string | null): { text: string; warn: boolean } {
  if (prot === null || prot === 'unknown') return { text: '보호 미보고', warn: false };
  if (prot === 'ok') return { text: '보호 정상', warn: false };
  return { text: `보호 ${prot.toUpperCase()}`, warn: true };
}

/** Meter digits with engineering prefix (m below 1). null → "—" with the base unit. */
export function eng(x: number | null, unit: string): { num: string; unit: string } {
  if (x === null) return { num: '—', unit };
  const a = Math.abs(x);
  if (a !== 0 && a < 1) return { num: (x * 1000).toFixed(a < 0.01 ? 3 : 1), unit: `m${unit}` };
  return { num: x.toFixed(3), unit };
}

/** Parse a setpoint field: '' → absent, otherwise a finite ≥ 0 number (gateway rule) or invalid. */
export function parseSetpoint(raw: string): { ok: true; value: number | null } | { ok: false } {
  const s = raw.trim();
  if (s === '') return { ok: true, value: null };
  const n = Number(s);
  return Number.isFinite(n) && n >= 0 ? { ok: true, value: n } : { ok: false };
}

/** PSU-01 B1-M2: limit digits — 2 decimals when exact at 0.01, otherwise the full JS number text. */
export function limitNum(x: number): string {
  return Number.isInteger(Math.round(x * 100 * 1e6) / 1e6) ? x.toFixed(2) : String(x);
}

/** PSU-01 B1-M3: `9.00 V / 2.50 A`, `9.00 V / — A`, or `미보고` when neither limit is known. */
export function limitText(s: PsuState): string {
  const v = s.vmax ?? null;
  const i = s.imax ?? null;
  if (v === null && i === null) return '미보고';
  return `${v === null ? '—' : limitNum(v)} V / ${i === null ? '—' : limitNum(i)} A`;
}

/** PSU-01 B1-M4: parseSetpoint + strict `value > limit` check (the gateway's `Limits::violation` rule). */
export function checkSetpoint(raw: string, limit: number | null, unit: 'V' | 'A'): SetpointCheck {
  const r = parseSetpoint(raw);
  if (!r.ok) {
    const n = Number(raw.trim());
    return { ok: false, reason: Number.isFinite(n) ? 'negative' : 'nan', message: '0 이상의 숫자를 입력하세요' };
  }
  if (r.value !== null && limit !== null && r.value > limit) {
    return { ok: false, reason: 'over', message: `${raw.trim()} ${unit} — 한도 ${limitNum(limit)} ${unit} 초과` };
  }
  return r;
}

/** PSU-02 B1-M8: record `ts.wall_ns` → seconds (ms resolution), null when not a decimal string. */
export function wallS(ev: LoomEvent): number | null {
  return /^\d+$/.test(ev.ts.wall_ns) ? Number(BigInt(ev.ts.wall_ns) / 1_000_000n) / 1000 : null;
}

/** PSU-02 B1-M7: local wall clock `HH:MM:SS` (or `HH:MM:SS.mmm` with `ms`), 24 h, zero padded. */
export function clockLabel(wallSec: number, ms = false): string {
  const msInt = Math.floor(Math.round(wallSec * 1e6) / 1e3);
  const d = new Date(msInt);
  const two = (n: number) => String(n).padStart(2, '0');
  const base = `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
  return ms ? `${base}.${String(d.getMilliseconds()).padStart(3, '0')}` : base;
}

/** PSU-02 B1-M10: the row at `idx` of the window, null when idx is not an in-range integer. */
export function readoutAt(s: Series, idx: number): ReadoutRow | null {
  if (!(Number.isInteger(idx) && idx >= 0 && idx < s.t.length)) return null;
  const v = s.v[idx] ?? null;
  const i = s.i[idx] ?? null;
  const p = s.p[idx] ?? null;
  return {
    tS: s.t[idx] as number,
    wallS: s.w[idx] ?? null,
    v,
    i,
    p,
    setV: s.sv[idx] ?? null,
    setI: s.si[idx] ?? null,
    gap: v === null && i === null && p === null,
  };
}

/** PSU-02 B1-M11: `커서 20:38:17.200 · 13.490 V · 352.0 mA · 4.744 W · 설정 13.50 V / 2.00 A`. */
export function readoutText(r: ReadoutRow | null, prefix: '커서' | '최신'): string {
  if (r === null) return prefix === '최신' ? '측정 없음' : '';
  const clock = r.wallS !== null ? clockLabel(r.wallS, true) : `t ${r.tS.toFixed(3)} s`;
  if (r.gap) return `${prefix} ${clock} · 측정 손실 (선 끊김)`;
  const q = (x: number | null, u: string) => {
    const f = eng(x, u);
    return `${f.num} ${f.unit}`;
  };
  const parts = [`${prefix} ${clock}`, q(r.v, 'V'), q(r.i, 'A'), q(r.p, 'W')];
  if (r.setV !== null || r.setI !== null) parts.push(`설정 ${fmt(r.setV, 'V', 2)} / ${fmt(r.setI, 'A', 2)}`);
  return parts.join(' · ');
}

/** PSU-02 B1-M12: x tick labels (x = daemon mono seconds) as wall clock from each point's own wall time. */
export function xTickLabels(s: Series, splits: readonly number[]): string[] {
  const n = s.t.length;
  if (n === 0) return splits.map((x) => x.toFixed(0));
  return splits.map((x) => {
    // last index with t <= x (binary search); none → 0
    let lo = 0;
    let hi = n - 1;
    let k = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if ((s.t[mid] as number) <= x) {
        k = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    if (k < 0) k = 0;
    let j = -1;
    for (let a = k; a >= 0; a--) {
      if (s.w[a] !== null && s.w[a] !== undefined) {
        j = a;
        break;
      }
    }
    if (j < 0) {
      for (let a = k + 1; a < n; a++) {
        if (s.w[a] !== null && s.w[a] !== undefined) {
          j = a;
          break;
        }
      }
    }
    if (j >= 0) return clockLabel((s.w[j] as number) + (x - (s.t[j] as number)));
    return `${(x - (s.t[n - 1] as number)).toFixed(0)} s`;
  });
}

/** PSU-02 B1-M13: y scale range; never uPlot's 0–100 default for flat/zero data. */
export function yRange(min: number | null, max: number | null, limit: number | null): [number, number] {
  const L = limit !== null && Number.isFinite(limit) && limit > 0 ? limit : 1;
  if (min === null || max === null || !Number.isFinite(min) || !Number.isFinite(max)) return [0, L];
  let lo: number;
  let hi: number;
  if (max - min <= 1e-12) {
    if (Math.abs(max) <= 1e-12) return [0, L];
    const pad = Math.max(Math.abs(max) * 0.02, 1e-3);
    lo = max - pad;
    hi = max + pad;
  } else {
    const pad = (max - min) * 0.1;
    lo = min - pad;
    hi = max + pad;
  }
  if (min >= 0 && lo < 0) lo = 0;
  return [lo, hi];
}

/** PSU-02 B1-M14: axis/grid/tick colours from theme tokens (`--muted`, `--line`), dark fallbacks. */
export function chartColors(read: (token: string) => string): ChartColors {
  const v = (t: string, fb: string) => read(t).trim() || fb;
  const axis = v('--muted', '#a9b8c9');
  const grid = v('--line', '#293849');
  const tick = v('--line', '#405267');
  return { axis, grid, tick, key: `${axis}|${grid}|${tick}` };
}

/** `source.gap.payload.cause` → footer label. Known causes are translated, others shown raw, missing → 원인 미상. */
export function gapCauseLabel(payload: unknown): string {
  const c = typeof payload === 'object' && payload !== null ? (payload as Record<string, unknown>).cause : undefined;
  if (typeof c !== 'string' || c.trim() === '') return '원인 미상';
  return c === 'scpi_no_response' ? 'SCPI 무응답' : c;
}

/** "SCPI 무응답 1, 원인 미상 2" (insertion order). */
export function gapCauseSummary(causes: ReadonlyMap<string, number>): string {
  return [...causes].map(([k, n]) => `${k} ${n}`).join(', ');
}

export type Lane = 'v' | 'i' | 'p';

/** time series for uPlot: x = seconds (daemon mono), null rows break the line at gaps. */
export class Series {
  t: number[] = [];
  v: (number | null)[] = [];
  i: (number | null)[] = [];
  p: (number | null)[] = [];
  /** PSU-02: per-point wall seconds / reported setpoints, index-aligned with `t` (B1-M9) */
  w: (number | null)[] = [];
  sv: (number | null)[] = [];
  si: (number | null)[] = [];
  // `at` fills w/sv/si (missing → null); prune keeps all seven arrays aligned.
  push(tS: number, v: number | null, i: number | null, p: number | null = null, at: SampleRef = {}): void {
    this.t.push(tS);
    this.v.push(v);
    this.i.push(i);
    this.p.push(p);
    this.w.push(at.wallS ?? null);
    this.sv.push(at.setV ?? null);
    this.si.push(at.setI ?? null);
    this.prune(tS);
  }
  /** gap: a null sample so the plot does not join across the loss (all lanes) */
  // a gap row carries its own wall time; setpoint refs are null at a gap.
  gap(tS: number, wallSec: number | null = null): void {
    this.push(tS, null, null, null, { wallS: wallSec });
  }
  private prune(nowS: number): void {
    let k = 0;
    while (k < this.t.length && (nowS - (this.t[k] as number) > WINDOW_S || this.t.length - k > MAX_POINTS)) k++;
    if (k > 0) {
      this.t.splice(0, k);
      this.v.splice(0, k);
      this.i.splice(0, k);
      this.p.splice(0, k);
      this.w.splice(0, k);
      this.sv.splice(0, k);
      this.si.splice(0, k);
    }
  }
  data(): [number[], (number | null)[], (number | null)[]] {
    return [this.t, this.v, this.i];
  }
  lane(l: Lane): [number[], (number | null)[]] {
    return [this.t, this[l]];
  }
  /** B1-M9: uPlot data per lane — v: [t, v, sv], i: [t, i, si], p: [t, p] (same array objects, no copy) */
  laneData(l: Lane): [number[], ...(number | null)[][]] {
    if (l === 'v') return [this.t, this.v, this.sv];
    if (l === 'i') return [this.t, this.i, this.si];
    return [this.t, this.p];
  }
}

export function monoS(ev: LoomEvent): number {
  return /^\d+$/.test(ev.ts.mono_ns) ? Number(BigInt(ev.ts.mono_ns) / 1_000_000n) / 1000 : 0;
}

/** Header provenance: actual backend/address/IDN from psu.state (the env badge is the daemon environment). */
export function sourceLabel(s: PsuState): string {
  const parts = [s.backend ?? 'backend 미보고', s.address ?? null, s.idn ? `IDN ${s.idn}` : 'IDN 미보고'].filter(Boolean);
  return parts.join(' · ');
}

/** Output word: a disconnected supply shows its last confirmed state with an explicit qualifier. */
export function outText(s: PsuState): { text: string; cls: string } {
  if (s.out === null) return { text: s.connected === false ? '출력 ? (연결 끊김 · 확인값 없음)' : '출력 ?', cls: 'unknown' };
  const base = s.out ? '● 출력 ON' : '○ 출력 OFF';
  if (s.connected === false) return { text: `${base} (마지막 확인값 · 연결 끊김 — 현재 상태 미확인)`, cls: `${s.out ? 'on' : 'off'} stale` };
  return { text: base, cls: s.out ? 'on' : 'off' };
}

export const fmt = (x: number | null, unit: string, d = 3) => (x === null ? `— ${unit}` : `${x.toFixed(d)} ${unit}`);
