// Panel-safe, read-only view of the shared RowStore (request_id rows, plan §5.2.3).
// Panels cannot import src/store (I-UI1 lint), so the SDK hands them this view.
// The filtered list is recomputed lazily, at most once per RowStore version.

import type { Origin, Risk, WriteState } from '../proto/provisional';
import type { RowStore } from '../store/rowStore';

/** Read-only row shape exposed to panels. */
export interface RowLike {
  readonly requestId: string;
  readonly firstSeq: bigint;
  readonly state: WriteState;
  readonly lastSeq: bigint;
  readonly origin: Origin;
  readonly risk: Risk | null;
  readonly domain: string;
  readonly channel: string | null;
  readonly kind: string;
  readonly clientId: string | null;
  readonly payload: Readonly<Record<string, unknown>>;
  readonly updates: number;
  readonly firstWallMs: number;
  readonly firstSeenMs: number;
  readonly stageSeenMs: number;
}

export interface RowFilter {
  domain: string;
  channel?: string;
  kinds?: readonly string[];
  /** extra predicate (e.g. fold TesterPresent) */
  where?: (r: RowLike) => boolean;
}

export interface RowView {
  list(): readonly RowLike[];
  get(requestId: string): RowLike | undefined;
  onChange(cb: () => void): () => void;
  /** rows evicted from the shared store (oldest completed) — "older rows → query" */
  readonly evicted: number;
  readonly rejected: number;
}

export function createRowView(store: RowStore, f: RowFilter): RowView {
  let cacheVersion = -1;
  let cache: RowLike[] = [];
  const match = (r: RowLike) =>
    r.domain === f.domain &&
    (f.channel === undefined || r.channel === f.channel) &&
    (f.kinds === undefined || f.kinds.includes(r.kind)) &&
    (f.where === undefined || f.where(r));
  return {
    list() {
      if (cacheVersion !== store.version) {
        cache = store.rows().filter(match);
        cacheVersion = store.version;
      }
      return cache;
    },
    get(id) {
      const r = store.get(id);
      return r && match(r) ? r : undefined;
    },
    onChange: (cb) => store.subscribe(cb),
    get evicted() {
      return store.evicted;
    },
    get rejected() {
      return store.rejected;
    },
  };
}
