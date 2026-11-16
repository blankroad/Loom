// B1 spec (Opus): CAN-3 transmit-list persistence + JSON exchange. FUNCTIONS.md §B1-TXSTORE / §B1-TXLIST.
import { describe, expect, it } from 'vitest';
import type { KV } from './kv';
import { parseSendLine, type CanSendParams } from './sendLine';
import { COMMENT_MAX, TX_LIST_MAX, TxList } from './txList';
import {
  cansendText,
  CYCLE_MAX_MS,
  loadTxList,
  parseTxDoc,
  saveTxList,
  storeKey,
  TX_DOC_FORMAT,
  TX_DOC_MAX_CHARS,
  TX_DOC_VERSION,
  txDoc,
  txDocText,
  type TxImportItem,
} from './txStore';

const P = (line: string, ch = 'ch0'): CanSendParams => {
  const r = parseSendLine(line, ch);
  if (!r.ok) throw new Error(r.error);
  return r.params;
};
const item = (line: string, comment = '', cycleMs = 0): TxImportItem => {
  const r = parseSendLine(line, 'ch0');
  if (!r.ok) throw new Error(r.error);
  return { params: r.params, text: r.canonical, comment, cycleMs };
};
const memKV = (init: Record<string, string> = {}): KV & { data: Record<string, string> } => {
  const data = { ...init };
  return { data, getItem: (k) => (k in data ? data[k]! : null), setItem: (k, v) => void (data[k] = v), removeItem: (k) => void delete data[k] };
};
const T0 = Date.UTC(2026, 10, 15, 12, 0, 0, 123);

describe('cansendText — one text both `loom can send` and the send line read identically', () => {
  const cases: [string, string][] = [
    ['0x300#01 02', '300#0102'],
    ['0x7#', '007#'],
    ['18DAF110#02 10 03', '18DAF110#021003'],
    ['x7#', '00000007#'],
    [`fd brs 0x123#${'a5'.repeat(12)}`, `123##1${'A5'.repeat(12)}`],
    ['fd 0x123#', '123##0'],
    ['fd 18DAF110#11', '18DAF110##011'],
  ];
  it.each(cases)('%s → %s and back to the same params', (line, text) => {
    const p = P(line);
    expect(cansendText(p)).toBe(text);
    const back = parseSendLine(text, 'ch0');
    expect(back.ok && back.params).toEqual(p);
  });
  it('standard ids keep 3 digits (CLI: >3 digits = extended), extended ids 8', () => {
    expect(cansendText(P('0x7FF#'))).toBe('7FF#');
    expect(cansendText(P('x7FF#'))).toBe('000007FF#');
  });
});

describe('txDoc / txDocText — versioned document, session counters are not saved', () => {
  it('maps entries in order with cansend frames, comment and cycle', () => {
    const l = new TxList();
    l.upsert(null, P('0x300#01'), '0x300#01', 'wake');
    l.upsert(null, P('18DAF110#02 10 03'), '18DAF110x#02 10 03', '');
    l.noteSent(l.entries[0]!.uid, 'r-1');
    const d = txDoc('ch0', l.entries, T0);
    expect(d).toEqual({
      format: TX_DOC_FORMAT,
      version: TX_DOC_VERSION,
      channel: 'ch0',
      saved_at: '2026-11-15T12:00:00.123Z',
      entries: [
        { frame: '300#01', comment: 'wake', cycle_ms: 0 },
        { frame: '18DAF110#021003', comment: '', cycle_ms: 0 },
      ],
    });
    const text = txDocText(d);
    expect(text).toContain('\n  "format"');
    expect(JSON.parse(text)).toEqual(d);
  });
});

