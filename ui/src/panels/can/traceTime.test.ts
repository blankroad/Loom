// B1 spec (Opus): CAN-1 model columns (wall / clock) — FUNCTIONS.md §B1-MODEL.
import { describe, expect, it } from 'vitest';
import type { LoomEvent } from '../../sdk';
import { NO_CLOCK } from './timeFmt';
import { bytesForRows, CanTraceModel, K_GAP, monoMs } from './traceModel';

let seq = 0;
const T = (monoNs: string, wallNs: string | undefined, clock: string | undefined) =>
  ({ mono_ns: monoNs, ...(wallNs !== undefined ? { wall_ns: wallNs } : {}), ...(clock !== undefined ? { clock_id: clock } : {}), source: 'synthetic' }) as LoomEvent['ts'];
function frame(id: number, o: Partial<LoomEvent> = {}): LoomEvent {
  seq++;
  return {
    schema_version: 1, session_id: 's', domain: 'can', channel: 'ch0', kind: 'can.frame', epoch: '1', seq: String(seq),
    producer_id: 'p', producer_epoch: '1', source_seq: String(seq),
    ts: T(String(9_953_877_626_791 + seq * 1_000_000), String(1_794_742_414_837_452_625n + BigInt(seq) * 1_000_000n), 'proc-23427'),
    dir: 'rx', origin: 'device', payload: { id, ext: false, fd: false, dlc: 1, bytes: { encoding: 'hex', data: '01' } },
    ...o,
  } as LoomEvent;
}

describe('trace model keeps each row’s own wall clock and clock id', () => {
  it('frame rows: wallAt / clockAt / clockIdOf / timeAt', () => {
    const m = new CanTraceModel({ capacity: 1000 });
    const e = frame(0x301);
    m.apply(e);
    const abs = m.total - 1;
    const wns = BigInt(e.ts.wall_ns); // sub-ms kept (contract §B1-TIME wallMsOf): ms head + ns tail
    expect(m.wallAt(abs)).toBeCloseTo(Number(wns / 1_000_000n) + Number(wns % 1_000_000n) / 1e6, 3);
    expect(m.clockIdOf(m.clockAt(abs))).toBe('proc-23427');
    expect(m.timeAt(abs)).toEqual({ monoMs: monoMs(e), wallMs: m.wallAt(abs), clock: m.clockAt(abs), clockId: 'proc-23427', seq: Number(e.seq) });
  });
  it('missing wall / clock stay unknown (0 / NO_CLOCK), never borrowed from a neighbour', () => {
    const m = new CanTraceModel({ capacity: 1000 });
    m.apply(frame(0x300));
    m.apply(frame(0x300, { ts: T('5000000', undefined, undefined) }));
    m.apply(frame(0x300, { ts: T('6000000', '0', '') }));
    expect(m.wallAt(m.total - 2)).toBe(0);
    expect(m.clockAt(m.total - 2)).toBe(NO_CLOCK);
    expect(m.wallAt(m.total - 1)).toBe(0);
    expect(m.clockAt(m.total - 1)).toBe(NO_CLOCK);
    expect(m.clockIdOf(NO_CLOCK)).toBeNull();
  });
  it('a send row merged with its bus echo takes the echo’s mono AND wall AND clock', () => {
    const m = new CanTraceModel({ capacity: 1000 });
    m.apply({ ...frame(0x300), kind: 'can.tx', request_id: 'r-1', dir: 'tx', origin: 'user', ts: T('1000000', '1000000000000', 'gw'), payload: { write_state: 'sent', id: 0x300, ext: false, fd: false, bytes: { encoding: 'hex', data: '01' } } } as LoomEvent);
    const abs = m.total - 1;
    const echo = frame(0x300, { dir: 'tx', origin: 'user', request_id: 'r-1', ts: T('2000000', '2000000000000', 'proc-9') });
    m.apply(echo);
    expect(m.total - 1).toBe(abs); // merged, no new row
    expect(m.monoAt(abs)).toBe(monoMs(echo));
    expect(m.wallAt(abs)).toBeCloseTo(2_000_000, 6);
    expect(m.clockIdOf(m.clockAt(abs))).toBe('proc-9');
  });
  it('gap events keep their own stamps; synthetic gaps / omission rows have none', () => {
    const m = new CanTraceModel({ capacity: 1000 });
    m.apply({ ...frame(0), kind: 'record.gap', ts: T('3000000', '3000000000000', 'proc-23427'), payload: { seq_range: { from: '1', to: '2' }, count: 2, cause: 'disk' } } as LoomEvent);
    expect(m.kindAt(m.total - 1)).toBe(K_GAP);
    expect(m.wallAt(m.total - 1)).toBeCloseTo(3_000_000, 6);
    m.addGap({ kind: 'resync', first: null, last: null, count: 'unknown', cause: 'x', recordOk: false, producerOnly: false, local: true });
    expect(m.wallAt(m.total - 1)).toBe(0);
    expect(m.clockAt(m.total - 1)).toBe(NO_CLOCK);
  });
  it('firstFrameRef = first non-gap row ingested; survives eviction of that row', () => {
    const m = new CanTraceModel({ capacity: 1000 });
    expect(m.firstFrameRef).toBeNull();
    m.addGap({ kind: 'resync', first: null, last: null, count: 'unknown', cause: 'x', recordOk: false, producerOnly: false, local: true });
    expect(m.firstFrameRef).toBeNull();
    const first = frame(0x300);
    m.apply(first);
    const ref = m.firstFrameRef;
    expect(ref).toEqual({ monoMs: monoMs(first), clock: m.clockAt(m.total - 1), seq: Number(first.seq), kind: 'start' });
    for (let i = 0; i < 1500; i++) m.apply(frame(0x301));
    expect(m.evicted).toBeGreaterThan(0);
    expect(m.firstFrameRef).toEqual(ref);
  });
  it('memory accounting includes the two new columns (8 B wall + 2 B clock per row)', () => {
    expect(bytesForRows(2000) - bytesForRows(1000)).toBe(1000 * 59);
  });
});
