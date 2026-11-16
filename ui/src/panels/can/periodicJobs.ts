// CAN periodic send (CAN-2): daemon job table + request helpers. Pure (no DOM, no RPC, no timers).
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B3-JOBS.
//
// · Jobs are SERVER jobs (`can.periodic.*`); the browser never repeats a send itself.
// · The table is filled by one `can.periodic.list{channel}` read (mount / "새로고침") and live
//   `can.job_state` control events. Jobs of every owner are listed; only jobs this panel started or took
//   over are controlled directly — others need an explicit takeover (confirm card).
// · Unknown stats stay unknown (null → "?"), never 0. Lease time is the value at list time (not ticking).
// · Owner/origin/state/reason strings come from the daemon → the view renders them with textContent.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-3 work.

import type { LoomEvent } from '../../sdk';
import type { CanSendParams } from './sendLine';
import { CYCLE_MAX_MS, cansendText } from './txStore';

/** = loom_can_periodic::MAX_JOBS (list entries beyond are ignored) */
export const JOBS_MAX = 128;
/** stopped jobs kept for display (newest first) */
export const STOPPED_KEEP = 16;

export interface JobRow {
  job: string;
  id: number;
  ext: boolean;
  owner: string;
  origin: string;
  /** null = not known yet (event-only row before a list) */
  periodMs: number | null;
  /** 'running' | 'paused' | 'stopped' (other daemon states kept verbatim) */
  state: string;
  revision: number | null;
  sent: number | null;
  missed: number | null;
  /** lease time left at the last list (ms); null = no lease / unknown */
  leaseLeftMs: number | null;
  /** daemon reason of the last state change (e.g. "lease_expired", "channel_closed", "takeover") */
  reason: string | null;
}

export class JobTable {
  /** bumps on every visible change */
  version = 0;
  /** a list was applied at least once (else the view says "not read yet") */
  loaded = false;
  private readonly rows = new Map<string, JobRow>();
  /** stopped job ids, newest first (≤ STOPPED_KEEP) */
  private stopped: string[] = [];

  /** `can.periodic.list` result: replaces running/paused rows of `channel`, keeps stopped rows; false (unchanged) on a bad shape */
  applyList(v: unknown, channel: string): boolean {
    const count = (x: unknown): x is number => typeof x === 'number' && Number.isInteger(x) && x >= 0;
    if (typeof v !== 'object' || v === null) return false;
    const arr = (v as { jobs?: unknown }).jobs;
    if (!Array.isArray(arr)) return false;
    const listed: JobRow[] = [];
    for (const e of arr as unknown[]) {
      if (listed.length >= JOBS_MAX) break;
      if (typeof e !== 'object' || e === null) continue;
      const o = e as Record<string, unknown>;
      if (
        typeof o.job !== 'string' ||
        typeof o.channel !== 'string' ||
        !count(o.id) ||
        typeof o.ext !== 'boolean' ||
        typeof o.owner !== 'string' ||
        typeof o.origin !== 'string' ||
        !count(o.period_ms) ||
        typeof o.state !== 'string'
      ) {
        continue;
      }
      if (o.channel !== channel) continue;
      listed.push({
        job: o.job,
        id: o.id,
        ext: o.ext,
        owner: o.owner,
        origin: o.origin,
        periodMs: o.period_ms,
        state: o.state,
        revision: count(o.revision) ? o.revision : null,
        sent: count(o.sent) ? o.sent : null,
        missed: count(o.missed) ? o.missed : null,
        leaseLeftMs: count(o.lease_left_ms) ? o.lease_left_ms : null,
        reason: null,
      });
    }
    for (const job of [...this.rows.keys()]) {
      if (!this.stopped.includes(job)) this.rows.delete(job);
    }
    for (const r of listed) {
      this.stopped = this.stopped.filter((j) => j !== r.job);
      this.rows.set(r.job, r);
    }
    this.loaded = true;
    this.version++;
    return true;
  }

