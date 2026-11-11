// CAN Trace DOM view — imperative fixed-height row pool over the model's display index
// (plan UD-02: DOM virtualization; the SDK VirtualTable wants a JS array of items, which
// would mean row objects for up to 2M rows, so the Trace keeps its own pool).
//
// - Only visible rows (+ overscan) exist in the DOM; the pool is ≤ ceil(view/rowH) + 2·overscan.
// - Huge lists: the scroll spacer is capped at MAX_SPACER_PX (browsers cap element height
//   ≈ 33.5M px); beyond that scroll position maps proportionally onto row index.
// - Follow mode (at the bottom) shows new rows every frame. Scrolling up pins the view to
//   the top visible row (by absolute row number, so eviction at the front does not move it)
//   and shows "새 행 N ↓". Pause freezes the list end; collection continues in the model.
// - textContent only (device data is untrusted); DOM is written only when a cell changes.

import { STAGE_CHIP } from '../../sdk';
import {
  F_BRS,
  F_ECHO,
  F_EXT,
  F_FD,
  F_NOID,
  F_RTR,
  F_TX,
  K_GAP,
  K_TXREQ,
  type CanTraceModel,
  type GapInfo,
  type WState,
} from './traceModel';
import { fmtId } from './sendLine';

export const ROW_H = 18;
export const MAX_SPACER_PX = 8_000_000;
const OVERSCAN = 6;
const FALLBACK_ROWS = 40;

const ORG: Record<string, [string, string]> = {
  agent: ['AGT', 'org org-agt'],
  user: ['USR', 'org org-usr'],
  device: ['DEV', 'org org-dev'],
  script: ['SCR', 'org org-scr'],
};

const FINAL: ReadonlySet<WState> = new Set(['confirmed', 'effective_verified', 'effect_unverified', 'failed', 'outcome_unknown']);

/** gap row text (A13: the three loss layers never share wording) */
export function gapRowText(g: GapInfo): string {
  const n = g.count === 'unknown' ? '?' : g.count.toLocaleString('ko-KR');
  const range = g.first !== null && g.last !== null ? ` (seq ${g.first}–${g.last})` : '';
  switch (g.kind) {
    case 'display.gap':
    case 'omit':
      return `⋯ ${n} 프레임 화면 생략${range} — ${g.recordOk ? '기록 정상' : '기록 확인 안 됨'}${g.kind === 'omit' ? ' · 표시 예산 2,000행/s 초과' : g.local ? ` · 브라우저 (${g.cause})` : ` (${g.cause})`}`;
    case 'record.gap':
      return `⚠ 기록 손실 ${n} 프레임${range} — RECORDING INCOMPLETE (${g.cause})`;
    case 'source.gap':
      return `⚠ 장치·전송 손실 ${n} 프레임${g.producerOnly ? ' (stream seq 없음 — 허브 이전 손실)' : range} — SOURCE LOSS (${g.cause})`;
    default:
      return `⋯ 재동기화 — ${g.cause} (이 지점 앞뒤 사이 프레임은 질의로 확인)`;
  }
}

interface PoolRow {
  el: HTMLDivElement;
  cells: HTMLSpanElement[];
  abs: number;
  cache: string[];
}

// columns: bar · t(s) · dir · origin · id · len · data · state · note
// `len` = byte count (schema v1 payload.len); the DLC code (payload.dlc) is in its tooltip.
const COLS = ['bar', 't', 'dir', 'org', 'id', 'len', 'data', 'st', 'note'] as const;
const HEAD = ['', '시각(s)', 'dir', '출처', 'id', 'len', 'data', '상태', '비고'];

export interface TraceViewOptions {
  onNeedRender(): void;
  /** stage from the shared RowStore (control path may be ahead of the data batch) */
  rowState?(requestId: string): WState | null;
  /** ms clock for "N s 전" labels */
  now?(): number;
}

export class TraceView {
  readonly root: HTMLDivElement;
  private readonly scroll: HTMLDivElement;
  private readonly spacer: HTMLDivElement;
  private readonly topNote: HTMLDivElement;
  private readonly newBtn: HTMLButtonElement;
  private readonly emptyEl: HTMLDivElement;
  private pool: PoolRow[] = [];
  follow = true;
  private anchorAbs = -1;
  private anchorOffset = 0;
  private appendedAtUnfollow = 0;
  paused = false;
  private pauseEndAbs = -1;
  private lastProgScroll = -1;
  private spacerH = -1;
  /** first visible display index of the last render */
  firstVisible = 0;
  /** rows in the visible list (display entries ≤ pause end) */
  listCount = 0;

