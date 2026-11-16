// CAN transmit list persistence + JSON exchange (CAN-3).
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B1-TXSTORE.
//
// · Storage: this browser profile's localStorage, one key per channel alias (TX_STORE_PREFIX + channel).
//   Truthful scope: per viewer/browser, not daemon/team storage (that needs a shared owner — PLAN §Shared).
// · Exchange: a versioned JSON document the user copies/pastes (no browser download/upload claim).
//   Each entry's `frame` is cansend text accepted by BOTH `loom can send <frame>` (loom-can-frame
//   parse_frame) and the panel send line (parseSendLine) — the panel's single validator.
// · `cycle_ms` is carried now (0 = manual) so the periodic batch (CAN-2) needs no format change.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-1 work.

import type { KV } from './kv';
import { parseSendLine, type CanSendParams } from './sendLine';
import { COMMENT_MAX, TX_LIST_MAX, type TxEntry } from './txList';

export const TX_DOC_FORMAT = 'loom.can.txlist';
export const TX_DOC_VERSION = 1;
export const TX_STORE_PREFIX = 'loom.can.txlist.v1:';
/** import text cap (characters) — larger input is rejected, never truncated */
export const TX_DOC_MAX_CHARS = 65_536;
/** = crates/loom-can gateway PERIODIC_MAX_PERIOD_MS */
export const CYCLE_MAX_MS = 60_000;

export interface TxDocEntry {
  /** cansend text, e.g. "300#0102", "18DAF110#021003", "123##1A5A5" */
  frame: string;
  comment: string;
  /** 0 = manual; 1..CYCLE_MAX_MS = periodic period (CAN-2) */
  cycle_ms: number;
}

export interface TxDoc {
  format: typeof TX_DOC_FORMAT;
  version: typeof TX_DOC_VERSION;
  /** channel alias the list was saved/exported from */
  channel: string;
  /** ISO-8601 UTC of the save/export */
  saved_at: string;
  entries: TxDocEntry[];
}

/** a validated entry ready for TxList.load */
export interface TxImportItem {
  params: CanSendParams;
  /** canonical text (parseSendLine canonical) */
  text: string;
  comment: string;
  cycleMs: number;
}

export interface TxImportSkip {
  /** 0-based index in the input entries */
  index: number;
  reason: string;
}

export type TxParse =
  | { ok: true; items: TxImportItem[]; skipped: TxImportSkip[]; dropped: number; sourceChannel: string | null }
  | { ok: false; error: string };

export type TxSave = { ok: true } | { ok: false; error: string };

/** params → cansend text understood identically by loom-can-frame parse_frame and parseSendLine */
export function cansendText(p: Pick<CanSendParams, 'id' | 'ext' | 'fd' | 'brs' | 'data'>): string {
  const idHex = p.id.toString(16).toUpperCase();
  const id = p.ext ? idHex.padStart(8, '0') : idHex.padStart(3, '0');
  const D = p.data.toUpperCase();
  return p.fd ? `${id}##${p.brs ? 1 : 0}${D}` : `${id}#${D}`;
}

/** list → document (pure; savedWallMs = Date.now()-style ms) */
export function txDoc(channel: string, entries: readonly Readonly<TxEntry>[], savedWallMs: number): TxDoc {
  return {
    format: TX_DOC_FORMAT,
    version: TX_DOC_VERSION,
    channel,
    saved_at: new Date(savedWallMs).toISOString(),
    entries: entries.map((e) => ({ frame: cansendText(e.params), comment: e.comment, cycle_ms: e.cycleMs })),
  };
}

/** document → pretty JSON text for the export box (2-space indent) */
export function txDocText(doc: TxDoc): string {
  return JSON.stringify(doc, null, 2);
}

