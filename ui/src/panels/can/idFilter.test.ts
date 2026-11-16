// B2 spec (Opus): CAN-8 id filter ranges. FUNCTIONS.md §B2-IDF.
import { describe, expect, it } from 'vitest';
import { CAN_ID_MAX, IdFilter, fmtFilterId, parseIdFilter } from './idFilter';

describe('fmtFilterId', () => {
  it('standard ids as 0x%03X, extended as 0x%08X, upper case', () => {
    expect(fmtFilterId(0)).toBe('0x000');
    expect(fmtFilterId(0x7e0)).toBe('0x7E0');
    expect(fmtFilterId(0x7ff)).toBe('0x7FF');
    expect(fmtFilterId(0x800)).toBe('0x00000800');
    expect(fmtFilterId(0x18daf110)).toBe('0x18DAF110');
  });
});

describe('parseIdFilter', () => {
  it('empty or blank → null (all ids)', () => {
    expect(parseIdFilter('')).toBeNull();
    expect(parseIdFilter('  ,, \t')).toBeNull();
  });

  it('keeps the pre-range single-id syntax: 0x prefix, bare hex, x suffix, commas/spaces', () => {
    const f = parseIdFilter('0x300, 301 18DAF110x,0X7df')!;
    expect(f).toBeInstanceOf(IdFilter);
    for (const id of [0x300, 0x301, 0x18daf110, 0x7df]) expect(f.has(id)).toBe(true);
    expect(f.has(0x302)).toBe(false);
    expect(f.singles).toEqual([0x300, 0x301, 0x7df, 0x18daf110]);
    expect(f.ranges).toEqual([]);
    expect(f.text).toBe('0x300, 0x301, 0x7DF, 0x18DAF110');
  });

  it('bare digits are hex in the UI (CLI reads them as decimal — the text shows what was applied)', () => {
    const f = parseIdFilter('768')!;
    expect(f.has(0x768)).toBe(true);
    expect(f.has(768)).toBe(false);
    expect(f.text).toBe('0x768');
  });

  it('inclusive ranges, any prefix form on either end', () => {
    const f = parseIdFilter('0x7E0-0x7EF')!;
    expect(f.has(0x7df)).toBe(false);
    expect(f.has(0x7e0)).toBe(true);
    expect(f.has(0x7e8)).toBe(true);
    expect(f.has(0x7ef)).toBe(true);
    expect(f.has(0x7f0)).toBe(false);
    expect(f.ranges).toEqual([{ lo: 0x7e0, hi: 0x7ef }]);
    expect(f.text).toBe('0x7E0-0x7EF');
    expect(parseIdFilter('7e0-7efx')!.text).toBe('0x7E0-0x7EF');
    expect(parseIdFilter('18DA0000x-18DAFFFFx')!.has(0x18daf110)).toBe(true);
  });

  it('normalises: overlapping/adjacent ranges merge, covered singles drop, output sorted', () => {
    const f = parseIdFilter('0x7F0-0x7FF 0x100 0x7E0-0x7EF 0x7E5 0x7E8-0x7F2 0x050')!;
    expect(f.ranges).toEqual([{ lo: 0x7e0, hi: 0x7ff }]);
    expect(f.singles).toEqual([0x050, 0x100]);
    expect(f.text).toBe('0x050, 0x100, 0x7E0-0x7FF');
  });

  it('a one-id range is a single; duplicates collapse', () => {
    const f = parseIdFilter('0x300-0x300 300 0x300')!;
    expect(f.singles).toEqual([0x300]);
    expect(f.ranges).toEqual([]);
    expect(f.text).toBe('0x300');
  });

  it('rejects bad tokens with the token in the message', () => {
    expect(() => parseIdFilter('0xZZ')).toThrow('id "0xZZ"');
    expect(() => parseIdFilter('0x300 - 0x3FF')).toThrow('id "-"');
    expect(() => parseIdFilter('0x300-')).toThrow('id "0x300-"');
    expect(() => parseIdFilter('-0x300')).toThrow('id "-0x300"');
    expect(() => parseIdFilter('1-2-3')).toThrow('id "1-2-3"');
    expect(() => parseIdFilter('123456789')).toThrow('id "123456789"'); // 9 hex digits
  });

  it('rejects ids above 29 bits and reversed ranges', () => {
    expect(CAN_ID_MAX).toBe(0x1fffffff);
    expect(parseIdFilter('1FFFFFFF')!.has(0x1fffffff)).toBe(true);
    expect(() => parseIdFilter('20000000')).toThrow('id "20000000" > 0x1FFFFFFF');
    expect(() => parseIdFilter('0x100-0x20000000')).toThrow('id "0x100-0x20000000" > 0x1FFFFFFF');
    expect(() => parseIdFilter('0x3FF-0x300')).toThrow('범위 "0x3FF-0x300": 시작 > 끝');
  });

  it('bounds: ≤ 256 distinct single ids, ≤ 64 range tokens', () => {
    const ids = (n: number) => Array.from({ length: n }, (_, i) => (0x100 + i).toString(16)).join(',');
    expect(parseIdFilter(ids(256))!.singles.length).toBe(256);
    expect(() => parseIdFilter(ids(257))).toThrow('id ≤ 256개');
    expect(parseIdFilter(`${ids(256)},100`)!.singles.length).toBe(256); // duplicate is not a new id
    const ranges = (n: number) => Array.from({ length: n }, (_, i) => `${(i * 16).toString(16)}-${(i * 16 + 7).toString(16)}`).join(' ');
    expect(parseIdFilter(ranges(64))!.ranges.length).toBe(64);
    expect(() => parseIdFilter(ranges(65))).toThrow('범위 ≤ 64개');
  });
});

describe('IdFilter', () => {
  it('constructor accepts any order and normalises the same way', () => {
    const f = new IdFilter([0x7e5, 0x10], [{ lo: 0x7e8, hi: 0x7ef }, { lo: 0x7e0, hi: 0x7e7 }]);
    expect(f.ranges).toEqual([{ lo: 0x7e0, hi: 0x7ef }]);
    expect(f.singles).toEqual([0x10]);
    expect(f.text).toBe('0x010, 0x7E0-0x7EF');
    expect(f.has(0x10)).toBe(true);
    expect(f.has(0x11)).toBe(false);
  });

  it('a ReadonlySet<number> satisfies the same IdMatch shape', () => {
    const s: { has(id: number): boolean } = new Set([1]);
    expect(s.has(1)).toBe(true);
  });
});