  constructor(
    container: HTMLElement,
    private readonly m: CanTraceModel,
    private readonly o: TraceViewOptions,
  ) {
    this.root = el('div', 'ct-table');
    const head = el('div', 'ct-head ct-grid');
    head.setAttribute('role', 'row');
    for (const h of HEAD) {
      const s = el('span', 'ct-th');
      s.textContent = h;
      s.setAttribute('role', 'columnheader');
      head.append(s);
    }
    this.topNote = el('div', 'ct-topnote muted');
    this.topNote.hidden = true;
    this.scroll = el('div', 'ct-scroll');
    this.scroll.tabIndex = 0;
    this.scroll.setAttribute('role', 'grid');
    this.scroll.setAttribute('aria-label', 'CAN 트레이스');
    this.spacer = el('div', 'ct-spacer');
    this.emptyEl = el('div', 'ct-empty muted');
    this.emptyEl.textContent = '프레임 없음';
    this.scroll.append(this.spacer, this.emptyEl);
    this.newBtn = el('button', 'ct-newrows');
    this.newBtn.type = 'button';
    this.newBtn.hidden = true;
    this.newBtn.dataset.testid = 'can-new-rows';
    this.newBtn.addEventListener('click', () => this.jumpToLive());
    this.root.append(head, this.topNote, this.scroll, this.newBtn);
    container.append(this.root);
    this.scroll.addEventListener('scroll', this.onScroll);
  }

  setPaused(p: boolean): void {
    if (p === this.paused) return;
    this.paused = p;
    this.pauseEndAbs = p ? this.m.total - 1 : -1;
    if (!p) this.follow = true;
    this.o.onNeedRender();
  }

  jumpToLive(): void {
    this.follow = true;
    if (this.paused) this.setPaused(false);
    this.o.onNeedRender();
  }

  /** display rows appended since the user scrolled away from the bottom */
  get newRows(): number {
    return this.follow ? 0 : Math.max(0, this.m.dispAppended - this.appendedAtUnfollow);
  }

  private count(): number {
    const m = this.m;
    return this.paused ? m.dispIndexOf(this.pauseEndAbs + 1) : m.dispCount;
  }

  private viewH(): number {
    const h = this.scroll.clientHeight;
    return h > 0 ? h : FALLBACK_ROWS * ROW_H;
  }

  /** scrollTop (px) ↔ virtual top (px over n·ROW_H) */
  private toVirtual(scrollTop: number, n: number, viewH: number): number {
    const full = n * ROW_H;
    if (full <= MAX_SPACER_PX) return scrollTop;
    return (scrollTop * (full - viewH)) / Math.max(1, MAX_SPACER_PX - viewH);
  }
  private toScroll(virtualTop: number, n: number, viewH: number): number {
    const full = n * ROW_H;
    if (full <= MAX_SPACER_PX) return virtualTop;
    return (virtualTop * Math.max(1, MAX_SPACER_PX - viewH)) / Math.max(1, full - viewH);
  }

  render(): void {
    const m = this.m;
    m.dispTrim();
    const n = this.count();
    this.listCount = n;
    const viewH = this.viewH();
    const spacerH = Math.min(n * ROW_H, MAX_SPACER_PX);
    if (spacerH !== this.spacerH) {
      this.spacer.style.height = `${spacerH}px`;
      this.spacerH = spacerH;
    }
    this.emptyEl.hidden = n > 0;

    let virtualTop: number;
    if (this.follow) {
      virtualTop = Math.max(0, n * ROW_H - viewH);
    } else {
      const idx = this.anchorAbs >= 0 ? m.dispIndexOf(this.anchorAbs) : 0;
      virtualTop = Math.max(0, Math.min(idx * ROW_H + this.anchorOffset, n * ROW_H - viewH));
    }
    const wantScroll = Math.round(this.toScroll(virtualTop, n, viewH));
    if (Math.abs(this.scroll.scrollTop - wantScroll) >= 1) {
      this.scroll.scrollTop = wantScroll;
      this.lastProgScroll = this.scroll.scrollTop;
    }
    const scrollTop = this.scroll.scrollTop;

    const first = Math.max(0, Math.floor(virtualTop / ROW_H) - OVERSCAN);
    const want = Math.ceil(viewH / ROW_H) + 2 * OVERSCAN;
    const count = Math.max(0, Math.min(n - first, want));
    this.ensurePool(count);
    this.firstVisible = Math.min(n, Math.floor(virtualTop / ROW_H));
    const nowMs = (this.o.now ?? (() => performance.now()))();

    for (let k = 0; k < this.pool.length; k++) {
      const p = this.pool[k]!;
      const idx = first + k;
      if (k >= count || idx >= n) {
        if (!p.el.hidden) p.el.hidden = true;
        p.abs = -1;
        continue;
      }
      const abs = m.dispAt(idx);
      if (p.el.hidden) p.el.hidden = false;
      const top = scrollTop + idx * ROW_H - virtualTop;
      p.el.style.transform = `translateY(${top}px)`;
      this.fillRow(p, abs, nowMs);
    }

    // "보관 범위 밖" marker at the top of the kept range
    const showTop = m.evicted > 0 && this.firstVisible === 0 && n > 0;
    if (showTop !== !this.topNote.hidden) this.topNote.hidden = !showTop;
    if (showTop) setText(this.topNote, `↑ 이전 ${m.evicted.toLocaleString('ko-KR')}행은 브라우저 보관 범위 밖 — 질의로 조회 (can.trace.query, M3)`);

    const nr = this.newRows;
    const showNew = !this.follow && !this.paused && nr > 0;
    if (showNew !== !this.newBtn.hidden) this.newBtn.hidden = !showNew;
    if (showNew) setText(this.newBtn, `새 행 ${nr.toLocaleString('ko-KR')} ↓`);
  }

