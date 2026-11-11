import { describe, expect, it, vi } from 'vitest';
import type { GapData } from '../proto/provisional';
import { DeliveryQueue } from './budget';
import { batch, ev, range, snapPart } from './testing/fixtures';
import type { LocalCtx } from './wire';

const ctx: LocalCtx = { sessionId: 's1', epoch: '1', clientId: 'ui-test' };
const filter = { domains: ['can'], channels: ['ch0'] };
const small = { dataBytes: 1000, dataBatches: 3, totalBytes: 10_000, totalItems: 50 };

describe('DeliveryQueue', () => {
  it('merges consecutive data batches of one subscription (≤ 1 per pull)', () => {
    const q = new DeliveryQueue(() => undefined);
    q.pushLive(1, filter, batch('s', range(1, 3)), 100, ctx);
    q.pushLive(2, filter, batch('t', range(4, 4)), 100, ctx);
    q.pushLive(1, filter, batch('s', range(5, 6)), 100, ctx);
    const { items } = q.drain();
    expect(items).toHaveLength(2);
    const b1 = items[0]!;
    expect(b1.k === 'batch' && b1.batch.events.map((e) => e.seq)).toEqual(['1', '2', '3', '5', '6']);
    expect(b1.k === 'batch' && [b1.batch.first, b1.batch.last]).toEqual(['1', '6']);
    expect(q.size).toBe(0);
  });

  it('does not merge across a snapshot/resync marker', () => {
    const q = new DeliveryQueue(() => undefined);
    q.pushLive(1, filter, batch('s', range(1, 1)), 10, ctx);
    q.pushExempt({ k: 'resync', eid: 1, reason: 'x' }, 0);
    q.pushLive(1, filter, batch('s', range(2, 2)), 10, ctx);
    expect(q.drain().items.map((i) => i.k)).toEqual(['batch', 'resync', 'batch']);
  });

  it('over budget: drops data, keeps control rows, emits ONE widening local display.gap', () => {
    const q = new DeliveryQueue(() => undefined, small);
    q.pushLive(1, filter, batch('s', range(1, 2)), 450, ctx);
    q.pushLive(1, filter, batch('s', range(3, 4)), 450, ctx); // 900 bytes, 2 batches
    // next one exceeds 1000 bytes -> dropped
    const final = ev(6, 'can.tx', { request_id: 'r1', payload: { write_state: 'confirmed' } });
    q.pushLive(1, filter, batch('s', [ev(5), final, ev(7)]), 300, ctx);
    q.pushLive(1, filter, batch('s', range(8, 10)), 300, ctx); // still over -> widens the same gap
    const { items, gaps } = q.drain();
    expect(gaps).toHaveLength(1);
    const g = gaps[0]!;
    expect(g.kind).toBe('display.gap');
    expect(g.domain).toBe('can');
    expect(g.channel).toBe('ch0');
    expect(g.payload as unknown as GapData).toMatchObject({ seq_range: { from: '5', to: '10' }, count: '5', cause: 'client_decode_budget' });
    expect(items).toHaveLength(1);
    const seqs = items[0]!.k === 'batch' ? items[0]!.batch.events.map((e) => `${e.kind}:${e.seq}`) : [];
    // data 1-4, then the gap at the hole, then the surviving control row
    expect(seqs).toEqual(['can.frame:1', 'can.frame:2', 'can.frame:3', 'can.frame:4', 'display.gap:5', 'can.tx:6']);
    expect(q.stats.droppedEvents).toBe(5);
  });

  it('batch-count limit triggers too (128 by default, 3 here)', () => {
    const q = new DeliveryQueue(() => undefined, small);
    for (let i = 1; i <= 5; i++) q.pushLive(1, filter, batch('s', range(i, i)), 1, ctx);
    const { gaps } = q.drain();
    expect(gaps).toHaveLength(1);
    expect((gaps[0]!.payload as unknown as GapData).count).toBe('2');
  });

  it('a drained queue accepts data again; a later drop opens a new gap', () => {
    const q = new DeliveryQueue(() => undefined, small);
    for (let i = 1; i <= 4; i++) q.pushLive(1, filter, batch('s', range(i, i)), 1, ctx);
    expect(q.drain().gaps).toHaveLength(1);
    q.pushLive(1, filter, batch('s', range(5, 5)), 1, ctx);
    expect(q.drain().gaps).toHaveLength(0);
  });

  it('control-only batches are never dropped by the data budget', () => {
    const q = new DeliveryQueue(() => undefined, small);
    for (let i = 1; i <= 3; i++) q.pushLive(1, filter, batch('s', range(i, i)), 300, ctx);
    q.pushLive(1, filter, batch('s', [ev(9, 'core.latch', { payload: { on: true } })]), 500, ctx);
    const { items, gaps } = q.drain();
    expect(gaps).toHaveLength(0);
    expect(items[0]!.k === 'batch' && items[0]!.batch.events.at(-1)!.kind).toBe('core.latch');
  });

  it('hard cap: purges the subscription and asks for a resync (no silent loss)', () => {
    const onOverflow = vi.fn();
    const q = new DeliveryQueue(onOverflow, { ...small, totalBytes: 500 });
    q.pushLive(2, filter, batch('t', range(1, 1)), 100, ctx);
    q.pushExempt({ k: 'part', eid: 1, part: snapPart('s', [], 1, 2) }, 300);
    q.pushExempt({ k: 'part', eid: 1, part: snapPart('s', [], 2, 2) }, 300);
    expect(onOverflow).toHaveBeenCalledWith(1);
    expect(q.drain().items.map((i) => i.eid)).toEqual([2]); // other subscription untouched
  });

  it('purge removes a closed subscription', () => {
    const q = new DeliveryQueue(() => undefined);
    q.pushLive(1, filter, batch('s', range(1, 1)), 10, ctx);
    q.pushLive(2, filter, batch('t', range(2, 2)), 10, ctx);
    q.purge(1);
    expect(q.drain().items.map((i) => i.eid)).toEqual([2]);
  });
});
