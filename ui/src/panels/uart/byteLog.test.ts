import { describe, expect, it } from 'vitest';
import { BYTES_PER_ROW, ByteLog, MAX_LOG_BYTES, MAX_LOG_ROWS, rowAscii, rowHex, rowsToHex } from './byteLog';

const bytesOf = (...b: number[]) => new Uint8Array(b);

describe('ByteLog (original-byte HEX view model)', () => {
  it('keeps the original bytes 16 per row, splitting on direction and origin', () => {
    const log = new ByteLog();
    const data = new Uint8Array(20).map((_, i) => i);
    log.append('rx', 'device', 1, data);
    log.append('tx', 'user', 2, bytesOf(0x6c, 0x73, 0x0d));
    log.append('tx', 'agent', 3, bytesOf(0x0d));
    expect(log.rows.map((r) => [r.dir, r.origin, r.len])).toEqual([
      ['rx', 'device', BYTES_PER_ROW],
      ['rx', 'device', 4],
      ['tx', 'user', 3],
      ['tx', 'agent', 1],
    ]);
    expect(log.bytes).toBe(24);
    expect(rowHex(log.rows[2]!, false)).toBe('6C 73 0D');
  });

  it('renders bytes that are not valid UTF-8 or are control codes exactly (not via symbolize)', () => {
    const log = new ByteLog();
    log.append('rx', 'device', 0, bytesOf(0x1b, 0x5b, 0xff, 0xc3, 0x28, 0x00, 0x41, 0x7f));
    expect(rowHex(log.rows[0]!, false)).toBe('1B 5B FF C3 28 00 41 7F');
    expect(rowAscii(log.rows[0]!)).toBe('.[..(.A.');
  });

  it('pads the hex column to a fixed width with a gap after 8 bytes', () => {
    const log = new ByteLog();
    log.append('rx', 'device', 0, new Uint8Array(16).fill(0xab));
    log.append('rx', 'device', 0, bytesOf(1));
    const full = rowHex(log.rows[0]!);
    expect(full).toBe('AB AB AB AB AB AB AB AB  AB AB AB AB AB AB AB AB');
    expect(rowHex(log.rows[1]!)).toHaveLength(full.length);
  });

  it('a gap ends the open row; bytes after it start a new row', () => {
    const log = new ByteLog();
    log.append('rx', 'device', 0, bytesOf(1, 2));
    log.gap('lost 3');
    log.append('rx', 'device', 0, bytesOf(3));
    expect(log.rows.map((r) => r.kind)).toEqual(['data', 'gap', 'data']);
    expect(log.rows[2]!.len).toBe(1);
  });

  it('bounds payload bytes and accounts evicted bytes exactly', () => {
    const log = new ByteLog();
    const chunk = new Uint8Array(4096).fill(0x55);
    const n = MAX_LOG_BYTES / chunk.length + 40;
    for (let i = 0; i < n; i++) log.append('rx', 'device', i, chunk);
    expect(log.bytes).toBeLessThanOrEqual(MAX_LOG_BYTES);
    expect(log.bytes + log.evictedBytes).toBe(n * chunk.length);
    expect(log.rows.reduce((a, r) => a + r.len, 0)).toBe(log.bytes);
  });

  it('regression: gap-only rows are capped too (byte cap alone would not bound them)', () => {
    const log = new ByteLog();
    log.append('rx', 'device', 0, bytesOf(0x41, 0x42));
    for (let i = 0; i < MAX_LOG_ROWS * 3; i++) log.gap(`gap ${i}`);
    expect(log.rows.length).toBeLessThanOrEqual(MAX_LOG_ROWS);
    expect(log.rows.at(-1)!.text).toBe(`gap ${MAX_LOG_ROWS * 3 - 1}`);
    // the evicted data row's payload is accounted; gap rows carry no payload
    expect(log.bytes).toBe(0);
    expect(log.evictedBytes).toBe(2);
    expect(log.evictedRows + log.rows.length).toBe(MAX_LOG_ROWS * 3 + 1);
  });

  it('alternating 1-byte RX/TX rows are bounded by the row cap', () => {
    const log = new ByteLog();
    for (let i = 0; i < MAX_LOG_ROWS * 2; i++) log.append(i % 2 ? 'tx' : 'rx', i % 2 ? 'user' : 'device', 0, bytesOf(i & 0xff));
    expect(log.rows.length).toBeLessThanOrEqual(MAX_LOG_ROWS);
    expect(log.bytes + log.evictedBytes).toBe(MAX_LOG_ROWS * 2);
    expect(log.rows.reduce((a, r) => a + r.len, 0)).toBe(log.bytes);
  });

  it('clear() empties the local view; clipboard text is space-separated hex', () => {
    const log = new ByteLog();
    log.append('rx', 'device', 0, new TextEncoder().encode('Hello World'));
    log.gap('x');
    expect(rowsToHex(log.rows)).toBe('48 65 6C 6C 6F 20 57 6F 72 6C 64');
    log.clear();
    expect([log.rows.length, log.bytes, log.evictedBytes, log.evictedRows]).toEqual([0, 0, 0, 0]);
  });
});
