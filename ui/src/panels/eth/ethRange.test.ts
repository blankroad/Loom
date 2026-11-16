// operator-upgrade B2 (ETH-01) — pure model contract: explicit ranges, daemon honesty fields, badge text, paging.
// Spec tests authored with FUNCTIONS.md B2 (F2.1–F2.12); implementation must not change them.
import { describe, expect, it } from 'vitest';
import {
  completeBadge,
  MAX_PAGE_STACK,
  navNewer,
  navOlder,
  navResume,
  navStart,
  PAGE_ROWS,
  parseRangeChoice,
  rangeChoiceValue,
  readQueryPage,
  readReadMeta,
  reasonText,
  resolveRange,
  scopeText,
  type ReadMeta,
} from './ethModel';

const B1 = (o: Record<string, unknown> = {}) => ({
  rows: [],
  range: { from: null, to: null },
  matched: 0,
  truncated: false,
  complete: true,
  scan_complete: true,
  partial_reasons: [],
  scope: { from: 'e1:seq:1', to: null, from_source: 'explicit', lookback_secs: null },
  scanned: { first_seq: '1380', last_seq: '2000', bytes: 4096 },
  resume_cursor: null,
  prev_cursor: null,
  next_cursor: null,
  gaps: [],
  gaps_omitted: 0,
  ...o,
});
const row = (seq: number) => ({ ref: `eth:qa@seq:${seq}`, seq: String(seq), ts: { mono_ns: '1', wall_ns: '1' }, proto: 'UDP', info: 'x', len: 60 });

describe('resolveRange (F2.1) — complete views never send a from-less range', () => {
  it('capture: daemon anchor when known, head as `to`', () => {
    expect(resolveRange({ kind: 'capture' }, { startCursor: 'e1:seq:1', headSeq: 2000n })).toEqual({ req: { from: 'e1:seq:1', to: 'seq:2000' }, label: '캡처 시작 이후', fallback: false });
    expect(resolveRange({ kind: 'capture' }, { startCursor: 'e1:seq:1', headSeq: null })).toEqual({ req: { from: 'e1:seq:1' }, label: '캡처 시작 이후', fallback: false });
  });
  it('capture with unknown anchor: explicit recent-10-minutes fallback, labelled so', () => {
    expect(resolveRange({ kind: 'capture' }, { startCursor: null, headSeq: 0n })).toEqual({ req: { from: '-600s' }, label: '최근 10분 (캡처 시작 위치 모름)', fallback: true });
  });
  it('recent and seq choices', () => {
    expect(resolveRange({ kind: 'recent', secs: 3600 }, { startCursor: 'e1:seq:1', headSeq: 9n })).toEqual({ req: { from: '-3600s', to: 'seq:9' }, label: '최근 1시간', fallback: false });
    expect(resolveRange({ kind: 'recent', secs: 600 }, { startCursor: null, headSeq: null }).label).toBe('최근 10분');
    expect(resolveRange({ kind: 'recent', secs: 21600 }, { startCursor: null, headSeq: null }).label).toBe('최근 6시간');
    expect(resolveRange({ kind: 'seq', from: 10n, to: 20n }, { startCursor: null, headSeq: 99n })).toEqual({ req: { from: 'seq:10', to: 'seq:20' }, label: 'seq 10–20', fallback: false });
    expect(resolveRange({ kind: 'seq', from: 10n, to: null }, { startCursor: null, headSeq: 99n })).toEqual({ req: { from: 'seq:10', to: 'seq:99' }, label: 'seq 10–최신', fallback: false });
    expect(resolveRange({ kind: 'seq', from: 10n, to: null }, { startCursor: null, headSeq: null })).toEqual({ req: { from: 'seq:10' }, label: 'seq 10–최신', fallback: false });
  });
});

