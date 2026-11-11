import { describe, expect, it } from 'vitest';
import { EventMirror } from './storeMirror';

const item = (target: string) => ({ domain: 'uds', kind: 'uds.snapshot', state: {
  kind: 'uds.state', payload: { target, generation: 1, link: { state: 'up' } },
} });

describe('native snapshot state accessor', () => {
  it('publishes multipart state at snapshot_end without inventing events or sequence numbers', () => {
    const mirror = new EventMirror(10);
    mirror.snapshotPart({ sub: 's', n: 1, total: 2, items: [item('a')], events: [] });
    mirror.snapshotPart({ sub: 's', n: 2, total: 2, items: [item('b')] });
    expect(mirror.snapshot).toBeNull();
    expect(mirror.headSeq).toBe(0n);
    expect(mirror.tail(10)).toEqual([]);
    mirror.snapshotEnd('123');
    expect(mirror.snapshot).toEqual({ items: [item('a'), item('b')], throughSeq: 123n, version: 1, truncated: false });
    expect(mirror.after(-1n)).toEqual([]);
    expect(Object.isFrozen(mirror.snapshot?.items)).toBe(true);
  });

  it('bounds both retained item count and bytes, replaces on reconnect, releases on dispose', () => {
    const mirror = new EventMirror(2);
    mirror.snapshotPart({ sub: 's', n: 1, total: 1, items: [item('a'), item('b'), item('c')] });
    mirror.snapshotEnd('5');
    expect(mirror.snapshot?.items).toHaveLength(2);
    expect(mirror.snapshot?.truncated).toBe(true);
    mirror.resync('reconnect');
    expect(mirror.snapshot).toBeNull();
    mirror.snapshotPart({ sub: 's', n: 1, total: 1, items: [item('x'.repeat(600_000)), item('new')] });
    mirror.snapshotEnd('6');
    expect(mirror.snapshot).toMatchObject({ items: [item('new')], version: 2, truncated: true });
    mirror.snapshotPart({ sub: 's', n: 1, total: 1, items: [] });
    mirror.snapshotEnd('7');
    expect(mirror.snapshot).toMatchObject({ items: [], version: 3, truncated: false });
    mirror.dispose();
    expect(mirror.snapshot).toBeNull();
    expect(mirror.tail(10)).toEqual([]);
  });
});
