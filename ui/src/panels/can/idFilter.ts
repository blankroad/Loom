// CAN trace / Receive id filter with ranges (CAN-8).
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B2-IDF.
//
// Grammar (UI convention: every number is HEX, the id column is hex):
//   filter := token ( [\s,]+ token )*        empty / whitespace only → null (= all ids)
//   token  := id | id '-' id                 (no spaces around '-')
//   id     := ['0x'|'0X'] hex{1,8} ['x'|'X'] value ≤ CAN_ID_MAX (0x1FFFFFFF)
// The CLI (`loom can trace --id`) reads bare digits as DECIMAL; the UI reads them as hex. The
// normalised text (`IdFilter.text`) is always shown, so the user sees what was applied.
// The ext flag is not part of the match (0x300 matches a standard and an extended 0x300), as before.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-2 work.

/** = loom_can_frame::MAX_EXTENDED_ID */
export const CAN_ID_MAX = 0x1fff_ffff;
/** distinct single ids per filter (unchanged from the pre-range parser) */
export const ID_FILTER_MAX_IDS = 256;
/** range tokens per filter (before merging) */
export const ID_FILTER_MAX_RANGES = 64;

/** What TraceFilter.ids / the Receive view need: a ReadonlySet<number> also satisfies it. */
export interface IdMatch {
  has(id: number): boolean;
}

/** inclusive, lo ≤ hi */
export interface IdRange {
  readonly lo: number;
  readonly hi: number;
}

/** A parsed, normalised filter: singles not covered by a range + merged ranges. Immutable. */
export class IdFilter implements IdMatch {
  /** singles NOT inside any range, ascending */
  readonly singles: readonly number[];
  /** merged (overlapping or adjacent joined), ascending by lo, never overlapping */
  readonly ranges: readonly IdRange[];
  /** normalised display text, e.g. "0x300, 0x7E0-0x7EF, 0x18DAF110" */
  readonly text: string;
  private readonly set: ReadonlySet<number>;

  /** `singles` / `ranges` in any order; validated by parseIdFilter (the constructor does not throw) */
  constructor(singles: Iterable<number>, ranges: Iterable<IdRange>) {
    const sorted = [...ranges].sort((a, b) => a.lo - b.lo);
    const merged: IdRange[] = [];
    for (const r of sorted) {
      const cur = merged[merged.length - 1];
      if (cur && r.lo <= cur.hi + 1) merged[merged.length - 1] = { lo: cur.lo, hi: Math.max(cur.hi, r.hi) };
      else merged.push({ lo: r.lo, hi: r.hi });
    }
    this.ranges = merged;
    const inRange = (id: number): boolean => merged.some((r) => r.lo <= id && id <= r.hi);
    this.singles = [...new Set(singles)].filter((id) => !inRange(id)).sort((a, b) => a - b);
    this.set = new Set(this.singles);
    const items: { start: number; text: string }[] = [
      ...this.singles.map((id) => ({ start: id, text: fmtFilterId(id) })),
      ...merged.map((r) => ({ start: r.lo, text: `${fmtFilterId(r.lo)}-${fmtFilterId(r.hi)}` })),
    ];
    items.sort((a, b) => a.start - b.start);
    this.text = items.map((i) => i.text).join(', ');
  }

  has(id: number): boolean {
    return this.set.has(id) || this.ranges.some((r) => r.lo <= id && id <= r.hi);
  }
}

/** one id in the normalised text: ≤ 0x7FF → "0x%03X", else "0x%08X" (upper-case hex) */
export function fmtFilterId(id: number): string {
  const hex = id.toString(16).toUpperCase();
  return id <= 0x7ff ? `0x${hex.padStart(3, '0')}` : `0x${hex.padStart(8, '0')}`;
}

/** filter text → IdFilter; null = all ids; throws Error(message) on bad input (message shown after "필터 오류: ") */
export function parseIdFilter(s: string): IdFilter | null {
  const tokens = s.split(/[\s,]+/).filter(Boolean);
  if (tokens.length === 0) return null;
  const singles = new Set<number>();
  const ranges: IdRange[] = [];
  const readEnd = (tok: string, e: string): number => {
    let r = e;
    if (/^0x/i.test(r)) r = r.slice(2);
    if (/x$/i.test(r)) r = r.slice(0, -1);
    if (!/^[0-9a-fA-F]{1,8}$/.test(r)) throw new Error(`id "${tok}"`);
    const v = parseInt(r, 16);
    if (v > CAN_ID_MAX) throw new Error(`id "${tok}" > 0x1FFFFFFF`);
    return v;
  };
  for (const tok of tokens) {
    const m = /^([^-]+)(?:-([^-]+))?$/.exec(tok);
    if (!m) throw new Error(`id "${tok}"`);
    const lo = readEnd(tok, m[1]!);
    const hi = m[2] !== undefined ? readEnd(tok, m[2]) : lo;
    if (lo > hi) throw new Error(`범위 "${tok}": 시작 > 끝`);
    if (lo === hi) {
      singles.add(lo);
      if (singles.size > ID_FILTER_MAX_IDS) throw new Error('id ≤ 256개');
    } else {
      ranges.push({ lo, hi });
      if (ranges.length > ID_FILTER_MAX_RANGES) throw new Error('범위 ≤ 64개');
    }
  }
  return new IdFilter(singles, ranges);
}
