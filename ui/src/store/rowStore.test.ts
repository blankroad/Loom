import { describe, expect, it } from 'vitest';
import { RowStore } from './rowStore';
import { ev, writeEv } from './testing/fakeNet';

describe('RowStore', () => {
  it('creates one row per request_id and progresses through stages', () => {
    const rs = new RowStore();
    expect(rs.apply(writeEv('r1', 'requested', { seq: 1 }))).toBe('created');
    expect(rs.apply(writeEv('r1', 'accepted', { seq: 2 }))).toBe('updated');
    expect(rs.apply(writeEv('r1', 'sent', { seq: 3 }))).toBe('updated');
    expect(rs.apply(writeEv('r1', 'confirmed', { seq: 4 }))).toBe('updated');
    expect(rs.size).toBe(1);
    expect(rs.get('r1')?.state).toBe('confirmed');
  });

  it('ignores a late lower stage (monotonic)', () => {
    const rs = new RowStore();
    rs.apply(writeEv('r1', 'confirmed', { seq: 10 }));
    expect(rs.apply(writeEv('r1', 'sent', { seq: 11 }))).toBe('ignored');
    expect(rs.apply(writeEv('r1', 'requested', { seq: 5 }))).toBe('ignored');
    expect(rs.get('r1')?.state).toBe('confirmed');
  });

  it('accepts same-stage updates with newer seq (UDS pending ×N) but not older duplicates', () => {
    const rs = new RowStore();
    rs.apply(writeEv('r1', 'sent', { seq: 3, payload: { pending_count: 1 } }));
    expect(rs.apply(writeEv('r1', 'sent', { seq: 4, payload: { pending_count: 2 } }))).toBe('updated');
    expect(rs.get('r1')?.payload.pending_count).toBe(2);
    expect(rs.apply(writeEv('r1', 'sent', { seq: 2, payload: { pending_count: 0 } }))).toBe('ignored');
    expect(rs.get('r1')?.payload.pending_count).toBe(2);
  });

  it('keeps row order fixed by first request time, not by update order', () => {
    const rs = new RowStore();
    rs.apply(writeEv('a', 'requested', { seq: 1, monoMs: 100 }));
    rs.apply(writeEv('b', 'requested', { seq: 2, monoMs: 200 }));
    rs.apply(writeEv('c', 'requested', { seq: 3, monoMs: 150 })); // arrives late, sorted in
    rs.apply(writeEv('a', 'confirmed', { seq: 4, monoMs: 300 })); // update does not move it
    expect(rs.rows().map((r) => r.requestId)).toEqual(['a', 'c', 'b']);
  });

  it('ignores events without request_id or write_state', () => {
    const rs = new RowStore();
    expect(rs.apply(ev({ kind: 'uart.data' }))).toBe('ignored');
    expect(rs.apply({ ...ev({ kind: 'uds.request' }), request_id: 'x' })).toBe('ignored');
    expect(rs.size).toBe(0);
  });

  it('evicts oldest COMPLETED rows over the cap, never in-flight ones, and counts them', () => {
    const rs = new RowStore({ maxCompleted: 20 });
    rs.apply(writeEv('inflight', 'sent', { seq: 1, monoMs: 1 }));
    for (let i = 0; i < 25; i++) rs.apply(writeEv(`d${i}`, 'confirmed', { seq: 10 + i, monoMs: 10 + i }));
    expect(rs.get('inflight')).toBeDefined();
    expect(rs.evicted).toBeGreaterThan(0);
    const completed = rs.rows().filter((r) => r.state === 'confirmed').length;
    expect(completed).toBeLessThanOrEqual(20);
    expect(rs.get('d0')).toBeUndefined(); // oldest went first
    expect(rs.get('d24')).toBeDefined();
  });

  it('rejects new in-flight rows beyond maxInFlight and counts them', () => {
    const rs = new RowStore({ maxInFlight: 2 });
    rs.apply(writeEv('a', 'sent'));
    rs.apply(writeEv('b', 'sent'));
    expect(rs.apply(writeEv('c', 'sent'))).toBe('rejected');
    expect(rs.rejected).toBe(1);
    expect(rs.size).toBe(2);
  });

  it('notifies subscribers once per applyMany batch', () => {
    const rs = new RowStore();
    let n = 0;
    rs.subscribe(() => n++);
    rs.applyMany([writeEv('a', 'requested'), writeEv('b', 'requested'), ev({ kind: 'uart.data' })]);
    expect(n).toBe(1);
  });
});
