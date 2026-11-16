// CAN trace row detail (CAN-7): full data bytes and the row's provenance on one line.
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B2-DETAIL.
//
// · The model's ring is the only source (no RPC): the detail shows what the panel already holds.
//   Bytes the ring does not hold (no inline bytes / reused FD slot) are said so, never zero-filled.
// · Epoch is not kept per ring row; the detail shows the stream seq and the clock id instead.
// · Values come from devices/the daemon → the view renders them with textContent only.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-2 work.

import { fmtWall } from '../../sdk';
import type { GapInfo, Origin, WState } from './traceModel';
import {
  F_BRS,
  F_ECHO,
  F_EXT,
  F_FD,
  F_NOID,
  F_RTR,
  F_TX,
  K_FRAME,
  K_GAP,
  type CanTraceModel,
} from './traceModel';

/** shown instead of bytes when the payload carried none inline (`F_NODATA`, dataHexAt '?') */
export const NO_INLINE_BYTES = '(바이트 없음 — 페이로드에 포함되지 않음)';
/** zero-length data or RTR (dataHexAt '') */
export const NO_BYTES = '(데이터 없음)';
/** shown when the FD slot was reused (dataHexAt null) */
export const FD_SLOT_REUSED = '(바이트 없음 — FD 슬롯 재사용됨)';

/** Everything the detail shows, read from the model once (plain values; no ring positions kept). */
export interface RowFacts {
  abs: number;
  kind: 'frame' | 'txreq' | 'gap';
  seq: number;
  /** null when the row has no id (F_NOID) or is a gap */
  id: number | null;
  ext: boolean;
  fd: boolean;
  brs: boolean;
  rtr: boolean;
  tx: boolean;
  echo: boolean;
  len: number;
  dlc: number | null;
  /** model form: "01 A5 …" | '' (no bytes / RTR) | '?' (not inline) | null (FD slot reused) */
  dataHex: string | null;
  origin: Origin;
  requestId: string | null;
  clientId: string | null;
  state: WState | null;
  risk: string | null;
  error: string | null;
  frameIndex: number | null;
  /** ms since epoch UTC; 0 = unknown */
  wallMs: number;
  monoMs: number;
  clockId: string | null;
  gap: GapInfo | null;
}

export interface DetailField {
  /** stable key, also the data-k attribute of the rendered field */
  key: string;
  label: string;
  value: string;
}

/** model row → facts; null when `abs` is no longer in the ring */
export function rowFacts(model: CanTraceModel, abs: number): RowFacts | null {
  if (!Number.isInteger(abs) || !model.has(abs)) return null;
  const k = model.kindAt(abs);
  const kind = k === K_FRAME ? 'frame' : k === K_GAP ? 'gap' : 'txreq';
  const fl = model.flagsAt(abs);
  const req = model.reqAt(abs);
  const t = model.timeAt(abs);
  return {
    abs,
    kind,
    seq: t.seq,
    id: kind === 'gap' || fl & F_NOID ? null : model.idAt(abs),
    ext: (fl & F_EXT) !== 0,
    fd: (fl & F_FD) !== 0,
    brs: (fl & F_BRS) !== 0,
    rtr: (fl & F_RTR) !== 0,
    tx: (fl & F_TX) !== 0,
    echo: (fl & F_ECHO) !== 0,
    len: model.lenAt(abs),
    dlc: model.dlcAt(abs),
    dataHex: kind === 'gap' ? '' : model.dataHexAt(abs),
    origin: model.originAt(abs),
    requestId: req?.requestId ?? null,
    clientId: req?.clientId ?? null,
    state: model.stateAt(abs),
    risk: model.riskAt(abs),
    error: req?.error ?? null,
    frameIndex: model.frameIndexAt(abs),
    wallMs: t.wallMs,
    monoMs: t.monoMs,
    clockId: t.clockId,
    gap: model.gapAt(abs),
  };
}

/** full data text for the data cell title and the detail: hex as given; '' → NO_BYTES; '?' → NO_INLINE_BYTES; null → FD_SLOT_REUSED */
export function fullDataText(dataHex: string | null): string {
  if (dataHex === null) return FD_SLOT_REUSED;
  if (dataHex === '?') return NO_INLINE_BYTES;
  if (dataHex === '') return NO_BYTES;
  return dataHex;
}

/** ordered detail fields (see FUNCTIONS.md §B2-DETAIL table; absent values are omitted, never "0") */
export function rowDetailFields(f: RowFacts): DetailField[] {
  const out: DetailField[] = [];
  const add = (key: string, label: string, value: string): void => {
    out.push({ key, label, value });
  };
  const gap = f.kind === 'gap';
  add('kind', '종류', f.kind === 'frame' ? (f.tx ? 'TX 프레임' : 'RX 프레임') + (f.echo ? ' · 에코' : '') : f.kind === 'txreq' ? '송신 요청' : `갭 · ${f.gap?.kind ?? '?'}`);
  add('seq', 'seq', String(f.seq));
  if (!gap && f.id !== null) {
    add('id', 'ID', f.ext ? `0x${f.id.toString(16).toUpperCase().padStart(8, '0')} (29-bit)` : `0x${f.id.toString(16).toUpperCase().padStart(3, '0')} (11-bit)`);
  }
  if (!gap) {
    add('fmt', '형식', [f.fd ? 'CAN FD' : 'CAN', f.brs && 'BRS', f.rtr && 'RTR'].filter(Boolean).join(' · '));
    add('len', '길이', `${f.len} B · DLC ${f.dlc ?? '?'}`);
    add('data', '데이터', fullDataText(f.dataHex));
    add('origin', '출처', f.origin);
    if (f.requestId !== null) add('req', '요청', f.requestId + (f.clientId ? ` · client ${f.clientId}` : ''));
    if (f.frameIndex !== null) add('frameIndex', '프레임 순번', String(f.frameIndex));
    if (f.state !== null) add('state', '상태', f.state);
    if (f.risk !== null) add('risk', '등급', f.risk);
    if (f.error !== null) add('error', '오류', f.error);
  } else if (f.gap) {
    add('gap', '손실', f.gap.count === 'unknown' ? '알 수 없음' : `${f.gap.count}행`);
    if (f.gap.first !== null && f.gap.last !== null) add('gapRange', '범위', `seq ${f.gap.first}–${f.gap.last}`);
    add('cause', '원인', f.gap.cause);
  }
  add('wall', '벽시계', f.wallMs > 0 ? `${fmtWall(f.wallMs)} (${new Date(f.wallMs).toISOString()})` : '알 수 없음');
  add('mono', 'mono', `${(f.monoMs / 1000).toFixed(6)} s`);
  add('clock', 'clock', f.clockId ?? '알 수 없음');
  return out;
}

/** copy text: one "label: value" per line, same order as rowDetailFields, '\n'-joined, no trailing newline */
export function rowDetailText(f: RowFacts): string {
  return rowDetailFields(f)
    .map((d) => `${d.label}: ${d.value}`)
    .join('\n');
}
