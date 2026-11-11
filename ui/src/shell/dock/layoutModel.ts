// Loom layout model (plan §5.7). The app owns the format; the docking library's own
// serialization is kept only as a sub-field (`vendor.dockview`) and is used only when it was
// written by the same library version AND describes the same panels as `root` — otherwise
// the dock is rebuilt from `root` (so a dockview upgrade never loses a saved layout).
//
//   {"loom_layout":1, "name":"can-analysis", "env_hint":"bench",
//    "root":{"split":"h","sizes":[60,40],"children":[
//       {"panel":"can.trace","props":{"channel":"ch0"}},
//       {"split":"v","children":[{"tabs":[{"panel":"uds.console","props":{"targets":"bcm"}}, …],"active":0}, …]}]},
//    "pinned":["core.activity"],
//    "vendor":{"dockview":{"version":"8.8.3","state":{…toJSON()…}}}}
//
// Nodes: split (h = side by side, v = stacked; sizes = relative weights), tabs (one dock
// group), or a single panel leaf (= a group with one tab).
// Bounds (C2): serialized ≤ MAX_LAYOUT_BYTES (1 MiB), depth ≤ MAX_DEPTH, panels ≤ MAX_PANELS.
// Unknown panel ids are KEPT (shown as "이 버전에 없는 패널" placeholders) so saving again does
// not silently delete a newer panel. Props are validated against the manifest's propsSchema.

import type { Env } from '../../proto/provisional';
import type { Props } from '../../sdk/manifest';
import { validateProps } from '../../sdk/propsSchema';
import { getPanel } from '../../sdk/registry';
import type { Preset, Slot } from '../routes';

export const LAYOUT_VERSION = 1;
export const MAX_LAYOUT_BYTES = 1024 * 1024;
export const MAX_DEPTH = 8;
export const MAX_PANELS = 32;
export const DOCKVIEW_VERSION = '8.8.3';
/** panels the shell renders itself (not in the panel registry) */
export const SHELL_PANELS: ReadonlySet<string> = new Set(['shell.test']);
const NAME_RE = /^[a-z0-9][a-z0-9-]{0,47}$/;

export interface LayoutLeaf {
  panel: string;
  props: Props;
}
export interface LayoutTabs {
  tabs: LayoutLeaf[];
  active?: number;
}
export interface LayoutSplit {
  split: 'h' | 'v';
  sizes?: number[];
  children: LayoutNode[];
}
export type LayoutNode = LayoutLeaf | LayoutTabs | LayoutSplit;

export interface LoomLayout {
  loom_layout: 1;
  name: string;
  env_hint: Env | null;
  root: LayoutNode;
  pinned: string[];
  vendor?: { dockview?: { version: string; state: unknown } };
}

export interface LayoutIssue {
  level: 'warn' | 'error';
  text: string;
}

export const isSplit = (n: LayoutNode): n is LayoutSplit => 'split' in n;
export const isTabs = (n: LayoutNode): n is LayoutTabs => 'tabs' in n;

/** every leaf, depth-first (tab order) */
export function leaves(n: LayoutNode): LayoutLeaf[] {
  if (isSplit(n)) return n.children.flatMap(leaves);
  if (isTabs(n)) return n.tabs;
  return [n];
}

export function isKnownPanel(id: string): boolean {
  return SHELL_PANELS.has(id) || getPanel(id) !== undefined;
}

// ---------------------------------------------------------------- presets

/** M1 fixed presets → initial dock arrangement (all panels visible as splits, not hidden tabs) */
export function presetToLayout(p: Preset): LoomLayout {
  const leaf = (s: Slot): LayoutLeaf => ({ panel: s.panel, props: { ...s.props } });
  const [a, b, c] = p.slots;
  let root: LayoutNode;
  if (p.grid === 'single' || p.slots.length === 1) root = leaf(a as Slot);
  else if (p.grid === 'two-top-one-bottom' && a && b && c) {
    root = { split: 'v', sizes: [60, 40], children: [{ split: 'h', children: [leaf(a), leaf(b)] }, leaf(c)] };
  } else root = { split: 'v', children: p.slots.map(leaf) };
  return { loom_layout: 1, name: p.name, env_hint: null, root, pinned: [] };
}

