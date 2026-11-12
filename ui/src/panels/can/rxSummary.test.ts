import { describe, expect, it } from 'vitest';
import { ev } from '../../sdk/testing/harness';
import type { LoomEvent } from '../../sdk';
import { fmtData, RxSummary, rxKey, typeFlags } from './rxSummary';

let seq = 1;
const at = (ms: number) => ({ mono_ns: String(Math.round(ms * 1e6)) });
const rx = (id: number, data: string, ms: number, o: Partial<LoomEvent> = {}, p: Record<string, unknown> = {}) =>
  ev({ kind: 'can.frame', channel: 'ch0', dir: 'rx', origin: 'device', seq: String(seq++), ts: at(ms) as LoomEvent['ts'], payload: { id, ext: false, fd: false, dlc: data.length / 2, bytes: { encoding: 'hex', data }, ...p }, ...o });

describe('RxSummary (per-ID latest receive list)', () => {
  it('one row per id: latest bytes, count, cycle from consecutive frames, last time', () => {
    const s = new RxSummary();
    s.apply(rx(0x310, '0102', 1000));
    s.apply(rx(0x310, '0103', 1010));
    s.apply(rx(0x310, '0103', 1020.5));
    s.apply(rx(0x100, 'ff', 1015));
    expect(s.size).toBe(2);
    const r = s.get(rxKey('ch0', false, 0x310))!;
    expect(r).toMatchObject({ count: 3, data: '0103', len: 2, changed: false, gapsSince: 0 });
    expect(r.cycleMs).toBeCloseTo(10.5, 6);
    expect(r.lastMonoMs).toBeCloseTo(1020.5, 6);
    expect(s.window).toMatchObject({ frames: 4, ids: 2, gaps: 0, untrackedFrames: 0, startMonoMs: 1000 });
    // reference ordering: 11-bit by id
    expect(s.list().map((x) => x.id)).toEqual([0x100, 0x310]);
  });

  it('changed flag when bytes differ from the previous frame of the id', () => {
    const s = new RxSummary();
    s.apply(rx(0x1, '00', 0));
    s.apply(rx(0x1, '01', 1));
    expect(s.get(rxKey('ch0', false, 1))!.changed).toBe(true);
    s.apply(rx(0x1, '01', 2));
    expect(s.get(rxKey('ch0', false, 1))!.changed).toBe(false);
  });

  it('key = channel + format + id: 11-bit 0x123 and 29-bit 0x123 and another channel stay apart', () => {
    const s = new RxSummary();
    s.apply(rx(0x123, '00', 0));
    s.apply(rx(0x123, '00', 1, {}, { ext: true }));
    s.apply(rx(0x123, '00', 2, { channel: 'ch1' }));
    expect(s.size).toBe(3);
    expect(s.list().map((r) => r.key)).toEqual(['ch0|s|291', 'ch0|x|291', 'ch1|s|291']);
  });

  it('only device rx is "received": Loom tx echoes / request-tagged frames / non-frames are not aggregated', () => {
    const s = new RxSummary();
    expect(s.apply(rx(0x300, '00', 0, { dir: 'tx', origin: 'user' }))).toBe(false);
    expect(s.apply(rx(0x300, '00', 0, { request_id: 'r-1' }))).toBe(false);
    expect(s.apply(ev({ kind: 'can.tx', channel: 'ch0', payload: { id: 0x300 } }))).toBe(false);
    expect(s.size).toBe(0);
    expect(s.window.frames).toBe(0);
  });

  it('FD/BRS/ESI/RTR flags and FD 64-byte data; base64 bytes decoded; raw_ref bytes shown as unknown', () => {
    const s = new RxSummary();
    s.apply(rx(0x18daf110, 'aa'.repeat(64), 0, {}, { ext: true, fd: true, brs: true, esi: true, dlc: 15, len: 64 }));
    const fd = s.get(rxKey('ch0', true, 0x18daf110))!;
    expect(typeFlags(fd)).toEqual(['EXT', 'FD', 'BRS', 'ESI']);
    expect(fd).toMatchObject({ len: 64, dlc: 15 });
    expect(fmtData(fd.data, fd.rtr).split(' ')).toHaveLength(64);
    s.apply(rx(0x200, '', 0, {}, { rtr: true, dlc: 4, len: 4, bytes: undefined }));
    const rtr = s.get(rxKey('ch0', false, 0x200))!;
    expect(typeFlags(rtr)).toEqual(['STD', 'RTR']);
    expect(fmtData(rtr.data, rtr.rtr)).toBe('RTR');
    s.apply(rx(0x201, '', 0, {}, { bytes: { encoding: 'base64', data: btoa('\x01\xa5') } }));
    expect(s.get(rxKey('ch0', false, 0x201))!.data).toBe('01a5');
    s.apply(rx(0x202, '', 0, {}, { bytes: { encoding: 'raw_ref', data: 'zz' }, len: 3 }));
    const unk = s.get(rxKey('ch0', false, 0x202))!;
    expect(fmtData(unk.data, false)).toBe('?');
    expect(unk.len).toBe(3);
  });

  it('gap vs window: gap marks make existing counts lower bounds and invalidate the cycle across the gap', () => {
    const s = new RxSummary();
    s.apply(rx(0x310, '00', 0));
    s.apply(rx(0x310, '00', 10));
    s.noteGap(25);
    s.noteGap('unknown');
    s.apply(rx(0x310, '00', 50));
    const r = s.get(rxKey('ch0', false, 0x310))!;
    expect(r.count).toBe(3);
    expect(r.gapsSince).toBe(2);
    expect(r.cycleMs).toBeNull(); // 10 → 50 spans a gap
    s.apply(rx(0x310, '00', 60));
    expect(s.get(rxKey('ch0', false, 0x310))!.cycleMs).toBeCloseTo(10, 6);
    // an id first seen after the gaps is not a lower bound
    s.apply(rx(0x311, '00', 61));
    expect(s.get(rxKey('ch0', false, 0x311))!.gapsSince).toBe(0);
    expect(s.window).toMatchObject({ gaps: 2, gapFramesKnown: 25, gapsUnknown: 1, frames: 5 });
  });

  it('bounded: ids beyond maxIds are counted as untracked frames, never added', () => {
    const s = new RxSummary({ maxIds: 3 });
    for (let id = 0; id < 5; id++) s.apply(rx(id, '00', id));
    s.apply(rx(4, '00', 9));
    s.apply(rx(0, '00', 10));
    expect(s.size).toBe(3);
    expect(s.window).toMatchObject({ ids: 3, maxIds: 3, untrackedFrames: 3, frames: 4 });
    expect(s.get(rxKey('ch0', false, 0))!.count).toBe(2);
  });

  it('reset starts a new window (rows, counters and gaps cleared)', () => {
    const s = new RxSummary();
    s.apply(rx(1, '00', 0));
    s.noteGap(1);
    const v = s.version;
    s.reset(5_000);
    expect(s.version).toBeGreaterThan(v);
    expect(s.size).toBe(0);
    expect(s.window).toMatchObject({ frames: 0, gaps: 0, startMonoMs: null, startedAtMs: 5_000 });
  });
});
