// U5 (operator-upgrade/uart) — scrollback search over LOGICAL lines of the xterm buffer.
// Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B1.1 (Opus owns signatures/types/constants; bodies are
// Sonnet batch 1). Pure: reads a BufferLike (the xterm 6 IBuffer subset below), never touches the DOM or the device.
// Device text is untrusted data: it is only matched, never interpreted.

/** xterm 6 IBufferCell subset */
export interface CellLike {
  getChars(): string;
  getWidth(): number;
}
/** xterm 6 IBufferLine subset */
export interface LineLike {
  readonly isWrapped: boolean;
  readonly length: number;
  translateToString(trimRight?: boolean): string;
  getCell(x: number): CellLike | undefined;
}
/** xterm 6 IBuffer subset (`term.buffer.active`) */
export interface BufferLike {
  readonly length: number;
  getLine(y: number): LineLike | undefined;
}

export interface SearchOptions {
  /** Aa toggle; default false (case-insensitive) */
  caseSensitive: boolean;
  /** .* toggle; default false (literal text) */
  regex: boolean;
}
export const DEFAULT_SEARCH_OPTIONS: SearchOptions = { caseSensitive: false, regex: false };

export type CompiledQuery =
  | { kind: 'empty' }
  | { kind: 'error'; message: string }
  | { kind: 'ok'; re: RegExp };

/** longest accepted query (UTF-16 code units) */
export const MAX_QUERY_CHARS = 256;
/** a wrapped chain longer than this many rows is split into several logical lines (matches across the split are missed) */
export const MAX_LOGICAL_ROWS = 256;
/** most hits collected per search; more → `truncated` */
export const MAX_HITS = 10_000;

/** One logical line: buffer rows `startRow .. startRow + rowStarts.length - 1` joined. */
export interface LogicalLine {
  /** absolute buffer row of the first physical row */
  startRow: number;
  /** rowStarts[i] = offset in `text` where physical row startRow+i begins; rowStarts[0] === 0 */
  rowStarts: number[];
  text: string;
}

/** A match, in xterm selection coordinates: `term.select(col, row, cells)`. */
export interface SearchHit {
  /** absolute buffer row of the first matched cell */
  row: number;
  /** cell column of the first matched cell */
  col: number;
  /** matched cells, counted across wrapped rows (row-major with `cols` cells per row); ≥ 1 */
  cells: number;
}

export interface SearchResult {
  hits: SearchHit[];
  /** MAX_HITS reached; more matches exist */
  truncated: boolean;
  /** compile error message (query kind 'error'), else null */
  error: string | null;
  /** buffer rows searched (= buf.length) */
  rows: number;
}

/** Compile the search box text. See FUNCTIONS.md §B1.1 compileQuery. */
export function compileQuery(q: string, o: SearchOptions): CompiledQuery {
  if (q === '') return { kind: 'empty' };
  if (q.length > MAX_QUERY_CHARS) return { kind: 'error', message: `검색어가 너무 깁니다 (최대 ${MAX_QUERY_CHARS}자)` };
  const source = o.regex ? q : q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const flags = 'g' + (o.caseSensitive ? '' : 'i');
  try {
    return { kind: 'ok', re: new RegExp(source, flags) };
  } catch (err) {
    return { kind: 'error', message: `정규식 오류: ${err instanceof Error ? err.message : String(err)}` };
  }
}

/** Logical lines from row 0 to buf.length-1, lazily (the body may return a nested generator). See §B1.1 logicalLines. */
export function logicalLines(buf: BufferLike): Iterable<LogicalLine> {
  return (function* () {
    const n = buf.length;
    let y = 0;
    while (y < n) {
      const startRow = y;
      const rowStarts: number[] = [];
      let text = '';
      let rows = 0;
      for (;;) {
        const line = buf.getLine(y);
        const next = y + 1 < n ? buf.getLine(y + 1) : undefined;
        const nextJoins = next !== undefined && next.isWrapped && rows + 1 < MAX_LOGICAL_ROWS;
        const rowText = line === undefined ? '' : line.translateToString(!nextJoins);
        rowStarts.push(text.length);
        text += rowText;
        rows++;
        y++;
        if (!nextJoins) break;
      }
      yield { startRow, rowStarts, text };
    }
  })();
}

/** String index (into `line.translateToString(false)`) → cell column. See FUNCTIONS.md §B1.1 cellColumn. */
export function cellColumn(line: LineLike, index: number): number {
  let acc = 0;
  for (let x = 0; x < line.length; x++) {
    const cell = line.getCell(x);
    const w = cell?.getWidth() ?? 1;
    if (w === 0) continue;
    const chars = cell?.getChars() || ' ';
    if (index < acc + chars.length) return x;
    acc += chars.length;
  }
  return line.length;
}

/** Search the whole buffer. See FUNCTIONS.md §B1.1 searchBuffer. */
export function searchBuffer(buf: BufferLike, cols: number, q: CompiledQuery): SearchResult {
  if (q.kind !== 'ok') return { hits: [], truncated: false, error: q.kind === 'error' ? q.message : null, rows: buf.length };
  const re = q.re;
  const hits: SearchHit[] = [];
  const lastLE = (starts: number[], off: number): number => {
    let k = 0;
    for (let a = 0; a < starts.length; a++) {
      if (starts[a]! <= off) k = a;
      else break;
    }
    return k;
  };
  for (const ll of logicalLines(buf)) {
    const { text, rowStarts, startRow } = ll;
    re.lastIndex = 0;
    for (;;) {
      const m = re.exec(text);
      if (m === null) break;
      if (m[0].length === 0) {
        re.lastIndex = m.index + 1;
        if (re.lastIndex > text.length) break;
        continue;
      }
      const s = m.index;
      const e = m.index + m[0].length - 1;
      const i = lastLE(rowStarts, s);
      const j = lastLE(rowStarts, e);
      const row = startRow + i;
      const endRow = startRow + j;
      const startLine = buf.getLine(row);
      const endLine = buf.getLine(endRow);
      if (!startLine || !endLine) continue;
      const col = cellColumn(startLine, s - rowStarts[i]!);
      const endCol = cellColumn(endLine, e - rowStarts[j]!);
      const endW = Math.max(1, endLine.getCell(endCol)?.getWidth() ?? 1);
      hits.push({ row, col, cells: endRow * cols + endCol + endW - (row * cols + col) });
      if (hits.length === MAX_HITS) return { hits, truncated: true, error: null, rows: buf.length };
    }
  }
  return { hits, truncated: false, error: null, rows: buf.length };
}

/** Next/previous hit index with wrap-around. See FUNCTIONS.md §B1.1 stepHit. */
export function stepHit(current: number, count: number, dir: 1 | -1): number {
  if (count <= 0) return -1;
  if (current < 0) return dir > 0 ? 0 : count - 1;
  return (current + dir + count) % count;
}

/** Text of the search count cell. See FUNCTIONS.md §B1.1 searchCountText. */
export function searchCountText(q: CompiledQuery, r: SearchResult, at: number): string {
  if (q.kind === 'empty') return '';
  if (q.kind === 'error') return q.message;
  const n = r.hits.length;
  if (n === 0) return '0건';
  const plus = r.truncated ? '+' : '';
  return at < 0 ? `${n}건${plus}` : `${at + 1}/${n}${plus}`;
}
