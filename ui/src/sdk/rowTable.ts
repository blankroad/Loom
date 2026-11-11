// RowTable — the common request_id table (plan §5.2.3, uds-ui Q1). UDS console,
// CAN TX rows, DLT CTRL rows and TEST steps reuse it with their own columns.
//
// Always shown (F1): stage chip (D4/A14), risk badge R0–R4, origin badge with
// letters. In-flight rows show elapsed time since first seen. Rows keep their
// position (RowStore order). display.gap marks become synthetic lines placed by
// seq ("⋯ N 화면 생략 …"). DOM: data-request-id · data-origin · data-state · data-gap.

import type { WriteState } from '../proto/provisional';
import type { RowLike } from './rowView';
import type { GapMark } from './storeMirror';
import { VirtualTable, type Cell, type Column } from './virtualTable';

export const STAGE_CHIP: Record<WriteState, string> = {
  requested: '○요청',
  accepted: '◔수락',
  sent: '◑보냄',
  confirmed: '●확정',
  effective_verified: '●●검증',
  effect_unverified: '●?미검증',
  failed: '✗실패',
  outcome_unknown: '⌛?불확정',
};

const FINAL: ReadonlySet<WriteState> = new Set([
  'confirmed',
  'effective_verified',
  'effect_unverified',
  'failed',
  'outcome_unknown',
]);

export const isFinal = (s: WriteState): boolean => FINAL.has(s);

const ORG: Record<string, [string, string]> = {
  agent: ['AGT', 'org org-agt'],
  user: ['USR', 'org org-usr'],
  device: ['DEV', 'org org-dev'],
  script: ['SCR', 'org org-scr'],
};

export type RowItem = { t: 'row'; row: RowLike } | { t: 'gap'; gap: GapMark; key: string };

export function fmtWall(ms: number): string {
  if (!ms) return '--:--:--.---';
  const d = new Date(ms);
  const p = (n: number, w = 2) => String(n).padStart(w, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

export function originCell(o: string, ext?: string): Cell {
  const [text, cls] = ORG[o] ?? ['?', 'org'];
  return ext ? { text: `${text} EXT`, cls: `${cls} org-ext`, title: `external: ${ext}` } : { text, cls };
}

export function stageCell(r: RowLike, nowMs: number): Cell {
  const chip = STAGE_CHIP[r.state];
  if (isFinal(r.state)) return { text: chip, cls: `st st-${r.state}` };
  const s = Math.max(0, (nowMs - r.firstSeenMs) / 1000);
  return { text: `${chip} ${s.toFixed(1)}s`, cls: `st st-${r.state}`, title: r.state === 'sent' ? '드라이버·소켓에 넘김 (wire 완료 아님)' : r.state };
}

export function riskCell(r: RowLike): Cell {
  return r.risk ? { text: r.risk, cls: `risk risk-${r.risk}` } : { text: 'R?', cls: 'risk', title: '등급 정보 없음' };
}

export function gapText(g: GapMark): string {
  const n = g.count === 'unknown' ? '?' : g.count.toLocaleString('ko-KR');
  const range = g.firstSeq !== null && g.lastSeq !== null ? ` (seq ${g.firstSeq}–${g.lastSeq})` : '';
  switch (g.kind) {
    case 'display.gap':
      return `⋯ ${n}건 화면 생략${range} — 기록은 데몬 저널 (${g.cause})`;
    case 'record.gap':
      return `⚠ 기록 손실 ${n}건${range} (${g.cause})`;
    case 'source.gap':
      return `⚠ 장치·전송 손실 ${n}건${range} (${g.cause})`;
    default:
      return `⋯ 재동기화 — ${g.cause}`;
  }
}

/** Merge rows (already ordered) with gap marks placed by seq; seq-less gaps go last. */
export function mergeGaps(rows: readonly RowLike[], gaps: readonly GapMark[]): RowItem[] {
  const items: RowItem[] = [];
  const placed = gaps
    .map((g, i) => ({ g, i }))
    .filter((x) => x.g.firstSeq !== null)
    .sort((a, b) => (a.g.firstSeq! < b.g.firstSeq! ? -1 : a.g.firstSeq! > b.g.firstSeq! ? 1 : 0));
  let gi = 0;
  for (const r of rows) {
    while (gi < placed.length && (placed[gi]!.g.firstSeq as bigint) < r.firstSeq) {
      const { g, i } = placed[gi++]!;
      items.push({ t: 'gap', gap: g, key: `gap-${i}` });
    }
    items.push({ t: 'row', row: r });
  }
  while (gi < placed.length) {
    const { g, i } = placed[gi++]!;
    items.push({ t: 'gap', gap: g, key: `gap-${i}` });
  }
  gaps.forEach((g, i) => {
    if (g.firstSeq === null) items.push({ t: 'gap', gap: g, key: `gap-${i}` });
  });
  return items;
}

export interface RowTableOptions {
  /** domain columns between the fixed lead (time, origin) and tail (stage, risk) */
  columns: Column<RowLike>[];
  detail?(r: RowLike): [string, string][];
  onNeedRender(): void;
  now?: () => number;
  ariaLabel?: string;
  emptyText?: string;
}

/** Lead/tail columns are fixed; gap lines span the row as their first cell. */
export function createRowTable(container: HTMLElement, o: RowTableOptions): VirtualTable<RowItem> {
  const wrap = (c: Column<RowLike>): Column<RowItem> => ({
    key: c.key,
    header: c.header,
    width: c.width,
    cell: (it, now) => (it.t === 'row' ? c.cell(it.row, now) : ''),
  });
  const columns: Column<RowItem>[] = [
    {
      key: 'time',
      header: '시각',
      width: '8.5em',
      cell: (it) => (it.t === 'row' ? fmtWall(it.row.firstWallMs) : { text: gapText(it.gap), cls: `gap-text gap-${it.gap.kind.replace('.', '-')}` }),
    },
    { key: 'origin', header: '출처', width: '3.5em', cell: (it) => (it.t === 'row' ? originCell(it.row.origin) : '') },
    ...o.columns.map(wrap),
    { key: 'stage', header: '단계', width: '8em', cell: (it, now) => (it.t === 'row' ? stageCell(it.row, now) : '') },
    { key: 'risk', header: '등급', width: '3em', cell: (it) => (it.t === 'row' ? riskCell(it.row) : '') },
  ];
  return new VirtualTable<RowItem>(container, {
    columns,
    keyOf: (it) => (it.t === 'row' ? it.row.requestId : it.key),
    rowAttrs: (it): Record<string, string> =>
      it.t === 'row'
        ? { 'data-request-id': it.row.requestId, 'data-origin': it.row.origin, 'data-state': it.row.state }
        : { 'data-gap': it.gap.kind },
    rowClass: (it) => (it.t === 'row' ? `st-row-${it.row.state}` : 'gap-row'),
    detail: o.detail ? (it) => (it.t === 'row' ? o.detail!(it.row) : [['손실', gapText(it.gap)]]) : undefined,
    onNeedRender: o.onNeedRender,
    now: o.now,
    ariaLabel: o.ariaLabel,
    emptyText: o.emptyText,
  });
}
