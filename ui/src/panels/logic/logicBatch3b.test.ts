// @vitest-environment jsdom
// operator-upgrade logic — batch 3b specification tests (Opus-authored; Sonnet must not edit).
// Findings: L1-R2 whole evidence vs bounded preview (archive view, daemon export), L5-R2 honest backfill.
// Contract: work/reports/operator-upgrade/logic/FUNCTIONS.md (B3b-*).
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';
import {
  ARCHIVE_EDGE_PAGES,
  ARCHIVE_NOTE,
  archiveLabel,
  archiveViewCoverage,
  CaptureBook,
  CaptureData,
  readArchiveEdges,
  readArchiveInfo,
  readArchiveRows,
  readEdges,
  type ArchiveInfo,
} from './logicModel';

const CAP = 'lc-3b-1';
const RATE = 1_000_000;
const info = (o: Partial<ArchiveInfo>): ArchiveInfo => ({
  state: 'complete',
  dir: '/h/logic-archive/lc-3b-1',
  reason: null,
  error: null,
  rawEndSample: 5000,
  rawBytes: 1234,
  annotationRows: 1515,
  rawTruncatedAt: null,
  annotationTruncatedAt: null,
  pruned: [],
  ...o,
});

describe('B3b model', () => {
  it('reads the capture_state archive object tolerantly', () => {
    expect(
      readArchiveInfo({ state: 'partial', dir: '/d', error: 'archive quota 256 MiB reached', raw_end_sample: 900, raw_bytes: 10, annotation_rows: 7, raw_truncated_at_sample: 900, annotation_truncated_at_row: null, pruned: ['lc-1-1', 3] }),
    ).toEqual({ state: 'partial', dir: '/d', reason: null, error: 'archive quota 256 MiB reached', rawEndSample: 900, rawBytes: 10, annotationRows: 7, rawTruncatedAt: 900, annotationTruncatedAt: null, pruned: ['lc-1-1'] });
    expect(readArchiveInfo({ state: 'disabled', reason: 'archive not configured' })).toMatchObject({ state: 'disabled', reason: 'archive not configured', annotationRows: null });
    expect(readArchiveInfo({ state: 'weird' })!.state).toBe('unknown');
    expect(readArchiveInfo({})!.state).toBe('unknown');
    for (const bad of [null, 'x', 3, []]) expect(readArchiveInfo(bad)).toBeNull();
  });

  it('capture_state keeps the last reported archive until a newer one replaces it', () => {
    const b = new CaptureBook();
    b.applyState({ capture_id: CAP, state: 'running', archive: { state: 'recording', dir: '/d', annotation_rows: 1 } }, CAP, 1n);
    expect(b.captures.get(CAP)!.info!.archive!.state).toBe('recording');
    b.applyState({ capture_id: CAP, state: 'stopping' }, CAP, 2n);
    expect(b.captures.get(CAP)!.info!.archive!.state).toBe('recording');
    b.applyState({ capture_id: CAP, state: 'stopped', archive: { state: 'complete', dir: '/d', annotation_rows: 9 } }, CAP, 3n);
    expect(b.captures.get(CAP)!.info!.archive).toMatchObject({ state: 'complete', annotationRows: 9 });
    b.applyState({ capture_id: 'lc-old-1', state: 'stopped' }, 'lc-old-1', 4n);
    expect(b.captures.get('lc-old-1')!.info!.archive ?? null).toBeNull();
  });

  it('chip texts are exact and never claim completeness they do not have', () => {
    const t = (a: ArchiveInfo | null) => archiveLabel(a);
    expect(t(null)).toEqual({ text: '아카이브 정보 없음', tone: 'muted', title: '이 캡처의 capture_state에 archive가 없습니다 (이전 엔진이거나 아직 수신 전)' });
    expect(t(info({ state: 'disabled', reason: 'archive not configured', dir: null }))).toMatchObject({ text: '아카이브 꺼짐 — archive not configured', tone: 'muted' });
    expect(t(info({ state: 'pending', dir: null })).text).toBe('아카이브 준비 중');
    expect(t(info({ state: 'recording', annotationRows: 12_000 }))).toMatchObject({ text: '아카이브 기록 중 · 디코드 12,000행', tone: 'ok', title: '/h/logic-archive/lc-3b-1' });
    expect(t(info({})).text).toBe('아카이브 완전 · 디코드 1,515행 · 원시 sample 5,000');
    expect(t(info({ state: 'partial', error: 'archive quota 256 MiB reached', rawTruncatedAt: 900, rawEndSample: 900 }))).toMatchObject({
      text: '아카이브 일부 — archive quota 256 MiB reached · 원시 sample 900에서 잘림',
      tone: 'warn',
    });
    expect(t(info({ state: 'partial', error: null, annotationTruncatedAt: 70 })).text).toBe('아카이브 일부 — 잘림 · 디코드 70행에서 잘림');
    expect(t(info({ state: 'failed', error: 'archive dir exists', dir: null }))).toMatchObject({ text: '아카이브 실패 — archive dir exists', tone: 'bad' });
    expect(t(info({ state: 'unknown' })).text).toBe('아카이브 상태 미확인');
  });

  it('archive pages become ordinary chunks / rows (same identity as live rows)', () => {
    const e = readArchiveEdges(
      { capture_id: CAP, kind: 'edges', chunks: [{ capture_id: CAP, seq: null, source: 'archive', samplerate: RATE, sample_start: 0, sample_end: 50, mask: 255, initial: 1, final: 0, active: 1, edges: [[10, 0]], dropped: 0 }], next_from: 50, complete: false, truncated_at: null },
      CAP,
    );
    expect([e.chunks.length, e.chunks[0]!.start, e.chunks[0]!.end, e.nextFrom, e.complete, e.truncatedAt]).toEqual([1, 0, 50, 50, false, null]);
    expect(readArchiveEdges({ chunks: 'x' }, CAP)).toEqual({ chunks: [], nextFrom: null, complete: false, truncatedAt: null });
    const r = readArchiveRows(
      { rows: [{ decoder: 'uart-1', class: null, start_sample: 10, end_sample: 90, text: '41', row: 0, source: 'archive' }, { decoder: null, class: '_raw', start_sample: null, end_sample: null, text: 'garbled', row: 1 }], next_row: 1, complete: false, truncated_at: 7 },
      CAP,
      RATE,
    );
    expect([r.rows.map((a) => a.key), r.rawLines, r.nextRow, r.complete, r.truncatedAt, r.count]).toEqual([['uart-1:10:90:41'], ['?: garbled'], 1, false, 7, 2]);
  });

  it('clearChunks / markNotLoaded keep the raw view honest', () => {
    const c = new CaptureData(CAP);
    c.addChunk(readEdges({ capture_id: CAP, samplerate: RATE, sample_start: 0, sample_end: 100, initial: 1, edges: [[50, 0]], dropped: 2 }, CAP, 1n, null)!);
    const v = c.version;
    c.clearChunks();
    expect([c.chunks.length, c.edgeCount, c.unknown.length, c.droppedEdges, c.evictedTo, c.range]).toEqual([0, 0, 0, 0, 0, null]);
    expect(c.version).toBeGreaterThan(v);
    expect(c.samplerate).toBe(RATE); // edgeRate kept
    c.addChunk(readEdges({ capture_id: CAP, samplerate: RATE, sample_start: 0, sample_end: 100, initial: 1, edges: [], dropped: 0 }, CAP, 1n, null)!);
    c.markNotLoaded(100, 400, 'x');
    c.markNotLoaded(5, 5, 'empty ignored');
    expect(c.unknown).toEqual([{ start: 100, end: 400, cause: 'not_loaded', detail: 'x' }]);
    expect(c.stateAt(150)).toBeNull();
    expect(c.stateAt(50)).toBe(1);
  });

  it('archive view coverage', () => {
    expect(archiveViewCoverage(null, true)).toBe('unknown');
    expect(archiveViewCoverage(info({ state: 'recording' }), true)).toBe('live');
    expect(archiveViewCoverage(info({}), true)).toBe('complete');
    expect(archiveViewCoverage(info({}), false)).toBe('partial');
    expect(archiveViewCoverage(info({ state: 'partial' }), true)).toBe('partial');
  });
});

