import { describe, expect, it } from 'vitest';
import { parseSendLine } from './sendLine';
import { draftToParams, emptyDraft, paramsToDraft, TX_LIST_MAX, TxList, type TxDraft } from './txList';

const draft = (o: Partial<TxDraft>): TxDraft => ({ ...emptyDraft(), ...o });

describe('transmit editor → the shared send-line validator', () => {
  it('editor and send line produce identical can.send params', () => {
    const cases: [Partial<TxDraft>, string][] = [
      [{ id: '300', len: 1, bytes: ['0'] }, '0x300#00'],
      [{ id: '18DAF110', ext: true, len: 3, bytes: ['02', '10', '3'] }, '18DAF110#02 10 03'],
      [{ id: '7', ext: true, len: 0 }, 'x7#'],
      [{ id: '123', fd: true, brs: true, len: 12, bytes: Array(12).fill('a5') }, `fd brs 0x123#${'a5'.repeat(12)}`],
      [{ id: '123', fd: true, len: 12, bytes: [] }, `fd 0x123#${'00'.repeat(12)}`],
    ];
    for (const [d, line] of cases) {
      const a = draftToParams(draft(d), 'ch0');
      const b = parseSendLine(line, 'ch0');
      expect(a.ok && b.ok).toBe(true);
      if (a.ok && b.ok) {
        expect(a.params).toEqual(b.params);
        expect(a.canonical).toBe(b.canonical);
      }
    }
  });

  it('rejects what the reference dialog would not build, with a reason', () => {
    expect(draftToParams(draft({ id: '800' }), 'ch0')).toMatchObject({ ok: false, error: expect.stringMatching(/Extended/) });
    expect(draftToParams(draft({ id: '20000000', ext: true }), 'ch0')).toMatchObject({ ok: false });
    expect(draftToParams(draft({ id: 'xyz' }), 'ch0')).toMatchObject({ ok: false });
    expect(draftToParams(draft({ id: '1', brs: true }), 'ch0')).toMatchObject({ ok: false, error: expect.stringMatching(/BRS/) });
    expect(draftToParams(draft({ id: '1', len: 12 }), 'ch0')).toMatchObject({ ok: false });
    expect(draftToParams(draft({ id: '1', fd: true, len: 10 }), 'ch0')).toMatchObject({ ok: false });
    expect(draftToParams(draft({ id: '1', len: 2, bytes: ['0', 'g1'] }), 'ch0')).toMatchObject({ ok: false, error: expect.stringMatching(/Data\[1\]/) });
  });

  it('edit round-trip: params → draft → same params', () => {
    const p = parseSendLine('fd 18DAF110#' + '11'.repeat(16), 'ch0');
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    const back = draftToParams(paramsToDraft(p.params, 'note'), 'ch0');
    expect(back.ok && back.params).toEqual(p.params);
  });
});

describe('TxList (manual transmit list)', () => {
  const P = (id: number) => ({ channel: 'ch0', id, ext: false, fd: false, data: '00' });
  it('add selects the new entry; edit replaces in place; remove moves selection', () => {
    const l = new TxList();
    const a = l.upsert(null, P(1), '0x001#00', 'a');
    const b = l.upsert(null, P(2), '0x002#00', 'b');
    expect(a.ok && b.ok).toBe(true);
    if (!a.ok || !b.ok) return;
    expect(l.selected).toBe(b.uid);
    l.upsert(a.uid, P(9), '0x009#00', 'a2');
    expect(l.entries.map((e) => e.params.id)).toEqual([9, 2]);
    expect(l.get(a.uid)!.comment).toBe('a2');
    l.select(a.uid);
    l.remove(a.uid);
    expect(l.selected).toBe(b.uid);
    l.remove(b.uid);
    expect(l.selected).toBeNull();
  });

  it('keyboard move clamps; count only grows on an accepted send with a request id', () => {
    const l = new TxList();
    for (let i = 0; i < 3; i++) l.upsert(null, P(i), '', '');
    l.select(null);
    l.move(1);
    expect(l.selected).toBe(l.entries[0]!.uid);
    l.move(5);
    expect(l.selected).toBe(l.entries[2]!.uid);
    const u = l.entries[2]!.uid;
    l.noteSent(u, 'r-1');
    l.noteSent(u, null);
    l.noteFailed(u, 'operate 권한 필요');
    expect(l.get(u)).toMatchObject({ count: 1, lastRequestId: null, lastError: 'operate 권한 필요' });
  });

  it(`bounded at ${TX_LIST_MAX} entries`, () => {
    const l = new TxList();
    for (let i = 0; i < TX_LIST_MAX; i++) expect(l.upsert(null, P(i), '', '').ok).toBe(true);
    expect(l.upsert(null, P(99), '', '')).toMatchObject({ ok: false });
    expect(l.entries).toHaveLength(TX_LIST_MAX);
  });
});
