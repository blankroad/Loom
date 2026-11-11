import { describe, expect, it } from 'vitest';
import type { LoomEvent } from '../../sdk';
import {
  AGENT_RANGES,
  CanTraceModel,
  dlcForLen,
  F_ECHO,
  F_EXT,
  F_HIDDEN,
  F_NOID,
  F_RTR,
  parseCansend,
  txFrame,
  K_FRAME,
  K_GAP,
  K_TXREQ,
  REQ_SLOTS,
  ROWS_MAX,
  ROWS_MIN,
  monoMs,
} from './traceModel';

let seq = 0;
function frame(id: number, dataHex: string, o: Partial<LoomEvent> = {}): LoomEvent {
  seq++;
  return {
    schema_version: 1,
    session_id: 's',
    domain: 'can',
    channel: 'ch0',
    kind: 'can.frame',
    epoch: '1',
    seq: String(seq),
    producer_id: 'p',
    producer_epoch: '1',
    source_seq: String(seq),
    ts: { mono_ns: String(812_004_000_000 + seq * 1_000_000), wall_ns: '0', clock_id: 'm', source: 'host_read_complete' },
    dir: 'rx',
    origin: 'device',
    payload: { id, ext: id > 0x7ff, fd: dataHex.length > 16, dlc: dataHex.length / 2, bytes: { encoding: 'hex', data: dataHex } },
    ...o,
  };
}
function tx(rid: string, state: string, o: Partial<LoomEvent> = {}, p: Record<string, unknown> = {}): LoomEvent {
  const f = frame(0x300, '01', { kind: 'can.tx', dir: 'tx', origin: 'agent', request_id: rid, client_id: 'agent:claude-1', risk: 'R1', ...o });
  return { ...f, payload: { ...f.payload, write_state: state, ...p } };
}
function gap(kind: string, from: number | null, to: number | null, count: string, cause = 'slow_client', extra: Record<string, unknown> = {}): LoomEvent {
  const f = frame(0, '');
  return { ...f, kind, payload: { ...(from !== null && to !== null ? { seq_range: { from: String(from), to: String(to) } } : {}), count, cause, ...extra } };
}
const shown = (m: CanTraceModel) => Array.from({ length: m.dispCount }, (_, k) => m.dispAt(k));

describe('CanTraceModel ring', () => {
  it('clamps capacity to [1k, 2M]', () => {
    expect(new CanTraceModel({ capacity: 10 }).capacity).toBe(ROWS_MIN);
    expect(new CanTraceModel({ capacity: 5e9 }).capacity).toBe(ROWS_MAX);
    expect(new CanTraceModel().capacity).toBe(100_000);
  });

  it('wraps around: keeps the newest `capacity` rows, counts evictions, trims the display index', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    for (let i = 0; i < 2_500; i++) m.apply(frame(0x100 + (i % 4), (i & 0xff).toString(16).padStart(2, '0')));
    expect(m.count).toBe(1_000);
    expect(m.total).toBe(2_500);
    expect(m.evicted).toBe(1_500);
    m.dispTrim();
    expect(m.dispCount).toBe(1_000);
    const first = m.dispAt(0);
    expect(first).toBe(1_500);
    expect(m.dataHexAt(first)).toBe((1_500 & 0xff).toString(16).toUpperCase().padStart(2, '0'));
    expect(m.has(1_499)).toBe(false);
    // ascending abs → binary search finds positions
    expect(m.dispIndexOf(2_000)).toBe(500);
  });

  it('stores id/flags/len/data and the mono time without row objects', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(frame(0x18daf110, '021003'));
    const abs = m.dispAt(0);
    expect(m.kindAt(abs)).toBe(K_FRAME);
    expect(m.idAt(abs)).toBe(0x18daf110);
    expect(m.lenAt(abs)).toBe(3);
    expect(m.dataHexAt(abs)).toBe('02 10 03');
    expect(m.originAt(abs)).toBe('device');
    expect(m.monoAt(abs)).toBeCloseTo(812_004 + seq, 6);
  });

  it('FD payloads > 8 B live in a bounded slot ring; reused slots never show wrong bytes', () => {
    const m = new CanTraceModel({ capacity: 1_000 }); // 1024 FD slots
    m.apply(frame(0x123, 'ab'.repeat(64)));
    const firstFd = m.dispAt(0);
    expect(m.dataHexAt(firstFd)).toBe(Array(64).fill('AB').join(' '));
    for (let i = 0; i < 1_024; i++) m.apply(frame(0x124, 'cd'.repeat(12)));
    // the first FD row was evicted from the ring anyway (capacity 1000); check a live row whose slot was reused
    const m2 = new CanTraceModel({ capacity: 20_000 }); // 1250 FD slots
    m2.apply(frame(0x123, 'ab'.repeat(16)));
    const a = m2.dispAt(0);
    for (let i = 0; i < 1_250; i++) m2.apply(frame(0x124, 'cd'.repeat(12)));
    expect(m2.has(a)).toBe(true);
    expect(m2.dataHexAt(a)).toBeNull(); // shown as "(FD 데이터 보관 범위 밖)"
    expect(m2.dataHexAt(m2.dispAt(m2.dispCount - 1))).toBe(Array(12).fill('CD').join(' '));
  });

  it('raw_ref / undecodable bytes keep the length and show "?"', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    const f = frame(0x100, '');
    m.apply({ ...f, payload: { id: 0x100, dlc: 8, bytes: { encoding: 'raw_ref', ref: 'r1' } } });
    expect(m.dataHexAt(m.dispAt(0))).toBe('?');
    expect(m.lenAt(m.dispAt(0))).toBe(8);
  });
});

