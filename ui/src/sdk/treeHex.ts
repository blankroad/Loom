// TreeHex — packet details tree ↔ bytes view with two-way range sync
// (eth-ui "선택 행 동기"; reusable for CAN/ISO-TP detail later).
//   tree node click  → highlight bytes [off, off+len) and scroll them into view
//   byte click       → select the DEEPEST node whose range contains the byte
//                      (ancestors expanded), highlight that node's range
// The domain supplies `{off, len}` per field; the UI never decodes (labels are
// domain text, rendered with textContent).
// C2: at most MAX_HEX_BYTES are rendered as spans; the rest is offered as a raw link.

export interface TreeNode {
  label: string;
  off?: number;
  len?: number;
  children?: TreeNode[];
}

export interface TreeHexOptions {
  collapseTopLayers?: boolean;
  onSelectNode?: (node: TreeNode | null) => void;
}

export const MAX_HEX_BYTES = 16 * 1024;

interface FlatNode {
  node: TreeNode;
  path: string;
  depth: number;
  el: HTMLDivElement;
  parent: FlatNode | null;
  childrenEl: HTMLDivElement | null;
}

export class TreeHex {
  readonly root: HTMLDivElement;
  private readonly treeEl: HTMLDivElement;
  private readonly hexEl: HTMLDivElement;
  private readonly noteEl: HTMLDivElement;
  private flat: FlatNode[] = [];
  private byteEls: HTMLSpanElement[] = [];
  private asciiEls: HTMLSpanElement[] = [];
  private selected: FlatNode | null = null;
  private range: { off: number; len: number } | null = null;

  constructor(container: HTMLElement, private readonly options: TreeHexOptions = {}) {
    this.root = document.createElement('div');
    this.root.className = 'treehex';
    this.treeEl = document.createElement('div');
    this.treeEl.className = 'th-tree';
    this.treeEl.setAttribute('role', 'tree');
    this.hexEl = document.createElement('div');
    this.hexEl.className = 'th-hex mono';
    this.noteEl = document.createElement('div');
    this.noteEl.className = 'th-note muted';
    const right = document.createElement('div');
    right.className = 'th-right';
    right.append(this.hexEl, this.noteEl);
    this.root.append(this.treeEl, right);
    container.append(this.root);
    this.treeEl.addEventListener('click', this.onTreeClick);
    this.treeEl.addEventListener('keydown', this.onTreeKey);
    this.hexEl.addEventListener('click', this.onHexClick);
  }

  /** Replace content. `note` = e.g. "큰 프레임 — 원본 링크" (text only). */
  set(tree: TreeNode[], bytes: Uint8Array | null, note: { text: string; href?: string } | null = null): void {
    this.flat = [];
    this.selected = null;
    this.range = null;
    this.treeEl.replaceChildren(...tree.map((n, i) => this.buildNode(n, String(i), 0, null)));
    this.renderHex(bytes);
    this.options.onSelectNode?.(null);
    this.noteEl.replaceChildren();
    if (note) {
      if (note.href) {
        const a = document.createElement('a');
        a.href = note.href; // same-origin artifact URL from the daemon (cookie-checked)
        a.textContent = note.text;
        a.target = '_blank';
        a.rel = 'noopener noreferrer';
        this.noteEl.append(a);
      } else {
        this.noteEl.textContent = note.text;
      }
    }
  }

  clear(text = ''): void {
    this.set([], null, text ? { text } : null);
  }

  get selectedLabel(): string | null {
    return this.selected?.node.label ?? null;
  }
  get highlighted(): { off: number; len: number } | null {
    return this.range;
  }

  selectPath(path: string): void {
    const f = this.flat.find((x) => x.path === path);
    if (f) this.selectFlat(f);
  }

  /** byte → deepest containing node */
  selectByte(off: number): void {
    let best: FlatNode | null = null;
    for (const f of this.flat) {
      const { off: o, len } = f.node;
      if (o === undefined || len === undefined || len <= 0) continue;
      if (off >= o && off < o + len && (best === null || f.depth > best.depth || (f.depth === best.depth && len < (best.node.len ?? Infinity)))) {
        best = f;
      }
    }
    if (best) this.selectFlat(best);
    else this.highlight({ off, len: 1 });
  }

  dispose(): void {
    this.treeEl.removeEventListener('click', this.onTreeClick);
    this.treeEl.removeEventListener('keydown', this.onTreeKey);
    this.hexEl.removeEventListener('click', this.onHexClick);
    this.root.remove();
  }

  private buildNode(n: TreeNode, path: string, depth: number, parent: FlatNode | null): HTMLDivElement {
    const wrap = document.createElement('div');
    const el = document.createElement('div');
    el.className = 'th-node';
    el.setAttribute('role', 'treeitem');
    el.tabIndex = 0;
    el.dataset.path = path;
    if (n.off !== undefined) el.dataset.off = String(n.off);
    if (n.len !== undefined) el.dataset.len = String(n.len);
    el.style.paddingLeft = `${depth * 12}px`;
    const hasKids = !!n.children?.length;
    const tog = document.createElement('span');
    tog.className = 'th-tog';
    tog.textContent = hasKids ? (depth === 0 && !this.options.collapseTopLayers ? '▾' : '▸') : ' ';
    const label = document.createElement('span');
    label.textContent = n.label;
    el.append(tog, label);
    wrap.append(el);
    const f: FlatNode = { node: n, path, depth, el, parent, childrenEl: null };
    this.flat.push(f);
    if (hasKids) {
      const kids = document.createElement('div');
      kids.className = 'th-kids';
      kids.hidden = depth !== 0 || !!this.options.collapseTopLayers; // top layers open, fields collapsed
      el.setAttribute('aria-expanded', String(!kids.hidden));
      n.children!.forEach((c, i) => kids.append(this.buildNode(c, `${path}.${i}`, depth + 1, f)));
      wrap.append(kids);
      f.childrenEl = kids;
    }
    return wrap;
  }