describe('parseRangeChoice / rangeChoiceValue (F2.2, F2.3)', () => {
  it('round-trips every select value', () => {
    for (const v of ['capture', 'recent:600', 'recent:3600', 'recent:21600']) {
      const c = parseRangeChoice(v, '', '');
      expect('error' in c).toBe(false);
      expect(rangeChoiceValue(c as never)).toBe(v);
    }
    expect(parseRangeChoice('seq', ' 10 ', '')).toEqual({ kind: 'seq', from: 10n, to: null });
    expect(rangeChoiceValue({ kind: 'seq', from: 1n, to: 2n })).toBe('seq');
    expect(parseRangeChoice('seq', '18446744073709551615', '18446744073709551615')).toEqual({ kind: 'seq', from: 18446744073709551615n, to: 18446744073709551615n });
  });
  it('rejects bad input with user-facing errors (never a bare number cursor)', () => {
    expect(parseRangeChoice('seq', '', '')).toEqual({ error: '시작 seq를 숫자로 입력하세요' });
    expect(parseRangeChoice('seq', '1e3', '')).toEqual({ error: '시작 seq를 숫자로 입력하세요' });
    expect(parseRangeChoice('seq', '-1', '')).toEqual({ error: '시작 seq를 숫자로 입력하세요' });
    expect(parseRangeChoice('seq', '18446744073709551616', '')).toEqual({ error: '시작 seq를 숫자로 입력하세요' });
    expect(parseRangeChoice('seq', '5', 'x')).toEqual({ error: '끝 seq를 숫자로 입력하세요' });
    expect(parseRangeChoice('seq', '10', '5')).toEqual({ error: '시작 seq가 끝 seq보다 큽니다' });
    expect(parseRangeChoice('recent:60', '', '')).toEqual({ error: '알 수 없는 범위' });
  });
});

describe('readReadMeta / readQueryPage (F2.4, F2.5)', () => {
  it('reads the B1 honesty fields exactly', () => {
    const m = readReadMeta(B1({ partial_reasons: ['loss', 'row_cap', 'loss', 'future_x', 7], complete: false, scan_complete: true, resume_cursor: 'e1:seq:77', gaps: [{}, {}], gaps_omitted: 3 }));
    expect(m).toEqual({
      known: true,
      complete: false,
      scanComplete: true,
      reasons: ['loss', 'row_cap'],
      unknownReasons: ['future_x'],
      scope: { from: 'e1:seq:1', to: null, fromSource: 'explicit', lookbackSecs: null },
      scannedFirst: 1380n,
      scannedLast: 2000n,
      resumeCursor: 'e1:seq:77',
      gaps: 5,
    } satisfies ReadMeta);
  });
  it('complete only when the daemon says so AND reports no reason; an older daemon is unknown, never complete', () => {
    expect(readReadMeta(B1()).complete).toBe(true);
    expect(readReadMeta(B1({ partial_reasons: ['loss'] })).complete).toBe(false); // inconsistent daemon complete:true + loss
    expect(readReadMeta(B1({ partial_reasons: ['future_x'] })).complete).toBe(false);
    expect(readReadMeta(B1({ complete: false })).complete).toBe(false);
    const legacy = readReadMeta({ rows: [], complete: true, truncated: false });
    expect(legacy).toMatchObject({ known: false, complete: false, scanComplete: null, reasons: [], scope: null, scannedFirst: null, resumeCursor: null, gaps: 0 });
    expect(readReadMeta(null)).toMatchObject({ known: false, complete: false });
    expect(readReadMeta(B1({ scope: { from: 7, from_source: 'bogus', lookback_secs: '600' } })).scope).toEqual({ from: null, to: null, fromSource: null, lookbackSecs: 600 });
  });
  it('page rows / seq range / cursors', () => {
    const p = readQueryPage(B1({ rows: [row(1380), row(1381), { nope: 1 }], range: { from: '1380', to: '1381' }, matched: 9, truncated: true, prev_cursor: 'seq:1379', next_cursor: 'seq:1382' }), 'qa');
    expect(p.rows.map((r) => r.seq)).toEqual([1380n, 1381n]);
    expect([p.rowsFrom, p.rowsTo, p.matched, p.truncated, p.prevCursor, p.nextCursor]).toEqual([1380n, 1381n, 9, true, 'seq:1379', 'seq:1382']);
    const q = readQueryPage({ rows: [row(5), row(6)] }, 'qa');
    expect([q.rowsFrom, q.rowsTo, q.matched, q.prevCursor, q.meta.known]).toEqual([5n, 6n, null, null, false]);
    expect(readQueryPage(undefined, 'qa').rows).toEqual([]);
  });
});

