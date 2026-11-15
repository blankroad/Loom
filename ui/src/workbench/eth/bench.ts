// Bench Light diagnostic-workbench building blocks shared by eth.packets and uds.console.
// Ported from the selected web references (work/reports/ethernet-workbench/opus,
// work/reports/uds-read-did/app): 46px icon toolbar buttons, segmented controls, pane headers,
// resizable dividers. DOM only — no data, no timers, no simulated values.
import './bench.css';

/** Stroke icons (16×16 viewBox) copied from the reference symbol sheet. */
const ICONS: Record<string, string> = {
  plus: 'M8 3v10M3 8h10',
  x: 'M4 4l8 8M12 4l-8 8',
  play: 'M4.5 2.8v10.4L13 8z',
  stop: 'M3.5 3.5h9v9h-9z',
  pause: 'M5.5 3v10M10.5 3v10',
  trash: 'M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4M6.8 6.5v4.5M9.2 6.5v4.5',
  down: 'M8 2.5v8M4.5 7 8 10.5 11.5 7M3 13.5h10',
  eth: 'M6 1.8h4v3.4H6zM1.8 10.8h4v3.4h-4zM10.2 10.8h4v3.4h-4zM8 5.2v2.8M3.8 10.8V8h8.4v2.8',
  follow: 'M2 5h9M8.5 2.5 11 5 8.5 7.5M14 11H5M7.5 8.5 5 11l2.5 2.5',
  send: 'M2.5 8h9M8 4.5 11.5 8 8 11.5M13.5 3v10',
  filter: 'M2 3h12L9.5 8.5V13l-3-1.5v-3z',
  color: 'M2 3h12v3H2zM2 7h12v3H2zM2 12.5h12',
  scroll: 'M8 2.5v10M4.5 9 8 12.5 11.5 9M3 14h10',
  edit: 'M10.5 2.5l3 3L6 13H3v-3z',
  copy: 'M5 5h8.5v8.5H5zM3 10.5V3.8A1.3 1.3 0 0 1 4.3 2.5H10.5',
  chev: 'M4 6l4 4 4-4',
  clock: 'M8 2a6 6 0 1 0 0 12A6 6 0 0 0 8 2zM8 4.5V8l2.5 1.5',
  pin: 'M8 1.5v4M5 5.5h6l-1 4H6zM8 9.5V14.5',
  link: 'M6.5 9.5l3-3M7 4.5l1-1a2.5 2.5 0 0 1 3.5 3.5l-1 1M9 11.5l-1 1A2.5 2.5 0 0 1 4.5 9l1-1',
  gear: 'M8 5.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4',
  read: 'M2.5 8h9M8 4.5 11.5 8 8 11.5M13.5 3v10',
  panel: 'M2 2.5h12v11H2zM10 2.5v11',
  kbd: 'M1.5 4h13v8.5h-13zM4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M5 10h6',
};

const SVGNS = 'http://www.w3.org/2000/svg';
export function icon(name: string): SVGSVGElement {
  const svg = document.createElementNS(SVGNS, 'svg');
  svg.setAttribute('class', 'wb-i');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('aria-hidden', 'true');
  const p = document.createElementNS(SVGNS, 'path');
  p.setAttribute('d', ICONS[name] ?? '');
  svg.append(p);
  return svg;
}

export function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** 46px module toolbar button: icon over a short label. The label lives in `.wb-tl` so callers can change it. */
export function tbtn(iconName: string, label: string, title: string, cmd?: string): HTMLButtonElement & { label: HTMLSpanElement } {
  const b = el('button', 'wb-tbtn') as HTMLButtonElement & { label: HTMLSpanElement };
  b.type = 'button';
  b.title = title;
  if (cmd) b.dataset.cmd = cmd;
  const l = el('span', 'wb-tl', label);
  b.append(icon(iconName), l);
  b.label = l;
  return b;
}

export function btn(text: string, cls = 'wb-btn', title?: string): HTMLButtonElement {
  const b = el('button', cls, text);
  b.type = 'button';
  if (title) b.title = title;
  return b;
}

export const vdiv = (): HTMLSpanElement => el('span', 'wb-vdiv');
export const grow = (): HTMLSpanElement => el('span', 'wb-grow');