// ---------------------------------------------------------------------------------------------
describe('B3b panel', () => {
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
  const flush = async () => {
    for (let i = 0; i < 12; i++) await Promise.resolve();
  };
  const settle = async () => {
    for (let k = 0; k < 6; k++) {
      await flush();
      h!.frame();
    }
  };
  const stopped = (archive: Record<string, unknown> | null) =>
    ev({
      kind: 'logic.capture_state',
      domain: 'logic',
      channel: CAP,
      seq: '20',
      origin: 'device',
      payload: { capture_id: CAP, state: 'stopped', reason: 'duration', source: 'synthetic', samplerate: RATE, channels: ['D0'], counters: { samples: 5000, annotations: 3, dropped_annotations: 1512 }, ...(archive ? { archive } : {}) },
    });
  /** live panel holds 1 row and raw 0..1000; the archive holds 3 rows (2 pages) and raw to 5000 */
  const setup = (archive: Record<string, unknown> | null = { state: 'complete', dir: '/h/logic-archive/lc-3b-1', annotation_rows: 3, raw_end_sample: 5000 }) => {
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: (t: string) => (written.push(t), Promise.resolve()) } });
    h = mountPanel('logic.analyzer');
    h.net.responders.set('logic.archive.read', (p) => {
      const r = p as { kind: string; after_row?: number; from_sample?: number; to_sample?: number };
      if (r.kind === 'annotations') {
        return r.after_row === undefined
          ? { capture_id: CAP, kind: 'annotations', rows: [{ decoder: 'uart-1', class: null, start_sample: 10, end_sample: 90, text: '4C', row: 0, source: 'archive' }, { decoder: 'uart-1', class: null, start_sample: 110, end_sample: 190, text: '4F', row: 1, source: 'archive' }], next_row: 1, complete: false, truncated_at: null }
          : { capture_id: CAP, kind: 'annotations', rows: [{ decoder: 'uart-1', class: null, start_sample: 4100, end_sample: 4180, text: '4D', row: 2, source: 'archive' }], next_row: null, complete: true, truncated_at: null };
      }
      // edges: always more than one page can carry → the page limit leaves the rest "not loaded"
      const from = r.from_sample ?? 0;
      return { capture_id: CAP, kind: 'edges', chunks: [{ capture_id: CAP, seq: null, source: 'archive', samplerate: RATE, sample_start: from, sample_end: from + 10, mask: 255, initial: 1, final: 1, active: 0, edges: [], dropped: 0 }], next_from: from + 10, complete: false, truncated_at: null };
    });
    h.net.responders.set('logic.export', () => ({ path: '/h/logic-archive/lc-3b-1/export-annotations-0-5000-1.csv', bytes: 120, rows: 3, complete: true, limit_hit: false, archive_state: 'complete', truncated_at: null }));
    h.net.emitControl(ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, seq: '10', origin: 'device', payload: { capture_id: CAP, state: 'running', source: 'synthetic', samplerate: RATE, channels: ['D0'] } }));
    h.batch([
      ev({ kind: 'logic.edges', domain: 'logic', channel: CAP, seq: '11', payload: { capture_id: CAP, samplerate: RATE, sample_start: 0, sample_end: 1000, initial: 1, edges: [], dropped: 0 } }),
      ev({ kind: 'logic.annotations', domain: 'logic', channel: CAP, seq: '12', payload: { capture_id: CAP, samplerate: RATE, annotations: [{ decoder: 'uart-1', class: null, start_sample: 10, end_sample: 90, text: '4C' }], dropped: 1512 } }),
    ]);
    h.net.emitControl(stopped(archive));
    h.frame();
  };
  const archiveCalls = (kind: string) => h!.net.calls.filter((c) => c.method === 'logic.archive.read' && (c.params as { kind: string }).kind === kind);

  it('status chip names the archive; the archive view is only offered when it can be read', () => {
    setup({ state: 'disabled', reason: 'archive not configured' });
    expect(q('logic-archive-state').textContent).toBe('아카이브 꺼짐 — archive not configured');
    expect(q<HTMLSelectElement>('logic-view-source').disabled).toBe(true);
    expect(q<HTMLButtonElement>('logic-export').disabled).toBe(true);
    h!.dispose();
    setup();
    expect(q('logic-archive-state').textContent).toBe('아카이브 완전 · 디코드 3행 · 원시 sample 5,000');
    expect(q('logic-archive-state').dataset.tone).toBe('ok');
    expect(q<HTMLSelectElement>('logic-view-source').disabled).toBe(false);
    expect(h!.net.calls.some((c) => c.method.startsWith('logic.archive'))).toBe(false); // nothing read until asked
  });

  it('archive view pages the whole decoded history and labels it as archive evidence', async () => {
    setup();
    const sel = q<HTMLSelectElement>('logic-view-source');
    sel.value = 'archive';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    const rows = archiveCalls('annotations').map((c) => c.params);
    expect(rows).toEqual([
      { capture_id: CAP, kind: 'annotations', limit: 1000 },
      { capture_id: CAP, kind: 'annotations', limit: 1000, after_row: 1 },
    ]);
    expect([...h!.host.querySelectorAll('.lg-table .vt-row[data-start]')].map((r) => r.getAttribute('data-start'))).toEqual(['10', '110', '4100']);
    expect(q('logic-ann-info').textContent).toContain('3행 · 아카이브에서 불러옴 · 행 1–3 / 전체 3행 (전체 표시)') // B6 amended label;
    expect(q('logic-follow').getAttribute('aria-pressed')).toBe('false');
    // back to the live view: the panel's own rows again
    sel.value = 'live';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    h!.frame();
    expect(h!.host.querySelectorAll('.lg-table .vt-row[data-start]').length).toBe(1);
  });

  it('archive edges load per view window, bounded, with the remainder marked not loaded — no refetch loop', async () => {
    setup();
    const sel = q<HTMLSelectElement>('logic-view-source');
    sel.value = 'archive';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    const edges = archiveCalls('edges');
    expect(edges.length).toBe(ARCHIVE_EDGE_PAGES);
    const first = edges[0]!.params as { from_sample: number; to_sample: number; limit: number };
    expect(first.limit).toBe(2048);
    expect(first.to_sample).toBeGreaterThan(first.from_sample);
    expect((edges[1]!.params as { from_sample: number }).from_sample).toBe(first.from_sample + 10);
    await settle();
    expect(archiveCalls('edges').length).toBe(ARCHIVE_EDGE_PAGES); // same window: no new reads
  });

  it('export writes on the daemon and says exactly where; never claims a browser download', async () => {
    setup();
    q<HTMLButtonElement>('logic-export').click();
    await settle();
    const call = h!.net.calls.find((c) => c.method === 'logic.export')!;
    expect(call.params).toEqual({ capture_id: CAP, kind: 'annotations', format: 'csv' });
    const line = q('logic-export-result').textContent!;
    expect(line).toBe('daemon에 저장됨: /h/logic-archive/lc-3b-1/export-annotations-0-5000-1.csv · 3행 · 완전');
    expect(line).not.toContain('다운로드');
    h!.net.responders.set('logic.export', () => {
      throw Object.assign(new Error('archive quota 256 MiB reached; export refused'), { code: 'usage' });
    });
    const kind = q<HTMLSelectElement>('logic-export-kind');
    kind.value = 'edges:csv';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    q<HTMLButtonElement>('logic-export').click();
    await settle();
    expect(h!.net.calls.filter((c) => c.method === 'logic.export').pop()!.params).toEqual({ capture_id: CAP, kind: 'edges', format: 'csv' });
    expect(q('logic-export-result').textContent).toContain('내보내기 실패');
  });

  it('copy from the archive view is labelled as archive evidence', async () => {
    setup();
    const sel = q<HTMLSelectElement>('logic-view-source');
    sel.value = 'archive';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    q('logic-copy').click();
    await settle();
    expect(written[0]).toContain('# source=archive\n');
    expect(written[0]).toContain('# coverage=complete\n');
    expect(written[0]).toContain('# rows=3\n');
  });

  it('the pre-start overload warning points to the archive', () => {
    h = mountPanel('logic.analyzer');
    h.frame();
    expect(q('logic-archive-note').hidden).toBe(true);
    const f = q('logic-config').querySelector<HTMLInputElement>('[name="text"]')!;
    f.value = 'A'.repeat(256);
    f.dispatchEvent(new Event('input', { bubbles: true }));
    const p = q('logic-config').querySelector<HTMLInputElement>('[name="frame_period"]')!;
    p.value = '25';
    p.dispatchEvent(new Event('input', { bubbles: true }));
    h.frame();
    expect(q('logic-archive-note').hidden).toBe(false);
    expect(q('logic-archive-note').textContent).toBe(ARCHIVE_NOTE);
  });
});
