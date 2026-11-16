// PSU-03 (operator upgrade B4): journal history section of psu.gauge. No new logger — pages of the
// domain read RPC `psu.history` (crates/loom-psu/src/gateway/history.rs) over an explicit wall-clock
// window: merged statistics, a truthful coverage line (requested vs actual range, losses, journal
// gaps, index retention, partial/aborted) and a CSV built from row pages (bounded by CSV_MAX_ROWS).
// Contract: work/reports/operator-upgrade/psu/FUNCTIONS.md §Batch 4.

import type { PanelContext } from '../../sdk';

/** Window presets — sent as `last_ms` on the first page (server clock), then pinned to the returned range. */
export const HISTORY_PRESETS: readonly { id: '15m' | '1h' | '8h' | '24h'; label: string; ms: number }[] = [
  { id: '15m', label: '15분', ms: 900_000 },
  { id: '1h', label: '1시간', ms: 3_600_000 },
  { id: '8h', label: '8시간', ms: 28_800_000 },
  { id: '24h', label: '24시간', ms: 86_400_000 },
];
/** Rows per CSV page (gateway HISTORY_MAX_ROWS). */
export const HISTORY_PAGE_ROWS = 5_000;
/** Most pages one run reads (each page scans ≤ 32 MiB of journal). */
export const HISTORY_MAX_PAGES = 200;
/** Most CSV rows held in the browser (≈ 13.9 h at 5 Hz); beyond → partial CSV, said so in file and UI. */
export const CSV_MAX_ROWS = 250_000;

/** Mergeable statistics of one quantity (gateway HistStat). */
export interface HistStat {
  n: number;
  min: number;
  max: number;
  sum: number;
  last: number | null;
}
export interface HistPoint {
  seq: number;
  wallMs: number;
}
export interface HistRow {
  seq: number;
  wallMs: number;
  v: number;
  i: number;
  p: number;
  /** B6: canonical journal ref of this record, `e<epoch>:seq:<seq>`; null when the daemon did not send one. */
  ref: string | null;
  /** B6: device/link generation of this record; null when unknown. */
  generation: number | null;
  /** B6: clock id of this record's timestamp; null when unknown. */
  clockId: string | null;
}
/** One validated `psu.history` page. */
export interface HistoryPage {
  fromMs: number;
  toMs: number;
  nowMs: number | null;
  samples: number;
  losses: number;
  v: HistStat | null;
  i: HistStat | null;
  p: HistStat | null;
  first: HistPoint | null;
  last: HistPoint | null;
  nextCursor: string | null;
  beforeIndex: boolean;
  indexOldestMs: number | null;
  /** number of journal gap entries reported on this page */
  journalGaps: number;
  rows: HistRow[];
}
/** Pages of one window merged in order. */
export interface HistoryAgg {
  fromMs: number;
  toMs: number;
  nowMs: number | null;
  pages: number;
  samples: number;
  losses: number;
  v: HistStat | null;
  i: HistStat | null;
  p: HistStat | null;
  first: HistPoint | null;
  last: HistPoint | null;
  beforeIndex: boolean;
  indexOldestMs: number | null;
  journalGaps: number;
  /** the last merged page had no next_cursor */
  done: boolean;
  /** stopped early by this client */
  cut: null | 'aborted' | 'page_cap' | 'row_cap';
}
export type HistoryQuery = { lastMs: number } | { fromMs: number; toMs: number };
export interface HistoryRun {
  agg: HistoryAgg | null;
  rows: HistRow[];
}

