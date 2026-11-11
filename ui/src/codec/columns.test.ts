/// <reference types="node" />

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { Buffer } from 'node:buffer';
import { describe, expect, it } from 'vitest';
import { decode } from './columns';
import { COLUMNS, LAYOUT_IDENTITY, MAX_EXPANDED, SCHEMA_HASH } from './layout';
import { canPayload, encode, event, fixtures } from './columns.test-support';
import type { Fixture } from './columns.test-support';

function mustDecode(bytes: ArrayBuffer) {
  const result = decode(bytes); if (!result.ok) throw new Error(JSON.stringify(result.error)); return result.value;
}
function mutate(fixture: Fixture, change: (data: DataView, bytes: Uint8Array) => void) {
  const buffer = encode(fixture); change(new DataView(buffer), new Uint8Array(buffer)); return buffer;
}
function column(data: DataView, name: string) {
  const index = COLUMNS.findIndex(item => item.name === name); return 160 + index * 32;
}
function dataOffset(data: DataView, name: string) { return data.getUint32(column(data, name) + 4, true); }
function rejected(buffer: ArrayBuffer, code?: string) {
  const result = decode(buffer); expect(result.ok).toBe(false);
  if (!result.ok && code) expect(result.error.code).toBe(code);
}
describe('loom.columns.v1 proposal', () => {
  it('rejects JSON timestamp sources outside the fixed proposal enum', () => {
    for (const source of ['pcap_host', 'pcap_adapter', 'replay', 'sim'] as const) {
      const row = event('1', canPayload(1), { ts: { clock_id: 'host', mono_ns: '1', wall_ns: '1', source } });
      expect(() => encode({ name: source, rows: [row] })).toThrow(`Timestamp source outside columns proposal: ${source}`);
    }
  });
  it('pins the declared schema identity SHA-256', () => {
    expect(createHash('sha256').update(JSON.stringify(LAYOUT_IDENTITY)).digest('hex')).toBe(SCHEMA_HASH);
  });
  for (const fixture of fixtures) {
    it(`golden bytes and D3 JSON roundtrip: ${fixture.name}`, () => {
      const hex = readFileSync(new URL(`./golden/${fixture.name}.hex`, import.meta.url), 'utf8').trim();
      const expected = JSON.parse(readFileSync(new URL(`./golden/${fixture.name}.json`, import.meta.url), 'utf8')) as Fixture;
      expect(Buffer.from(encode(fixture)).toString('hex')).toBe(hex);
      const view = mustDecode(Uint8Array.from(Buffer.from(hex, 'hex')).buffer);
      expect(view.materializeAll()).toEqual({ ok: true, value: expected.rows });
      expect(view.header.displayGap).toEqual(expected.displayGap);
      expect(view.header.recordHealth).toEqual(expected.recordHealth);
      expect(view.header.hubPublishMonoNs?.toString()).toEqual(expected.hub);
      expect(view.header.daemonWallNs?.toString()).toEqual(expected.daemon);
      expect(view.header.count).toBe(expected.rows.length);
    });
  }
  it('reads columns without materializing rows and guards indexes', () => {
    const view = mustDecode(encode(fixtures[2]!));
    expect(view.get('seq', 1)).toEqual({ ok: true, value: 18446744073709551615n });
    expect(view.get('raw', 1)).toEqual({ ok: true, value: new Uint8Array([0, 255, 128]) });
    expect(view.get('channel', 0)).toEqual({ ok: true, value: undefined });
    expect(view.get('source_seq', 0)).toEqual({ ok: true, value: 18446744073709551615n });
    for (const index of [-1, 2, 0.5, NaN]) { expect(view.get('seq', index).ok).toBe(false); expect(view.materialize(index).ok).toBe(false); }
  });
  it('returns independently owned materialized metadata', () => {
    const view = mustDecode(encode(fixtures[1]!)); const first = view.materialize(1);
    if (!first.ok) throw new Error(first.error.message);
    first.value.quality!.push('changed'); first.value.ts.quality = 'changed';
    (first.value.payload.device_ts as Record<string, unknown>).clock_id = 'changed';
    expect(view.materialize(1)).toEqual({ ok: true, value: fixtures[1]!.rows[1] });
  });
  it('covers every DLC code and UART hex/base64 byte tails', () => {
    const lengths = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];
    const rows = lengths.map((length, i) => event(String(i + 1), canPayload(length)));
    for (let length = 0; length < 8; length++) for (const encoding of ['hex', 'base64'] as const) {
      rows.push(event(String(rows.length + 1), { bytes: { encoding, data: Buffer.from(Array.from({ length }, (_, i) => 255 - i)).toString(encoding) } }, { domain: 'uart', kind: 'uart.data' }));
    }
    expect(mustDecode(encode({ name: 'all-dlc', rows })).materializeAll()).toEqual({ ok: true, value: rows });
  });
  it('permits omitted optional columns without losing absence', () => {
    const f = fixtures[3]!; const buffer = encode(f); const bytes = new Uint8Array(buffer); const d = new DataView(buffer);
    const entry = column(d, 'request_id'); const end = 160 + COLUMNS.length * 32;
    bytes.copyWithin(entry, entry + 32, end); bytes.fill(0, end - 32, end);
    d.setUint16(108, COLUMNS.length - 1, true);
    expect(mustDecode(buffer).materializeAll()).toEqual({ ok: true, value: f.rows });
  });
  it('copies extension keys safely, including own __proto__', () => {
    const extension = JSON.parse('{"__proto__":{"polluted":true},"extension":{"a":[1,null]}}') as Record<string, unknown>;
    const row = event('1', canPayload(1, extension));
    const result = mustDecode(encode({ name: 'extension', rows: [row] })).materializeAll();
    expect(result).toEqual({ ok: true, value: [row] });
    expect(Object.prototype).not.toHaveProperty('polluted');
  });
  it('rejects unknown version/hash, flags, frame/count limits and detached input', () => {
    const f = fixtures[1]!;
    rejected(mutate(f, d => d.setUint16(8, 2, true)), 'version');
    rejected(mutate(f, (_, b) => { b[16]! ^= 1; }), 'schema');
    rejected(mutate(f, d => d.setUint16(110, 0x8000, true)), 'header');
    rejected(mutate(f, d => d.setUint32(104, 201, true)), 'limit');
    rejected(mutate(f, d => d.setUint32(12, 65536, true)), 'header');
    rejected(new ArrayBuffer(65537), 'bounds'); rejected(new ArrayBuffer(159), 'bounds');
    const detached = encode(f); structuredClone(detached, { transfer: [detached] }); rejected(detached, 'bounds');
  });
  it('rejects truncated, overlapping, misaligned and overflowing offsets', () => {
    const f = fixtures[1]!;
    rejected(mutate(f, d => d.setUint32(116, 0xffff_ffff, true)), 'bounds');
    rejected(mutate(f, d => d.setUint32(column(d, 'seq') + 4, dataOffset(d, 'seq') + 1, true)), 'bounds');
    rejected(mutate(f, d => d.setUint32(column(d, 'seq') + 8, 0xffff_ffff, true)), 'layout');
    rejected(mutate(f, d => d.setUint32(column(d, 'mono_ns') + 4, dataOffset(d, 'seq'), true)), 'layout');
    rejected(mutate(f, d => d.setUint32(dataOffset(d, 'raw') + 4, 0xffff_ffff, true)), 'bounds');
    rejected(mutate(f, d => d.setUint32(dataOffset(d, 'raw') + 12, 2, true)), 'bounds');
    const bytes = encode(f); rejected(bytes.slice(0, bytes.byteLength - 1), 'header');
  });
  it('rejects duplicate/unknown/missing columns and invalid bitmap state', () => {
    const f = fixtures[1]!;
    rejected(mutate(f, d => d.setUint16(192, 1, true)), 'layout');
    rejected(mutate(f, d => d.setUint16(160, 999, true)), 'layout');
    rejected(mutate(f, d => d.setUint16(108, 0, true)), 'layout');
    rejected(mutate(f, (d, b) => { b[d.getUint32(column(d, 'channel') + 12, true)]! |= 0x80; }), 'layout');
    rejected(mutate(f, (d, b) => { b[d.getUint32(column(d, 'channel') + 12, true)]! |= 4; }), 'layout');
    rejected(mutate(f, d => d.setUint32(dataOffset(d, 'channel'), 1, true)), 'layout');
  });
  it('rejects bad refs, UTF-8, enum, seq overflow/order and CAN semantic corruption', () => {
    const f = fixtures[1]!;
    rejected(mutate(f, d => d.setUint32(dataOffset(d, 'session_id'), 9999, true)), 'value');
    rejected(mutate(f, (d, b) => { b[d.getUint32(116, true) + 2] = 0xff; }), 'value');
    rejected(mutate(f, d => d.setUint8(dataOffset(d, 'origin'), 4)), 'value');
    rejected(mutate(f, d => d.setUint8(dataOffset(d, 'ts_source'), 6)), 'value');
    rejected(mutate(f, d => d.setBigUint64(dataOffset(d, 'seq') + 8, 0n, true)), 'value');
    rejected(mutate(fixtures[2]!, d => d.setBigUint64(dataOffset(d, 'seq') + 8, 2n, true)), 'value');
    rejected(mutate(f, d => d.setUint8(dataOffset(d, 'dlc') + 3, 8)), 'value');
    rejected(mutate(f, d => d.setUint32(dataOffset(d, 'can_id'), 0x800, true)), 'value');
    rejected(mutate(f, d => d.setUint8(dataOffset(d, 'can_flags') + 4, 31)), 'value');
  });
  it('rejects metadata collisions, unsafe numbers, oversized expansion, malformed gaps and source u64', () => {
    const base = event('1', canPayload(1));
    const invalids: Fixture[] = [
      { name: 'deep', rows: [event('1', canPayload(1, { custom: Array.from({ length: 20 }).reduce<unknown>(v => [v], 0) }))] },
      { name: 'number', rows: [event('1', canPayload(1, { custom: Number.MAX_SAFE_INTEGER + 1 }))] },
      { name: 'device-ts', rows: [event('1', canPayload(1, { device_ts: { value_ns: '18446744073709551616', clock_id: 'x', source: 'hardware' } }))] },
      { name: 'gap', rows: [base], displayGap: { count: '18446744073709551616' } },
      { name: 'gen', rows: [{ ...base, generation: 0x1_0000_0000 }] },
      { name: 'expansion', rows: Array.from({ length: 200 }, (_, i) => event(String(i + 1), canPayload(1, { note: 'x'.repeat(MAX_EXPANDED / 400) }))) },
    ];
    for (const f of invalids) rejected(encode(f));
    // Same-length dictionary edit injects an owned key into the metadata sidecar.
    const collision = encode({ name: 'collision', rows: [{ ...base, ts: { ...base.ts, value_ns: 'x' } as typeof base.ts }] });
    const offset = Buffer.from(collision).indexOf(Buffer.from('value_ns'));
    expect(offset).toBeGreaterThan(0);
    new Uint8Array(collision).set(new TextEncoder().encode('clock_id'), offset);
    rejected(collision, 'value');
  });
  it('accepts the 64 KiB boundary and rejects one extra byte', () => {
    const f = fixtures[3]!; const encoded = encode(f);
    const padded = new ArrayBuffer(65536); new Uint8Array(padded).set(new Uint8Array(encoded)); new DataView(padded).setUint32(12, 65536, true);
    expect(decode(padded).ok).toBe(true); rejected(new ArrayBuffer(65537), 'bounds');
  });
  it('seeded 10,000 random truncations / bit flips finish with an explicit result', () => {
    let seed = 0x51c0ffee;
    const random = () => { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return seed >>> 0; };
    const corpus = fixtures.map(encode); let failures = 0; let valid = 0;
    for (let n = 0; n < 10_000; n++) {
      const source = corpus[random() % corpus.length]!;
      const buffer = n % 2 ? source.slice(0, random() % source.byteLength) : source.slice(0);
      if (!(n % 2)) { const bytes = new Uint8Array(buffer); const flips = 1 + random() % 4; for (let k = 0; k < flips; k++) bytes[random() % bytes.length]! ^= 1 << (random() % 8); }
      const result = decode(buffer);
      if (result.ok) { valid++; expect(result.value.materializeAll().ok).toBe(true); }
      else { failures++; expect(result.error.message.length).toBeGreaterThan(0); }
    }
    expect(valid + failures).toBe(10_000); expect(failures).toBeGreaterThan(9000);
  }, 15_000);
});