/** Segmented radio group (reference `.seg`). */
export function seg<T extends string>(
  items: readonly { value: T; label: string; title?: string; disabled?: boolean }[],
  value: T,
  onChange: (v: T) => void,
  aria: string,
): { root: HTMLSpanElement; set(v: T): void; buttons: Map<T, HTMLButtonElement> } {
  const root = el('span', 'wb-seg');
  root.setAttribute('role', 'radiogroup');
  root.setAttribute('aria-label', aria);
  const buttons = new Map<T, HTMLButtonElement>();
  let cur = value;
  const set = (v: T) => {
    cur = v;
    for (const [k, b] of buttons) b.setAttribute('aria-checked', String(k === cur));
  };
  for (const it of items) {
    const b = btn(it.label, '', it.title);
    b.setAttribute('role', 'radio');
    b.dataset.v = it.value;
    b.disabled = it.disabled === true;
    b.addEventListener('click', () => {
      if (b.disabled || cur === it.value) return;
      set(it.value);
      onChange(it.value);
    });
    buttons.set(it.value, b);
    root.append(b);
  }
  set(value);
  return { root, set, buttons };
}

/** Pane with the reference 26px header. */
export function pane(cls: string, title: string): { root: HTMLElement; head: HTMLElement; title: HTMLSpanElement } {
  const root = el('section', `wb-pane ${cls}`);
  root.setAttribute('aria-label', title);
  const head = el('div', 'wb-pane-h');
  const t = el('span', 'wb-pane-t', title);
  head.append(t);
  root.append(head);
  return { root, head, title: t };
}

export function loadJson<T>(key: string, check: (v: unknown) => v is T, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    const v = JSON.parse(raw) as unknown;
    return check(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

export function saveJson(key: string, v: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* storage unavailable: keep in memory */
  }
}

/**
 * Drag + keyboard divider between two panes. `get`/`set` work in percent of the container.
 * Arrow keys step ±4 %, Home/dblclick restore the default.
 */
export function divider(
  sp: HTMLElement,
  o: {
    orientation: 'horizontal' | 'vertical';
    label: string;
    container: () => HTMLElement;
    get: () => number;
    set: (pct: number, persist: boolean) => void;
    def: number;
    min?: number;
    max?: number;
  },
): void {
  const min = o.min ?? 12;
  const max = o.max ?? 88;
  const clamp = (v: number) => Math.max(min, Math.min(max, v));
  sp.classList.add('wb-divider', o.orientation === 'horizontal' ? 'h' : 'v');
  sp.setAttribute('role', 'separator');
  sp.setAttribute('aria-orientation', o.orientation);
  sp.setAttribute('aria-label', o.label);
  sp.setAttribute('aria-valuemin', String(min));
  sp.setAttribute('aria-valuemax', String(max));
  sp.tabIndex = 0;
  sp.title = `${o.label} — 끌기 또는 화살표, 더블클릭 기본값`;
  sp.addEventListener('keydown', (e) => {
    const back = o.orientation === 'horizontal' ? 'ArrowUp' : 'ArrowLeft';
    const fwd = o.orientation === 'horizontal' ? 'ArrowDown' : 'ArrowRight';
    let v: number | null = null;
    if (e.key === back) v = clamp(o.get() - 4);
    else if (e.key === fwd) v = clamp(o.get() + 4);
    else if (e.key === 'Home') v = o.def;
    if (v === null) return;
    e.preventDefault();
    e.stopPropagation();
    o.set(v, true);
  });
  sp.addEventListener('dblclick', () => o.set(o.def, true));
  sp.addEventListener('pointerdown', (e) => {
    const box = o.container().getBoundingClientRect();
    const size = o.orientation === 'horizontal' ? box.height : box.width;
    if (size <= 0) return;
    e.preventDefault();
    sp.setPointerCapture?.(e.pointerId);
    sp.classList.add('dragging');
    const move = (m: PointerEvent) => {
      const pos = o.orientation === 'horizontal' ? m.clientY - box.top : m.clientX - box.left;
      o.set(clamp((pos / size) * 100), false);
    };
    const up = () => {
      sp.classList.remove('dragging');
      sp.removeEventListener('pointermove', move);
      sp.removeEventListener('pointerup', up);
      sp.removeEventListener('pointercancel', up);
      o.set(o.get(), true);
    };
    sp.addEventListener('pointermove', move);
    sp.addEventListener('pointerup', up);
    sp.addEventListener('pointercancel', up);
  });
}
