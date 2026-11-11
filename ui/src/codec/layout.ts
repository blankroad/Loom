/** Proposal only: CORE must ratify this layout before transport integration. */
export const MAX_FRAME = 65_536;
export const MAX_ROWS = 200;
export const MAX_EXPANDED = 4 * 1024 * 1024;
export const HEADER_BYTES = 160;
export const DIRECTORY_BYTES = 32;
export const U64_MAX = (1n << 64n) - 1n;
export const MAGIC = 'LOOMCOL1';
export const VERSION = 1;
export const TYPES = { u8: 1, u32: 2, u64: 3, dict: 4, bytes: 5 } as const;
export type WireType = keyof typeof TYPES;
// optional: presence bitmap; nullable: independent validity bitmap. Neither means required.
export const COLUMNS = [
  { id: 1, name: 'session_id', type: 'dict', flags: 0 },
  { id: 2, name: 'domain', type: 'dict', flags: 0 },
  { id: 3, name: 'kind', type: 'dict', flags: 0 },
  { id: 4, name: 'producer_id', type: 'dict', flags: 0 },
  { id: 5, name: 'producer_epoch', type: 'u64', flags: 0 },
  { id: 6, name: 'source_seq', type: 'u64', flags: 0 },
  { id: 7, name: 'seq', type: 'u64', flags: 0 },
  { id: 8, name: 'mono_ns', type: 'u64', flags: 0 },
  { id: 9, name: 'wall_ns', type: 'u64', flags: 0 },
  { id: 10, name: 'clock_id', type: 'dict', flags: 0 },
  { id: 11, name: 'ts_source', type: 'u8', flags: 0 },
  { id: 12, name: 'origin', type: 'u8', flags: 0 },
  { id: 13, name: 'channel', type: 'dict', flags: 3 },
  { id: 14, name: 'client_id', type: 'dict', flags: 3 },
  { id: 15, name: 'extra', type: 'dict', flags: 0 },
  { id: 16, name: 'encoding', type: 'u8', flags: 0 },
  { id: 17, name: 'raw', type: 'bytes', flags: 0 },
  { id: 18, name: 'text', type: 'dict', flags: 1 },
  { id: 19, name: 'can_id', type: 'u32', flags: 1 },
  { id: 20, name: 'can_flags', type: 'u8', flags: 1 },
  { id: 21, name: 'dlc', type: 'u8', flags: 1 },
  { id: 22, name: 'len', type: 'u8', flags: 1 },
  { id: 23, name: 'dir', type: 'u8', flags: 3 },
  { id: 24, name: 'request_id', type: 'dict', flags: 3 },
  { id: 25, name: 'quality', type: 'dict', flags: 1 },
] as const satisfies readonly { id: number; name: string; type: WireType; flags: number }[];
export type ColumnName = typeof COLUMNS[number]['name'];
export const ORIGINS = ['agent', 'user', 'device', 'script'] as const;
export const TS_SOURCES = ['host_read_complete', 'driver', 'hardware', 'device', 'gateway', 'synthetic'] as const;
export const DIRS = ['rx', 'tx'] as const;
/** SHA-256 of UTF-8 JSON.stringify(LAYOUT_IDENTITY); changes require a new hash. */
export const LAYOUT_IDENTITY = {
  format: 'loom.columns.v1/proposal-1', header: 160, directory: 32,
  endian: 'little', dictionary: 'utf8-json-string-array',
  flags: 'delta=1,hub=2,daemon=4,gap-null=8',
  columns: COLUMNS, origins: ORIGINS, tsSources: TS_SOURCES, dirs: DIRS,
  limits: { frame: MAX_FRAME, rows: MAX_ROWS, expanded: MAX_EXPANDED, dictionary: 4096, jsonDepth: 16, jsonNodes: 8192 },
};
export const SCHEMA_HASH = '2e01eea1b6346a1cb35caeee041ebdbb171c025648d646bd83daf77e004f02c6';
