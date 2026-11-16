// CAN export from the bound panel (CAN-4): request params and result text. Pure (no DOM, no RPC).
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B4-UI.
//
// · `can.export` writes a BLF/ASC file + `.loom.jsonl` sidecar ON THE DAEMON MACHINE. The panel never
//   implies a browser download: the result says where the file is and offers to copy the path.
// · Scope 'ring' = the frames this panel holds (first..last frame seq of its ring); 'recent' = last 10 min.
// · The current id/dir/origin filter can be applied; the id list becomes CLI-style tokens
//   ("0x300", "0x7E0-0x7EF") — at most EXPORT_IDS_MAX (the RPC schema's maxItems).
// · Result values come from the daemon → the view renders them with textContent only.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-4 work.

import { fmtFilterId, type IdFilter } from './idFilter';
import type { Origin } from './traceModel';

/** = crates/loom-can gateway `can.export` params schema ids.maxItems */
export const EXPORT_IDS_MAX = 64;
/** daemon path length bound (= schema path.maxLength) */
export const EXPORT_PATH_MAX = 4096;
/** 'recent' scope cursor */
export const EXPORT_RECENT_SINCE = '-10m';
/** ring rows examined from each end to find a frame row's seq */
export const RING_PROBE_ROWS = 64;
/** fixed order of origins in params */
export const ORIGIN_ORDER: readonly Origin[] = ['agent', 'user', 'device', 'script'];

export type ExportFormatUi = 'blf' | 'asc';
export type ExportScope = 'ring' | 'recent';

export interface ExportInputs {
  channel: string;
  format: ExportFormatUi;
  scope: ExportScope;
  /** stream seq of the first / last frame row held by the panel (null = none) */
  ringFirstSeq: number | null;
  ringLastSeq: number | null;
  useFilter: boolean;
  ids: IdFilter | null;
  dir: 'all' | 'rx' | 'tx';
  /** null = all origins */
  origins: ReadonlySet<Origin> | null;
  /** user text (trimmed by the function); '' = daemon default temp file */
  path: string;
}

export type ExportReq = { ok: true; params: Record<string, unknown>; scopeText: string } | { ok: false; error: string };

export interface ExportView {
  lines: string[];
  /** daemon-side primary path (copy button), null when no file */
  path: string | null;
  /** the result is not complete (gaps / cut / unknown shape) */
  warn: boolean;
}

/** The part of CanTraceModel ringSeqs reads. */
export interface RingReader {
  readonly oldestAbs: number;
  readonly total: number;
  kindAt(abs: number): number;
  seqAt(abs: number): number;
}

/** IdFilter → RPC id tokens in its normalised order (singles "0x300", ranges "0x7E0-0x7EF") */
export function exportIdTokens(f: IdFilter): string[] {
  const items: { start: number; text: string }[] = [
    ...f.singles.map((id) => ({ start: id, text: fmtFilterId(id) })),
    ...f.ranges.map((r) => ({ start: r.lo, text: `${fmtFilterId(r.lo)}-${fmtFilterId(r.hi)}` })),
  ];
  items.sort((a, b) => a.start - b.start);
  return items.map((i) => i.text);
}

/** first/last frame-row seq of the ring (≤ RING_PROBE_ROWS rows probed from each end; gap rows and seq 0 skipped) */
export function ringSeqs(m: RingReader, gapKind: number): { first: number | null; last: number | null } {
  const isFrame = (abs: number): boolean => m.kindAt(abs) !== gapKind && m.seqAt(abs) > 0;
  let first: number | null = null;
  const upTo = Math.min(m.total, m.oldestAbs + RING_PROBE_ROWS);
  for (let abs = m.oldestAbs; abs < upTo; abs++) {
    if (isFrame(abs)) {
      first = m.seqAt(abs);
      break;
    }
  }
  let last: number | null = null;
  const downTo = Math.max(m.oldestAbs, m.total - RING_PROBE_ROWS);
  for (let abs = m.total - 1; abs >= downTo; abs--) {
    if (isFrame(abs)) {
      last = m.seqAt(abs);
      break;
    }
  }
  return { first, last };
}