/** B4-M1: validates one RPC result (bare or `{data}` envelope); null when the shape is not a history page. */
export function readHistoryPage(raw: unknown): HistoryPage | null {
  const isObj = (x: unknown): x is Record<string, unknown> => typeof x === 'object' && x !== null && !Array.isArray(x);
  const fin = (x: unknown): x is number => typeof x === 'number' && Number.isFinite(x);
  const cnt = (x: unknown): x is number => fin(x) && Number.isInteger(x) && x >= 0;
  if (!isObj(raw)) return null;
  const o = isObj(raw.data) ? raw.data : raw;
  const range = o.range;
  if (!isObj(range) || !fin(range.from_ms) || !fin(range.to_ms)) return null;
  if (!cnt(o.samples) || !cnt(o.losses)) return null;
  if (o.next_cursor !== null && typeof o.next_cursor !== 'string') return null;
  if (typeof o.before_index !== 'boolean') return null;
  if (!Array.isArray(o.journal_gaps)) return null;
  const stats = o.stats;
  if (!isObj(stats)) return null;
  let bad = false;
  const st = (x: unknown): HistStat | null => {
    if (x === null) return null;
    if (!isObj(x) || !cnt(x.n) || x.n < 1 || !fin(x.min) || !fin(x.max) || !fin(x.sum) || !(x.last === null || fin(x.last))) {
      bad = true;
      return null;
    }
    return { n: x.n, min: x.min, max: x.max, sum: x.sum, last: x.last };
  };
  const pt = (x: unknown): HistPoint | null => {
    if (x === null) return null;
    if (!isObj(x) || !fin(x.seq) || !fin(x.wall_ms)) {
      bad = true;
      return null;
    }
    return { seq: x.seq, wallMs: x.wall_ms };
  };
  const v = st(stats.voltage);
  const i = st(stats.current);
  const p = st(stats.power);
  const first = pt(o.first);
  const last = pt(o.last);
  if (bad) return null;
  const rows: HistRow[] = [];
  if (o.rows !== undefined) {
    if (!Array.isArray(o.rows)) return null;
    for (const r of o.rows as unknown[]) {
      if (!isObj(r) || !fin(r.seq) || !fin(r.wall_ms) || !fin(r.v) || !fin(r.i) || !fin(r.p)) return null;
      const ref = typeof r.ref === 'string' && /^e\d+:seq:\d+$/.test(r.ref) ? r.ref : null;
      const generation = typeof r.generation === 'number' && Number.isInteger(r.generation) && r.generation >= 0 ? r.generation : null;
      const clockId = typeof r.clock_id === 'string' && r.clock_id !== '' ? r.clock_id : null;
      rows.push({ seq: r.seq, wallMs: r.wall_ms, v: r.v, i: r.i, p: r.p, ref, generation, clockId });
    }
  }
  const index = o.index;
  return {
    fromMs: range.from_ms,
    toMs: range.to_ms,
    nowMs: fin(o.now_ms) ? o.now_ms : null,
    samples: o.samples,
    losses: o.losses,
    v,
    i,
    p,
    first,
    last,
    nextCursor: o.next_cursor,
    beforeIndex: o.before_index,
    indexOldestMs: isObj(index) && fin(index.oldest_ms) ? index.oldest_ms : null,
    journalGaps: o.journal_gaps.length,
    rows,
  };
}

/** B4-M2: merges a LATER stat into an earlier one (null = no samples). Pure, never mutates inputs. */
export function mergeHistStat(a: HistStat | null, b: HistStat | null): HistStat | null {
  if (a === null && b === null) return null;
  if (a === null) return { ...(b as HistStat) };
  if (b === null) return { ...a };
  return { n: a.n + b.n, min: Math.min(a.min, b.min), max: Math.max(a.max, b.max), sum: a.sum + b.sum, last: b.last ?? a.last };
}

/** B4-M3: folds the next page into the aggregate (`agg === null` = first page). Pure. */
export function addPage(agg: HistoryAgg | null, page: HistoryPage): HistoryAgg {
  if (agg === null) {
    return {
      fromMs: page.fromMs,
      toMs: page.toMs,
      nowMs: page.nowMs,
      pages: 1,
      samples: page.samples,
      losses: page.losses,
      v: page.v,
      i: page.i,
      p: page.p,
      first: page.first,
      last: page.last,
      beforeIndex: page.beforeIndex,
      indexOldestMs: page.indexOldestMs,
      journalGaps: page.journalGaps,
      done: page.nextCursor === null,
      cut: null,
    };
  }
  return {
    ...agg,
    first: agg.first ?? page.first,
    pages: agg.pages + 1,
    samples: agg.samples + page.samples,
    losses: agg.losses + page.losses,
    journalGaps: agg.journalGaps + page.journalGaps,
    v: mergeHistStat(agg.v, page.v),
    i: mergeHistStat(agg.i, page.i),
    p: mergeHistStat(agg.p, page.p),
    last: page.last ?? agg.last,
    done: page.nextCursor === null,
  };
}