  private fillRow(p: PoolRow, abs: number, nowMs: number): void {
    const m = this.m;
    const kind = m.kindAt(abs);
    const vals: string[] = new Array<string>(COLS.length).fill('');
    const cls: string[] = new Array<string>(COLS.length).fill('');
    const attrs: Record<string, string> = { 'data-seq': String(m.seqAt(abs)) };
    let rowCls = 'ct-row ct-grid';
    let title = '';
    const titles: string[] = new Array<string>(COLS.length).fill('');

    const agent = m.agentReadAt(m.seqAt(abs));
    if (agent) {
      cls[0] = 'ct-bar agent-read';
      title = `에이전트가 읽은 범위 seq ${agent.a}–${agent.b} (${agent.actor}, ${Math.max(0, Math.round((nowMs - agent.atMs) / 1000))} s 전)`;
      attrs['data-agent-read'] = '1';
    } else cls[0] = 'ct-bar';

    if (kind === K_GAP) {
      const g = m.gapAt(abs);
      vals[1] = g ? gapRowText(g) : '⋯ 생략·손실 행 (세부 정보 만료)';
      const k = g?.kind ?? 'display.gap';
      cls[1] = `gap-text ct-gap-${k.replace('.', '-')}`;
      rowCls += ` ct-gaprow${k === 'record.gap' || k === 'source.gap' ? ' ct-loss' : ''}`;
      attrs['data-gap'] = k;
    } else {
      const fl = m.flagsAt(abs);
      const origin = m.originAt(abs);
      const req = m.reqAt(abs);
      vals[1] = (m.monoAt(abs) / 1000).toFixed(3);
      vals[2] = fl & F_TX ? 'tx' : 'rx';
      cls[2] = fl & F_TX ? 'ct-tx' : 'ct-rx';
      const [ot, oc] = ORG[origin] ?? ['?', 'org'];
      vals[3] = req?.mine ? `${ot}(나)` : ot;
      cls[3] = oc;
      const ext = (fl & F_EXT) !== 0;
      vals[4] = fl & F_NOID ? '?' : fmtId(m.idAt(abs), ext);
      titles[4] = fl & F_NOID ? '요청에 읽을 수 있는 id가 없음' : ext ? '29bit (ext) id' : '11bit id';
      const len = m.lenAt(abs);
      const fi = m.frameIndexAt(abs);
      vals[5] = `${fl & F_FD ? (fl & F_BRS ? 'FB ' : 'F ') : ''}${fl & F_RTR ? 'R ' : ''}${len}`;
      const dlc = m.dlcAt(abs);
      titles[5] = `${len} B · DLC 코드 ${dlc ?? '?'}${fl & F_FD ? ` · CAN FD${fl & F_BRS ? ' BRS' : ''}` : ''}${fl & F_RTR ? ' · 원격 프레임(RTR)' : ''}`;
      const data = fl & F_RTR ? 'RTR' : m.dataHexAt(abs);
      vals[6] = data ?? '(FD 데이터 보관 범위 밖)';
      cls[6] = data === null || fl & F_RTR ? 'muted' : 'mono';
      attrs['data-origin'] = origin;
      attrs['data-dir'] = fl & F_TX ? 'tx' : 'rx';
      attrs['data-id'] = vals[4];
      attrs['data-len'] = String(len);
      attrs['data-dlc'] = dlc === null ? '' : String(dlc);
      attrs['data-frame-index'] = fi === null ? '' : String(fi);
      if (req) {
        attrs['data-request-id'] = req.requestId;
        const fromRows = this.o.rowState?.(req.requestId) ?? null;
        if (fromRows) m.raiseState(abs, fromRows);
      }
      const st = m.stateAt(abs);
      if (st) {
        vals[7] = STAGE_CHIP[st] + (fl & F_ECHO && !FINAL.has(st) ? ' ·에코' : '');
        cls[7] = `st st-${st}`;
        attrs['data-state'] = st;
        rowCls += ` st-row-${st}`;
      }
      const notes: string[] = [];
      if (req?.clientId) notes.push(req.clientId);
      if (req) notes.push(req.requestId);
      if (req && fi !== null && fi > 0) notes.push(`#${fi}`);
      if (req?.error) notes.push(`오류: ${req.error}`);
      const risk = m.riskAt(abs);
      if (risk && kind === K_TXREQ) notes.push(risk);
      vals[8] = notes.join(' ');
    }

    // write only what changed
    if (p.abs !== abs || p.el.className !== rowCls) p.el.className = rowCls;
    p.abs = abs;
    for (let c = 0; c < COLS.length; c++) {
      const key = `${kind}\u0000${cls[c]}\u0000${vals[c]}\u0000${titles[c]}`;
      if (p.cache[c] === key) continue;
      p.cache[c] = key;
      const span = p.cells[c]!;
      span.textContent = vals[c]!;
      span.className = `ct-td ${cls[c]}`;
      span.hidden = kind === K_GAP && c > 1;
      if (c > 0) {
        if (titles[c]) span.title = titles[c]!;
        else span.removeAttribute('title');
      }
    }
    if (title) p.cells[0]!.title = title;
    else p.cells[0]!.removeAttribute('title');
    setAttrs(p.el, attrs);
  }