/** deep link `#/p/…` → a one-panel layout (not saved) */
export function slotLayout(s: Slot): LoomLayout {
  return { loom_layout: 1, name: '', env_hint: null, root: { panel: s.panel, props: { ...s.props } }, pinned: [] };
}

// ---------------------------------------------------------------- parse / migrate / validate

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

export function layoutBytes(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * Parse a stored/imported layout. Returns null (+ error issues) when it cannot be used.
 * Migration: no `loom_layout` + `{preset}` or `{slots[]}` (M1 local shapes) → v1; a version
 * NEWER than this build is refused (never guessed).
 */
export function parseLayout(input: string | unknown): { layout: LoomLayout | null; issues: LayoutIssue[] } {
  const issues: LayoutIssue[] = [];
  let raw: unknown = input;
  if (typeof input === 'string') {
    if (layoutBytes(input) > MAX_LAYOUT_BYTES) return { layout: null, issues: [{ level: 'error', text: `레이아웃이 1 MiB를 넘음 (${layoutBytes(input)} B)` }] };
    try {
      raw = JSON.parse(input);
    } catch {
      return { layout: null, issues: [{ level: 'error', text: 'JSON이 아님' }] };
    }
  }
  if (!isObj(raw)) return { layout: null, issues: [{ level: 'error', text: '레이아웃이 객체가 아님' }] };
  const v = raw.loom_layout;
  if (typeof v === 'number' && v > LAYOUT_VERSION) {
    return { layout: null, issues: [{ level: 'error', text: `더 새 레이아웃 형식 (loom_layout ${v}) — 이 UI는 ${LAYOUT_VERSION}까지` }] };
  }
  let rootRaw: unknown = raw.root;
  if (v === undefined) {
    // M1 local shapes
    if (Array.isArray(raw.slots)) {
      rootRaw = { split: 'v', children: raw.slots };
      issues.push({ level: 'warn', text: 'M1 slots 형식을 loom_layout 1로 옮김' });
    } else if (typeof raw.preset === 'string') {
      return { layout: null, issues: [{ level: 'error', text: `프리셋 참조 "${raw.preset}" — 프리셋으로 여세요` }] };
    }
  } else if (v !== LAYOUT_VERSION) {
    return { layout: null, issues: [{ level: 'error', text: `알 수 없는 loom_layout ${String(v)}` }] };
  }
  let count = 0;
  const node = (n: unknown, depth: number): LayoutNode | null => {
    if (depth > MAX_DEPTH) {
      issues.push({ level: 'warn', text: `깊이 > ${MAX_DEPTH} — 그 아래 버림` });
      return null;
    }
    if (!isObj(n)) return null;
    if (n.split === 'h' || n.split === 'v') {
      const kids = (Array.isArray(n.children) ? n.children : []).map((c) => node(c, depth + 1)).filter((c): c is LayoutNode => c !== null);
      if (kids.length === 0) return null;
      if (kids.length === 1) return kids[0] as LayoutNode;
      const sizes = Array.isArray(n.sizes) && n.sizes.length === kids.length && n.sizes.every((x) => typeof x === 'number' && x > 0) ? (n.sizes as number[]) : undefined;
      return { split: n.split, ...(sizes ? { sizes } : {}), children: kids };
    }
    if (Array.isArray(n.tabs)) {
      const tabs = n.tabs.map((t) => leaf(t)).filter((t): t is LayoutLeaf => t !== null);
      if (tabs.length === 0) return null;
      const active = typeof n.active === 'number' && n.active >= 0 && n.active < tabs.length ? n.active : 0;
      return tabs.length === 1 ? (tabs[0] as LayoutLeaf) : { tabs, active };
    }
    return leaf(n);
  };
  const leaf = (n: unknown): LayoutLeaf | null => {
    if (!isObj(n) || typeof n.panel !== 'string' || !/^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$/.test(n.panel)) {
      issues.push({ level: 'warn', text: '패널 id가 없거나 형식이 틀린 칸 — 버림' });
      return null;
    }
    if (++count > MAX_PANELS) {
      if (count === MAX_PANELS + 1) issues.push({ level: 'warn', text: `패널 > ${MAX_PANELS} — 뒤는 버림` });
      return null;
    }
    const reg = getPanel(n.panel);
    if (!reg && !SHELL_PANELS.has(n.panel)) {
      issues.push({ level: 'warn', text: `이 버전에 없는 패널 "${n.panel}" — 자리표시로 둠` });
      return { panel: n.panel, props: isObj(n.props) ? { ...n.props } : {} };
    }
    const chk = validateProps(reg?.manifest.propsSchema, n.props ?? {});
    for (const p of chk.problems) issues.push({ level: 'warn', text: `${n.panel}: ${p}` });
    return { panel: n.panel, props: chk.props };
  };
  const root = node(rootRaw, 0);
  if (!root) return { layout: null, issues: [...issues, { level: 'error', text: '패널이 하나도 없음' }] };
  const name = typeof raw.name === 'string' && NAME_RE.test(raw.name) ? raw.name : '';
  const env = raw.env_hint === 'sim' || raw.env_hint === 'bench' || raw.env_hint === 'vehicle' ? raw.env_hint : null;
  const pinned = Array.isArray(raw.pinned) ? raw.pinned.filter((x): x is string => typeof x === 'string').slice(0, 16) : [];
  const vendorRaw = isObj(raw.vendor) && isObj(raw.vendor.dockview) ? raw.vendor.dockview : null;
  const vendor =
    vendorRaw && typeof vendorRaw.version === 'string' && vendorRaw.state !== undefined ? { dockview: { version: vendorRaw.version, state: vendorRaw.state } } : undefined;
  return { layout: { loom_layout: 1, name, env_hint: env, root, pinned, ...(vendor ? { vendor } : {}) }, issues };
}

/** JSON text, refused over 1 MiB (the caller keeps the previous saved copy) */
export function serializeLayout(l: LoomLayout): { ok: true; text: string } | { ok: false; error: string } {
  const text = JSON.stringify(l);
  const n = layoutBytes(text);
  return n > MAX_LAYOUT_BYTES ? { ok: false, error: `레이아웃이 1 MiB를 넘음 (${n} B) — 저장 안 함` } : { ok: true, text };
}

/** name collision → `<name>-2`, `-3`, … (never overwrite; presets are reserved names) */
export function uniqueName(want: string, taken: ReadonlySet<string>): string {
  const base = (want.toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '') || 'layout').slice(0, 40);
  if (!taken.has(base)) return base;
  for (let i = 2; i < 10_000; i++) if (!taken.has(`${base}-${i}`)) return `${base}-${i}`;
  return `${base}-${Date.now()}`;
}