describe('badge text (F2.6–F2.8) — 완전 only for complete daemon results', () => {
  const meta = (o: Record<string, unknown>) => readReadMeta(B1(o));
  it('reason texts', () => {
    const m = meta({ gaps: [{}], gaps_omitted: 2 });
    expect(reasonText('default_lookback', m)).toBe('기본 조회(최근 10분)만');
    expect(reasonText('scan_budget', m)).toBe('스캔 한도(64 MiB) 도달');
    expect(reasonText('scan_incomplete', m)).toBe('저널 스캔 불완전');
    expect(reasonText('decode_budget', m)).toBe('디코드 한도 도달');
    expect(reasonText('loss', m)).toBe('손실 구간 3건');
    expect(reasonText('row_cap', m)).toBe(`최대 ${PAGE_ROWS.toLocaleString('ko-KR')}행 초과`);
    expect(reasonText('output_cap', m)).toBe('출력 한도 도달');
  });
  it('scope text states requested range, scanned seqs and coverage', () => {
    expect(scopeText(meta({}))).toBe('요청 범위 e1:seq:1 → 최신 · 스캔 seq 1380–2000 · 스캔 범위 전체 확인');
    expect(scopeText(meta({ scope: { from: '-600s', to: 'seq:9', from_source: 'default_lookback', lookback_secs: 600 }, scan_complete: false, scanned: { first_seq: null, last_seq: null, bytes: 0 } }))).toBe(
      '요청 범위 -600s → seq:9 (기본 최근 10분) · 스캔 seq ?–? · 스캔 중단',
    );
    expect(scopeText(readReadMeta({ rows: [] }))).toBe('요청/스캔 범위 정보 없음 (이전 daemon)');
  });
  it('complete / partial / unknown badges', () => {
    const one = readQueryPage(B1({ rows: [row(1380)], range: { from: '1380', to: '1380' }, matched: 1 }), 'qa');
    expect(completeBadge(one, '캡처 시작 이후')).toEqual({ text: '완전 · 캡처 시작 이후 · 1행 · seq 1380–1380', state: 'complete', title: scopeText(one.meta) });
    const cut = readQueryPage(B1({ rows: [row(1), row(2)], range: { from: '1', to: '2' }, matched: 5000, truncated: true, complete: true, partial_reasons: ['loss', 'row_cap', 'future_x'], gaps: [{}] }), 'qa');
    const b = completeBadge(cut, 'seq 1–2');
    expect(b.state).toBe('partial');
    expect(b.text).toBe('부분 · seq 1–2 · 2행 / 일치 5,000 · seq 1–2 — 손실 구간 1건 · 최대 1,000행 초과 · 기타 사유 future_x');
    expect(b.text).not.toContain('완전');
    const none = readQueryPage(B1({ complete: false }), 'qa');
    expect(completeBadge(none, '최근 1시간')).toMatchObject({ text: '부분 · 최근 1시간 · 0행 — 완전 아님(사유 미보고)', state: 'partial' });
    const legacy = readQueryPage({ rows: [row(5)], range: { from: '5', to: '5' }, complete: true }, 'qa');
    expect(completeBadge(legacy, '최근 10분 (캡처 시작 위치 모름)')).toEqual({
      text: '완전 여부 미확인 · 최근 10분 (캡처 시작 위치 모름) · 1행 · seq 5–5',
      state: 'unknown',
      title: '요청/스캔 범위 정보 없음 (이전 daemon)',
    });
  });
});

describe('paging state (F2.9–F2.12)', () => {
  it('older keeps `from`, uses prev_cursor as `to`; newer walks back; resume continues from resume_cursor', () => {
    let n = navStart({ from: 'e1:seq:1' });
    expect(n).toEqual({ base: { from: 'e1:seq:1' }, current: { from: 'e1:seq:1' }, newer: [] });
    n = navOlder(n, 'seq:3999');
    expect(n.current).toEqual({ from: 'e1:seq:1', to: 'seq:3999' });
    n = navOlder(n, 'seq:2999');
    expect(n.current).toEqual({ from: 'e1:seq:1', to: 'seq:2999' });
    expect(n.newer).toEqual([undefined, 'seq:3999']);
    n = navNewer(n)!;
    expect(n.current).toEqual({ from: 'e1:seq:1', to: 'seq:3999' });
    n = navNewer(n)!;
    expect(n.current).toEqual({ from: 'e1:seq:1' });
    expect('to' in n.current).toBe(false);
    expect(navNewer(n)).toBeNull();
    const r = navResume(navStart({ from: 'e1:seq:1', to: 'seq:9000' }), 'e1:seq:5000');
    expect(r).toEqual({ base: { from: 'e1:seq:1', to: 'seq:9000' }, current: { from: 'e1:seq:5000', to: 'seq:9000' }, newer: [] });
  });
  it('the newer stack is bounded (oldest dropped); inputs are not mutated', () => {
    const start = navStart({ from: 'seq:0', to: 'seq:100000' });
    let n = start;
    for (let i = 0; i < MAX_PAGE_STACK + 5; i++) n = navOlder(n, `seq:${99_000 - i * 1000}`);
    expect(n.newer).toHaveLength(MAX_PAGE_STACK);
    expect(n.newer.at(-1)).toBe(`seq:${99_000 - (MAX_PAGE_STACK + 3) * 1000}`);
    expect(start).toEqual({ base: { from: 'seq:0', to: 'seq:100000' }, current: { from: 'seq:0', to: 'seq:100000' }, newer: [] });
  });
});
