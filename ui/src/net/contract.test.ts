// src/net against REAL frames of CORE's `loomd --fixture-replay` (fixtures/wire/replay-*.jsonl,
// recorded by tools/mock/capture-replay.ts). Every server frame must parse; snapshot parts are
// 1-based; the control path carries exactly the priority kinds; echo-write rows are complete.
// Known contract differences are pinned here so a CORE change shows up as a test diff
// (report p1-m1-accept §4).

import { describe, expect, it } from 'vitest';
import lateJoinRaw from '../../fixtures/wire/replay-late-join.jsonl?raw';
import writesRaw from '../../fixtures/wire/replay-writes.jsonl?raw';
import type { LoomEvent } from '../proto/provisional';
import { isControlEvent } from '../proto/provisional';
import { parseInbound, readHello, readSubscribeResult } from './wire';

interface Line {
  t_ms: number;
  dir: 'c2s' | 's2c';
  msg: Record<string, unknown>;
}

const RAW: Record<string, string> = { 'replay-late-join.jsonl': lateJoinRaw, 'replay-writes.jsonl': writesRaw };

function load(name: string): Line[] {
  return (RAW[name] ?? '')
    .split('\n')
    .filter(Boolean)
    .slice(1)
    .map((l) => JSON.parse(l) as Line);
}

const replies = (lines: Line[]) => {
  const byId = new Map<unknown, string>();
  for (const l of lines) if (l.dir === 'c2s') byId.set(l.msg.id, String(l.msg.method));
  return (method: string) => lines.find((l) => l.dir === 's2c' && byId.get(l.msg.id) === method)?.msg as { result?: unknown; error?: { data?: { code?: string } } } | undefined;
};

describe('net vs loomd --fixture-replay frames', () => {
  for (const f of ['replay-late-join.jsonl', 'replay-writes.jsonl']) {
    it(`${f}: every server frame parses (no invalid)`, () => {
      const kinds = load(f)
        .filter((l) => l.dir === 's2c')
        .map((l) => parseInbound(JSON.stringify(l.msg)));
      expect(kinds.filter((k) => k.t === 'invalid')).toEqual([]);
    });
  }

  it('hello / subscribe results read; snapshot parts are 1-based and n === 1 starts the snapshot', () => {
    const lines = load('replay-late-join.jsonl');
    const reply = replies(lines);
    expect(readHello(reply('core.hello')!.result)).not.toBeNull();
    expect(readSubscribeResult(reply('core.subscribe')!.result)).not.toBeNull();
    const parts = lines.filter((l) => l.msg.method === 'core.snapshot_part').map((l) => parseInbound(JSON.stringify(l.msg)));
    expect(parts.length).toBeGreaterThan(0);
    expect(parts.map((p) => (p.t === 'part' ? p.part.n : -1))).toEqual(parts.map((_, i) => i + 1));
    expect(parts.map((p) => p.t === 'part' && p.first)).toEqual(parts.map((_, i) => i === 0));
    // the core reducer state rides on part 1 as items[{domain:'core', kind:'core.snapshot'}]
    const first = lines.find((l) => l.msg.method === 'core.snapshot_part')!.msg.params as { items: { kind: string }[] };
    expect(first.items.map((i) => i.kind)).toContain('core.snapshot');
  });

  it('control path (sub "control") carries only priority kinds; data subs carry none (replay routes priority ONLY to control)', () => {
    const lines = load('replay-writes.jsonl');
    const batches = lines.filter((l) => l.msg.method === 'core.events').map((l) => l.msg.params as { sub: string; events: LoomEvent[] });
    const control = batches.filter((b) => b.sub === 'control').flatMap((b) => b.events);
    const data = batches.filter((b) => b.sub !== 'control').flatMap((b) => b.events);
    expect(control.length).toBeGreaterThan(0);
    expect(control.every(isControlEvent)).toBe(true);
    expect(data.filter(isControlEvent)).toEqual([]); // C-3: unlike tools/mock, write rows never ride the data sub
  });

  it('echo writes: requested→accepted→sent→confirmed rows with request_id, before the reply', () => {
    const lines = load('replay-writes.jsonl');
    const reply = replies(lines);
    for (const m of ['uart.send', 'can.send', 'uds.request']) {
      const rid = (reply(m)!.result as { request_id: string }).request_id;
      const idx = lines.findIndex((l) => l.dir === 's2c' && (l.msg.result as { request_id?: string } | undefined)?.request_id === rid);
      const rows = lines
        .slice(0, idx)
        .filter((l) => l.msg.method === 'core.events')
        .flatMap((l) => (l.msg.params as { events: LoomEvent[] }).events)
        .filter((e) => e.request_id === rid);
      expect(rows.map((e) => (e.payload as { write_state: string }).write_state)).toEqual(['requested', 'accepted', 'sent', 'confirmed']);
      expect(rows.every((e) => e.origin === 'user')).toBe(true);
    }
    // the provisional name is not served: CAN v1 / CORE use can.send (report §4)
    expect(reply('can.tx.send')!.error?.data?.code).toBe('E_NOT_IMPLEMENTED');
  });

  it('pinned differences: safe_state.list is a bare array; run result {items[{id, outcome}]}', () => {
    const reply = replies(load('replay-writes.jsonl'));
    const list = reply('core.safe_state.list')!.result;
    expect(Array.isArray(list)).toBe(true); // shell adapter reads {items}: report §4 C-8
    expect((list as { description: string; blocked: boolean }[])[0]).toMatchObject({ description: expect.any(String), blocked: false });
    expect(reply('core.safe_state.run')!.result).toMatchObject({ items: [{ id: 'can.periodic_stop', outcome: 'sent' }] });
  });
});
