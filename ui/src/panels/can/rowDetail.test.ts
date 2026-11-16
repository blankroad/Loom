// B2 spec (Opus): CAN-7 row detail. FUNCTIONS.md §B2-DETAIL.
import { describe, expect, it } from 'vitest';
import { fmtWall, type LoomEvent } from '../../sdk';
import { ev } from '../../sdk/testing/harness';
import { CanTraceModel } from './traceModel';
import { FD_SLOT_REUSED, NO_BYTES, NO_INLINE_BYTES, fullDataText, rowDetailFields, rowDetailText, rowFacts, type RowFacts } from './rowDetail';

const WALL = 1_794_742_414_837.5; // ms
const base: RowFacts = {
  abs: 7, kind: 'frame', seq: 42, id: 0x7e8, ext: false, fd: false, brs: false, rtr: false, tx: false, echo: false,
  len: 3, dlc: 3, dataHex: '02 50 03', origin: 'device', requestId: null, clientId: null, state: null, risk: null,
  error: null, frameIndex: null, wallMs: WALL, monoMs: 1234.5, clockId: 'proc-1', gap: null,
};
const kv = (f: RowFacts) => Object.fromEntries(rowDetailFields(f).map((d) => [d.key, d.value]));

describe('fullDataText', () => {
  it('hex as given; explicit wording when bytes are absent (never zero-filled)', () => {
    expect(fullDataText('01 A5 FF')).toBe('01 A5 FF');
    expect(fullDataText('')).toBe(NO_BYTES);
    expect(fullDataText('?')).toBe(NO_INLINE_BYTES);
    expect(fullDataText(null)).toBe(FD_SLOT_REUSED);
  });
});

describe('rowDetailFields', () => {
  it('RX standard frame: ordered fields, absent request fields omitted', () => {
    const f = rowDetailFields(base);
    expect(f.map((d) => d.key)).toEqual(['kind', 'seq', 'id', 'fmt', 'len', 'data', 'origin', 'wall', 'mono', 'clock']);
    expect(kv(base)).toEqual({
      kind: 'RX 프레임',
      seq: '42',
      id: '0x7E8 (11-bit)',
      fmt: 'CAN',
      len: '3 B · DLC 3',
      data: '02 50 03',
      origin: 'device',
      wall: `${fmtWall(WALL)} (${new Date(WALL).toISOString()})`,
      mono: '1.234500 s',
      clock: 'proc-1',
    });
    expect(f.find((d) => d.key === 'id')!.label).toBe('ID');
  });

  it('TX echo of a request, FD+BRS extended, all request fields', () => {
    const f: RowFacts = {
      ...base, tx: true, echo: true, ext: true, fd: true, brs: true, id: 0x18daf110, len: 12, dlc: 9,
      dataHex: '00 01 02 03 04 05 06 07 08 09 0A 0B', origin: 'agent', requestId: 'r-9', clientId: 'cli-2',
      state: 'confirmed', risk: 'R1', frameIndex: 2,
    };
    expect(rowDetailFields(f).map((d) => d.key)).toEqual(['kind', 'seq', 'id', 'fmt', 'len', 'data', 'origin', 'req', 'frameIndex', 'state', 'risk', 'wall', 'mono', 'clock']);
    expect(kv(f)).toMatchObject({
      kind: 'TX 프레임 · 에코', id: '0x18DAF110 (29-bit)', fmt: 'CAN FD · BRS', len: '12 B · DLC 9',
      req: 'r-9 · client cli-2', frameIndex: '2', state: 'confirmed', risk: 'R1',
    });
  });

  it('send request row with an error; unknown DLC, wall, clock are said, not zeroed', () => {
    const f: RowFacts = { ...base, kind: 'txreq', tx: true, dlc: null, requestId: 'r-1', error: 'E_DENIED', wallMs: 0, clockId: null, rtr: true, dataHex: '' };
    const m = kv(f);
    expect(m.kind).toBe('송신 요청');
    expect(m.fmt).toBe('CAN · RTR');
    expect(m.len).toBe('3 B · DLC ?');
    expect(m.data).toBe(NO_BYTES);
    expect(m.req).toBe('r-1');
    expect(m.error).toBe('E_DENIED');
    expect(m.wall).toBe('알 수 없음');
    expect(m.clock).toBe('알 수 없음');
  });

  it('row without an id omits the id field', () => {
    expect(rowDetailFields({ ...base, id: null }).some((d) => d.key === 'id')).toBe(false);
  });

  it('gap row: loss facts instead of frame fields', () => {
    const g: RowFacts = {
      ...base, kind: 'gap', id: null, dataHex: '',
      gap: { kind: 'source.gap', first: 10, last: 19, count: 10, cause: 'overflow', recordOk: true, producerOnly: false, local: false },
    };
    expect(rowDetailFields(g).map((d) => d.key)).toEqual(['kind', 'seq', 'gap', 'gapRange', 'cause', 'wall', 'mono', 'clock']);
    expect(kv(g)).toMatchObject({ kind: '갭 · source.gap', gap: '10행', gapRange: 'seq 10–19', cause: 'overflow' });
    const u = kv({ ...g, gap: { ...g.gap!, count: 'unknown', first: null, last: null } });
    expect(u.gap).toBe('알 수 없음');
    expect(u.gapRange).toBeUndefined();
  });
});

describe('rowDetailText', () => {
  it('"label: value" lines in field order', () => {
    const t = rowDetailText(base);
    const lines = t.split('\n');
    expect(lines.length).toBe(rowDetailFields(base).length);
    expect(lines[0]).toBe('종류: RX 프레임');
    expect(lines).toContain('데이터: 02 50 03');
    expect(t.endsWith('\n')).toBe(false);
  });
});

describe('rowFacts (model → facts)', () => {
  it('reads one frame row back from the ring; evicted rows → null', () => {
    const m = new CanTraceModel({ capacity: 1_000 });
    const e = ev({
      kind: 'can.frame', channel: 'ch0', dir: 'rx', origin: 'device', seq: '5',
      ts: { mono_ns: '2000000000', wall_ns: '1794742414837000000', clock_id: 'proc-7', source: 'synthetic' } as LoomEvent['ts'],
      payload: { id: 0x123, ext: false, fd: false, dlc: 2, bytes: { encoding: 'hex', data: 'a55a' } },
    });
    m.apply(e);
    const abs = m.total - 1;
    const f = rowFacts(m, abs)!;
    expect(f).toMatchObject({
      abs, kind: 'frame', seq: 5, id: 0x123, ext: false, fd: false, tx: false, echo: false, len: 2, dlc: 2,
      dataHex: 'A5 5A', origin: 'device', requestId: null, gap: null, clockId: 'proc-7', monoMs: 2000,
    });
    expect(f.wallMs).toBeCloseTo(1_794_742_414_837, 3);
    expect(rowFacts(m, abs + 1)).toBeNull();
    expect(rowFacts(m, -1)).toBeNull();
  });
});