// ---------------------------------------------------------------- dockview grid conversion

/** dockview SerializedDockview subset we read/write (structural, version-independent fields) */
export interface DvGridObject {
  type: 'leaf' | 'branch';
  data: DvGroupState | DvGridObject[];
  size?: number;
}
export interface DvGroupState {
  views: string[];
  activeView?: string;
  id: string;
}
export interface DvPanelState {
  id: string;
  contentComponent?: string;
  title?: string;
  params?: Record<string, unknown>;
}
export interface DvSerialized {
  grid: { root: DvGridObject; width: number; height: number; orientation: 'HORIZONTAL' | 'VERTICAL' };
  panels: Record<string, DvPanelState>;
  activeGroup?: string;
}

export const PANEL_COMPONENT = 'loom-panel';

/**
 * root → dockview JSON. A branch's orientation alternates with depth in dockview, so the
 * root orientation is chosen from the root split and nested splits with the same direction
 * are flattened into their parent; `title` comes from `titleOf`.
 */
export function toDockview(root: LayoutNode, width: number, height: number, titleOf: (l: LayoutLeaf) => string): DvSerialized {
  let pid = 0;
  let gid = 0;
  const panels: Record<string, DvPanelState> = {};
  const group = (tabs: LayoutLeaf[], active: number, size: number): DvGridObject => {
    const ids = tabs.map((t) => {
      const id = `p${++pid}`;
      panels[id] = { id, contentComponent: PANEL_COMPONENT, title: titleOf(t), params: { panel: t.panel, props: t.props } };
      return id;
    });
    return { type: 'leaf', data: { views: ids, activeView: ids[active] ?? ids[0], id: `g${++gid}` } satisfies DvGroupState, size };
  };
  // orientation of a branch at depth d: root orientation, then alternating
  const rootOrient: 'HORIZONTAL' | 'VERTICAL' = isSplit(root) && root.split === 'v' ? 'VERTICAL' : 'HORIZONTAL';
  const orientAt = (d: number) => ((d % 2 === 0) === (rootOrient === 'HORIZONTAL') ? 'h' : 'v');
  const build = (n: LayoutNode, d: number, size: number, alongPx: number, crossPx: number): DvGridObject => {
    if (!isSplit(n)) return isTabs(n) ? group(n.tabs, n.active ?? 0, size) : group([n], 0, size);
    const want = orientAt(d);
    if (n.split !== want) {
      // a split in the other direction at this depth → wrap one level deeper
      return { type: 'branch', data: [build(n, d + 1, alongPx, crossPx, alongPx)], size };
    }
    const w = n.sizes ?? n.children.map(() => 1);
    const sum = w.reduce((a, b) => a + b, 0);
    const kids = n.children.map((c, i) => build(c, d + 1, Math.max(40, Math.round(((w[i] ?? 1) / sum) * alongPx)), crossPx, Math.max(40, Math.round(((w[i] ?? 1) / sum) * alongPx))));
    return { type: 'branch', data: kids, size };
  };
  const along = rootOrient === 'HORIZONTAL' ? width : height;
  const cross = rootOrient === 'HORIZONTAL' ? height : width;
  let gridRoot = build(root, 0, cross, along, cross);
  if (gridRoot.type === 'leaf') gridRoot = { type: 'branch', data: [{ ...gridRoot, size: along }], size: cross };
  return { grid: { root: gridRoot, width, height, orientation: rootOrient }, panels };
}

