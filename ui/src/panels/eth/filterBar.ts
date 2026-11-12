// Display-filter bar (eth-ui, eth/plan §4.1). M1: every check is the server's
// `eth.filter.check{expr}` (≤ 100 ms target) — the UI does not parse the language.
// Error → underline at `pos` + message; OK → [적용] pushes the filter down.
// Saved filters and apply history live in localStorage (per-viewer convenience,
// try/catch — works without storage). C2: 20 entries each.
//
// Fidelity (research/fidelity/eth.md row 3, Wireshark filter toolbar Fig 3.15): the input
// itself carries validity — `data-validity` idle | checking | ok | err | fail drives a
// green / red background — and whether the edit differs from the applied filter
// (`data-dirty`). Apply (→, Enter) and Clear (✕) sit inside the field frame; the field
// language stays Loom's server grammar (no Wireshark-equivalence claim).

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
  private readonly frame: HTMLDivElement;
  private readonly appliedEl: HTMLSpanElement;
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
    const label = document.createElement('label');
    label.textContent = '표시 필터';
    this.input = document.createElement('input');
    this.input.id = `eth-filter-${Math.random().toString(36).slice(2, 8)}`;
    label.htmlFor = this.input.id;
    this.input.placeholder = '표시 필터 (Loom 문법) 예: doip && doip.target_address == 0x1001 — Ctrl+/';
    this.input.spellcheck = false;
    this.input.setAttribute('aria-label', '표시 필터');
    this.input.setAttribute('data-testid', 'eth-filter-input');
    this.status = document.createElement('span');
    this.status.className = 'eth-filter-status';
    this.status.setAttribute('data-testid', 'eth-filter-status');
    const clearBtn = this.button('✕', () => void this.clear());
    clearBtn.className = 'eth-filter-clear';
    clearBtn.title = '표시 필터 지우기 — 전체 표시로 되돌림 (캡처 필터는 그대로)';
    clearBtn.setAttribute('aria-label', '표시 필터 지우기');
    clearBtn.setAttribute('data-testid', 'eth-filter-clear');
    const applyBtn = this.button('→ 적용', () => void this.applyNow());
    applyBtn.className = 'eth-filter-apply';
    applyBtn.title = '표시 필터 적용 (Enter) — 저장된 기록은 바뀌지 않음';
    applyBtn.setAttribute('data-testid', 'eth-filter-apply');
    const saveBtn = this.button('★', () => this.save());
    saveBtn.className = 'eth-filter-save';
    saveBtn.title = '이 필터를 저장';
    saveBtn.setAttribute('aria-label', '표시 필터 저장');
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
    this.frame = document.createElement('div');
    this.frame.className = 'eth-filter-frame';
    this.frame.setAttribute('data-testid', 'eth-filter-frame');
    this.frame.append(saveBtn, this.input, this.status, clearBtn, applyBtn, this.listEl);
    this.appliedEl = document.createElement('span');
    this.appliedEl.className = 'eth-filter-applied';
    this.appliedEl.setAttribute('data-testid', 'eth-filter-applied');
    const row = document.createElement('div');
    row.className = 'eth-filter-row';
    row.append(label, this.frame, this.appliedEl);
    this.el.append(row, this.errEl);
    this.setValidity('idle');
    host.append(this.el);
    this.renderList();

    this.input.addEventListener('input', () => {
      this.setValidity(this.input.value.trim() === '' ? 'idle' : 'checking');
      this.schedule(this.o.debounceMs ?? 150);
    });
    this.input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        void this.applyNow();
      } else if (e.key === 'Escape' && this.input.value.trim() !== this.applied) {
        // revert the edit to the applied filter (the view is unchanged until applied)
        e.preventDefault();
        e.stopPropagation();
        this.input.value = this.applied;
        this.schedule(0);
        this.syncDirty();
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
    this.syncDirty();
    if (expr) {
      this.history = [expr, ...this.history.filter((h) => h !== expr)].slice(0, MAX_LIST);
      store(HIST_KEY, this.history);
      this.renderList();
    }
    this.o.apply(expr);
    return true;
  }

  /** Clear (✕): empty the field and apply "no display filter" when one is applied. */
  async clear(): Promise<void> {
    this.input.value = '';
    this.last = null;
    this.status.textContent = '';
    delete this.status.dataset.state;
    this.errEl.hidden = true;
    this.setValidity('idle');
    if (this.applied !== '') await this.applyNow();
    else this.syncDirty();
  }

  focus(): void {
    this.input.focus();
    this.input.select();
  }

  /** green / red field background (Wireshark filter toolbar semantics) */
  private setValidity(v: 'idle' | 'checking' | 'ok' | 'err' | 'fail'): void {
    this.input.dataset.validity = v;
    this.frame.dataset.validity = v;
    this.input.setAttribute('aria-invalid', String(v === 'err'));
    this.syncDirty();
  }

  /** edit ≠ applied → the list still shows the applied filter; say so */
  private syncDirty(): void {
    const dirty = this.input.value.trim() !== this.applied;
    this.frame.dataset.dirty = String(dirty);
    this.appliedEl.dataset.active = String(this.applied !== '');
    this.appliedEl.textContent =
      this.applied === '' ? (dirty ? '미적용 — Enter로 적용' : '필터 없음 · 전체 표시') : dirty ? `적용 중: ${this.applied} · 편집 미적용 (Esc 되돌림)` : '적용 중';
    this.appliedEl.title = this.applied ? `적용된 표시 필터: ${this.applied}` : '';
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
        this.setValidity('idle');
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
    this.setValidity('checking');
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
        this.setValidity('fail');
      }
      return null;
    }
  }

  private render(expr: string, res: FilterCheck, ms: number): void {
    this.status.dataset.state = res.ok ? 'ok' : 'err';
    this.status.textContent = res.ok ? `✓ ${ms} ms` : `✗ ${ms} ms`;
    this.setValidity(res.ok ? 'ok' : 'err');
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
      opt('', '▾'),
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
