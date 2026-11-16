// U5 batch 1 — contract tests for scrollSearch.ts (FUNCTIONS.md §B1.1). Opus-owned specification; Sonnet must not edit.
import { describe, expect, it } from 'vitest';
import {
  cellColumn,
  compileQuery,
  DEFAULT_SEARCH_OPTIONS,
  logicalLines,
  MAX_HITS,
  MAX_LOGICAL_ROWS,
  MAX_QUERY_CHARS,
  searchBuffer,
  searchCountText,
  stepHit,
  type BufferLike,
  type CellLike,
  type LineLike,
  type SearchResult,
} from './scrollSearch';

// ---- fake xterm buffer: one cell per char, East-Asian wide chars take 2 cells (char + width-0 placeholder) ----
const WIDE = /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿＀-｠]/;
type RowSpec = string | { t: string; w?: boolean };
function fakeLine(cols: number, spec: RowSpec): LineLike {
  const t = typeof spec === 'string' ? spec : spec.t;
  const cells: CellLike[] = [];
  for (const ch of t) {
    if (WIDE.test(ch)) {
      cells.push({ getChars: () => ch, getWidth: () => 2 }, { getChars: () => '', getWidth: () => 0 });
    } else cells.push({ getChars: () => ch, getWidth: () => 1 });
  }
  while (cells.length < cols) cells.push({ getChars: () => '', getWidth: () => 1 });
  if (cells.length > cols) throw new Error(`fixture row wider than ${cols}: ${t}`);
  return {
    isWrapped: typeof spec !== 'string' && !!spec.w,
    length: cols,
    getCell: (x) => cells[x],
    translateToString(trimRight = false) {
      let s = '';
      for (const c of cells) if (c.getWidth() > 0) s += c.getChars() || ' ';
      return trimRight ? s.replace(/ +$/, '') : s;
    },
  };
}
function fakeBuffer(cols: number, rows: RowSpec[], missing: number[] = []): BufferLike {
  const lines = rows.map((r) => fakeLine(cols, r));
  return { length: lines.length, getLine: (y) => (missing.includes(y) ? undefined : lines[y]) };
}
const opts = (o: Partial<typeof DEFAULT_SEARCH_OPTIONS> = {}) => ({ ...DEFAULT_SEARCH_OPTIONS, ...o });
const ok = (q: string, o = opts()) => {
  const c = compileQuery(q, o);
  if (c.kind !== 'ok') throw new Error(`expected ok, got ${JSON.stringify(c)}`);
  return c;
};

describe('compileQuery', () => {
  it('empty text is not a search', () => {
    expect(compileQuery('', opts())).toEqual({ kind: 'empty' });
  });
  it('literal mode escapes regex metacharacters', () => {
    const c = ok('a.b(1)');
    expect('xa.b(1)y'.search(c.re)).toBe(1);
    c.re.lastIndex = 0;
    expect(c.re.test('axb1')).toBe(false);
  });
  it('is case-insensitive by default and case-sensitive with Aa', () => {
    expect(ok('panic').re.flags).toContain('i');
    expect(ok('panic').re.flags).toContain('g');
    expect(ok('panic', opts({ caseSensitive: true })).re.flags).not.toContain('i');
  });
  it('regex mode compiles user patterns and reports syntax errors as data', () => {
    expect(ok('err(or)?\\b', opts({ regex: true })).re.source).toBe('err(or)?\\b');
    const bad = compileQuery('(', opts({ regex: true }));
    expect(bad.kind).toBe('error');
    expect(bad.kind === 'error' && bad.message.startsWith('정규식 오류')).toBe(true);
    // the same text is fine as a literal
    expect(compileQuery('(', opts()).kind).toBe('ok');
  });
  it('refuses queries longer than MAX_QUERY_CHARS', () => {
    expect(compileQuery('a'.repeat(MAX_QUERY_CHARS), opts()).kind).toBe('ok');
    const long = compileQuery('a'.repeat(MAX_QUERY_CHARS + 1), opts());
    expect(long.kind).toBe('error');
    expect(long.kind === 'error' && long.message).toContain(String(MAX_QUERY_CHARS));
  });
});

describe('logicalLines', () => {
  it('joins wrapped rows, keeping mid-chain spaces and trimming the end of the logical line', () => {
    const b = fakeBuffer(6, ['hello', { t: 'ab cd ', w: true }, { t: 'ef', w: true }, 'next  ']);
    expect([...logicalLines(b)]).toEqual([
      { startRow: 0, rowStarts: [0, 6, 12], text: 'hello ab cd ef' },
      { startRow: 3, rowStarts: [0], text: 'next' },
    ]);
  });
  it('a wrapped first row (its start scrolled out) still starts a logical line', () => {
    const b = fakeBuffer(4, [{ t: 'tail', w: true }, { t: 'more', w: true }, 'x']);
    expect([...logicalLines(b)].map((l) => [l.startRow, l.text])).toEqual([[0, 'tailmore'], [2, 'x']]);
  });
  it('splits chains longer than MAX_LOGICAL_ROWS rows', () => {
    const rows: RowSpec[] = ['aa', ...Array.from({ length: MAX_LOGICAL_ROWS + 43 }, () => ({ t: 'aa', w: true }))];
    const ls = [...logicalLines(fakeBuffer(2, rows))];
    expect(ls.map((l) => [l.startRow, l.rowStarts.length])).toEqual([[0, MAX_LOGICAL_ROWS], [MAX_LOGICAL_ROWS, 44]]);
  });
  it('a missing row reads as an empty unwrapped row', () => {
    const ls = [...logicalLines(fakeBuffer(3, ['abc', 'def', 'ghi'], [1]))];
    expect(ls.map((l) => l.text)).toEqual(['abc', '', 'ghi']);
  });
});