  private renderHex(bytes: Uint8Array | null): void {
    this.byteEls = [];
    this.asciiEls = [];
    if (!bytes || bytes.length === 0) {
      this.hexEl.replaceChildren();
      return;
    }
    const n = Math.min(bytes.length, MAX_HEX_BYTES);
    const lines: HTMLDivElement[] = [];
    for (let base = 0; base < n; base += 16) {
      const line = document.createElement('div');
      line.className = 'th-line';
      const off = document.createElement('span');
      off.className = 'th-off';
      off.textContent = base.toString(16).padStart(4, '0');
      line.append(off);
      const a = document.createElement('span');
      a.className = 'th-ascii';
      for (let i = base; i < Math.min(base + 16, n); i++) {
        const b = bytes[i] as number;
        const s = document.createElement('span');
        s.className = 'th-b';
        s.dataset.off = String(i);
        s.textContent = b.toString(16).padStart(2, '0');
        line.append(s);
        this.byteEls.push(s);
        const char = document.createElement('span');
        char.className = 'th-char';
        char.dataset.off = String(i);
        char.textContent = b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '.';
        a.append(char);
        this.asciiEls.push(char);
      }
      line.append(a);
      lines.push(line);
    }
    if (bytes.length > n) {
      const more = document.createElement('div');
      more.className = 'muted';
      more.textContent = `… ${bytes.length - n} B 더 (원본 링크 사용)`;
      lines.push(more);
    }
    this.hexEl.replaceChildren(...lines);
  }

  private selectFlat(f: FlatNode): void {
    if (this.selected) {
      this.selected.el.classList.remove('selected');
      this.selected.el.setAttribute('aria-selected', 'false');
    }
    this.selected = f;
    f.el.classList.add('selected');
    f.el.setAttribute('aria-selected', 'true');
    for (let p = f.parent; p; p = p.parent) {
      if (p.childrenEl) p.childrenEl.hidden = false;
      p.el.setAttribute('aria-expanded', 'true');
      const t = p.el.querySelector('.th-tog');
      if (t) t.textContent = '▾';
    }
    const { off, len } = f.node;
    this.highlight(off !== undefined && len !== undefined ? { off, len } : null);
    f.el.scrollIntoView?.({ block: 'nearest' });
    this.options.onSelectNode?.(f.node);
  }

  private highlight(r: { off: number; len: number } | null): void {
    if (this.range) for (let i = Math.max(0, this.range.off); i < Math.min(this.byteEls.length, this.range.off + this.range.len); i++) {
      this.byteEls[i]?.classList.remove('hl');
      this.asciiEls[i]?.classList.remove('hl');
    }
    this.range = r;
    if (!r) return;
    for (let i = Math.max(0, r.off); i < Math.min(this.byteEls.length, r.off + r.len); i++) {
      this.byteEls[i]?.classList.add('hl');
      this.asciiEls[i]?.classList.add('hl');
    }
    this.byteEls[r.off]?.scrollIntoView?.({ block: 'nearest' });
  }

  private onTreeClick = (e: Event): void => {
    const el = (e.target as HTMLElement).closest('.th-node') as HTMLElement | null;
    const path = el?.dataset.path;
    if (!path) return;
    const f = this.flat.find((x) => x.path === path);
    if (!f) return;
    // clicking the toggle of an already-selected node folds/unfolds it
    if ((e.target as HTMLElement).classList.contains('th-tog') && f.childrenEl) {
      f.childrenEl.hidden = !f.childrenEl.hidden;
      f.el.setAttribute('aria-expanded', String(!f.childrenEl.hidden));
      (e.target as HTMLElement).textContent = f.childrenEl.hidden ? '▸' : '▾';
      return;
    }
    this.selectFlat(f);
  };

  private onTreeKey = (e: KeyboardEvent): void => {
    const path = (e.target as HTMLElement).closest<HTMLElement>('.th-node')?.dataset.path;
    const f = this.flat.find((item) => item.path === path);
    if (!f || !['ArrowLeft', 'ArrowRight', 'Enter', ' '].includes(e.key)) return;
    e.preventDefault();
    this.selectFlat(f);
    if (f.childrenEl && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      f.childrenEl.hidden = e.key === 'ArrowLeft';
      f.el.setAttribute('aria-expanded', String(!f.childrenEl.hidden));
      const toggle = f.el.querySelector('.th-tog');
      if (toggle) toggle.textContent = f.childrenEl.hidden ? '▸' : '▾';
    }
  };

  private onHexClick = (e: Event): void => {
    const off = (e.target as HTMLElement).dataset.off;
    if (off !== undefined) this.selectByte(Number(off));
  };
}
