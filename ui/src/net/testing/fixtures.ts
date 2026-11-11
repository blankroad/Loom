// Test helpers for src/net (not shipped: only *.test.ts import this).

import type { EventsBatch, LoomEvent, SnapshotPart } from '../../proto/provisional';
import type { Inbound } from '../wire';

export function ev(seq: number | bigint, kind = 'can.frame', extra: Partial<LoomEvent> = {}): LoomEvent {
  return {
    schema_version: 1,
    session_id: 's1',
    domain: kind.split('.')[0]!,
    channel: 'ch0',
    kind,
    epoch: '1',
    seq: String(seq),
    producer_id: 'can-sim',
    producer_epoch: '1',
    source_seq: String(seq),
    ts: { mono_ns: String(BigInt(seq) * 1000n), wall_ns: '0', clock_id: 'c1', source: 'hardware' },
    dir: 'rx',
    origin: 'device',
    payload: {},
    ...extra,
  };
}

export function batch(sub: string, events: LoomEvent[]): EventsBatch {
  return { sub, first: events[0]?.seq ?? '0', last: events[events.length - 1]?.seq ?? '0', lag_ms: 1, events };
}

export function range(a: number, b: number, kind = 'can.frame'): LoomEvent[] {
  const out: LoomEvent[] = [];
  for (let i = a; i <= b; i++) out.push(ev(i, kind));
  return out;
}

export const evNote = (subId: string, events: LoomEvent[], bytes = 100): Extract<Inbound, { t: 'events' }> => ({
  t: 'events',
  subId,
  batch: batch(subId, events),
  bytes,
});

export function snapPart(sub: string, events: LoomEvent[], n = 1, total = 1): SnapshotPart {
  return { sub, n, total, items: [], events };
}

export const partNote = (subId: string, events: LoomEvent[], n = 1, N = 1): Extract<Inbound, { t: 'part' }> => ({
  t: 'part',
  subId,
  part: snapPart(subId, events, n, N),
  first: n === 1, // 1-based (schema/v1)
  bytes: 100,
});

export const endNote = (subId: string, h: number): Extract<Inbound, { t: 'end' }> => ({
  t: 'end',
  subId,
  h: BigInt(h),
  hRaw: String(h),
  bytes: 50,
});

export const tick = () => new Promise<void>((r) => setTimeout(r, 0));

/** Manual timers: run with advance(ms). */
export class ManualTimers {
  now = 0;
  private seq = 0;
  private readonly jobs = new Map<number, { at: number; cb: () => void }>();
  setTimeout = (cb: () => void, ms: number): unknown => {
    const id = ++this.seq;
    this.jobs.set(id, { at: this.now + ms, cb });
    return id;
  };
  clearTimeout = (h: unknown): void => {
    this.jobs.delete(h as number);
  };
  advance(ms: number): void {
    const end = this.now + ms;
    for (;;) {
      let next: [number, { at: number; cb: () => void }] | null = null;
      for (const j of this.jobs) if (j[1].at <= end && (!next || j[1].at < next[1].at)) next = j;
      if (!next) break;
      this.jobs.delete(next[0]);
      this.now = next[1].at;
      next[1].cb();
    }
    this.now = end;
  }
  get pending(): number {
    return this.jobs.size;
  }
}
