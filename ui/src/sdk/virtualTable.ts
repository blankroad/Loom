// VirtualTable — framework-free virtualized table for panels (fixed row height,
// pooled row elements). Why not @tanstack/react-virtual: panels mount imperatively
// (PanelInstance.mount(el)) and draw from the RenderScheduler frame callback; a
// React root per panel would add a second render loop and reconcile cost on every
// data frame. A fixed-height DOM pool keeps per-frame work = visible rows only and
// matches the S1 spike result (DOM viewport p95 draw ≈ 0.5 ms for 28 rows).
//
// Rules: text only (textContent, never innerHTML — device data is untrusted);
// rows never move because of updates (callers keep order fixed); the view follows
// the tail only while the user is scrolled to the bottom.

export interface Cell {
  text: string;
  cls?: string;
  title?: string;
}

export interface Column<T> {
  key: string;
  header: string;
  /** CSS grid track, e.g. '7em' or '1fr' */
  width: string;
  cell(item: T, nowMs: number): string | Cell;
}

export interface VirtualTableOptions<T> {
  columns: Column<T>[];
  keyOf(item: T): string;
  rowHeight?: number;
  rowAttrs?(item: T): Record<string, string>;
  rowClass?(item: T): string;
  /** label/value pairs shown in the detail drawer for the selected row */
  detail?(item: T): [string, string][];
  /** ask the owner for a frame (ctx.scheduler.invalidate) */
  onNeedRender(): void;
  now?: () => number;
  /** rows to draw when the viewport has no layout height yet (tests, hidden panels) */
  fallbackRows?: number;
  overscan?: number;
  emptyText?: string;
  ariaLabel?: string;
  /** selection changed (click, keys, select()) */
  onSelect?(key: string | null): void;
  /** a cell was clicked; return true to consume the click (no selection change) */
  onCellClick?(item: T, columnKey: string): boolean;
  /** the user scrolled away from the live tail (scroll-back) */
  onScrollAway?(): void;
}

/**
 * U-4: a scroll counts as the USER's scroll-back only within this window after a wheel /
 * key / pointer / touch input on the list (or while a pointer drag is held). Scrolls from
 * layout changes (STOP panel opening, resize, spacer growth) keep following the tail.
 */
