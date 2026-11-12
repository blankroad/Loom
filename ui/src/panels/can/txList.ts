// CAN transmit list — PCAN-View "Transmit" list + "New Transmit Message" editor analogue
// (research/fidelity/can.md rows 4–5), panel-local.
//
// What it is: a bounded list of MANUAL messages the user composed in this panel. Sending a
// selected entry goes through the very same path as the send line: parseSendLine → the
// structured `can.send` params → ctx.act('send') (ActionGate, `operate` right, dry-run risk
// preview). There is exactly one validator (parseSendLine); the editor only builds its text.
//
// Intentional deviations (honest, not imitated):
//   · Cycle time: the reference's cycle field (>0 = periodic) is NOT offered — periodic jobs
//     are daemon-owned (`can.periodic.*`) and are not wired to this panel; the editor shows
//     the field as deferred instead of faking a local timer.
//   · RTR: the structured can.send form has no rtr → the checkbox is shown disabled.
//   · Count = sends accepted by the gate from this panel (request id received), not bus echoes.
//   · Entries live in panel memory only (no daemon/shared persistence).

import { FD_LENGTHS, MAX_EFF_ID, MAX_SFF_ID, parseSendLine, type CanSendParams } from './sendLine';

export const TX_LIST_MAX = 32;
export const COMMENT_MAX = 80;
export const CLASSIC_LENGTHS: readonly number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
export const FD_LENGTH_LIST: readonly number[] = [...FD_LENGTHS].sort((a, b) => a - b);

export interface TxDraft {
  /** hex digits, optional 0x */
  id: string;
  ext: boolean;
  fd: boolean;
  brs: boolean;
  len: number;
  /** one entry per byte (hex 1–2 digits; '' = 00) — only the first `len` are used */
  bytes: string[];
  comment: string;
}

export interface TxEntry {
  readonly uid: number;
  params: CanSendParams;
  /** canonical text (formatFrame) */
  text: string;
  comment: string;
  /** sends accepted by the gate (request id received) */
  count: number;
  lastRequestId: string | null;
  lastError: string | null;
}

export type DraftResult = { ok: true; line: string; params: CanSendParams; canonical: string } | { ok: false; error: string };

export const emptyDraft = (): TxDraft => ({ id: '', ext: false, fd: false, brs: false, len: 8, bytes: Array.from({ length: 8 }, () => '00'), comment: '' });

/** editor fields → send-line text → the shared validator */
export function draftToParams(d: TxDraft, channel: string): DraftResult {
  const digits = d.id.trim().replace(/^0x/i, '');
  if (!/^[0-9a-fA-F]{1,8}$/.test(digits)) return { ok: false, error: 'ID: 16진수 1–8자리' };
  const id = parseInt(digits, 16);
  if (!d.ext && id > MAX_SFF_ID) return { ok: false, error: `11bit ID는 ≤ 0x7FF — 29bit면 Extended 체크` };
  if (d.ext && id > MAX_EFF_ID) return { ok: false, error: '29bit ID 범위 초과 (≤ 0x1FFFFFFF)' };
  if (d.brs && !d.fd) return { ok: false, error: 'BRS는 CAN FD에서만' };
  const lens = d.fd ? FD_LENGTH_LIST : CLASSIC_LENGTHS;
  if (!lens.includes(d.len)) return { ok: false, error: `길이 ${d.len}는 ${d.fd ? 'FD' : '클래식'}에서 허용되지 않음` };
  const out: string[] = [];
  for (let i = 0; i < d.len; i++) {
    const b = (d.bytes[i] ?? '').trim();
    if (b === '') {
      out.push('00');
      continue;
    }
    if (!/^[0-9a-fA-F]{1,2}$/.test(b)) return { ok: false, error: `Data[${i}] "${b}": 16진수 1–2자리` };
    out.push(b.padStart(2, '0'));
  }
  const idText = d.ext ? `x${digits}` : `0x${digits}`;
  const line = `${d.fd ? 'fd ' : ''}${d.brs ? 'brs ' : ''}${idText}#${out.join(' ')}`;
  const r = parseSendLine(line, channel);
  if (!r.ok) return r;
  return { ok: true, line, params: r.params, canonical: r.canonical };
}

/** an existing entry → editor fields (edit flow) */
export function paramsToDraft(p: CanSendParams, comment = ''): TxDraft {
  const n = p.data.length / 2;
  const bytes = Array.from({ length: Math.max(n, 8) }, (_, i) => (i < n ? p.data.slice(i * 2, i * 2 + 2).toUpperCase() : '00'));
  return { id: p.id.toString(16).toUpperCase(), ext: p.ext, fd: p.fd, brs: p.brs === true, len: n, bytes, comment };
}

export class TxList {
  private items: TxEntry[] = [];
  private nextUid = 1;
  selected: number | null = null;
  version = 0;

  get entries(): readonly Readonly<TxEntry>[] {
    return this.items;
  }
  get(uid: number): TxEntry | undefined {
    return this.items.find((e) => e.uid === uid);
  }
  /** add (uid null) or replace an entry; returns its uid or an error when full */
  upsert(uid: number | null, params: CanSendParams, text: string, comment: string): { ok: true; uid: number } | { ok: false; error: string } {
    const c = comment.slice(0, COMMENT_MAX);
    if (uid !== null) {
      const e = this.get(uid);
      if (e) {
        e.params = params;
        e.text = text;
        e.comment = c;
        this.version++;
        return { ok: true, uid };
      }
    }
    if (this.items.length >= TX_LIST_MAX) return { ok: false, error: `송신 목록은 최대 ${TX_LIST_MAX}개 — 항목을 지우세요` };
    const e: TxEntry = { uid: this.nextUid++, params, text, comment: c, count: 0, lastRequestId: null, lastError: null };
    this.items.push(e);
    this.selected = e.uid;
    this.version++;
    return { ok: true, uid: e.uid };
  }
  remove(uid: number): void {
    const i = this.items.findIndex((e) => e.uid === uid);
    if (i < 0) return;
    this.items.splice(i, 1);
    if (this.selected === uid) this.selected = this.items[Math.min(i, this.items.length - 1)]?.uid ?? null;
    this.version++;
  }
  select(uid: number | null): void {
    if (uid !== null && !this.get(uid)) return;
    if (this.selected !== uid) {
      this.selected = uid;
      this.version++;
    }
  }
  /** keyboard: move selection by delta (clamped) */
  move(delta: number): void {
    if (this.items.length === 0) return;
    const i = this.items.findIndex((e) => e.uid === this.selected);
    const j = i < 0 ? (delta > 0 ? 0 : this.items.length - 1) : Math.max(0, Math.min(this.items.length - 1, i + delta));
    this.select(this.items[j]!.uid);
  }
  noteSent(uid: number, requestId: string | null): void {
    const e = this.get(uid);
    if (!e) return;
    if (requestId) e.count++;
    e.lastRequestId = requestId;
    e.lastError = null;
    this.version++;
  }
  noteFailed(uid: number, why: string): void {
    const e = this.get(uid);
    if (!e) return;
    e.lastError = why;
    this.version++;
  }
}
