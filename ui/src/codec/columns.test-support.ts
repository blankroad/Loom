/// <reference types="node" />

/** Hand-written TEST-ONLY writer; never import this from the transport/runtime. */
import { Buffer } from 'node:buffer';
import type { GapSummary, LoomEvent } from '../proto/provisional';
import { COLUMNS, DIRS, MAGIC, ORIGINS, SCHEMA_HASH, TS_SOURCES, TYPES } from './layout';
import type { ColumnName } from './layout';
export interface Fixture {
  name: string; rows: LoomEvent[]; delta?: boolean; epoch?: string;
  displayGap?: GapSummary | null; recordHealth?: 'ok' | 'degraded' | 'failed' | 'disabled' | null;
  hub?: string; daemon?: string;
}
const owned = ['schema_version', 'session_id', 'domain', 'kind', 'producer_id', 'producer_epoch', 'source_seq', 'epoch', 'seq', 'ts', 'origin', 'payload', 'channel', 'client_id', 'dir', 'request_id', 'quality'];
function rest(value: Record<string, unknown>, keys: readonly string[]) {
  return Object.fromEntries(Object.entries(value).filter(([key]) => !keys.includes(key)));
}
export function encode(fixture: Fixture): ArrayBuffer {
  const dictionary: string[] = [];
  const intern = (text: string) => { let n = dictionary.indexOf(text); if (n < 0) { n = dictionary.length; dictionary.push(text); } return n; };
  const sub = intern('trace-1');
  const health = fixture.recordHealth === undefined ? 0xffff_ffff : fixture.recordHealth === null ? 0xffff_fffe : intern(fixture.recordHealth);
  const rows = fixture.rows;
  const first = BigInt(rows[0]?.seq ?? '0');
  const values = rows.map(row => {
    // The proposal has a fixed enum/hash; CORE's JSON enum may grow independently.
    const tsSource = (TS_SOURCES as readonly string[]).indexOf(row.ts.source);
    if (tsSource < 0) throw new RangeError(`Timestamp source outside columns proposal: ${row.ts.source}`);
    const p = row.payload; const b = p.bytes as { encoding: 'hex' | 'base64'; data: string };
    const can = row.domain === 'can';
    const raw = new Uint8Array(Buffer.from(b.data, b.encoding));
    const v: Partial<Record<ColumnName, string | number | bigint | Uint8Array | null>> = {
      session_id: row.session_id, domain: row.domain, kind: row.kind, producer_id: row.producer_id,
      producer_epoch: BigInt(row.producer_epoch), source_seq: BigInt(row.source_seq), seq: BigInt(row.seq) - (fixture.delta ? first : 0n),
      mono_ns: BigInt(row.ts.mono_ns), wall_ns: BigInt(row.ts.wall_ns), clock_id: row.ts.clock_id,
      ts_source: tsSource, origin: ORIGINS.indexOf(row.origin),
      extra: JSON.stringify({ record: rest(row as unknown as Record<string, unknown>, owned), ts: rest(row.ts as unknown as Record<string, unknown>, ['clock_id', 'mono_ns', 'wall_ns', 'source']), payload: rest(p, can ? ['bytes', 'id', 'ext', 'fd', 'brs', 'esi', 'rtr', 'dlc', 'len'] : ['bytes']), bytes: rest(b, ['encoding', 'data']) }),
      encoding: b.encoding === 'hex' ? 0 : 1, raw,
    };
    if (b.data !== Buffer.from(raw).toString(b.encoding)) v.text = b.data;
    for (const name of ['channel', 'client_id', 'request_id'] as const) if (row[name] !== undefined) v[name] = row[name];
    if (row.dir !== undefined) v.dir = row.dir === null ? null : DIRS.indexOf(row.dir);
    if (row.quality !== undefined) v.quality = JSON.stringify(row.quality);
    if (can) Object.assign(v, { can_id: p.id, can_flags: (p.ext ? 1 : 0) | (p.fd ? 2 : 0) | (p.brs ? 4 : 0) | (p.esi ? 8 : 0) | (p.rtr ? 16 : 0), dlc: p.dlc, len: p.len });
    return v;
  });
  for (const v of values) for (const spec of COLUMNS) if (spec.type === 'dict' && typeof v[spec.name] === 'string') intern(v[spec.name] as string);
  const buffer = new ArrayBuffer(65_536); const d = new DataView(buffer); const bytes = new Uint8Array(buffer);
  const u16 = (offset: number, value: number) => d.setUint16(offset, value, true);
  const u32 = (offset: number, value: number) => d.setUint32(offset, value, true);
  const u64 = (offset: number, value: bigint) => d.setBigUint64(offset, value, true);
  let cursor = 160 + COLUMNS.length * 32;
  function put(content: Uint8Array, alignment = 1) {
    cursor = Math.ceil(cursor / alignment) * alignment;
    const offset = cursor; bytes.set(content, cursor); cursor += content.length; return offset;
  }
  const utf8 = new TextEncoder(); const dict = utf8.encode(JSON.stringify(dictionary));
  const dictionaryOffset = put(dict);
  let gapOffset = 0; let gapLength = 0;
  if (fixture.displayGap != null) { const g = utf8.encode(JSON.stringify(fixture.displayGap)); gapLength = g.length; gapOffset = put(g); }
  for (let n = 0; n < COLUMNS.length; n++) {
    const spec = COLUMNS[n]!; const dir = 160 + n * 32; const width = spec.type === 'u64' ? 8 : spec.type === 'u8' ? 1 : 4;
    const content = new Uint8Array(width * (rows.length + (spec.type === 'bytes' ? 1 : 0))); const out = new DataView(content.buffer);
    const presence = new Uint8Array(Math.ceil(rows.length / 8)); const validity = presence.slice();
    const rawValues: number[] = [];
    for (let r = 0; r < rows.length; r++) {
      const value = values[r]![spec.name];
      if (value !== undefined) presence[r >> 3]! |= 1 << (r & 7);
      if (value !== undefined && value !== null) validity[r >> 3]! |= 1 << (r & 7);
      if (spec.type === 'bytes') {
        out.setUint32(r * 4, rawValues.length, true); rawValues.push(...value as Uint8Array);
      } else if (value !== undefined && value !== null) {
        if (spec.type === 'u64') out.setBigUint64(r * 8, value as bigint, true);
        else if (spec.type === 'u8') out.setUint8(r, value as number);
        else out.setUint32(r * 4, spec.type === 'dict' ? intern(value as string) : value as number, true);
      }
    }
    if (spec.type === 'bytes') out.setUint32(rows.length * 4, rawValues.length, true);
    u16(dir, spec.id); d.setUint8(dir + 2, TYPES[spec.type]); d.setUint8(dir + 3, spec.flags);
    u32(dir + 4, put(content, width)); u32(dir + 8, content.length);
    if (spec.flags & 2 && rows.length) u32(dir + 12, put(validity));
    if (spec.flags & 1 && rows.length) u32(dir + 16, put(presence));
    if (spec.type === 'bytes') { u32(dir + 20, put(new Uint8Array(rawValues))); u32(dir + 24, rawValues.length); }
  }
  bytes.set(utf8.encode(MAGIC)); u16(8, 1); u16(10, 160); u32(12, cursor);
  bytes.set(Buffer.from(SCHEMA_HASH, 'hex'), 16); u32(48, sub);
  u64(56, 17n); u64(64, BigInt(fixture.epoch ?? rows[0]?.epoch ?? '0')); u64(72, first); u64(80, BigInt(rows.at(-1)?.seq ?? '0'));
  u64(88, BigInt(fixture.hub ?? '0')); u64(96, BigInt(fixture.daemon ?? '0'));
  u32(104, rows.length); u16(108, COLUMNS.length);
  u16(110, (fixture.delta ? 1 : 0) | (fixture.hub !== undefined ? 2 : 0) | (fixture.daemon !== undefined ? 4 : 0) | (fixture.displayGap === null ? 8 : 0));
  u32(112, 160); u32(116, dictionaryOffset); u32(120, dict.length); u32(124, gapOffset); u32(128, gapLength); u32(132, health); d.setFloat64(136, 2.5, true);
  return buffer.slice(0, cursor);
}
export function event(seq: string, payload: Record<string, unknown>, overrides: Partial<LoomEvent> = {}): LoomEvent {
  return { schema_version: 1, session_id: 'session-α', domain: 'can', kind: 'can.frame', producer_id: 'pcan:ch0', producer_epoch: '3', source_seq: seq, epoch: '9', seq,
    ts: { clock_id: 'host', mono_ns: '9007199254740993', wall_ns: '123', source: 'hardware' }, origin: 'device', payload, ...overrides };
}
export function canPayload(length: number, options: Record<string, unknown> = {}): Record<string, unknown> {
  return { id: 0x123, ext: false, fd: length > 8, brs: length > 8, esi: false, rtr: false,
    dlc: [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64].indexOf(length), len: length,
    bytes: { encoding: 'hex', data: Buffer.from(Array.from({ length }, (_, i) => i)).toString('hex') }, ...options };
}
const max = '18446744073709551615';
export const fixtures: Fixture[] = [
  { name: 'empty', rows: [], displayGap: null, recordHealth: null },
  { name: 'can-dlc', delta: true, hub: max, daemon: '0', recordHealth: 'degraded', displayGap: { count: '3', causes: ['display_queue_full'], seq_range: { from: '2', to: '4' } }, rows: [
    event('5', canPayload(0), { channel: null, client_id: null, request_id: null, dir: null, quality: [] }),
    event('8', canPayload(3, { bytes: { encoding: 'hex', data: 'aAbBcC', ref: null }, device_ts: { value_ns: max, source: 'hardware', clock_id: 'pcan' }, frame_index: 0 }), { channel: 'ch0', client_id: 'agent:1', dir: 'tx', origin: 'agent', request_id: 'req-α', quality: ['timestamp_estimated'], env: 'bench', risk: 'R1', generation: 0xffff_ffff, actor_id: 'run:1', external_actor: null, original_origin: 'script', purpose: 'fixture', test_session: 'test-1', test_step: 'step-1', job_id: null, metrics: [{ name: 'latency', value: 1.5, unit: null }] }),
    event('9', canPayload(8, { rtr: true, bytes: { encoding: 'hex', data: '' } }), { origin: 'user', dir: 'rx' }),
    event('10', canPayload(12)), event('11', canPayload(64, { id: 0x1fff_ffff, ext: true, esi: true })),
  ] },
  { name: 'mixed-max', delta: true, epoch: max, displayGap: { count: 'unknown', causes: [], seq_range: null }, recordHealth: 'ok', rows: [
    event('18446744073709551614', canPayload(1), { epoch: max, producer_epoch: max, source_seq: max, ts: { clock_id: 'device', mono_ns: max, wall_ns: max, source: 'device', quality: null } }),
    event(max, { bytes: { encoding: 'base64', data: 'AP+A', ref: null }, custom: { nested: [null, true, '한글'] } }, { domain: 'uart', kind: 'uart.data', epoch: max, producer_id: 'serial:dut', origin: 'script', channel: 'dut', client_id: 'web:1', request_id: 'uart-1', dir: 'tx', ts: { clock_id: 'host', mono_ns: '0', wall_ns: max, source: 'host_read_complete', quality: 'coarse' }, quality: ['driver_overrun'], generation: null }),
  ] },
  { name: 'absolute-uart', rows: [event(max, { bytes: { encoding: 'base64', data: '' } }, { domain: 'uart', kind: 'uart.data' })] },
];