describe('cellColumn', () => {
  it('maps string indexes to cell columns across wide characters', () => {
    const line = fakeLine(6, '가나ab');
    expect([0, 1, 2, 3].map((i) => cellColumn(line, i))).toEqual([0, 2, 4, 5]);
  });
  it('an index past the text maps to the line length', () => {
    expect(cellColumn(fakeLine(4, 'ab'), 10)).toBe(4);
  });
});

describe('searchBuffer', () => {
  it('finds a match that crosses a wrap boundary (old per-row indexOf missed it)', () => {
    const b = fakeBuffer(5, ['xxxPA', { t: 'NIC', w: true }, 'ok']);
    const r = searchBuffer(b, 5, ok('panic'));
    expect(r).toEqual({ hits: [{ row: 0, col: 3, cells: 5 }], truncated: false, error: null, rows: 3 });
  });
  it('returns hits oldest first, case-insensitive by default', () => {
    const b = fakeBuffer(8, ['err1', 'ok', 'ERR2 err', 'fine']);
    expect(searchBuffer(b, 8, ok('err')).hits).toEqual([
      { row: 0, col: 0, cells: 3 },
      { row: 2, col: 0, cells: 3 },
      { row: 2, col: 5, cells: 3 },
    ]);
    expect(searchBuffer(b, 8, ok('err', opts({ caseSensitive: true }))).hits.map((h) => [h.row, h.col])).toEqual([[0, 0], [2, 5]]);
  });
  it('skips zero-length regex matches without looping', () => {
    expect(searchBuffer(fakeBuffer(4, ['ab']), 4, ok('x*', opts({ regex: true }))).hits).toEqual([]);
    expect(searchBuffer(fakeBuffer(4, ['axxb']), 4, ok('x*', opts({ regex: true }))).hits).toEqual([{ row: 0, col: 1, cells: 2 }]);
  });
  it('counts cells, not UTF-16 units, for wide characters', () => {
    expect(searchBuffer(fakeBuffer(6, ['가나ab']), 6, ok('나a')).hits).toEqual([{ row: 0, col: 2, cells: 3 }]);
  });
  it('stops at MAX_HITS and says so', () => {
    const rows = Array.from({ length: Math.ceil(MAX_HITS / 5) + 1 }, () => 'aaaaa');
    const r = searchBuffer(fakeBuffer(5, rows), 5, ok('a'));
    expect(r.hits).toHaveLength(MAX_HITS);
    expect(r.truncated).toBe(true);
  });
  it('passes compile errors through and treats an empty query as no search', () => {
    const b = fakeBuffer(4, ['(((']);
    const bad = compileQuery('(', opts({ regex: true }));
    const r = searchBuffer(b, 4, bad);
    expect(r.hits).toEqual([]);
    expect(r.error).toBe(bad.kind === 'error' ? bad.message : 'x');
    expect(searchBuffer(b, 4, { kind: 'empty' })).toEqual({ hits: [], truncated: false, error: null, rows: 1 });
  });
});

describe('stepHit / searchCountText', () => {
  it('wraps in both directions and starts at the first or last hit', () => {
    expect(stepHit(-1, 3, 1)).toBe(0);
    expect(stepHit(-1, 3, -1)).toBe(2);
    expect(stepHit(2, 3, 1)).toBe(0);
    expect(stepHit(0, 3, -1)).toBe(2);
    expect(stepHit(1, 0, 1)).toBe(-1);
  });
  it('states position, total, truncation and errors', () => {
    const res = (n: number, truncated = false): SearchResult => ({ hits: Array.from({ length: n }, (_, i) => ({ row: i, col: 0, cells: 1 })), truncated, error: null, rows: 10 });
    const q = ok('a');
    expect(searchCountText({ kind: 'empty' }, res(0), -1)).toBe('');
    expect(searchCountText({ kind: 'error', message: '정규식 오류: x' }, { ...res(0), error: '정규식 오류: x' }, -1)).toBe('정규식 오류: x');
    expect(searchCountText(q, res(0), -1)).toBe('0건');
    expect(searchCountText(q, res(3), -1)).toBe('3건');
    expect(searchCountText(q, res(3), 1)).toBe('2/3');
    expect(searchCountText(q, res(MAX_HITS, true), 0)).toBe(`1/${MAX_HITS}+`);
  });
});