/** dockview JSON → root (sizes become relative weights); unknown shapes → null */
export function fromDockview(s: unknown): LayoutNode | null {
  if (!isObj(s) || !isObj(s.grid) || !isObj(s.panels)) return null;
  const panels = s.panels as Record<string, DvPanelState>;
  const orient = s.grid.orientation === 'VERTICAL' ? 'v' : 'h';
  const leafOf = (id: string): LayoutLeaf | null => {
    const p = panels[id]?.params;
    return isObj(p) && typeof p.panel === 'string' ? { panel: p.panel, props: isObj(p.props) ? { ...p.props } : {} } : null;
  };
  const walk = (o: unknown, dir: 'h' | 'v'): LayoutNode | null => {
    if (!isObj(o)) return null;
    if (o.type === 'leaf' && isObj(o.data)) {
      const g = o.data as unknown as DvGroupState;
      const tabs = (Array.isArray(g.views) ? g.views : []).map(leafOf).filter((x): x is LayoutLeaf => x !== null);
      if (tabs.length === 0) return null;
      const active = Math.max(0, g.views.indexOf(g.activeView ?? ''));
      return tabs.length === 1 ? (tabs[0] as LayoutLeaf) : { tabs, active };
    }
    if (o.type === 'branch' && Array.isArray(o.data)) {
      const next = dir === 'h' ? 'v' : 'h';
      const kids: LayoutNode[] = [];
      const sizes: number[] = [];
      for (const c of o.data) {
        const k = walk(c, next);
        if (!k) continue;
        kids.push(k);
        sizes.push(isObj(c) && typeof c.size === 'number' && c.size > 0 ? c.size : 1);
      }
      if (kids.length === 0) return null;
      if (kids.length === 1) return kids[0] as LayoutNode;
      const sum = sizes.reduce((a, b) => a + b, 0);
      const rel = sizes.map((x) => Math.round((x / sum) * 1000) / 10);
      // an even split is stored without sizes (same as a hand-written layout)
      const even = Math.max(...rel) - Math.min(...rel) < 1;
      return { split: dir, ...(even ? {} : { sizes: rel }), children: kids };
    }
    return null;
  };
  return walk(s.grid.root, orient);
}

/** the vendor state is usable iff same library version and the same panel multiset as root */
export function vendorMatches(l: LoomLayout): boolean {
  const v = l.vendor?.dockview;
  if (!v || v.version !== DOCKVIEW_VERSION) return false;
  const r = fromDockview(v.state);
  if (!r) return false;
  const key = (n: LayoutNode) =>
    leaves(n)
      .map((x) => `${x.panel}?${JSON.stringify(x.props)}`)
      .sort()
      .join('|');
  return key(r) === key(l.root);
}