describe('TX rows (can.tx request_id + write_state)', () => {
  it('one request_id = one row; stages only move forward; tx echo merges', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(tx('r-1', 'requested'));
    m.apply(tx('r-1', 'accepted'));
    m.apply(tx('r-1', 'sent'));
    m.apply(frame(0x300, '01', { dir: 'tx', origin: 'agent', request_id: 'r-1' })); // echo
    m.apply(frame(0x301, '01a5'));
    m.apply(tx('r-1', 'confirmed'));
    m.apply(tx('r-1', 'sent')); // late duplicate: never moves back
    const rows = shown(m);
    expect(rows).toHaveLength(2);
    const r = rows[0]!;
    expect(m.kindAt(r)).toBe(K_TXREQ);
    expect(m.stateAt(r)).toBe('confirmed');
    expect(m.flagsAt(r) & F_ECHO).toBeTruthy();
    expect(m.reqAt(r)).toMatchObject({ requestId: 'r-1', clientId: 'agent:claude-1', mine: false });
    expect(m.riskAt(r)).toBe('R1');
    expect(m.originAt(r)).toBe('agent');
    expect(m.idAt(rows[1]!)).toBe(0x301);
  });

  it('failed rows keep the error; markMine flags USR(나)', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(tx('r-2', 'requested', { origin: 'user' }));
    m.apply(tx('r-2', 'failed', { origin: 'user' }, { error: 'bus_off' }));
    m.markMine('r-2');
    const r = m.dispAt(0);
    expect(m.stateAt(r)).toBe('failed');
    expect(m.reqAt(r)).toMatchObject({ error: 'bus_off', mine: true });
    expect(m.stateOf('r-2')).toBe('failed');
  });

  it('raiseState (RowStore / control path) is monotonic', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(tx('r-3', 'sent'));
    const r = m.dispAt(0);
    expect(m.raiseState(r, 'accepted')).toBe(false);
    expect(m.raiseState(r, 'confirmed')).toBe(true);
    expect(m.stateAt(r)).toBe('confirmed');
  });

  it('request dictionary is bounded (oldest ids forgotten)', () => {
    const m = new CanTraceModel({ capacity: 20_000 });
    for (let i = 0; i < REQ_SLOTS + 10; i++) m.apply(tx(`q-${i}`, 'requested'));
    expect(m.stateOf('q-0')).toBeNull();
    expect(m.stateOf(`q-${REQ_SLOTS + 9}`)).toBe('requested');
    // the old row is still in the ring but no longer linked to its id
    expect(m.reqAt(m.dispAt(0))).toBeNull();
  });
});