/** JSON text (or an already-parsed value) → validated items for `channel` */
export function parseTxDoc(input: unknown, channel: string): TxParse {
  let v: unknown = input;
  if (typeof input === 'string') {
    if (input.length > TX_DOC_MAX_CHARS) return { ok: false, error: '가져오기 텍스트가 너무 큽니다 (≤ 65,536자)' };
    try {
      v = JSON.parse(input);
    } catch (e) {
      return { ok: false, error: `JSON 형식 오류: ${e instanceof Error ? e.message : String(e)}` };
    }
  }
  let list: unknown[];
  let sourceChannel: string | null = null;
  if (Array.isArray(v)) {
    list = v;
  } else if (typeof v === 'object' && v !== null) {
    const d = v as Record<string, unknown>;
    if (d.format !== TX_DOC_FORMAT) return { ok: false, error: '알 수 없는 문서 형식 — format "loom.can.txlist" 필요' };
    if (d.version !== 1) return { ok: false, error: `지원하지 않는 버전 ${String(d.version)} (지원: 1)` };
    if (!Array.isArray(d.entries)) return { ok: false, error: 'entries 배열이 없습니다' };
    list = d.entries;
    sourceChannel = typeof d.channel === 'string' ? d.channel : null;
  } else {
    return { ok: false, error: '가져오기: JSON 문서 또는 문자열 배열이 필요합니다' };
  }
  const items: TxImportItem[] = [];
  const skipped: TxImportSkip[] = [];
  let dropped = 0;
  for (let i = 0; i < list.length; i++) {
    const it = list[i];
    let frame: string;
    let comment = '';
    let cycle: unknown = 0;
    if (typeof it === 'string') {
      frame = it;
    } else if (typeof it === 'object' && it !== null && typeof (it as Record<string, unknown>).frame === 'string') {
      const o = it as Record<string, unknown>;
      frame = o.frame as string;
      comment = typeof o.comment === 'string' ? o.comment.slice(0, COMMENT_MAX) : '';
      cycle = o.cycle_ms ?? 0;
    } else {
      skipped.push({ index: i, reason: 'frame 없음 (cansend 문자열 필요)' });
      continue;
    }
    const r = parseSendLine(frame, channel);
    if (!r.ok) {
      skipped.push({ index: i, reason: r.error });
      continue;
    }
    if (typeof cycle !== 'number' || !Number.isInteger(cycle) || cycle < 0 || cycle > CYCLE_MAX_MS) {
      skipped.push({ index: i, reason: 'cycle_ms: 0–60000 정수' });
      continue;
    }
    if (items.length < TX_LIST_MAX) items.push({ params: r.params, text: r.canonical, comment, cycleMs: cycle });
    else dropped++;
  }
  return { ok: true, items, skipped, dropped, sourceChannel };
}

/** localStorage key of a channel's list */
export function storeKey(channel: string): string {
  return TX_STORE_PREFIX + channel;
}

/** saved list of `channel` (absent → ok with no items) */
export function loadTxList(kv: KV | null, channel: string): TxParse {
  const empty: TxParse = { ok: true, items: [], skipped: [], dropped: 0, sourceChannel: null };
  if (kv === null) return empty;
  let raw: string | null;
  try {
    raw = kv.getItem(storeKey(channel));
  } catch (e) {
    return { ok: false, error: `브라우저 저장소 읽기 실패: ${e instanceof Error ? e.message : String(e)}` };
  }
  if (raw === null) return empty;
  return parseTxDoc(raw, channel);
}

/** persist `entries` for `channel` (empty list → key removed) */
export function saveTxList(kv: KV | null, channel: string, entries: readonly Readonly<TxEntry>[], savedWallMs: number): TxSave {
  if (kv === null) return { ok: false, error: '브라우저 저장소 없음 — 이 세션에서만 유지' };
  try {
    const key = storeKey(channel);
    if (entries.length === 0) kv.removeItem(key);
    else kv.setItem(key, JSON.stringify(txDoc(channel, entries, savedWallMs)));
    return { ok: true };
  } catch (e) {
    return { ok: false, error: `저장 실패: ${e instanceof Error ? e.message : String(e)}` };
  }
}