describe('parseTxDoc — untrusted import, one validator, bounded', () => {
  const doc = (entries: unknown, o: Record<string, unknown> = {}) => JSON.stringify({ format: TX_DOC_FORMAT, version: 1, channel: 'bench-a', saved_at: 'x', entries, ...o });

  it('a document from another channel imports onto this channel', () => {
    const r = parseTxDoc(doc([{ frame: '300#0102', comment: 'c', cycle_ms: 100 }]), 'ch0');
    expect(r).toEqual({ ok: true, items: [item('0x300#01 02', 'c', 100)], skipped: [], dropped: 0, sourceChannel: 'bench-a' });
    if (r.ok) expect(r.items[0]!.params.channel).toBe('ch0');
  });
  it('accepts a plain array of cansend lines (CLI-compatible) and an already-parsed value', () => {
    const r = parseTxDoc(['300#01', 'fd 0x123#00', 'zz'], 'ch0');
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.items.map((i) => i.text)).toEqual(['0x300#01', 'fd 0x123#00']);
    expect(r.skipped).toEqual([{ index: 2, reason: expect.stringContaining('#') }]);
    expect(r.sourceChannel).toBeNull();
  });
  it('entry-level problems skip that entry with its index and reason', () => {
    const r = parseTxDoc(
      doc([
        { frame: '300#R' }, // RTR not sendable from the UI
        { frame: '300#01', cycle_ms: CYCLE_MAX_MS + 1 },
        { frame: '300#01', cycle_ms: 1.5 },
        { frame: '300#01', cycle_ms: -1 },
        { comment: 'no frame' },
        42,
        { frame: '301#01', comment: 'x'.repeat(200) },
        { frame: '302#01', comment: 7 },
        { frame: '303#01' },
      ]),
      'ch0',
    );
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.skipped.map((s) => s.index)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(r.skipped[1]!.reason).toContain('cycle_ms');
    expect(r.skipped[4]!.reason).toContain('frame');
    expect(r.items.map((i) => [i.text, i.comment.length, i.cycleMs])).toEqual([
      ['0x301#01', COMMENT_MAX, 0],
      ['0x302#01', 0, 0],
      ['0x303#01', 0, 0],
    ]);
  });
  it(`keeps at most TX_LIST_MAX (${TX_LIST_MAX}) valid entries and counts the rest as dropped`, () => {
    const r = parseTxDoc(Array.from({ length: TX_LIST_MAX + 8 }, (_, i) => `${(0x100 + i).toString(16)}#00`), 'ch0');
    expect(r.ok && r.items.length).toBe(TX_LIST_MAX);
    expect(r.ok && r.dropped).toBe(8);
  });
  it('document-level problems fail the whole import (nothing half-applied)', () => {
    expect(parseTxDoc('{', 'ch0')).toMatchObject({ ok: false, error: expect.stringMatching(/^JSON 형식 오류/) });
    expect(parseTxDoc(doc([], { version: 2 }), 'ch0')).toMatchObject({ ok: false, error: expect.stringContaining('버전') });
    expect(parseTxDoc(JSON.stringify({ entries: [] }), 'ch0')).toMatchObject({ ok: false, error: expect.stringContaining(TX_DOC_FORMAT) });
    expect(parseTxDoc(doc('nope'), 'ch0')).toMatchObject({ ok: false, error: expect.stringContaining('entries') });
    expect(parseTxDoc(42, 'ch0')).toMatchObject({ ok: false });
    expect(parseTxDoc(null, 'ch0')).toMatchObject({ ok: false });
    expect(parseTxDoc(' '.repeat(TX_DOC_MAX_CHARS + 1), 'ch0')).toMatchObject({ ok: false, error: expect.stringContaining('너무 큽니다') });
  });
});