describe('gap and omission rows', () => {
  it('display / record / source gaps become rows that filters never hide', () => {
    const m = new CanTraceModel({ capacity: 1_000, recordOk: () => true });
    m.apply(frame(0x100, '00'));
    m.apply(gap('display.gap', 9001, 12812, '3812'));
    m.apply(gap('record.gap', 20, 25, '6', 'disk_full'));
    m.apply(gap('source.gap', null, null, '37', 'driver_overrun', { producer_range: { producer_id: 'can-sim', producer_epoch: '1', from: '1000', to: '1036' } }));
    m.setFilter({ ids: new Set([0x999]), dir: 'all', origins: null });
    const rows = shown(m);
    expect(rows).toHaveLength(3);
    expect(rows.every((a) => m.kindAt(a) === K_GAP)).toBe(true);
    expect(m.gapAt(rows[0]!)).toMatchObject({ kind: 'display.gap', first: 9001, last: 12812, count: 3812, recordOk: true });
    expect(m.gapAt(rows[1]!)).toMatchObject({ kind: 'record.gap', count: 6 });
    expect(m.gapAt(rows[2]!)).toMatchObject({ kind: 'source.gap', first: null, producerOnly: true, count: 37 });
  });

  it('degraded: rows over 2,000/s are kept but hidden, counted on one omission row per run', () => {
    let t = 0;
    const m = new CanTraceModel({ capacity: 20_000, now: () => t, recordOk: () => false });
    m.setDegraded(true); // burst 500 tokens
    for (let i = 0; i < 1_000; i++) m.apply(frame(0x100, '00'));
    // 500 shown, then one omission row for the other 500
    const rows = shown(m);
    expect(rows).toHaveLength(501);
    const g = m.gapAt(rows[500]!)!;
    expect(g).toMatchObject({ kind: 'omit', count: 500, recordOk: false });
    expect(m.omitted).toBe(500);
    expect(m.count).toBe(1_001); // every frame stays in the ring (+ the omission row)
    expect(m.flagsAt(rows[500]! + 1) & F_HIDDEN).toBeTruthy();
    // 0.1 s later 200 more tokens: a shown row ends the run, the next overflow opens a new one
    t += 100;
    for (let i = 0; i < 300; i++) m.apply(frame(0x100, '00'));
    const rows2 = shown(m);
    expect(rows2).toHaveLength(501 + 200 + 1);
    expect(m.gapAt(rows2[rows2.length - 1]!)).toMatchObject({ kind: 'omit', count: 100 });
    // the omission count + shown frames account for every frame received
    const shownFrames = rows2.filter((a) => m.kindAt(a) === K_FRAME).length;
    expect(shownFrames + m.omitted).toBe(m.framesIn);
    // writes are never hidden by the display budget
    m.apply(tx('r-9', 'requested'));
    expect(m.kindAt(m.dispAt(m.dispCount - 1))).toBe(K_TXREQ);
  });

  it('not degraded: no omission at any rate', () => {
    const m = new CanTraceModel({ capacity: 20_000, now: () => 0 });
    for (let i = 0; i < 10_000; i++) m.apply(frame(0x100, '00'));
    expect(m.dispCount).toBe(10_000);
    expect(m.omitted).toBe(0);
  });
});

describe('filters and agent read ranges', () => {
  it('client filter by id, dir and origin rebuilds the display index', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(frame(0x100, '00'));
    m.apply(tx('r-1', 'requested'));
    m.apply(frame(0x301, '01', { origin: 'device' }));
    m.apply(frame(0x300, '00', { dir: 'tx', origin: 'user' }));
    m.setFilter({ ids: new Set([0x300, 0x301]), dir: 'all', origins: null });
    expect(shown(m).map((a) => m.idAt(a))).toEqual([0x300, 0x301, 0x300]);
    m.setFilter({ ids: null, dir: 'tx', origins: null });
    expect(shown(m)).toHaveLength(2);
    m.setFilter({ ids: null, dir: 'all', origins: new Set(['user']) });
    expect(shown(m).map((a) => m.originAt(a))).toEqual(['user']);
    // new rows follow the active filter
    m.apply(frame(0x300, '00', { dir: 'tx', origin: 'agent' }));
    m.apply(frame(0x300, '00', { dir: 'tx', origin: 'user' }));
    expect(shown(m)).toHaveLength(2);
  });

  it('core.activity ranges for this channel mark rows; bounded to the newest 16', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    const act = (domain: string, channel: string, a?: string, b?: string): LoomEvent => ({
      ...frame(0, ''),
      domain: 'core',
      kind: 'core.activity',
      origin: 'agent',
      payload: { actor: 'claude-1', verb: 'read', domain, channel, ...(a ? { range: { seq_a: a, seq_b: b } } : {}) },
    });
    expect(m.applyActivity(act('can', 'ch0', '10', '20'), 'ch0', 0)).toBe(true);
    expect(m.applyActivity(act('can', 'ch1', '30', '40'), 'ch0', 0)).toBe(false);
    expect(m.applyActivity(act('uart', 'ch0', '30', '40'), 'ch0', 0)).toBe(false);
    expect(m.applyActivity(act('can', 'ch0'), 'ch0', 0)).toBe(false);
    expect(m.agentReadAt(15)).toMatchObject({ a: 10, b: 20, actor: 'claude-1' });
    expect(m.agentReadAt(21)).toBeNull();
    for (let i = 0; i < 40; i++) m.applyActivity(act('can', 'ch0', String(100 + i), String(100 + i)), 'ch0', 0);
    expect(m.agentRanges).toHaveLength(AGENT_RANGES);
    expect(m.agentReadAt(15)).toBeNull();
  });
});