export const USER_SCROLL_WINDOW_MS = 800;
const SCROLL_KEYS = new Set(['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' ']);

interface PoolRow {
  el: HTMLDivElement;
  cells: HTMLSpanElement[];
  key: string | null;
}

export class VirtualTable<T> {
  readonly root: HTMLDivElement;
  private readonly scroll: HTMLDivElement;
  private readonly spacer: HTMLDivElement;
  private readonly detailEl: HTMLDivElement;
  private readonly empty: HTMLDivElement;
  private pool: PoolRow[] = [];
  private items: readonly T[] = [];
  private readonly h: number;
  private readonly grid: string;
  private follow = true;
  private _selected: string | null = null;
  private lastUserInputMs = Number.NEGATIVE_INFINITY;
  private dragging = false;

  constructor(
    container: HTMLElement,
    private readonly o: VirtualTableOptions<T>,
  ) {
    this.h = o.rowHeight ?? 20;
    this.grid = o.columns.map((c) => c.width).join(' ');
    this.root = el('div', 'vt');
    const head = el('div', 'vt-head');
    head.style.gridTemplateColumns = this.grid;
    head.setAttribute('role', 'row');
    for (const c of o.columns) {
      const s = el('span', 'vt-th');
      s.textContent = c.header;
      s.setAttribute('role', 'columnheader');
      head.append(s);
    }
    this.scroll = el('div', 'vt-scroll');
    this.scroll.tabIndex = 0;
    this.scroll.setAttribute('role', 'grid');
    if (o.ariaLabel) this.scroll.setAttribute('aria-label', o.ariaLabel);
    this.spacer = el('div', 'vt-spacer');
    this.empty = el('div', 'vt-empty muted');
    this.empty.textContent = o.emptyText ?? '행 없음';
    this.scroll.append(this.spacer, this.empty);
    this.detailEl = el('div', 'vt-detail');
    this.detailEl.hidden = true;
    this.root.append(head, this.scroll, this.detailEl);
    container.append(this.root);

    this.scroll.addEventListener('scroll', this.onScroll);
    this.scroll.addEventListener('wheel', this.markInput, { passive: true });
    this.scroll.addEventListener('touchstart', this.markInput, { passive: true });
    this.scroll.addEventListener('keydown', this.onScrollKey);
    this.scroll.addEventListener('pointerdown', this.onPointerDown);
    if (typeof window !== 'undefined') window.addEventListener('pointerup', this.onPointerUp);
    this.scroll.addEventListener('click', this.onClick);
    this.scroll.addEventListener('keydown', this.onKey);
  }

  get selected(): string | null {
    return this._selected;
  }

  setItems(items: readonly T[]): void {
    this.items = items;
    this.o.onNeedRender();
  }

  select(key: string | null): void {
    if (this._selected === key) return;
    this._selected = key;
    this.o.onSelect?.(key);
    this.o.onNeedRender();
  }

  /** true while the view sticks to the newest row */
  get following(): boolean {
    return this.follow;
  }

  /** go back to the live tail */
  followTail(): void {
    this.follow = true;
    this.o.onNeedRender();
  }

  /** select a row and scroll it into view (cross-panel reveal) */
  reveal(key: string): boolean {
    const idx = this.items.findIndex((it) => this.o.keyOf(it) === key);
    if (idx < 0) return false;
    this.follow = false;
    this.scroll.scrollTop = Math.max(0, idx * this.h - this.scroll.clientHeight / 2);
    this.select(key);
    this.scroll.focus();
    return true;
  }

  /** Draw the visible window. Call from the panel's frame callback. */
  render(): void {
    const n = this.items.length;
    const atBottomBefore = this.follow;
    this.spacer.style.height = `${n * this.h}px`;
    this.empty.hidden = n > 0;
    if (atBottomBefore) this.scroll.scrollTop = Math.max(0, n * this.h - this.scroll.clientHeight);

    const viewH = this.scroll.clientHeight;
    const visible = viewH > 0 ? Math.ceil(viewH / this.h) : (this.o.fallbackRows ?? 40);
    const over = this.o.overscan ?? 4;
    // following: the window is the last `visible` rows (independent of layout, so it also
    // holds before the first layout pass); otherwise it follows the user's scrollTop
    const top = atBottomBefore ? Math.max(0, n - visible) : Math.floor(this.scroll.scrollTop / this.h);
    const first = Math.max(0, top - over);
    const count = Math.min(n - first, visible + 2 * over);
    this.ensurePool(Math.max(0, count));
    const now = (this.o.now ?? (() => performance.now()))();

    for (let i = 0; i < this.pool.length; i++) {
      const p = this.pool[i] as PoolRow;
      const idx = first + i;
      if (i >= count || idx >= n) {
        p.el.hidden = true;
        p.key = null;
        continue;
      }
      const item = this.items[idx] as T;
      const key = this.o.keyOf(item);
      p.el.hidden = false;
      p.el.style.transform = `translateY(${idx * this.h}px)`;
      p.key = key;
      p.el.dataset.key = key;
      setAttrs(p.el, this.o.rowAttrs?.(item) ?? {});
      p.el.className = `vt-row${this.o.rowClass ? ` ${this.o.rowClass(item)}` : ''}${key === this._selected ? ' selected' : ''}`;
      p.el.setAttribute('aria-selected', String(key === this._selected));
      this.o.columns.forEach((c, ci) => {
        const v = c.cell(item, now);
        const cell = typeof v === 'string' ? { text: v } : v;
        const span = p.cells[ci] as HTMLSpanElement;
        if (span.textContent !== cell.text) span.textContent = cell.text;
        const cls = `vt-td${cell.cls ? ` ${cell.cls}` : ''}`;
        if (span.className !== cls) span.className = cls;
        if (cell.title) span.title = cell.title;
        else span.removeAttribute('title');
      });
    }
    this.renderDetail();
  }

  dispose(): void {
    this.scroll.removeEventListener('scroll', this.onScroll);
    this.scroll.removeEventListener('wheel', this.markInput);
    this.scroll.removeEventListener('touchstart', this.markInput);
    this.scroll.removeEventListener('keydown', this.onScrollKey);
    this.scroll.removeEventListener('pointerdown', this.onPointerDown);
    if (typeof window !== 'undefined') window.removeEventListener('pointerup', this.onPointerUp);
    this.scroll.removeEventListener('click', this.onClick);
    this.scroll.removeEventListener('keydown', this.onKey);
    this.root.remove();
    this.pool = [];
  }

  private renderDetail(): void {
    const sel = this._selected === null ? undefined : this.items.find((it) => this.o.keyOf(it) === this._selected);
    if (!sel || !this.o.detail) {
      this.detailEl.hidden = true;
      return;
    }
    this.detailEl.hidden = false;
    const dl = el('dl', 'disp-obj');
    for (const [k, v] of this.o.detail(sel)) {
      const row = el('div', 'disp-row');
      const dt = el('dt');
      dt.textContent = k;
      const dd = el('dd', 'mono');
      dd.textContent = v;
      row.append(dt, dd);
      dl.append(row);
    }
    this.detailEl.replaceChildren(dl);
  }

  private ensurePool(count: number): void {
    while (this.pool.length < count) {
      const row = el('div', 'vt-row') as HTMLDivElement;
      row.setAttribute('role', 'row');
      row.style.gridTemplateColumns = this.grid;
      row.style.height = `${this.h}px`;
      const cells = this.o.columns.map(() => {
        const s = el('span', 'vt-td');
        s.setAttribute('role', 'gridcell');
        row.append(s);
        return s;
      });
      this.scroll.append(row);
      this.pool.push({ el: row, cells, key: null });
    }
  }

  private nowMs(): number {
    return (this.o.now ?? (() => performance.now()))();
  }
  private markInput = (): void => {
    this.lastUserInputMs = this.nowMs();
  };
  private onScrollKey = (e: KeyboardEvent): void => {
    if (SCROLL_KEYS.has(e.key)) this.markInput();
  };
  private onPointerDown = (): void => {
    this.dragging = true; // scrollbar drag: scroll events keep coming while held
    this.markInput();
  };
  private onPointerUp = (): void => {
    if (this.dragging) this.markInput();
    this.dragging = false;
  };

  private onScroll = (): void => {
    const s = this.scroll;
    const was = this.follow;
    const atBottom = s.scrollTop + s.clientHeight >= s.scrollHeight - this.h;
    if (atBottom) this.follow = true;
    else if (was) {
      const byUser = this.dragging || this.nowMs() - this.lastUserInputMs <= USER_SCROLL_WINDOW_MS;
      if (byUser) {
        this.follow = false;
        this.o.onScrollAway?.();
      }
      // else: a layout change moved the scroll position — stay on the tail (render re-pins)
    } else this.follow = false;
    this.o.onNeedRender();
  };

  private onClick = (e: Event): void => {
    const target = e.target as HTMLElement;
    const row = target.closest('.vt-row') as HTMLElement | null;
    const key = row?.dataset.key ?? null;
    if (key === null || !row) return;
    if (this.o.onCellClick) {
      const cell = target.closest('.vt-td');
      const ci = cell ? [...row.children].indexOf(cell) : -1;
      const col = this.o.columns[ci];
      const item = this.items.find((it) => this.o.keyOf(it) === key);
      if (col && item !== undefined && this.o.onCellClick(item, col.key)) return;
    }
    this.select(this._selected === key ? null : key);
  };

  private onKey = (e: KeyboardEvent): void => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Escape') return;
    e.preventDefault();
    if (e.key === 'Escape') return this.select(null);
    const keys = this.items.map((it) => this.o.keyOf(it));
    const i = this._selected === null ? keys.length : keys.indexOf(this._selected);
    const j = e.key === 'ArrowDown' ? Math.min(keys.length - 1, i + 1) : Math.max(0, i - 1);
    this.select(keys[j] ?? null);
  };
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  return e;
}

const managedAttrs = new WeakMap<HTMLElement, string[]>();
function setAttrs(e: HTMLElement, attrs: Record<string, string>): void {
  for (const k of managedAttrs.get(e) ?? []) if (!(k in attrs)) e.removeAttribute(k);
  for (const [k, v] of Object.entries(attrs)) if (e.getAttribute(k) !== v) e.setAttribute(k, v);
  managedAttrs.set(e, Object.keys(attrs));
}