/** B4-M4: complete = done, not cut, no journal gaps, start inside the index. */
export function isComplete(a: HistoryAgg): boolean {
  return a.done && a.cut === null && a.journalGaps === 0 && !a.beforeIndex;
}

/** B4-M5: local `MM-DD HH:MM:SS` (24 h, zero padded) of an epoch-ms instant. */
export function dateClock(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, '0');
  return `${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

/** B4-M6: one-line coverage statement (requested vs actual range, counts, completeness or every partial reason). */
export function coverageText(a: HistoryAgg): string {
  const range = `요청 ${dateClock(a.fromMs)} – ${dateClock(a.toMs)} · 실제 ${a.first ? dateClock(a.first.wallMs) : '—'} – ${a.last ? dateClock(a.last.wallMs) : '—'} · 샘플 ${a.samples} · 측정 손실 ${a.losses}`;
  const reasons: string[] = [];
  if (a.beforeIndex) reasons.push(`색인 시작 ${a.indexOldestMs !== null ? dateClock(a.indexOldestMs) : '없음'} 이전 미포함`);
  if (a.journalGaps > 0) reasons.push(`저널 손실 ${a.journalGaps}건`);
  if (a.cut === 'aborted') reasons.push('중지됨');
  else if (a.cut === 'page_cap') reasons.push(`페이지 상한 ${HISTORY_MAX_PAGES} 도달`);
  else if (a.cut === 'row_cap') reasons.push(`CSV 행 상한 ${CSV_MAX_ROWS} 도달`);
  if (!a.done && a.cut === null) reasons.push('미완료');
  const status = isComplete(a) ? '완전 (요청 구간 전체 조회)' : `부분: ${reasons.join(', ')}`;
  const tail = a.nowMs !== null && a.toMs > a.nowMs ? ` · 끝 시각이 조회 시각 이후 — ${dateClock(a.nowMs)}까지만 존재` : '';
  return `${range} · ${status}${tail}`;
}

/** B4-M7: RPC params of one page. */
export function historyParams(ch: string, q: HistoryQuery, rows: number, cursor: string | null): Record<string, unknown> {
  return {
    ch,
    ...('lastMs' in q ? { last_ms: q.lastMs } : { from_ms: q.fromMs, to_ms: q.toMs }),
    ...(rows > 0 ? { rows } : {}),
    ...(cursor !== null ? { cursor } : {}),
  };
}

/** B4-M8: reads pages until done/cap/abort; the window is pinned to the first page's range for every later page. */
export async function runHistory(
  query: (params: Record<string, unknown>, signal: AbortSignal) => Promise<unknown>,
  ch: string,
  q: HistoryQuery,
  opts: { rows: number; maxPages: number; maxRows: number; signal: AbortSignal; onPage?: (agg: HistoryAgg) => void },
): Promise<HistoryRun> {
  let agg: HistoryAgg | null = null;
  const rows: HistRow[] = [];
  let cursor: string | null = null;
  for (let n = 0; ; n++) {
    if (opts.signal.aborted) {
      if (agg) agg = { ...agg, cut: 'aborted' };
      break;
    }
    if (n >= opts.maxPages) {
      if (agg) agg = { ...agg, cut: 'page_cap' };
      break;
    }
    const qn: HistoryQuery = n === 0 || agg === null ? q : { fromMs: agg.fromMs, toMs: agg.toMs };
    const raw = await query(historyParams(ch, qn, opts.rows, cursor), opts.signal);
    if (opts.signal.aborted) {
      if (agg) agg = { ...agg, cut: 'aborted' };
      break;
    }
    const page = readHistoryPage(raw);
    if (page === null) throw new Error('psu.history: unexpected result shape');
    agg = addPage(agg, page);
    rows.push(...page.rows);
    opts.onPage?.(agg);
    if (opts.rows > 0 && rows.length >= opts.maxRows) {
      if (rows.length > opts.maxRows || page.nextCursor !== null) {
        rows.length = opts.maxRows;
        agg = { ...agg, cut: 'row_cap' };
      }
      break;
    }
    if (page.nextCursor === null) break;
    cursor = page.nextCursor;
  }
  return { agg, rows };
}

/** B4-M9: CSV text — one `#` metadata line, a header, one line per row. */
export function csvText(ch: string, a: HistoryAgg, rows: readonly HistRow[]): string {
  const q = (s: string) => (/["\n,]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);
  const iso = (ms: number) => new Date(ms).toISOString();
  const lines = [
    `# loom psu history,ch=${ch},from=${iso(a.fromMs)},to=${iso(a.toMs)},samples=${a.samples},rows=${rows.length},losses=${a.losses},complete=${isComplete(a)},${q(`coverage=${coverageText(a)}`)}`,
    'seq,wall_ms,wall_iso,v,i,p,ref,generation,clock_id',
    ...rows.map((r) => `${r.seq},${r.wallMs},${iso(r.wallMs)},${String(r.v)},${String(r.i)},${String(r.p)},${q(r.ref ?? '')},${r.generation ?? ''},${q(r.clockId ?? '')}`),
  ];
  return `${lines.join('\n')}\n`;
}

/** B4-M10: browser download of `text` as `name` (Blob URL + temporary <a download>); false when the environment cannot. */
function download(name: string, text: string): boolean {
  try {
    if (typeof URL.createObjectURL !== 'function') return false;
    const url = URL.createObjectURL(new Blob([text], { type: 'text/csv' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1_000);
    return true;
  } catch {
    return false;
  }
}

export interface PsuHistorySection {
  root: HTMLElement;
  dispose(): void;
}

/** B4-D1: the history section of a bound gauge (DOM contract in FUNCTIONS.md §B4.3). */
export function createPsuHistory(ctx: PanelContext, ch: string): PsuHistorySection {
  const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  };
  const tid = <E extends HTMLElement>(e: E, id: string): E => {
    e.setAttribute('data-testid', id);
    return e;
  };
  const root = tid(el('section', 'psu-history'), 'psu-history');
  root.setAttribute('aria-label', '측정 기록 조회 (저널)');
  const head = el('div', 'psu-hist-head');
  head.append(el('span', 'psu-hist-title', '기록 조회 (저널)'));
  const presetBtns: HTMLButtonElement[] = [];
  for (const p of HISTORY_PRESETS) {
    const b = tid(el('button', undefined, p.label), `psu-hist-preset-${p.id}`);
    b.type = 'button';
    b.title = `최근 ${p.label} — 저널에 기록된 psu.meas 통계`;
    presetBtns.push(b);
    head.append(b);
    b.addEventListener('click', () => void run({ lastMs: p.ms }, 'stats'));
  }
  const dtIn = (id: string) => {
    const i = tid(el('input'), id);
    i.type = 'datetime-local';
    return i;
  };
  const fromIn = dtIn('psu-hist-from');
  const toIn = dtIn('psu-hist-to');
  const fromL = el('label', 'psu-field');
  fromL.append(el('span', 'psu-setlabel', '시작'), fromIn);
  const toL = el('label', 'psu-field');
  toL.append(el('span', 'psu-setlabel', '끝'), toIn);
  const rangeBtn = tid(el('button', undefined, '구간 조회'), 'psu-hist-range');
  rangeBtn.type = 'button';
  const csvBtn = tid(el('button', undefined, 'CSV'), 'psu-hist-csv');
  csvBtn.type = 'button';
  csvBtn.title = `마지막 조회 구간의 측정 행을 CSV로 (최대 ${CSV_MAX_ROWS}행, 브라우저 다운로드)`;
  csvBtn.disabled = true;
  const stopBtn = tid(el('button', undefined, '중지'), 'psu-hist-stop');
  stopBtn.type = 'button';
  stopBtn.hidden = true;
  head.append(fromL, toL, rangeBtn, csvBtn, stopBtn);

  const status = tid(el('div', 'psu-hist-status', '구간을 고르면 저널에 기록된 측정으로 통계를 냅니다'), 'psu-hist-status');
  status.setAttribute('role', 'status');
  const cover = tid(el('div', 'psu-hist-coverage'), 'psu-hist-coverage');
  const table = tid(el('table', 'psu-hist-stats'), 'psu-hist-stats');
  table.hidden = true;
  const thr = el('tr');
  for (const t of ['', '최소', '최대', '평균', '마지막']) thr.append(el('th', undefined, t));
  table.append(el('thead'), el('tbody'));
  table.tHead!.append(thr);
  const rowEls = new Map<'v' | 'i' | 'p', HTMLTableRowElement>();
  for (const [k, label] of [['v', '전압'], ['i', '전류'], ['p', '전력']] as const) {
    const tr = tid(el('tr'), `psu-hist-row-${k}`);
    for (let c = 0; c < 5; c++) tr.append(el('td', undefined, c === 0 ? label : '—'));
    rowEls.set(k, tr);
    table.tBodies[0]!.append(tr);
  }
  root.append(head, status, cover, table);

  let running: AbortController | null = null;
  let lastWindow: { fromMs: number; toMs: number } | null = null;
  let disposed = false;

  const render = (a: HistoryAgg) => {
    cover.textContent = coverageText(a);
    cover.classList.toggle('partial', !isComplete(a));
    const units = { v: 'V', i: 'A', p: 'W' } as const;
    const f = (x: number | null, u: string) => (x === null ? '—' : `${x.toFixed(3)} ${u}`);
    for (const k of ['v', 'i', 'p'] as const) {
      const s = a[k];
      const u = units[k];
      const cells = rowEls.get(k)!.cells;
      cells[1]!.textContent = s ? f(s.min, u) : '—';
      cells[2]!.textContent = s ? f(s.max, u) : '—';
      cells[3]!.textContent = s ? f(s.sum / s.n, u) : '—';
      cells[4]!.textContent = s ? f(s.last, u) : '—';
    }
    table.hidden = false;
  };
  const setBusy = (busy: boolean) => {
    for (const b of presetBtns) b.disabled = busy;
    rangeBtn.disabled = busy;
    csvBtn.disabled = busy || lastWindow === null;
    stopBtn.hidden = !busy;
  };

  const run = async (q: HistoryQuery, mode: 'stats' | 'csv'): Promise<void> => {
    if (running) return;
    const ac = new AbortController();
    running = ac;
    const csv = mode === 'csv';
    setBusy(true);
    status.textContent = csv ? 'CSV 행 읽는 중…' : '조회 중…';
    try {
      const r = await runHistory((p, s) => ctx.query('psu.history', p, s), ch, q, {
        rows: csv ? HISTORY_PAGE_ROWS : 0,
        maxPages: HISTORY_MAX_PAGES,
        maxRows: CSV_MAX_ROWS,
        signal: ac.signal,
        onPage: (a) => {
          if (!disposed) status.textContent = `${csv ? 'CSV 행 읽는 중' : '조회 중'} — ${a.pages}쪽 · 샘플 ${a.samples}`;
        },
      });
      if (disposed) return;
      const agg = r.agg;
      if (agg === null) {
        status.textContent = '중지됨 — 결과 없음';
        return;
      }
      lastWindow = { fromMs: agg.fromMs, toMs: agg.toMs };
      render(agg);
      if (!csv) {
        status.textContent = agg.cut === 'aborted' ? '중지됨 — 부분 결과' : `${agg.pages}쪽 조회 완료`;
        return;
      }
      const ok = download(`psu-${ch}-${agg.fromMs}.csv`, csvText(ch, agg, r.rows));
      if (ok) {
        const msg = `CSV 다운로드 요청됨 — 브라우저 다운로드 위치를 확인하세요 (${r.rows.length}행${isComplete(agg) ? '' : ', 부분'})`;
        ctx.ui.toast(msg);
        status.textContent = msg;
      } else {
        const msg = '내보내기 실패: 이 환경에서 다운로드를 만들 수 없음';
        ctx.ui.toast(msg);
        status.textContent = msg;
      }
    } catch (e) {
      if (!disposed) status.textContent = `조회 실패: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      if (!disposed) {
        running = null;
        setBusy(false);
      }
    }
  };

  rangeBtn.addEventListener('click', () => {
    const f = new Date(fromIn.value).getTime();
    const t = new Date(toIn.value).getTime();
    if (!Number.isFinite(f) || !Number.isFinite(t) || f >= t || t - f > 86_400_000) {
      status.textContent = '시작/끝 시각을 확인하세요 (시작 < 끝, 최대 24시간)';
      return;
    }
    void run({ fromMs: f, toMs: t }, 'stats');
  });
  csvBtn.addEventListener('click', () => {
    if (lastWindow) void run(lastWindow, 'csv');
  });
  stopBtn.addEventListener('click', () => running?.abort());

  return {
    root,
    dispose() {
      disposed = true;
      running?.abort();
    },
  };
}