describe('monoMs', () => {
  it('keeps µs precision for large ns values', () => {
    expect(monoMs({ ts: { mono_ns: '1762770000123456789' } } as LoomEvent)).toBeCloseTo(1762770000123.4568, 3);
    expect(monoMs({ ts: { mono_ns: '1500' } } as LoomEvent)).toBeCloseTo(0.0015, 9);
  });
});

// ---------------------------------------------------------------- CAN schema v1
describe('schema v1: one send = one row (can.tx + echo by request_id)', () => {
  const txv1 = (rid: string, state: string, echoOf: Record<string, unknown>, o: Partial<LoomEvent> = {}): LoomEvent => {
    const f = frame(0, '', { kind: 'can.tx', dir: undefined, origin: 'user', request_id: rid, client_id: 'viewer:web', risk: 'R1', ...o });
    return { ...f, payload: { write_state: state, echo_of: echoOf } }; // CORE fixture shape: no id/bytes of its own
  };
  const echo = (rid: string, id: number, hex: string, p: Record<string, unknown> = {}): LoomEvent => {
    const f = frame(id, hex, { dir: 'tx', origin: 'user', request_id: rid });
    return { ...f, payload: { ...(f.payload as object), ...p } };
  };

  it('write row reads the request (echo_of.frame cansend text); the echo merges with ITS time and bytes', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(txv1('u-1', 'requested', { channel: 'ch0', frame: '300#01' }));
    const r = m.dispAt(0);
    expect(m.kindAt(r)).toBe(K_TXREQ);
    expect(m.idAt(r)).toBe(0x300);
    expect(m.dataHexAt(r)).toBe('01');
    m.apply(txv1('u-1', 'sent', { channel: 'ch0', frame: '300#01' }));
    const e = echo('u-1', 0x300, 'A5');
    m.apply(e);
    m.apply(txv1('u-1', 'confirmed', { channel: 'ch0', frame: '300#01' }));
    expect(m.dispCount).toBe(1);
    expect(m.monoAt(r)).toBe(monoMs(e)); // echo time
    expect(m.dataHexAt(r)).toBe('A5'); // echo bytes
    expect(m.flagsAt(r) & F_ECHO).toBeTruthy();
    expect(m.stateAt(r)).toBe('confirmed');
    expect(m.frameIndexAt(r)).toBe(0);
  });

  it('structured request {id, ext, fd, brs, data} and echo-before-write both give one row', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(txv1('u-2', 'requested', { channel: 'ch0', id: 0x18daf110, ext: true, fd: true, brs: true, data: '00'.repeat(12) }));
    expect(m.idAt(m.dispAt(0))).toBe(0x18daf110);
    expect(m.flagsAt(m.dispAt(0)) & F_EXT).toBeTruthy();
    expect(m.lenAt(m.dispAt(0))).toBe(12);
    // echo first, write row later
    m.apply(echo('u-3', 0x301, '02'));
    m.apply(txv1('u-3', 'confirmed', { channel: 'ch0', frame: '301#02' }));
    expect(m.dispCount).toBe(2);
    expect(m.stateAt(m.dispAt(1))).toBe('confirmed');
  });

  it('count > 1: one row per frame_index, state per request on all rows, duplicate echo ignored', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(txv1('u-4', 'sent', { channel: 'ch0', frame: '300#01', count: 3 }));
    m.apply(echo('u-4', 0x300, '01', { frame_index: 0 }));
    m.apply(echo('u-4', 0x300, '01', { frame_index: 1 }));
    m.apply(frame(0x7e8, '02')); // unrelated rx in between
    m.apply(echo('u-4', 0x300, '01', { frame_index: 2 }));
    m.apply(echo('u-4', 0x300, '01', { frame_index: 1 })); // duplicate
    m.apply(txv1('u-4', 'confirmed', { channel: 'ch0', frame: '300#01', count: 3 }));
    const rows = shown(m).filter((a) => m.reqAt(a)?.requestId === 'u-4');
    expect(rows.map((a) => m.frameIndexAt(a))).toEqual([0, 1, 2]);
    expect(rows.map((a) => m.stateAt(a))).toEqual(['confirmed', 'confirmed', 'confirmed']);
    expect(m.dispCount).toBe(4);
    expect(m.dupEchoes).toBe(1);
    expect(m.rowsOf('u-4')).toEqual(rows);
  });

  it('a write row with no readable frame is still shown (id "?")', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(txv1('u-5', 'requested', { channel: 'ch0' }));
    expect(m.dispCount).toBe(1);
    expect(m.flagsAt(m.dispAt(0)) & F_NOID).toBeTruthy();
  });

  it('display budget never hides write rows or their echoes', () => {
    let t = 0;
    const m = new CanTraceModel({ capacity: 10_000, now: () => t });
    m.setDegraded(true);
    for (let i = 0; i < 600; i++) m.apply(frame(0x100, '00')); // exhaust the burst
    m.apply(txv1('u-6', 'sent', { channel: 'ch0', frame: '300#01', count: 2 }));
    m.apply(echo('u-6', 0x300, '01', { frame_index: 0 }));
    m.apply(echo('u-6', 0x300, '01', { frame_index: 1 }));
    t += 0; // no refill
    expect(m.rowsOf('u-6').every((a) => !(m.flagsAt(a) & F_HIDDEN))).toBe(true);
    expect(m.rowsOf('u-6')).toHaveLength(2);
  });
});