describe('loadTxList / saveTxList — per-channel browser storage, failures are explicit', () => {
  it('round-trips per channel and keeps channels apart', () => {
    const kv = memKV();
    const l = new TxList();
    l.upsert(null, P('0x300#01'), '0x300#01', 'a');
    expect(saveTxList(kv, 'ch0', l.entries, T0)).toEqual({ ok: true });
    expect(storeKey('ch0')).toBe('loom.can.txlist.v1:ch0');
    expect(Object.keys(kv.data)).toEqual(['loom.can.txlist.v1:ch0']);
    expect(loadTxList(kv, 'ch0')).toEqual({ ok: true, items: [item('0x300#01', 'a')], skipped: [], dropped: 0, sourceChannel: 'ch0' });
    expect(loadTxList(kv, 'ch1')).toEqual({ ok: true, items: [], skipped: [], dropped: 0, sourceChannel: null });
  });
  it('empty list removes the key; no storage → ok-empty load, explicit save error', () => {
    const kv = memKV({ 'loom.can.txlist.v1:ch0': '[]' });
    expect(saveTxList(kv, 'ch0', [], T0)).toEqual({ ok: true });
    expect(kv.data).toEqual({});
    expect(loadTxList(null, 'ch0')).toEqual({ ok: true, items: [], skipped: [], dropped: 0, sourceChannel: null });
    expect(saveTxList(null, 'ch0', [], T0)).toMatchObject({ ok: false, error: expect.stringContaining('저장소 없음') });
  });
  it('corrupt stored text / throwing storage are errors, not an empty list', () => {
    expect(loadTxList(memKV({ 'loom.can.txlist.v1:ch0': 'not json' }), 'ch0')).toMatchObject({ ok: false, error: expect.stringMatching(/^JSON 형식 오류/) });
    const bad: KV = {
      getItem: () => {
        throw new Error('SecurityError');
      },
      setItem: () => {
        throw new Error('QuotaExceededError');
      },
      removeItem: () => {
        throw new Error('SecurityError');
      },
    };
    expect(loadTxList(bad, 'ch0')).toMatchObject({ ok: false, error: expect.stringContaining('읽기 실패') });
    const l = new TxList();
    l.upsert(null, P('0x300#01'), '0x300#01', '');
    expect(saveTxList(bad, 'ch0', l.entries, T0)).toMatchObject({ ok: false, error: expect.stringContaining('QuotaExceededError') });
  });
});

describe('TxList.load / contentVersion / cycleMs (B1 additions)', () => {
  it('replace swaps the list (fresh counters, new uids), append respects the cap', () => {
    const l = new TxList();
    l.upsert(null, P('0x100#00'), '0x100#00', '');
    const oldUid = l.entries[0]!.uid;
    l.noteSent(oldUid, 'r-1');
    expect(l.load([item('0x300#01', 'a', 100), item('0x301#02')], 'replace')).toEqual({ added: 2, dropped: 0 });
    expect(l.entries.map((e) => [e.text, e.comment, e.cycleMs, e.count, e.lastRequestId, e.lastError])).toEqual([
      ['0x300#01', 'a', 100, 0, null, null],
      ['0x301#02', '', 0, 0, null, null],
    ]);
    expect(l.entries.some((e) => e.uid === oldUid)).toBe(false);
    expect(l.selected).toBe(l.entries[0]!.uid);
    const many = Array.from({ length: TX_LIST_MAX }, (_, i) => item(`0x${(0x200 + i).toString(16)}#00`));
    expect(l.load(many, 'append')).toEqual({ added: TX_LIST_MAX - 2, dropped: 2 });
    expect(l.entries).toHaveLength(TX_LIST_MAX);
    expect(l.load([], 'replace')).toEqual({ added: 0, dropped: 0 });
    expect(l.entries).toHaveLength(0);
    expect(l.selected).toBeNull();
  });
  it('contentVersion moves only on saved content (not selection / send counters)', () => {
    const l = new TxList();
    const v0 = l.contentVersion;
    const r = l.upsert(null, P('0x300#01'), '0x300#01', '');
    const uid = r.ok ? r.uid : -1;
    expect(l.contentVersion).toBeGreaterThan(v0);
    const v1 = l.contentVersion;
    l.select(null);
    l.select(uid);
    l.noteSent(uid, 'r-1');
    l.noteFailed(uid, 'x');
    expect(l.contentVersion).toBe(v1);
    l.upsert(uid, P('0x300#02'), '0x300#02', 'edited');
    expect(l.contentVersion).toBeGreaterThan(v1);
    const v2 = l.contentVersion;
    l.remove(uid);
    expect(l.contentVersion).toBeGreaterThan(v2);
  });
  it('new entries are manual (cycleMs 0); editing keeps an imported cycle', () => {
    const l = new TxList();
    l.load([item('0x300#01', '', 250)], 'replace');
    const uid = l.entries[0]!.uid;
    l.upsert(uid, P('0x300#09'), '0x300#09', '');
    expect(l.get(uid)!.cycleMs).toBe(250);
    l.upsert(null, P('0x301#01'), '0x301#01', '');
    expect(l.entries[1]!.cycleMs).toBe(0);
  });
});
