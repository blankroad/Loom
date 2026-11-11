// Display-filter bar (eth-ui, eth/plan §4.1). M1: every check is the server's
// `eth.filter.check{expr}` (≤ 100 ms target) — the UI does not parse the language.
// Error → underline at `pos` + message; OK → [적용] pushes the filter down.
// Saved filters and apply history live in localStorage (per-viewer convenience,
// try/catch — works without storage). C2: 20 entries each.

export interface FilterCheck {
  ok: boolean;
  pos?: number;
  expected?: string;
  message?: string;
}

export interface FilterBarOptions {
  check(expr: string, signal: AbortSignal): Promise<FilterCheck>;
  apply(expr: string): void;
  debounceMs?: number;
  now?: () => number;
}

const HIST_KEY = 'loom.eth.filter.history';
const SAVED_KEY = 'loom.eth.filter.saved';
const MAX_LIST = 20;

function load(key: string): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(key) ?? '[]') as unknown;
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string').slice(0, MAX_LIST) : [];
  } catch {
    return [];
  }
}
function store(key: string, list: string[]): void {
  try {
    localStorage.setItem(key, JSON.stringify(list.slice(0, MAX_LIST)));
  } catch {
    /* storage unavailable — keep in memory only */
  }
}

export class FilterBar {
  readonly el: HTMLDivElement;
  readonly input: HTMLInputElement;
  private readonly status: HTMLSpanElement;
  private readonly errEl: HTMLDivElement;
  private readonly listEl: HTMLSelectElement;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private ac: AbortController | null = null;
  private last: { expr: string; res: FilterCheck } | null = null;
  private history = load(HIST_KEY);
  private saved = load(SAVED_KEY);
  applied = '';

  constructor(
    host: HTMLElement,
    private readonly o: FilterBarOptions,
  ) {
    this.el = document.createElement('div');
    this.el.className = 'eth-filter';
    const label = document.createElement('span');
    label.textContent = '표시 필터';
    this.input = document.createElement('input');
    this.input.placeholder = 'doip && doip.target_address == 0x1001';
    this.input.spellcheck = false;
    this.input.setAttribute('aria-label', '표시 필터');
    this.input.setAttribute('data-testid', 'eth-filter-input');
    this.status = document.createElement('span');
    this.status.className = 'eth-filter-status';
    this.status.setAttribute('data-testid', 'eth-filter-status');
    const applyBtn = this.button('적용', () => void this.applyNow());
    const saveBtn = this.button('저장', () => this.save());
    this.listEl = document.createElement('select');
    this.listEl.setAttribute('aria-label', '저장·이력');
    this.listEl.addEventListener('change', () => {
      if (this.listEl.value) {
        this.input.value = this.listEl.value;
        this.listEl.value = '';
        this.schedule(0);
      }
    });
    this.errEl = document.createElement('div');
    this.errEl.className = 'eth-filter-err';
    this.errEl.hidden = true;
    this.errEl.setAttribute('data-testid', 'eth-filter-err');
    const row = document.createElement('div');
    row.className = 'eth-filter-row';
    row.append(label, this.input, this.status, applyBtn, saveBtn, this.listEl);
    this.el.append(row, this.errEl);
    host.append(this.el);
    this.renderList();

    this.input.addEventListener('input', () => this.schedule(this.o.debounceMs ?? 150));
    this.input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        void this.applyNow();
      }
    });
  }

  /** check now (no debounce) and apply when valid; empty = clear filter */
  async applyNow(): Promise<boolean> {
    const expr = this.input.value.trim();
    if (expr !== '') {
      const res = this.last?.expr === expr ? this.last.res : await this.check(expr);
      if (!res || !res.ok) return false;
    }
    this.applied = expr;
    if (expr) {
      this.history = [expr, ...this.history.filter((h) => h !== expr)].slice(0, MAX_LIST);
      store(HIST_KEY, this.history);
      this.renderList();
    }
    this.o.apply(expr);
    return true;
  }

  dispose(): void {
    if (this.timer) clearTimeout(this.timer);
    this.ac?.abort();
    this.el.remove();
  }

  private schedule(ms: number): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      const expr = this.input.value.trim();
      if (expr === '') {
        this.last = null;
        this.status.textContent = '';
        this.errEl.hidden = true;
        return;
      }
      void this.check(expr);
    }, ms);
  }

  private async check(expr: string): Promise<FilterCheck | null> {
    this.ac?.abort();
    const ac = new AbortController();
    this.ac = ac;
    const now = this.o.now ?? (() => performance.now());
    const t0 = now();
    this.status.textContent = '검사 중…';
    try {
      const res = await this.o.check(expr, ac.signal);
      if (ac.signal.aborted) return null;
      const ms = Math.round(now() - t0);
      this.last = { expr, res };
      this.render(expr, res, ms);
      return res;
    } catch (e) {
      if (!ac.signal.aborted) {
        this.status.textContent = `검사 실패: ${e instanceof Error ? e.message : String(e)}`;
        this.status.dataset.state = 'fail';
      }
      return null;
    }
  }

  private render(expr: string, res: FilterCheck, ms: number): void {
    this.status.dataset.state = res.ok ? 'ok' : 'err';
    this.status.textContent = res.ok ? `✓ ${ms} ms` : `✗ ${ms} ms`;
    if (res.ok) {
      this.errEl.hidden = true;
      this.errEl.replaceChildren();
      return;
    }
    const pos = Math.max(0, Math.min(expr.length, res.pos ?? expr.length));
    const pre = document.createElement('span');
    pre.textContent = expr.slice(0, pos);
    const mark = document.createElement('u');
    mark.className = 'err';
    mark.setAttribute('data-testid', 'eth-filter-err-mark');
    mark.textContent = expr.slice(pos, pos + 1) || '␣';
    const post = document.createElement('span');
    post.textContent = expr.slice(pos + 1);
    const code = document.createElement('div');
    code.className = 'mono';
    code.append(pre, mark, post);
    const msg = document.createElement('div');
    msg.textContent = `위치 ${pos}: ${res.message ?? (res.expected ? `${res.expected} 필요` : '구문 오류')}`;
    this.errEl.replaceChildren(code, msg);
    this.errEl.hidden = false;
  }

  private save(): void {
    const expr = this.input.value.trim();
    if (!expr) return;
    this.saved = [expr, ...this.saved.filter((x) => x !== expr)].slice(0, MAX_LIST);
    store(SAVED_KEY, this.saved);
    this.renderList();
  }

  private renderList(): void {
    const opt = (v: string, text: string, disabled = false) => {
      const o = document.createElement('option');
      o.value = v;
      o.textContent = text;
      o.disabled = disabled;
      return o;
    };
    this.listEl.replaceChildren(
      opt('', '저장·이력 ▾'),
      ...(this.saved.length ? [opt('', '— 저장', true), ...this.saved.map((s) => opt(s, s))] : []),
      ...(this.history.length ? [opt('', '— 이력', true), ...this.history.map((s) => opt(s, s))] : []),
    );
  }

  private button(text: string, on: () => void): HTMLButtonElement {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = text;
    b.addEventListener('click', on);
    return b;
  }
}