describe('schema v1: dlc = DLC code, len = bytes, rtr', () => {
  it('FD 12 bytes = DLC 9 (given or derived); raw_ref keeps len from the DLC code', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    const twelve = 'AB'.repeat(12);
    m.apply(frame(0x123, twelve, { payload: { id: 0x123, ext: false, fd: true, brs: true, dlc: 9, len: 12, bytes: { encoding: 'hex', data: twelve } } }));
    m.apply(frame(0x124, twelve, { payload: { id: 0x124, fd: true, bytes: { encoding: 'hex', data: twelve } } })); // no dlc → derived
    m.apply(frame(0x125, '', { payload: { id: 0x125, fd: true, dlc: 9, bytes: { encoding: 'raw_ref', ref: 'x' } } }));
    const [a, b, c] = shown(m);
    expect([m.lenAt(a!), m.dlcAt(a!)]).toEqual([12, 9]);
    expect([m.lenAt(b!), m.dlcAt(b!)]).toEqual([12, 9]);
    expect([m.lenAt(c!), m.dlcAt(c!), m.dataHexAt(c!)]).toEqual([12, 9, '?']);
    expect(dlcForLen(12)).toBe(9);
    expect(dlcForLen(64)).toBe(15);
  });

  it('classic remote frame: rtr, no data, len = requested length', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    m.apply(frame(0x300, '', { payload: { id: 0x300, ext: false, fd: false, rtr: true, dlc: 4, len: 4, bytes: { encoding: 'hex', data: '' } } }));
    const r = m.dispAt(0);
    expect(m.flagsAt(r) & F_RTR).toBeTruthy();
    expect(m.lenAt(r)).toBe(4);
    expect(m.dataHexAt(r)).toBe('');
  });

  it('cansend text in write rows: 29-bit, FD flags, RTR', () => {
    expect(parseCansend('18DAF110#021003')).toMatchObject({ id: 0x18daf110, ext: true, fd: false, hex: '021003', len: 3 });
    expect(parseCansend(`123##1${'00'.repeat(12)}`)).toMatchObject({ id: 0x123, fd: true, brs: true, len: 12 });
    expect(parseCansend('300#R')).toMatchObject({ rtr: true, hex: null });
    expect(parseCansend('nope')).toBeNull();
    expect(txFrame({ frame: { id: 768, data: '01' } })).toMatchObject({ id: 768, hex: '01' });
  });
});
