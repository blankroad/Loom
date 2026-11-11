import { describe, expect, it } from 'vitest';
import { fmtId, parseSendLine } from './sendLine';

const ok = (s: string) => {
  const r = parseSendLine(s, 'ch0');
  if (!r.ok) throw new Error(`expected ok for "${s}": ${r.error}`);
  return r;
};
const err = (s: string) => {
  const r = parseSendLine(s, 'ch0');
  if (r.ok) throw new Error(`expected error for "${s}"`);
  return r.error;
};

describe('CAN send line', () => {
  it('11-bit classic frames', () => {
    expect(ok('0x300#00').params).toEqual({ channel: 'ch0', id: 0x300, ext: false, fd: false, data: '00' });
    expect(ok('300#01.02.03').params.data).toBe('010203');
    expect(ok('7FF#').params).toMatchObject({ id: 0x7ff, ext: false, data: '' });
    expect(ok('0x300#00').canonical).toBe('0x300#00');
  });

  it('29-bit: 8 digits, > 0x7FF, or x marker', () => {
    expect(ok('18DAF110#02 10 03').params).toMatchObject({ id: 0x18daf110, ext: true, data: '021003' });
    expect(ok('0x800#00').params.ext).toBe(true);
    expect(ok('00000123#00').params).toMatchObject({ id: 0x123, ext: true });
    expect(ok('x123#00').params).toMatchObject({ id: 0x123, ext: true });
    expect(ok('123x#00').params).toMatchObject({ id: 0x123, ext: true });
    expect(err('0x20000000#00')).toMatch(/29bit ID 범위/);
  });

  it('DLC 0–8 for classic CAN', () => {
    expect(ok('0x100#0102030405060708').params.data).toHaveLength(16);
    expect(err('0x100#010203040506070809')).toMatch(/0–8바이트/);
  });

  it('FD forms and the FD length set', () => {
    const twelve = '00'.repeat(12);
    expect(ok(`fd 0x123#${twelve}`).params).toMatchObject({ fd: true, id: 0x123 });
    expect(ok(`fd brs 0x123#${'11'.repeat(64)}`).params).toMatchObject({ fd: true, brs: true });
    expect(ok(`123##1${twelve}`).params).toMatchObject({ fd: true, brs: true });
    expect(ok(`123##0${'aa'.repeat(8)}`).params).toMatchObject({ fd: true });
    // FD: brs is always explicit (daemon default would be true); Classic: absent (E_USAGE otherwise)
    expect(ok(`123##0${'aa'.repeat(8)}`).params.brs).toBe(false);
    expect(ok(`fd 0x123#${twelve}`).params.brs).toBe(false);
    expect('brs' in ok('0x300#00').params).toBe(false);
    for (const n of [9, 10, 11, 13, 33, 65]) expect(err(`fd 0x123#${'00'.repeat(n)}`)).toMatch(/허용되지 않음|0–8/);
    expect(err('brs 0x123#00')).toMatch(/BRS/);
  });

  it('rejects malformed input without guessing', () => {
    expect(err('')).toMatch(/입력 없음/);
    expect(err('0x300')).toMatch(/#/);
    expect(err('0xG00#00')).toMatch(/16진수/);
    expect(err('0x300#0')).toMatch(/홀수/);
    expect(err('0x300#zz')).toMatch(/16진수가 아닌/);
    expect(err('0x300#R')).toMatch(/RTR/);
    expect(err(`0x300#${'0'.repeat(300)}`)).toMatch(/너무 깁니다/);
  });

  it('formats ids like the Trace column', () => {
    expect(fmtId(0x300, false)).toBe('0x300');
    expect(fmtId(0x5, false)).toBe('0x005');
    expect(fmtId(0x18daf110, true)).toBe('18DAF110x');
  });
});
