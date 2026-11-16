// @vitest-environment jsdom
// operator-upgrade logic — batch 6 specification tests (Opus-authored; Sonnet must not edit).
// ROOT browser defect D-L5W (first3 build): the archive view evicted rows past MAX_ANNOTATIONS_PER_CAPTURE while
// its label said "아카이브에서 불러옴 (전체 27,499행)" and copy could claim coverage=complete for an evicted subset.
// Contract: work/reports/operator-upgrade/logic/FUNCTIONS.md §B6.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';
import {
  ARCHIVE_PAGE_ROWS,
  ARCHIVE_WINDOW_ROWS,
  archiveNextStart,
  archivePrevStart,
  archiveSeekStart,
  archiveWindowAdd,
  archiveWindowAfterRow,
  archiveWindowCoverage,
  archiveWindowFull,
  archiveWindowLimit,
  archiveWindowStart,
  archiveWindowText,
  MAX_ANNOTATIONS_PER_CAPTURE,
  type ArchiveInfo,
  type ArchiveRowWindow,
} from './logicModel';
import { copyHeader } from './logicTools';

const w = (o: Partial<ArchiveRowWindow>): ArchiveRowWindow => ({ startAfter: null, first: null, last: null, held: 0, eof: false, ...o });
const info = (o: Partial<ArchiveInfo>): ArchiveInfo => ({
  state: 'complete',
  dir: '/h/logic-archive/lc-6-1',
  reason: null,
  error: null,
  rawEndSample: 3_000_000,
  rawBytes: 1,
  annotationRows: 27_499,
  rawTruncatedAt: null,
  annotationTruncatedAt: null,
  pruned: [],
  ...o,
});

