import type { GapSummary, LoomEvent } from '../proto/provisional';
import {
  COLUMNS, DIRECTORY_BYTES, DIRS, HEADER_BYTES, MAGIC, MAX_EXPANDED, MAX_FRAME,
  MAX_ROWS, ORIGINS, SCHEMA_HASH, TS_SOURCES, TYPES, U64_MAX, VERSION,
} from './layout';
import type { ColumnName } from './layout';

export type ErrorCode = 'header' | 'version' | 'schema' | 'bounds' | 'layout' | 'limit' | 'value' | 'index';
export interface DecodeError { code: ErrorCode; message: string }
export type Result<T> = { ok: true; value: T } | { ok: false; error: DecodeError };
export interface Header {
  version: 1; schemaHash: string; frameBytes: number; count: number; sub: string;
  batchId: bigint; streamEpoch: bigint; firstSeq: bigint; lastSeq: bigint;
  hubPublishMonoNs?: bigint; daemonWallNs?: bigint; deltaSeq: boolean; lagMs: number;
  displayGap?: GapSummary | null;
  recordHealth?: 'ok' | 'degraded' | 'failed' | 'disabled' | null;
}
export type Cell = number | bigint | string | Uint8Array | null | undefined;
export interface ColumnsView {
  readonly header: Readonly<Header>;
  /** Borrowed read-only cells. Do not mutate the input buffer or returned raw bytes. */
  get(column: ColumnName, row: number): Result<Cell>;
  materialize(row: number): Result<LoomEvent>;
  materializeAll(): Result<LoomEvent[]>;
}
class Invalid extends Error {
  constructor(readonly code: ErrorCode, message: string) { super(message); }
}
function need(condition: unknown, code: ErrorCode, message: string): asserts condition {
  if (!condition) throw new Invalid(code, message);
}
function attempt<T>(operation: () => T): Result<T> {
  try { return { ok: true, value: operation() }; }
  catch (error) {
    return { ok: false, error: error instanceof Invalid
      ? { code: error.code, message: error.message }
      : { code: 'value', message: 'Invalid or detached buffer / malformed value' } };
  }
}
type ObjectValue = Record<string, unknown>;
function object(value: unknown): value is ObjectValue {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}
const utf8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
function parseJson(text: string): unknown {
  const value: unknown = JSON.parse(text);
  // Iterative bounded traversal: malformed deep JSON must not overflow the JS stack.
  const stack = [{ value, depth: 0 }];
  let nodes = 0;
  while (stack.length) {
    const item = stack.pop()!;
    need(++nodes <= 8192 && item.depth <= 16, 'limit', 'JSON complexity limit');
    if (typeof item.value === 'number') {
      need(Number.isFinite(item.value) && !Object.is(item.value, -0) && (!Number.isInteger(item.value) || Number.isSafeInteger(item.value)), 'value', 'Unsafe/noncanonical JSON number');
    } else if (item.value !== null && typeof item.value === 'object') {
      for (const child of Object.values(item.value)) stack.push({ value: child, depth: item.depth + 1 });
      need(stack.length <= 8192, 'limit', 'JSON node limit');
    }
  }
  return value;
}
function u64(value: unknown): value is string {
  return typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value) && BigInt(value) <= U64_MAX;
}
function isU32(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 0xffff_ffff;
}
function stringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(item => typeof item === 'string');
}
interface Extra { record: ObjectValue; ts: ObjectValue; payload: ObjectValue; bytes: ObjectValue }
const RECORD_OWNED = ['schema_version', 'session_id', 'domain', 'kind', 'producer_id', 'producer_epoch', 'source_seq', 'epoch', 'seq', 'ts', 'origin', 'payload', 'channel', 'client_id', 'dir', 'request_id', 'quality'];
const PAYLOAD_OWNED = ['bytes', 'id', 'ext', 'fd', 'brs', 'esi', 'rtr', 'dlc', 'len'];
function extra(value: unknown, can: boolean): Extra {
  need(object(value) && Object.keys(value).every(key => ['record', 'ts', 'payload', 'bytes'].includes(key)), 'value', 'Invalid extra envelope');
  const result: Extra = { record: {}, ts: {}, payload: {}, bytes: {} };
  for (const key of ['record', 'ts', 'payload', 'bytes'] as const) {
    const part = value[key];
    need(part === undefined || object(part), 'value', `Invalid extra.${key}`);
    result[key] = part ?? {};
  }
  const forbidden = { record: RECORD_OWNED, ts: ['clock_id', 'mono_ns', 'wall_ns', 'source'], payload: can ? PAYLOAD_OWNED : ['bytes'], bytes: ['encoding', 'data'] };
  for (const key of ['record', 'ts', 'payload', 'bytes'] as const) {
    need(Object.keys(result[key]).every(name => !forbidden[key].includes(name)), 'value', `Owned key in extra.${key}`);
  }
  for (const name of ['actor_id', 'external_actor', 'job_id', 'purpose', 'test_session', 'test_step']) {
    const item = result.record[name];
    need(item === undefined || item === null || typeof item === 'string', 'value', `Invalid ${name}`);
  }
  for (const [name, values] of [['env', ['sim', 'bench', 'vehicle']], ['risk', ['R0', 'R1', 'R2', 'R3', 'R4']], ['original_origin', ORIGINS]] as const) {
    const item = result.record[name];
    need(item === undefined || item === null || (typeof item === 'string' && (values as readonly string[]).includes(item)), 'value', `Invalid ${name}`);
  }
  const generation = result.record.generation;
  need(generation === undefined || generation === null || isU32(generation), 'value', 'Invalid generation');
  const quality = result.ts.quality;
  need(quality === undefined || quality === null || typeof quality === 'string', 'value', 'Invalid ts.quality');
  const metrics = result.record.metrics;
  need(metrics === undefined || (Array.isArray(metrics) && metrics.every(metric => object(metric) && typeof metric.name === 'string' && typeof metric.value === 'number' && (metric.unit === undefined || metric.unit === null || typeof metric.unit === 'string'))), 'value', 'Invalid metrics');
  const deviceTs = result.payload.device_ts;
  need(deviceTs === undefined || (object(deviceTs) && u64(deviceTs.value_ns) && ['hardware', 'kernel_sw', 'host'].includes(String(deviceTs.source)) && typeof deviceTs.clock_id === 'string'), 'value', 'Invalid device_ts');
  return result;
}
function gap(value: unknown): GapSummary {
  need(object(value) && (value.count === 'unknown' || u64(value.count)), 'value', 'Invalid display_gap count');
  need(value.causes === undefined || stringList(value.causes), 'value', 'Invalid display_gap causes');
  const range = value.seq_range;
  need(range === undefined || range === null || (object(range) && u64(range.from) && u64(range.to) && BigInt(range.from) <= BigInt(range.to)), 'value', 'Invalid display_gap range');
  return value as unknown as GapSummary;
}
/** Portable hex/base64 conversion; no Node Buffer dependency in the browser decoder. */
export function bytesText(bytes: Uint8Array, encoding: number): string {
  if (encoding === 0) return Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  let text = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i]!; const b = bytes[i + 1]; const c = bytes[i + 2];
    text += alphabet[a >> 2]! + alphabet[((a & 3) << 4) | ((b ?? 0) >> 4)]!
      + (b === undefined ? '=' : alphabet[((b & 15) << 2) | ((c ?? 0) >> 6)]!)
      + (c === undefined ? '=' : alphabet[c & 63]!);
  }
  return text;
}
interface Column {
  spec: typeof COLUMNS[number]; data: number; length: number; validity: number; presence: number; aux: number; auxLength: number;
}
/**
 * Fully validates before exposing column views. Frame <=64 KiB, <=200 rows, bounded
 * JSON and <=4 MiB logical expansion; reject overflow, never enqueue or truncate.
 * The caller transfers exclusive immutable ownership of buf for the view lifetime.
 */