/** panel inputs → `can.export` params, or a user-facing error */
export function exportRequest(i: ExportInputs): ExportReq {
  const params: Record<string, unknown> = { channel: i.channel, format: i.format };
  let scopeText: string;
  if (i.scope === 'ring') {
    if (i.ringFirstSeq === null || i.ringLastSeq === null) {
      return { ok: false, error: '이 패널에 보관된 프레임이 없습니다 — "최근 10분"을 고르세요' };
    }
    params.since = `seq:${i.ringFirstSeq}`;
    params.until = `seq:${i.ringLastSeq}`;
    scopeText = `이 패널 보관 범위 seq ${i.ringFirstSeq}–${i.ringLastSeq}`;
  } else {
    params.since = EXPORT_RECENT_SINCE;
    scopeText = '최근 10분';
  }
  const path = i.path.trim();
  if (path !== '') {
    if (path.length > EXPORT_PATH_MAX) return { ok: false, error: '경로가 너무 깁니다 (≤ 4096자)' };
    if (!(path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path))) {
      return { ok: false, error: '경로는 데몬 컴퓨터의 절대 경로여야 합니다 (비우면 임시 폴더)' };
    }
    params.path = path;
  }
  if (i.useFilter) {
    if (i.ids) {
      const tokens = exportIdTokens(i.ids);
      if (tokens.length > EXPORT_IDS_MAX) {
        return { ok: false, error: `id 필터 항목 ${tokens.length}개 — 내보내기는 ${EXPORT_IDS_MAX}개까지 (범위로 묶으세요)` };
      }
      params.ids = tokens;
    }
    if (i.dir !== 'all') params.dir = i.dir;
    if (i.origins !== null) {
      if (i.origins.size === 0) return { ok: false, error: '출처를 하나 이상 고르세요' };
      const origins = i.origins;
      params.origins = ORIGIN_ORDER.filter((o) => origins.has(o));
    }
    scopeText += ' · 현재 필터';
  }
  return { ok: true, params, scopeText };
}

/** `can.export` result → display lines */
export function exportResultLines(r: unknown): ExportView {
  const unknown: ExportView = { lines: ['결과 형식을 알 수 없음'], path: null, warn: true };
  if (typeof r !== 'object' || r === null) return unknown;
  const o = r as Record<string, unknown>;
  const path = o.path;
  if (typeof path !== 'string' && path !== null) return unknown;
  if (typeof path === 'string' && (typeof o.sidecar !== 'string' || typeof o.frames !== 'number' || typeof o.gaps !== 'number' || typeof o.format !== 'string')) {
    return unknown;
  }
  const gaps = typeof o.gaps === 'number' ? o.gaps : 0;
  const gd = typeof o.gaps_dropped === 'number' ? o.gaps_dropped : 0;
  const complete = o.complete === true;
  const truncated = o.truncated === true;
  const lines: string[] = [];
  if (typeof path !== 'string') {
    lines.push('선택 범위에 프레임이 없어 파일을 만들지 않았습니다');
  } else {
    lines.push(`데몬 컴퓨터에 저장됨 (브라우저 다운로드 아님): ${path}`);
    lines.push(`사이드카: ${String(o.sidecar)}`);
    const range = typeof o.range === 'object' && o.range !== null ? (o.range as Record<string, unknown>) : null;
    let l = `${String(o.frames)}프레임 · ${String(o.format).toUpperCase()} · ${range ? `seq ${String(range.from)}–${String(range.to)}` : 'seq ?'} · 갭 ${gaps}`;
    if (gd > 0) l += ` (+${gd} 미기록)`;
    if (o.wall_anchor_known !== true) l += ' · 벽시계 없음 (상대 시각)';
    lines.push(l);
    if (truncated) {
      const reasons: Record<string, string> = {
        max_frames: '파일당 1,000,000프레임 상한',
        scan_budget: '스캔 예산',
        clock_change: '데몬 시계 바뀜',
      };
      const raw = String(o.truncated_reason);
      lines.push(`잘림 (${reasons[raw] ?? raw}) — 이어서: since ${String(o.next_cursor)}`);
    }
  }
  if (gaps > 0 || gd > 0) lines.push('⚠ 범위 안에 저널 손실 있음 (파일·사이드카에 갭 기록)');
  else if (!complete && !truncated) lines.push('⚠ 저널 스캔이 범위 전체를 덮지 못함');
  return { lines, path: typeof path === 'string' ? path : null, warn: !complete };
}
