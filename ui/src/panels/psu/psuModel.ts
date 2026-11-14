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
  const m = mode.trim().toUpperCase();
  return m === 'CV' || m === 'CC' ? m : mode;
}

/** Protection readback (`ok|ovp|ocp|otp|unknown`, gateway PROT_VALUES). */
export function protLabel(prot: string | null): { text: string; warn: boolean } {
  if (prot === null || prot === 'unknown') return { text: '보호 ?', warn: false };
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
  push(tS: number, v: number | null, i: number | null, p: number | null = null): void {
    this.t.push(tS);
    this.v.push(v);
    this.i.push(i);
    this.p.push(p);
    this.prune(tS);
  }
  /** gap: a null sample so the plot does not join across the loss (all lanes) */
  gap(tS: number): void {
    this.push(tS, null, null, null);
  }
  private prune(nowS: number): void {
    let k = 0;
    while (k < this.t.length && (nowS - (this.t[k] as number) > WINDOW_S || this.t.length - k > MAX_POINTS)) k++;
    if (k > 0) {
      this.t.splice(0, k);
      this.v.splice(0, k);
      this.i.splice(0, k);
      this.p.splice(0, k);
    }
  }
  data(): [number[], (number | null)[], (number | null)[]] {
    return [this.t, this.v, this.i];
  }
  lane(l: Lane): [number[], (number | null)[]] {
    return [this.t, this[l]];
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