export function decode(buf: ArrayBuffer): Result<ColumnsView> {
  return attempt(() => {
    need(buf instanceof ArrayBuffer && buf.byteLength >= HEADER_BYTES && buf.byteLength <= MAX_FRAME, 'bounds', 'Frame size');
    const data = new DataView(buf);
    const bytes = new Uint8Array(buf);
    need(Array.from(bytes.subarray(0, 8), n => String.fromCharCode(n)).join('') === MAGIC, 'header', 'Magic');
    need(data.getUint16(8, true) === VERSION, 'version', 'Unsupported version');
    const hash = Array.from(bytes.subarray(16, 48), n => n.toString(16).padStart(2, '0')).join('');
    need(hash === SCHEMA_HASH, 'schema', 'Schema hash mismatch');
    need(data.getUint16(10, true) === HEADER_BYTES && data.getUint32(12, true) === buf.byteLength, 'header', 'Header/frame length');
    const count = data.getUint32(104, true); const columnCount = data.getUint16(108, true); const flags = data.getUint16(110, true);
    need(count <= MAX_ROWS && columnCount <= COLUMNS.length, 'limit', 'Count limit');
    need((flags & ~15) === 0 && data.getUint32(52, true) === 0 && bytes.subarray(144, 160).every(n => n === 0), 'header', 'Reserved header bits');
    const directory = data.getUint32(112, true);
    need(directory === HEADER_BYTES, 'layout', 'Directory position');
    type Range = { start: number; end: number };
    const ranges: Range[] = [{ start: 0, end: HEADER_BYTES }];
    function region(offset: number, length: number, alignment: number, allowEmpty = false) {
      need(Number.isSafeInteger(length) && length >= 0 && offset % alignment === 0 && offset >= HEADER_BYTES && offset <= buf.byteLength && length <= buf.byteLength - offset, 'bounds', 'Region bounds/alignment');
      need(allowEmpty || length > 0, 'layout', 'Empty region');
      if (length) ranges.push({ start: offset, end: offset + length });
    }
    region(directory, columnCount * DIRECTORY_BYTES, 8, true);
    const dictionaryOffset = data.getUint32(116, true); const dictionaryLength = data.getUint32(120, true);
    region(dictionaryOffset, dictionaryLength, 1);
    const gapOffset = data.getUint32(124, true); const gapLength = data.getUint32(128, true);
    if (gapLength) region(gapOffset, gapLength, 1);
    else need(gapOffset === 0, 'layout', 'Gap offset without data');
    need(!(flags & 8) || gapLength === 0, 'layout', 'Null and nonnull gap');
    const columns = new Map<ColumnName, Column>();
    const bitmapLength = Math.ceil(count / 8);
    for (let index = 0; index < columnCount; index++) {
      const pos = directory + index * DIRECTORY_BYTES;
      need(pos + DIRECTORY_BYTES <= buf.byteLength, 'bounds', 'Truncated directory');
      const spec = COLUMNS.find(item => item.id === data.getUint16(pos, true));
      need(spec && !columns.has(spec.name) && data.getUint8(pos + 2) === TYPES[spec.type] && data.getUint8(pos + 3) === spec.flags, 'layout', 'Column id/type/flags');
      const col: Column = { spec, data: data.getUint32(pos + 4, true), length: data.getUint32(pos + 8, true), validity: data.getUint32(pos + 12, true), presence: data.getUint32(pos + 16, true), aux: data.getUint32(pos + 20, true), auxLength: data.getUint32(pos + 24, true) };
      need(data.getUint32(pos + 28, true) === 0, 'layout', 'Reserved column bits');
      const width = spec.type === 'u8' ? 1 : spec.type === 'u64' ? 8 : 4;
      need(col.length === (spec.type === 'bytes' ? count + 1 : count) * width, 'layout', 'Column length');
      region(col.data, col.length, width, true);
      if (spec.type === 'bytes') region(col.aux, col.auxLength, 1, true);
      else need(col.aux === 0 && col.auxLength === 0, 'layout', 'Unexpected auxiliary data');
      for (const [offset, enabled] of [[col.presence, spec.flags & 1], [col.validity, spec.flags & 2]]) {
        if (enabled && count) {
          region(offset!, bitmapLength, 1);
          if (count % 8) need((data.getUint8(offset! + bitmapLength - 1) >> (count % 8)) === 0, 'layout', 'Bitmap padding');
        } else need(offset === 0, 'layout', 'Unexpected bitmap');
      }
      columns.set(spec.name, col);
    }
    need(COLUMNS.every(spec => (spec.flags & 1) || columns.has(spec.name)), 'layout', 'Missing required column');
    ranges.sort((a, b) => a.start - b.start);
    let end = 0;
    for (const range of ranges) {
      need(range.start >= end, 'layout', 'Overlapping regions');
      need(bytes.subarray(end, range.start).every(n => n === 0), 'layout', 'Nonzero padding');
      end = range.end;
    }
    need(bytes.subarray(end).every(n => n === 0), 'layout', 'Nonzero trailing padding');
    // No column typed-array view exists until every directory range has passed.
    const dictionaryValue = parseJson(utf8.decode(bytes.subarray(dictionaryOffset, dictionaryOffset + dictionaryLength)));
    need(stringList(dictionaryValue) && dictionaryValue.length <= 4096, 'limit', 'String dictionary');
    const dictionary = dictionaryValue;
    function dict(index: number): string {
      need(index < dictionary.length, 'value', 'Dictionary reference');
      return dictionary[index]!;
    }
    const first = data.getBigUint64(72, true); const last = data.getBigUint64(80, true);
    const header: Header = {
      version: 1, schemaHash: hash, frameBytes: buf.byteLength, count, sub: dict(data.getUint32(48, true)),
      batchId: data.getBigUint64(56, true), streamEpoch: data.getBigUint64(64, true), firstSeq: first, lastSeq: last,
      deltaSeq: !!(flags & 1), lagMs: data.getFloat64(136, true),
    };
    need(header.sub.length > 0 && header.sub !== 'control' && Number.isFinite(header.lagMs) && header.lagMs >= 0, 'value', 'Subscription/lag');
    if (flags & 2) header.hubPublishMonoNs = data.getBigUint64(88, true);
    else need(data.getBigUint64(88, true) === 0n, 'layout', 'Absent hub timestamp');
    if (flags & 4) header.daemonWallNs = data.getBigUint64(96, true);
    else need(data.getBigUint64(96, true) === 0n, 'layout', 'Absent daemon timestamp');
    if (flags & 8) header.displayGap = null;
    else if (gapLength) header.displayGap = gap(parseJson(utf8.decode(bytes.subarray(gapOffset, gapOffset + gapLength))));
    const health = data.getUint32(132, true);
    if (health === 0xffff_fffe) header.recordHealth = null;
    else if (health !== 0xffff_ffff) {
      const text = dict(health);
      need(['ok', 'degraded', 'failed', 'disabled'].includes(text), 'value', 'record_health');
      header.recordHealth = text as NonNullable<Header['recordHealth']>;
    }
    const bit = (offset: number, row: number) => (data.getUint8(offset + (row >> 3)) & (1 << (row & 7))) !== 0;
    function cell(name: ColumnName, row: number): Cell {
      const col = columns.get(name);
      if (!col) return undefined;
      if (col.spec.flags & 1 && !bit(col.presence, row)) return undefined;
      if (col.spec.flags & 2 && !bit(col.validity, row)) return null;
      const type = col.spec.type;
      if (type === 'u8') return data.getUint8(col.data + row);
      if (type === 'u64') {
        const value = data.getBigUint64(col.data + row * 8, true);
        return name === 'seq' && header.deltaSeq ? first + value : value;
      }
      const value = data.getUint32(col.data + row * 4, true);
      if (type === 'dict') return dict(value);
      if (type === 'bytes') return new Uint8Array(buf, col.aux + value, data.getUint32(col.data + (row + 1) * 4, true) - value);
      return value;
    }
    const jsonCache = new Map<string, unknown>();
    function json(text: string): unknown {
      if (!jsonCache.has(text)) jsonCache.set(text, parseJson(text));
      return jsonCache.get(text);
    }
    const extras: Extra[] = []; const qualities: (string[] | undefined)[] = [];
    let previous = -1n;
    // Validate offsets and unused slots before any cell() can expose a byte view.
    for (const col of columns.values()) {
      if (col.spec.type === 'bytes') {
        need(data.getUint32(col.data, true) === 0, 'value', 'Offsets must start at zero');
        let offset = 0;
        for (let row = 0; row <= count; row++) {
          const next = data.getUint32(col.data + row * 4, true);
          need(next >= offset && next <= col.auxLength, 'bounds', 'Variable offset'); offset = next;
        }
        need(offset === col.auxLength, 'layout', 'Unused variable values');
      }
      for (let row = 0; row < count; row++) {
        const present = !(col.spec.flags & 1) || bit(col.presence, row);
        const valid = !(col.spec.flags & 2) || bit(col.validity, row);
        need(present || !valid || !(col.spec.flags & 2), 'layout', 'Validity without presence');
        if (!present || !valid) {
          const width = col.spec.type === 'u8' ? 1 : col.spec.type === 'u64' ? 8 : 4;
          need(bytes.subarray(col.data + row * width, col.data + (row + 1) * width).every(n => n === 0), 'layout', 'Nonzero absent/null slot');
        }
      }
    }
    // Budget preflight uses only validated scalar offsets/references. In particular,
    // it completes before cell('raw') constructs the first column byte view.
    let expanded = buf.byteLength + dictionary.reduce((sum, text) => sum + text.length * 2 + 32, 0);
    const rawColumn = columns.get('raw')!;
    for (let row = 0; row < count; row++) {
      const metadata = cell('extra', row) as string;
      const quality = cell('quality', row);
      const rawLength = data.getUint32(rawColumn.data + (row + 1) * 4, true) - data.getUint32(rawColumn.data + row * 4, true);
      // Conservative accounting units (not a claim about VM heap bytes): 128 per
      // JSON character covers many tiny containers/keys, plus per-row and byte text.
      expanded += 1024 + metadata.length * 128 + (typeof quality === 'string' ? quality.length * 128 : 0) + rawLength * 8;
      need(expanded <= MAX_EXPANDED, 'limit', 'Expanded row budget');
    }
    for (let row = 0; row < count; row++) {
      const seq = cell('seq', row) as bigint;
      need(seq <= U64_MAX && seq > previous && (row !== 0 || seq === first) && (row !== count - 1 || seq === last), 'value', 'Sequence bounds/order/overflow'); previous = seq;
      const domain = cell('domain', row); const kind = cell('kind', row); const can = domain === 'can';
      need((can && kind === 'can.frame') || (domain === 'uart' && kind === 'uart.data'), 'value', 'Unsupported domain/kind: use JSON');
      for (const name of ['session_id', 'producer_id', 'clock_id'] as const) need((cell(name, row) as string).length > 0, 'value', 'Empty identity');
      need((cell('origin', row) as number) < ORIGINS.length && (cell('ts_source', row) as number) < TS_SOURCES.length, 'value', 'Origin/timestamp enum');
      const dir = cell('dir', row); need(dir === undefined || dir === null || (dir as number) < DIRS.length, 'value', 'Direction enum');
      const encoding = cell('encoding', row) as number; need(encoding <= 1, 'value', 'Unsupported bytes encoding: use JSON');
      const raw = cell('raw', row) as Uint8Array;
      const originalText = cell('text', row);
      if (originalText !== undefined) {
        const canonical = bytesText(raw, encoding);
        need(typeof originalText === 'string' && (encoding === 0 ? /^[0-9a-fA-F]*$/.test(originalText) && originalText.toLowerCase() === canonical : originalText === canonical), 'value', 'Bytes text mismatch');
      }
      for (const spec of COLUMNS) if (spec.type === 'dict') cell(spec.name, row); // Validate every live reference.
      const metadata = cell('extra', row) as string;
      const detail = extra(json(metadata), can); extras.push(detail);
      const quality = cell('quality', row);
      if (quality !== undefined) { const parsed = json(quality as string); need(stringList(parsed), 'value', 'Quality array'); qualities.push(parsed); }
      else qualities.push(undefined);
      for (const name of ['can_id', 'can_flags', 'dlc', 'len'] as const) need((cell(name, row) !== undefined) === can, 'value', 'CAN columns presence');
      if (can) {
        const id = cell('can_id', row) as number; const f = cell('can_flags', row) as number; const dlc = cell('dlc', row) as number; const len = cell('len', row) as number;
        const fd = !!(f & 2); const rtr = !!(f & 16);
        need(f < 32 && id <= (f & 1 ? 0x1fff_ffff : 0x7ff) && dlc <= 15 && len <= 64, 'value', 'CAN id/flags/length');
        need(fd ? !rtr && len === [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64][dlc] : dlc <= 8 && len === dlc && !(f & 12), 'value', 'CAN DLC/FD flags');
        need(raw.length === (rtr ? 0 : len), 'value', 'CAN byte count');
      }
    }
    need(count !== 0 || (first === 0n && last === 0n), 'value', 'Empty sequence bounds');
    function index(row: number) { need(Number.isInteger(row) && row >= 0 && row < count, 'index', 'Row index'); }
    function materialize(row: number): LoomEvent {
      index(row);
      // JSON clone is bounded, detached from the cache; spreading preserves own __proto__ safely.
      const detail = JSON.parse(JSON.stringify(extras[row])) as Extra;
      const encoding = cell('encoding', row) as number;
      const payload: ObjectValue = { ...detail.payload, bytes: { ...detail.bytes, encoding: encoding === 0 ? 'hex' : 'base64', data: cell('text', row) ?? bytesText(cell('raw', row) as Uint8Array, encoding) } };
      if (cell('domain', row) === 'can') {
        const f = cell('can_flags', row) as number;
        Object.assign(payload, { id: cell('can_id', row), ext: !!(f & 1), fd: !!(f & 2), brs: !!(f & 4), esi: !!(f & 8), rtr: !!(f & 16), dlc: cell('dlc', row), len: cell('len', row) });
      }
      const event: ObjectValue = {
        ...detail.record, schema_version: 1, session_id: cell('session_id', row), domain: cell('domain', row), kind: cell('kind', row), producer_id: cell('producer_id', row),
        producer_epoch: (cell('producer_epoch', row) as bigint).toString(), source_seq: (cell('source_seq', row) as bigint).toString(), epoch: header.streamEpoch.toString(), seq: (cell('seq', row) as bigint).toString(),
        ts: { ...detail.ts, clock_id: cell('clock_id', row), mono_ns: (cell('mono_ns', row) as bigint).toString(), wall_ns: (cell('wall_ns', row) as bigint).toString(), source: TS_SOURCES[cell('ts_source', row) as number] },
        origin: ORIGINS[cell('origin', row) as number], payload,
      };
      for (const name of ['channel', 'client_id', 'request_id', 'dir'] as const) {
        const value = cell(name, row);
        if (value !== undefined) event[name] = name === 'dir' && value !== null ? DIRS[value as number] : value;
      }
      if (qualities[row] !== undefined) event.quality = [...qualities[row]!];
      return event as unknown as LoomEvent;
    }
    Object.freeze(header);
    return {
      header,
      get: (name, row) => attempt(() => { index(row); need(COLUMNS.some(spec => spec.name === name), 'value', 'Unknown column'); return cell(name, row); }),
      materialize: row => attempt(() => materialize(row)),
      materializeAll: () => attempt(() => Array.from({ length: count }, (_, row) => materialize(row))),
    };
  });
}
