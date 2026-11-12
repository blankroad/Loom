// UART byte view model (CoolTerm "Hex" view analogue): the ORIGINAL uart.data bytes,
// 16 per row, RX and TX kept on separate rows, gaps as separator rows.
// Never built from symbolized/decoded text. Bounded: MAX_LOG_BYTES of payload; whole rows
// are evicted in chunks and counted so the view can say what fell out of its window.
// Rows are capped too (MAX_LOG_ROWS): gap rows and short alternating RX/TX rows carry few or no
// payload bytes, so the byte cap alone would not bound memory.

export const BYTES_PER_ROW = 16;
export const MAX_LOG_BYTES = 256 * 1024;
export const MAX_LOG_ROWS = 20_000;
const EVICT_CHUNK_ROWS = 512;

export interface ByteRow {
  key: string;
  kind: 'data' | 'gap';
  dir: 'rx' | 'tx' | null;
  origin: string;
  /** wall clock of the first byte in the row (0 = unknown) */
  wallMs: number;
  /** BYTES_PER_ROW slots; only the first `len` are valid */
  bytes: Uint8Array;
  len: number;
  /** gap rows: server/panel gap description */
  text?: string;
}

const EMPTY = new Uint8Array(0);

export class ByteLog {
  readonly rows: ByteRow[] = [];
  /** payload bytes currently retained */
  bytes = 0;
  /** payload bytes evicted from this view's window since the last clear (data rows only) */
  evictedBytes = 0;
  /** rows (data + gap) evicted since the last clear */
  evictedRows = 0;
  private n = 0;
  private open: ByteRow | null = null;

  append(dir: 'rx' | 'tx', origin: string, wallMs: number, data: Uint8Array): void {
    for (let i = 0; i < data.length; i++) {
      let row = this.open;
      if (!row || row.dir !== dir || row.origin !== origin || row.len === BYTES_PER_ROW) {
        row = { key: String(++this.n), kind: 'data', dir, origin, wallMs, bytes: new Uint8Array(BYTES_PER_ROW), len: 0 };
        this.rows.push(row);
        this.open = row;
      }
      row.bytes[row.len++] = data[i] as number;
    }
    this.bytes += data.length;
    this.evict();
  }

  /** separator row; the next bytes never continue a row from before the gap */
  gap(text: string, wallMs = 0): void {
    this.open = null;
    this.rows.push({ key: String(++this.n), kind: 'gap', dir: null, origin: '', wallMs, bytes: EMPTY, len: 0, text });
    this.evict();
  }

  /** local view clear only (CoolTerm "Clear Data" on the screen buffer) */
  clear(): void {
    this.rows.length = 0;
    this.open = null;
    this.bytes = 0;
    this.evictedBytes = 0;
    this.evictedRows = 0;
  }

  private evict(): void {
    if (this.bytes <= MAX_LOG_BYTES && this.rows.length <= MAX_LOG_ROWS) return;
    let drop = 0;
    let freed = 0;
    // chunked: at least EVICT_CHUNK_ROWS at once so a flood does not shift the array per row
    while (
      drop < this.rows.length &&
      (this.bytes - freed > MAX_LOG_BYTES || this.rows.length - drop > MAX_LOG_ROWS || drop < EVICT_CHUNK_ROWS)
    ) {
      freed += (this.rows[drop] as ByteRow).len;
      drop++;
    }
    const removed = this.rows.splice(0, drop);
    if (this.open && removed.includes(this.open)) this.open = null;
    this.bytes -= freed;
    this.evictedBytes += freed;
    this.evictedRows += drop;
  }
}

const HEX = '0123456789ABCDEF';
const hx = (b: number) => (HEX[b >> 4] as string) + (HEX[b & 15] as string);

/** "54 68 69 73 20 69 73 20  61 20 74 65 73 74" — padded to a fixed width for alignment */
export function rowHex(r: ByteRow, pad = true): string {
  let out = '';
  for (let i = 0; i < BYTES_PER_ROW; i++) {
    if (i >= r.len && !pad) break;
    if (i > 0) out += i === 8 ? '  ' : ' ';
    out += i < r.len ? hx(r.bytes[i] as number) : '  ';
  }
  return pad ? out : out.trimEnd();
}

/** printable ASCII as-is, everything else '.' (CoolTerm hex view convention) */
export function rowAscii(r: ByteRow): string {
  let out = '';
  for (let i = 0; i < r.len; i++) {
    const b = r.bytes[i] as number;
    out += b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '.';
  }
  return out;
}

/** clipboard form: space-separated uppercase hex of data rows, one line per row */
export function rowsToHex(rows: readonly ByteRow[]): string {
  return rows
    .filter((r) => r.kind === 'data' && r.len > 0)
    .map((r) => rowHex(r, false).replace(/ {2}/g, ' '))
    .join('\n');
}