describe('B6 window model', () => {
  it('window size equals the per-capture cap, so the archive view never has to evict', () => {
    expect(ARCHIVE_WINDOW_ROWS).toBe(MAX_ANNOTATIONS_PER_CAPTURE);
    expect(ARCHIVE_PAGE_ROWS).toBe(1000);
  });

  it('start / add / after_row follow the daemon paging (next_row = last row returned, null at the end)', () => {
    const a = archiveWindowStart(null);
    expect(a).toEqual(w({}));
    expect(archiveWindowAfterRow(a)).toBeNull();
    const b = archiveWindowAdd(a, { count: 3, nextRow: 2 });
    expect(b).toEqual(w({ first: 0, last: 2, held: 3 }));
    expect(a).toEqual(w({})); // never mutated
    expect(archiveWindowAfterRow(b)).toBe(2);
    const c = archiveWindowAdd(b, { count: 2, nextRow: null });
    expect(c).toEqual(w({ first: 0, last: 4, held: 5, eof: true }));
    expect(archiveWindowAdd(c, { count: 0, nextRow: null })).toEqual(c);
    // a window opened mid-archive
    const m = archiveWindowStart(19_999);
    expect(archiveWindowAfterRow(m)).toBe(19_999);
    expect(archiveWindowAdd(m, { count: 2, nextRow: 20_001 })).toEqual(w({ startAfter: 19_999, first: 20_000, last: 20_001, held: 2 }));
    // empty end page right after the start
    expect(archiveWindowAdd(m, { count: 0, nextRow: null })).toEqual(w({ startAfter: 19_999, eof: true }));
  });

  it('limit never lets the window grow past its cap', () => {
    expect(archiveWindowLimit(w({}))).toBe(1000);
    expect(archiveWindowLimit(w({ first: 0, last: 19_799, held: 19_800 }))).toBe(200);
    expect(archiveWindowLimit(w({ first: 0, last: 19_999, held: 20_000 }))).toBe(0);
    expect(archiveWindowFull(w({ held: 19_999 }))).toBe(false);
    expect(archiveWindowFull(w({ held: 20_000 }))).toBe(true);
  });

  it('previous / next / seek give exact window starts', () => {
    expect(archivePrevStart(w({}))).toBeNull();
    expect(archivePrevStart(w({ first: 0, last: 19_999, held: 20_000 }))).toBeNull();
    expect(archivePrevStart(w({ startAfter: 19_999, first: 20_000, last: 27_498, held: 7_499, eof: true }))).toEqual({ startAfter: null });
    expect(archivePrevStart(w({ startAfter: 24_999, first: 25_000, last: 27_498, held: 2_499, eof: true }))).toEqual({ startAfter: 4_999 });
    expect(archivePrevStart(w({ startAfter: 3, first: 4, last: 20_003, held: 20_000 }))).toEqual({ startAfter: null });
    expect(archiveNextStart(w({}))).toBeNull();
    expect(archiveNextStart(w({ first: 0, last: 19_999, held: 20_000 }))).toEqual({ startAfter: 19_999 });
    expect(archiveNextStart(w({ first: 0, last: 27_498, held: 27_499, eof: true }))).toBeNull();
    expect(archiveSeekStart(1, 27_499)).toEqual({ startAfter: null });
    expect(archiveSeekStart(2, 27_499)).toEqual({ startAfter: 0 });
    expect(archiveSeekStart(20_001, 27_499)).toEqual({ startAfter: 19_999 });
    expect(archiveSeekStart(27_499, 27_499)).toEqual({ startAfter: 27_497 });
    expect(archiveSeekStart(5, null)).toEqual({ startAfter: 3 });
    for (const bad of [0, -1, 27_500, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) expect(archiveSeekStart(bad, 27_499)).toBeNull();
  });

  it('coverage is complete only for a window that really holds every archive row', () => {
    const all = w({ first: 0, last: 27_498, held: 27_499, eof: true });
    expect(archiveWindowCoverage(null, all, 0)).toBe('unknown');
    expect(archiveWindowCoverage(info({ state: 'recording' }), all, 0)).toBe('live');
    expect(archiveWindowCoverage(info({}), all, 0)).toBe('complete');
    expect(archiveWindowCoverage(info({}), all, 1)).toBe('partial'); // evicted subset never complete
    expect(archiveWindowCoverage(info({}), w({ first: 0, last: 19_999, held: 20_000 }), 0)).toBe('partial'); // ROOT's case
    expect(archiveWindowCoverage(info({}), w({ startAfter: 19_999, first: 20_000, last: 27_498, held: 7_499, eof: true }), 0)).toBe('partial');
    expect(archiveWindowCoverage(info({ annotationRows: 30_000 }), all, 0)).toBe('partial'); // row count disagrees
    expect(archiveWindowCoverage(info({ annotationRows: null }), all, 0)).toBe('complete');
    expect(archiveWindowCoverage(info({ annotationRows: 0 }), w({ eof: true }), 0)).toBe('complete'); // empty archive
    expect(archiveWindowCoverage(info({ state: 'partial' }), all, 0)).toBe('partial');
  });

  it('window text names the held range, the total and the cap', () => {
    expect(archiveWindowText(w({}), 27_499, true)).toBe('아카이브 불러오는 중…');
    expect(archiveWindowText(w({}), 27_499, false)).toBe('아카이브 창 비어 있음');
    expect(archiveWindowText(w({ eof: true }), 0, false)).toBe('행 없음 (전체 0행)');
    expect(archiveWindowText(w({ startAfter: 9, eof: true }), null, false)).toBe('행 없음 (전체 ?행)');
    expect(archiveWindowText(w({ first: 0, last: 2, held: 3, eof: true }), 3, false)).toBe('행 1–3 / 전체 3행 (전체 표시)');
    expect(archiveWindowText(w({ first: 0, last: 19_999, held: 20_000 }), 27_499, false)).toBe('행 1–20,000 / 전체 27,499행 · 뒷부분 미표시 (창 최대 20,000행)');
    expect(archiveWindowText(w({ startAfter: 19_999, first: 20_000, last: 27_498, held: 7_499, eof: true }), 27_499, false)).toBe(
      '행 20,001–27,499 / 전체 27,499행 · 앞부분 미표시 (창 최대 20,000행)',
    );
    expect(archiveWindowText(w({ startAfter: 3, first: 4, last: 20_003, held: 20_000 }), null, false)).toBe('행 5–20,004 / 전체 ?행 · 앞뒤 미표시 (창 최대 20,000행)');
  });

  it('copy header carries the held archive rows', () => {
    const meta = { captureId: 'lc-6-1', samplerate: 1e6, source: 'archive' as const, scope: 'all' as const, fromSample: 0, toSample: 9, coverage: 'partial' as const };
    expect(copyHeader({ ...meta, archiveRows: { first: 0, last: 19_999, total: 27_499 } }, 20_000, 'hex')).toBe(
      '# loom logic copy\n# capture_id=lc-6-1\n# source=archive\n# scope=all\n# from_sample=0\n# to_sample=9\n# rows=20000\n# coverage=partial\n# archive_rows=0-19999/27499\n# radix=hex\n',
    );
    expect(copyHeader({ ...meta, archiveRows: { first: 4, last: 8, total: null } }, 5)).toContain('# archive_rows=4-8/?\n');
    expect(copyHeader(meta, 1)).not.toContain('archive_rows');
  });
});

// ---------------------------------------------------------------------------------------------
// Panel: ROOT's reproduction — complete archive of 27,499 rows, the daemon returns ≤275 rows per page (64 KiB).
const CAP = 'lc-6-1';
const RATE = 1_000_000;
const TOTAL = 27_499;
const PER_PAGE = 275;
const row = (n: number) => ({ decoder: 'uart-1', class: null, start_sample: 26 + n * 87, end_sample: 96 + n * 87, text: n === 26_000 ? '5A' : '55', row: n, source: 'archive', role: 'data' });

describe('B6 panel', () => {
  let h: Harness | null = null;
  const written: string[] = [];
  afterEach(() => {
    h?.dispose();
    h = null;
    document.body.replaceChildren();
    written.length = 0;
    vi.unstubAllGlobals();
    try {
      localStorage.clear();
    } catch {
      /* ignore */
    }
  });
  const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
  const settle = async () => {
    for (let k = 0; k < 8; k++) {
      for (let i = 0; i < 40; i++) await Promise.resolve();
      h!.frame();
    }
  };
  const rowCalls = () => h!.net.calls.filter((c) => c.method === 'logic.archive.read' && (c.params as { kind: string }).kind === 'annotations').map((c) => c.params as { limit: number; after_row?: number });
  const setup = async () => {
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: (t: string) => (written.push(t), Promise.resolve()) } });
    h = mountPanel('logic.analyzer');
    h.net.responders.set('logic.archive.read', (p) => {
      const r = p as { kind: string; after_row?: number; from_sample?: number; limit: number };
      if (r.kind === 'annotations') {
        const from = r.after_row === undefined ? 0 : r.after_row + 1;
        const n = Math.max(0, Math.min(r.limit, PER_PAGE, TOTAL - from));
        const rows = Array.from({ length: n }, (_, i) => row(from + i));
        const end = from + n >= TOTAL;
        return { capture_id: CAP, kind: 'annotations', rows, next_row: end ? null : from + n - 1, complete: end, truncated_at: null };
      }
      const f = r.from_sample ?? 0;
      return { capture_id: CAP, kind: 'edges', chunks: [], next_from: null, complete: true, truncated_at: null, range: { returned: [f, f] } };
    });
    h.net.emitControl(
      ev({
        kind: 'logic.capture_state',
        domain: 'logic',
        channel: CAP,
        seq: '20',
        origin: 'device',
        payload: { capture_id: CAP, state: 'stopped', reason: 'duration', source: 'synthetic', samplerate: RATE, channels: ['D0'], counters: { samples: 3_007_362, annotations: 9_847, dropped_annotations: 17_652 }, archive: { state: 'complete', dir: '/h/logic-archive/lc-6-1', annotation_rows: TOTAL, raw_end_sample: 3_007_362 } },
      }),
    );
    h.frame();
    const sel = q<HTMLSelectElement>('logic-view-source');
    sel.value = 'archive';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
  };
  const more = async () => {
    for (let i = 0; i < 10 && !q('logic-archive-more').hidden; i++) {
      q<HTMLButtonElement>('logic-archive-more').click();
      await settle();
    }
  };
  const copyText = async () => {
    q('logic-copy').click();
    await settle();
    return written[written.length - 1]!;
  };

  it("ROOT's case: loading to the end of the first window holds exactly 20,000 rows and says which part is missing", async () => {
    await setup();
    expect(q('logic-archive-window').hidden).toBe(false);
    await more();
    expect(q('logic-archive-more').hidden).toBe(true);
    const calls = rowCalls();
    expect(calls.every((c) => c.limit >= 1 && c.limit <= 1000)).toBe(true);
    expect(calls[calls.length - 1]!.limit).toBe(20_000 - Math.floor(20_000 / PER_PAGE) * PER_PAGE); // the last page is trimmed to the cap
    expect(q('logic-archive-window').textContent).toBe('행 1–20,000 / 전체 27,499행 · 뒷부분 미표시 (창 최대 20,000행)');
    const ann = q('logic-ann-info').textContent!;
    expect(ann.startsWith('20,000행 · 아카이브에서 불러옴 · 행 1–20,000 / 전체 27,499행 · 뒷부분 미표시 (창 최대 20,000행)')).toBe(true);
    expect(ann).not.toContain('(전체 27,499행)');
    expect(q<HTMLButtonElement>('logic-archive-prev').disabled).toBe(true);
    expect(q<HTMLButtonElement>('logic-archive-next').disabled).toBe(false);
    const t = await copyText();
    expect(t).toContain('# source=archive\n');
    expect(t).toContain('# rows=20000\n');
    expect(t).toContain('# coverage=partial\n');
    expect(t).toContain('# archive_rows=0-19999/27499\n');
  });

  it('next / previous / seek move a bounded window over the whole archive; search names its scope', async () => {
    await setup();
    await more();
    q<HTMLButtonElement>('logic-archive-next').click();
    await settle();
    await more();
    const afterNext = rowCalls().filter((c) => c.after_row !== undefined && c.after_row >= 19_999);
    expect(afterNext[0]).toEqual({ capture_id: CAP, kind: 'annotations', limit: 1000, after_row: 19_999 });
    expect(q('logic-archive-window').textContent).toBe('행 20,001–27,499 / 전체 27,499행 · 앞부분 미표시 (창 최대 20,000행)');
    expect(q('logic-ann-info').textContent!.startsWith('7,499행 · 아카이브에서 불러옴 · 행 20,001–27,499')).toBe(true);
    expect(q<HTMLButtonElement>('logic-archive-next').disabled).toBe(true);
    expect(q<HTMLButtonElement>('logic-archive-prev').disabled).toBe(false);
    expect(await copyText()).toContain('# archive_rows=20000-27498/27499\n# radix=');

    // search runs over the held window only and says so
    const s = q<HTMLInputElement>('logic-search');
    s.value = 'Z'; // text search over byte values: row 26,000 is 0x5A
    s.dispatchEvent(new Event('input', { bubbles: true }));
    h!.frame();
    expect(q('logic-search-status').textContent).toBe('0/1 · 창 행 20,001–27,499 안에서');

    // seek to row 5 → window rows 5–20,004
    const seek = q<HTMLInputElement>('logic-archive-seek');
    seek.value = '5';
    q<HTMLButtonElement>('logic-archive-seek-go').click();
    await settle();
    await more();
    expect(rowCalls().some((c) => c.after_row === 3)).toBe(true);
    expect(q('logic-archive-window').textContent).toBe('행 5–20,004 / 전체 27,499행 · 앞뒤 미표시 (창 최대 20,000행)');

    // an out-of-range seek does not move the window
    const before = rowCalls().length;
    seek.value = '27500';
    q<HTMLButtonElement>('logic-archive-seek-go').click();
    await settle();
    expect(rowCalls().length).toBe(before);
    expect(q('logic-archive-window').textContent).toBe('행 5–20,004 / 전체 27,499행 · 앞뒤 미표시 (창 최대 20,000행)');

    // previous from row 5 → back to the start (no after_row)
    const atPrev = rowCalls().length;
    q<HTMLButtonElement>('logic-archive-prev').click();
    await settle();
    expect(rowCalls()[atPrev]).toEqual({ capture_id: CAP, kind: 'annotations', limit: 1000 }); // the window's first request: no after_row
    expect(q('logic-archive-window').textContent!.startsWith('행 1–')).toBe(true);
  });

  it('the navigation is archive-view only', async () => {
    await setup();
    const sel = q<HTMLSelectElement>('logic-view-source');
    sel.value = 'live';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    h!.frame();
    for (const id of ['logic-archive-window', 'logic-archive-prev', 'logic-archive-next', 'logic-archive-seek', 'logic-archive-seek-go', 'logic-archive-more']) expect(q(id).hidden).toBe(true);
  });
});
