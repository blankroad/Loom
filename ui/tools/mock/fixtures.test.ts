// Shape + ordering gate over fixtures/ (wire transcripts and scenario scripts).
// When CORE's schema lands, the same files get Ajv-validated here.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import type { LoomEvent, WriteState } from '../../src/proto/provisional';
import { CONTROL_SUB, WRITE_STATE_RANK } from '../../src/proto/provisional';
import { listScenarios, loadScenario } from './scenario';

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'fixtures');
const WIRE = path.join(ROOT, 'wire');
const U64 = /^(0|[1-9]\d{0,19})$/;

interface Line {
  t_ms: number;
  dir: 'c2s' | 's2c';
  msg: { id?: number; method?: string; params?: Record<string, unknown>; result?: Record<string, unknown> };
}

function load(name: string): { header: Record<string, unknown>; lines: Line[] } {
  const [h, ...rest] = fs.readFileSync(path.join(WIRE, name), 'utf8').trim().split('\n').map((l) => JSON.parse(l) as unknown);
  return { header: h as Record<string, unknown>, lines: rest as Line[] };
}

const files = fs.readdirSync(WIRE).filter((f) => f.endsWith('.jsonl')).sort();

function checkEvent(e: LoomEvent) {
  for (const k of ['schema_version', 'session_id', 'domain', 'kind', 'epoch', 'seq', 'producer_id', 'producer_epoch', 'source_seq', 'ts', 'origin', 'payload'] as const) {
    expect(e, `${e.kind} lacks ${k}`).toHaveProperty(k);
  }
  for (const k of ['epoch', 'seq', 'producer_epoch', 'source_seq'] as const) expect(e[k]).toMatch(U64);
  expect(e.ts.mono_ns).toMatch(U64);
  expect(e.kind.split('.')).toHaveLength(2);
  expect(['agent', 'user', 'device', 'script']).toContain(e.origin);
  for (const [k, v] of Object.entries(e)) expect(v, `${e.kind}.${k} is null (omit instead)`).not.toBeNull();
  if (e.kind.endsWith('.gap')) {
    const p = e.payload as { count: unknown; seq_range?: unknown; producer_range?: unknown };
    expect(typeof p.count).toBe('string');
    expect(p.seq_range ?? p.producer_range).toBeDefined();
  }
  if (e.request_id && 'write_state' in e.payload) expect(e.env).toBeDefined(); // env required on write events
}

describe('fixtures/wire', () => {
  it('has the required cases', () => {
    expect(files).toEqual(expect.arrayContaining(['uart-normal.jsonl', 'can-bcm.jsonl', 'uds-pending.jsonl', 'gap.jsonl', 'outcome-unknown.jsonl', 'late-join.jsonl', 'stop-latch.jsonl']));
  });

  for (const f of files) {
    it(`${f}: envelope shape, snapshot order, monotonic write stages, publish-before-reply`, () => {
      const { header, lines } = load(f);
      expect(header.fixture).toBe(f.replace('.jsonl', ''));
      const events: LoomEvent[] = [];
      const subState = new Map<string, { parts: number; total: number | null; h: bigint | null }>();
      const lastRank = new Map<string, number>();
      const requestFrame = new Map<string, number>(); // request_id -> last frame index carrying it
      lines.forEach((l, i) => {
        const m = l.msg;
        if (m.method === 'core.subscribe' && l.dir === 'c2s') return;
        if (l.dir === 's2c' && m.result && typeof m.result.sub_id === 'string') subState.set(m.result.sub_id, { parts: 0, total: null, h: null });
        if (m.method === 'core.snapshot_part') {
          const s = subState.get(m.params!.sub as string);
          expect(s, 'snapshot_part before subscribe reply').toBeDefined();
          expect(m.params!.n).toBe(++s!.parts); // 1-based (schema/v1)
          s!.total ??= m.params!.total as number;
          expect(m.params!.total).toBe(s!.total);
          ((m.params!.events as LoomEvent[] | undefined) ?? []).forEach(checkEvent);
        }
        if (m.method === 'core.snapshot_end') {
          const s = subState.get(m.params!.sub as string)!;
          expect(s.parts).toBe(s.total ?? 0);
          s.h = BigInt(m.params!.stream_seq as string);
        }
        if (m.method === 'core.events') {
          const p = m.params as { sub: string; first: string; last: string; events: LoomEvent[] };
          expect(p.first).toBe(p.events[0]!.seq);
          expect(p.last).toBe(p.events.at(-1)!.seq);
          expect(JSON.stringify(m).length).toBeLessThanOrEqual(64 * 1024);
          for (const e of p.events) {
            checkEvent(e);
            if (p.sub !== CONTROL_SUB) {
              const s = subState.get(p.sub);
              expect(s, 'events before subscribe reply').toBeDefined();
              if (s!.h !== null) expect(BigInt(e.seq) > s!.h).toBe(true); // live strictly after H
              events.push(e);
            }
            const ws = (e.payload as { write_state?: WriteState }).write_state;
            if (e.request_id && ws && p.sub !== CONTROL_SUB) {
              const r = WRITE_STATE_RANK[ws];
              expect(r).toBeGreaterThanOrEqual(lastRank.get(e.request_id) ?? 0);
              lastRank.set(e.request_id, r);
            }
            if (e.request_id) requestFrame.set(e.request_id, i);
          }
        }
        // publish-before-reply: a reply naming a request_id comes after all its events
        const rid = m.result?.request_id;
        if (l.dir === 's2c' && typeof rid === 'string') expect(requestFrame.get(rid) ?? -1).toBeLessThan(i);
      });
      // a data sub sees each event once, in seq order
      const bySub = events.map((e) => BigInt(e.seq));
      expect(new Set(bySub).size).toBe(bySub.length);
    });
  }

  it('late-join: 1,000 backlog events across parts, then live > H', () => {
    const { lines } = load('late-join.jsonl');
    const parts = lines.filter((l) => l.msg.method === 'core.snapshot_part');
    expect(parts.flatMap((l) => (l.msg.params!.events as unknown[]) ?? [])).toHaveLength(1000);
    expect(lines.some((l) => l.msg.method === 'core.events')).toBe(true);
  });

  it('outcome-unknown: the write ends in outcome_unknown', () => {
    const { lines } = load('outcome-unknown.jsonl');
    const states = lines.flatMap((l) => (l.msg.method === 'core.events' && l.msg.params!.sub !== CONTROL_SUB ? (l.msg.params!.events as LoomEvent[]) : [])).filter((e) => e.kind === 'can.tx').map((e) => (e.payload as { write_state: string }).write_state);
    expect(states.at(-1)).toBe('outcome_unknown');
  });
});

describe('fixtures/scenarios', () => {
  it('every scenario parses and names a known action', () => {
    const names = listScenarios();
    expect(names).toEqual(['can-bcm.jsonl', 'uart-bootshell.jsonl', 'uds-read-did.jsonl']);
    for (const n of names) {
      const { lines } = loadScenario(n);
      expect(lines.length).toBeGreaterThan(0);
      for (const l of lines) if (l.agent) expect(['uart.send', 'can.send', 'uds.request']).toContain(l.agent.method);
    }
  });
});