  /** `can.job_state` event of `channel` → row update; true when something changed */
  applyEvent(ev: Pick<LoomEvent, 'kind' | 'channel' | 'payload'>, channel: string): boolean {
    const count = (x: unknown): x is number => typeof x === 'number' && Number.isInteger(x) && x >= 0;
    if (ev.kind !== 'can.job_state' || ev.channel !== channel) return false;
    const p = ev.payload;
    if (typeof p !== 'object' || p === null) return false;
    const o = p as Record<string, unknown>;
    if (typeof o.job !== 'string' || typeof o.state !== 'string') return false;
    const job = o.job;
    const prev = this.rows.get(job);
    const row: JobRow = prev
      ? { ...prev }
      : {
          job,
          id: count(o.id) ? o.id : 0,
          ext: o.ext === true,
          owner: '?',
          origin: '?',
          periodMs: null,
          state: 'running',
          revision: null,
          sent: null,
          missed: null,
          leaseLeftMs: null,
          reason: null,
        };
    if (typeof o.owner === 'string') row.owner = o.owner;
    if (typeof o.origin === 'string') row.origin = o.origin;
    if (count(o.revision)) row.revision = o.revision;
    row.reason = typeof o.reason === 'string' ? o.reason : null;
    const st = o.state;
    row.state = st === 'started' || st === 'patched' || st === 'renewed' || st === 'running' ? 'running' : st;
    if (row.state === 'stopped') {
      this.stopped = [job, ...this.stopped.filter((j) => j !== job)];
      while (this.stopped.length > STOPPED_KEEP) {
        const old = this.stopped.pop();
        if (old !== undefined) this.rows.delete(old);
      }
    } else {
      this.stopped = this.stopped.filter((j) => j !== job);
    }
    this.rows.set(job, row);
    this.version++;
    return true;
  }

  get(job: string): JobRow | undefined {
    return this.rows.get(job);
  }

  /** running/paused by job number ascending, then stopped newest first */
  list(): JobRow[] {
    const active = [...this.rows.values()].filter((r) => r.state !== 'stopped').sort((a, b) => jobNumber(a.job) - jobNumber(b.job));
    const stopped: JobRow[] = [];
    for (const j of this.stopped) {
      const r = this.rows.get(j);
      if (r) stopped.push(r);
    }
    return [...active, ...stopped];
  }
}

/** "p12" → 12; anything else → Number.MAX_SAFE_INTEGER */
export function jobNumber(job: string): number {
  const m = /^p(\d+)$/.exec(job);
  return m ? Number(m[1]) : Number.MAX_SAFE_INTEGER;
}

/** state cell text */
export function jobStateText(r: Pick<JobRow, 'state' | 'reason'>): string {
  const base = r.state === 'running' ? '실행 중' : r.state === 'paused' ? '일시정지' : r.state === 'stopped' ? '정지됨' : r.state;
  if (r.state === 'running') return base;
  return r.reason !== null ? `${base} (${r.reason})` : base;
}

/** editor "Cycle Time ms" text → ms (0 = manual) or an error */
export function parseCycle(text: string): { ok: true; ms: number } | { ok: false; error: string } {
  const t = text.trim();
  if (t === '') return { ok: true, ms: 0 };
  if (/^\d+$/.test(t) && Number(t) <= CYCLE_MAX_MS) return { ok: true, ms: Number(t) };
  return { ok: false, error: 'Cycle Time: 0(수동) 또는 1–60000 ms 정수' };
}

/** TX entry → `can.periodic.start` params (frame as cansend text) or an error */
export function startParams(
  e: { params: Pick<CanSendParams, 'id' | 'ext' | 'fd' | 'brs' | 'data'>; cycleMs: number },
  channel: string,
): { ok: true; params: { channel: string; frame: string; period_ms: number } } | { ok: false; error: string } {
  if (!(Number.isInteger(e.cycleMs) && e.cycleMs >= 1 && e.cycleMs <= CYCLE_MAX_MS)) {
    return { ok: false, error: '주기 0 = 수동 송신 — Cycle Time(ms)을 1–60000으로 정하세요' };
  }
  return { ok: true, params: { channel, frame: cansendText(e.params), period_ms: e.cycleMs } };
}