  private ensurePool(count: number): void {
    while (this.pool.length < count) {
      const row = el('div', 'ct-row ct-grid');
      row.setAttribute('role', 'row');
      row.style.height = `${ROW_H}px`;
      const cells = COLS.map(() => {
        const s = el('span', 'ct-td');
        s.setAttribute('role', 'gridcell');
        row.append(s);
        return s;
      });
      this.scroll.append(row);
      this.pool.push({ el: row, cells, abs: -1, cache: [] });
    }
  }

  private onScroll = (): void => {
    const s = this.scroll;
    if (this.lastProgScroll >= 0 && Math.abs(s.scrollTop - this.lastProgScroll) < 1) {
      this.lastProgScroll = -1;
      return; // our own scrollTop write
    }
    this.lastProgScroll = -1;
    const n = this.listCount;
    const atBottom = s.scrollTop + s.clientHeight >= s.scrollHeight - ROW_H;
    if (atBottom) {
      this.follow = true;
    } else {
      if (this.follow) this.appendedAtUnfollow = this.m.dispAppended;
      this.follow = false;
      const vt = this.toVirtual(s.scrollTop, n, this.viewH());
      const idx = Math.min(Math.max(0, n - 1), Math.floor(vt / ROW_H));
      this.anchorAbs = n > 0 ? this.m.dispAt(idx) : -1;
      this.anchorOffset = vt - idx * ROW_H;
    }
    this.o.onNeedRender();
  };

  /** tests / keyboard: scroll the list so display index `idx` is at the top (unfollows) */
  scrollToIndex(idx: number): void {
    const n = this.count();
    if (n === 0) return;
    const i = Math.max(0, Math.min(n - 1, idx));
    if (this.follow) this.appendedAtUnfollow = this.m.dispAppended;
    this.follow = false;
    this.anchorAbs = this.m.dispAt(i);
    this.anchorOffset = 0;
    this.o.onNeedRender();
  }

  dispose(): void {
    this.scroll.removeEventListener('scroll', this.onScroll);
    this.root.remove();
    this.pool = [];
  }
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  return e;
}

function setText(e: HTMLElement, t: string): void {
  if (e.textContent !== t) e.textContent = t;
}

const managed = new WeakMap<HTMLElement, string[]>();
function setAttrs(e: HTMLElement, attrs: Record<string, string>): void {
  for (const k of managed.get(e) ?? []) if (!(k in attrs)) e.removeAttribute(k);
  for (const [k, v] of Object.entries(attrs)) if (e.getAttribute(k) !== v) e.setAttribute(k, v);
  managed.set(e, Object.keys(attrs));
}
