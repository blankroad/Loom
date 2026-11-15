// Docking host (plan §5.7, §7: dockview-core, MIT, framework-independent). Imperative: panels
// are already mount/dispose objects. Everything the shell must always show (top bar, STOP,
// channel bar, Activity) stays OUTSIDE the dock, so STOP is in the same place in any layout.
//
// Panel lifecycle inside a dock tab (§3.3, UI-T10):
//   open    → createPanelContext + factory + mount (subscriptions start)
//   hidden  → setVisible(false): the RenderScheduler stops drawing it; subscriptions stay and
//             data keeps flowing into its mirror; shown again → one catch-up draw
//   resize  → coalesced to one resize per animation frame (xterm fit, uPlot setSize)
//   closed  → instance.dispose + context.dispose (subscription refcount drops)
// A panel id this build does not know renders "이 버전에 없는 패널" (and is kept on save).

import { createDockview, themeLight, type DockviewApi, type DockviewGroupPanel, type IDockviewGroupPanel, type GroupPanelPartInitParameters, type IContentRenderer, type IDockviewPanel, type IHeaderActionsRenderer, type IGroupHeaderProps, type ITabRenderer, type TabPartInitParameters } from 'dockview-core';
import { uartBench } from '../../workbench/uart/workbench';
import { icon } from '../../workbench/uart/icons';
import { createRoot, type Root } from 'react-dom/client';
import './dockview.css';
import { createElement } from 'react';
import { createPanelContext, type PanelContextHandle, type ShellUiHooks } from '../../sdk/context';
import type { Props } from '../../sdk/manifest';
import { validateProps } from '../../sdk/propsSchema';
import { getPanel, type PanelInstance } from '../../sdk/registry';
import { sameSlot, slotKey, type Slot } from '../routes';
import { availableSlot, deviceKey } from './activeWorkspace';
import type { WorkspaceChannel } from '../routes';
import type { ShellServices } from '../services';
import { ShellCtx } from '../ShellContext';
import { TestScreen } from '../test/TestScreen';
import {
  DOCKVIEW_VERSION,
  MAX_PANELS,
  fromDockview,
  PANEL_COMPONENT,
  toDockview,
  vendorMatches,
  type LayoutIssue,
  type LayoutLeaf,
  type LoomLayout,
} from './layoutModel';

export interface DockController {
  /** React wrapper owns automatic workspace synchronization when true. */
  lifecycleManaged?: boolean;
  /** the open dock panel showing this slot (props compared without the `at` cursor) */
  find(slot: Slot): IDockviewPanel | null;
  /** activate the slot's tab, or add it as a tab next to the active one */
  open(slot: Slot, options?: { activate?: boolean }): 'shown' | 'added';
}

let instanceSeq = 0;

export function panelTitle(l: { panel: string; props: Props }): string {
  const reg = getPanel(l.panel);
  const base = reg?.manifest.title ?? (l.panel === 'shell.test' ? '테스트 실행' : l.panel);
  const ch = l.props.channel ?? l.props.targets ?? l.props.ch;
  return typeof ch === 'string' && ch ? `${base} · ${ch}` : base;
}

type Raf = (cb: () => void) => number;

/** content of one dock tab */
class PanelRenderer implements IContentRenderer {
  readonly element: HTMLElement;
  private inst: PanelInstance | null = null;
  private handle: PanelContextHandle | null = null;
  private root: Root | null = null;
  private body: HTMLElement;
  private rafId: number | null = null;
  private size: { w: number; h: number } | null = null;
  private offVis: { dispose(): void } | null = null;
  /** dockview's own tab visibility; the effective visibility also needs the whole dock to be shown */
  private tabVisible = true;
  private shown = true;

  constructor(
    private readonly services: ShellServices,
    private readonly raf: Raf,
    private readonly cancelRaf: (id: number) => void,
    private readonly owner: Set<PanelRenderer> = new Set(),
    shown = true,
    private readonly ui: ShellUiHooks = services.uiHooks,
  ) {
    this.shown = shown;
    owner.add(this);
    this.element = document.createElement('div');
    this.element.className = 'dock-panel panel-slot';
    this.body = document.createElement('div');
    this.body.className = 'panel-body';
    this.element.append(this.body);
  }

  init(p: GroupPanelPartInitParameters): void {
    const prm = (p.params ?? {}) as { panel?: unknown; props?: unknown };
    const panel = typeof prm.panel === 'string' ? prm.panel : '?';
    this.element.dataset.panel = panel;
    if (panel === 'shell.test') {
      this.root = createRoot(this.body);
      this.root.render(createElement(ShellCtx.Provider, { value: this.services }, createElement(TestScreen)));
      return;
    }
    const reg = getPanel(panel);
    if (!reg) {
      this.placeholder(`이 버전에 없는 패널: ${panel}`, '저장된 레이아웃에는 남겨 둡니다 (새 버전에서 다시 열림).', 'panel-unknown');
      return;
    }
    const chk = validateProps(reg.manifest.propsSchema, prm.props ?? {});
    if (!chk.ok) {
      this.placeholder(`${panel}: props 검증 실패`, chk.problems.join(' · '), 'panel-bad-props');
      return;
    }
    const s = this.services;
    this.handle = createPanelContext(
      reg.manifest,
      chk.props,
      {
        net: s.net,
        gate: s.gate,
        scheduler: s.scheduler,
        rows: s.rows,
        shell: s.shell,
        cursor: s.cursor,
        agentCursor: s.activity,
        ui: this.ui,
        metrics: s.metrics,
        now: s.now,
      },
      `${panel}#d${++instanceSeq}`,
    );
    this.inst = reg.factory(this.handle.ctx);
    this.inst.mount(this.body);
    this.tabVisible = p.api.isVisible;
    this.handle.setVisible(this.tabVisible && this.shown);
    this.offVis = p.api.onDidVisibilityChange((e) => { this.tabVisible = e.isVisible; this.handle?.setVisible(e.isVisible && this.shown); });
  }

  /** the dock holding this panel was hidden (another module tab is in front) or shown again: a hidden module keeps
   * its instance, subscriptions and drafts, and only stops drawing (one catch-up draw when shown). */
  setShown(shown: boolean): void {
    if (this.shown === shown) return;
    this.shown = shown;
    this.handle?.setVisible(this.tabVisible && shown);
  }

  private placeholder(title: string, detail: string, testid: string): void {
    const d = document.createElement('div');
    d.className = 'panel-empty';
    d.dataset.testid = testid;
    const t = document.createElement('strong');
    t.textContent = title;
    const m = document.createElement('p');
    m.className = 'muted';
    m.textContent = detail;
    d.append(t, m);
    this.body.append(d);
  }

  /** dockview calls this on every size change — the panel resizes at most once per frame */
  layout(width: number, height: number): void {
    this.size = { w: width, h: height };
    if (this.rafId !== null || !this.inst) return;
    this.rafId = this.raf(() => {
      this.rafId = null;
      if (this.size && this.inst) this.inst.resize(this.size.w, this.size.h);
    });
  }

  dispose(): void {
    this.owner.delete(this);
    if (this.rafId !== null) this.cancelRaf(this.rafId);
    this.rafId = null;
    this.offVis?.dispose();
    this.inst?.dispose();
    this.handle?.dispose();
    this.root?.unmount();
    this.inst = null;
    this.handle = null;
    this.root = null;
    this.body.replaceChildren();
  }
}

const svgIcon = (id: string): string => icon(id);
const paneNoOf = (api: DockviewApi, group: IDockviewGroupPanel): number => api.groups.findIndex((g) => g.id === group.id) + 1;

/** REV6.1 tab: colour dot (state), alias, port, × — for every panel; UART tabs carry the session colour/port. */
class PaneTab implements ITabRenderer {
  readonly element: HTMLElement;
  private offs: (() => void)[] = [];
  private params: { panel?: string; props?: Props } = {};
  private title = '';
  private p: TabPartInitParameters | null = null;
  constructor(private readonly services: ShellServices) {
    this.element = document.createElement('div');
    this.element.className = 'tab';
  }
  init(p: TabPartInitParameters): void {
    this.p = p;
    this.params = (p.params ?? {}) as { panel?: string; props?: Props };
    this.title = p.title ?? '';
    this.render();
    this.offs.push(uartBench.subscribe(() => this.render()), this.services.shell.subscribe(() => this.render()));
    const t = p.api.onDidTitleChange((e) => { this.title = e.title; this.render(); });
    this.offs.push(() => t.dispose());
  }
  private render(): void {
    const panel = this.params.panel ?? '';
    const props = this.params.props ?? {};
    const ch = typeof props.channel === 'string' ? props.channel : '';
    const isUart = panel.startsWith('uart.') && ch;
    const sess = isUart ? uartBench.sessionFor(ch) : undefined;
    const term = isUart ? uartBench.terminal(ch) : undefined;
    const domain = panel.split('.')[0] ?? '';
    const state = ch ? this.services.shell.channels.get(`${domain}/${ch}`)?.state : undefined;
    const dotCls = state === 'closed' ? 'dot off' : state === 'unknown' ? 'dot lost' : 'dot';
    const label = isUart ? (panel === 'uart.trace' ? `${ch} · 트레이스` : ch) : this.title;
    const port = isUart ? (term?.path() ?? sess?.port ?? '') : '';
    const running = isUart && uartBench.runs.get(ch)?.status === 'running';
    this.element.style.setProperty('--tag', sess?.color ?? (isUart ? 'var(--accent)' : 'var(--accent)'));
    this.element.dataset.panel = panel;
    this.element.title = [label, port].filter(Boolean).join(' · ');
    const portText = port.replace(/^\/dev\//, '');
    this.element.innerHTML = `<span class="${dotCls}"></span><span class="t-label"></span>${portText ? '<span class="t-port"></span>' : ''}${running ? '<span class="run-badge" title="매크로 실행 중"></span>' : ''}<button type="button" class="x" data-act="close-tab" aria-label="탭 닫기">${svgIcon('x')}</button>`;
    (this.element.querySelector('.t-label') as HTMLElement).textContent = label;
    const tp = this.element.querySelector('.t-port');
    if (tp) tp.textContent = portText;
    const x = this.element.querySelector('.x') as HTMLButtonElement;
    x.setAttribute('aria-label', `${label} 탭 닫기`);
    x.onpointerdown = (e) => e.stopPropagation();
    x.onclick = (e) => { e.stopPropagation(); this.p?.api.close(); };
  }
  dispose(): void {
    for (const o of this.offs) o();
    this.offs = [];
  }
}

/** pane number (REV6.1 .pane-no) before the tabs */
class PaneNumber implements IHeaderActionsRenderer {
  readonly element: HTMLElement;
  private offs: { dispose(): void }[] = [];
  constructor() {
    this.element = document.createElement('span');
    this.element.className = 'pane-no';
  }
  init(p: IGroupHeaderProps): void {
    const sync = () => {
      const n = paneNoOf(p.containerApi, p.group);
      this.element.textContent = String(n);
      this.element.title = `pane ${n}`;
      (p.group as DockviewGroupPanel).element.dataset.paneNo = String(n);
    };
    this.offs.push(p.containerApi.onDidAddGroup(sync), p.containerApi.onDidRemoveGroup(sync), p.containerApi.onDidLayoutChange(sync));
    sync();
  }
  dispose(): void {
    for (const o of this.offs) o.dispose();
  }
}

/** "+" after the tabs: UART groups open a saved session here (REV6.1 .tab-add); hidden elsewhere. */
class PaneAdd implements IHeaderActionsRenderer {
  readonly element: HTMLButtonElement;
  private offs: { dispose(): void }[] = [];
  constructor() {
    this.element = document.createElement('button');
    this.element.type = 'button';
    this.element.className = 'tab-add';
    this.element.innerHTML = svgIcon('plus');
  }
  init(p: IGroupHeaderProps): void {
    const sync = () => {
      const uart = p.group.panels.some((x) => String((x.params as { panel?: string } | undefined)?.panel ?? '').startsWith('uart.'));
      this.element.hidden = !uart;
      const n = paneNoOf(p.containerApi, p.group);
      this.element.title = '이 pane에 세션 열기';
      this.element.setAttribute('aria-label', `pane ${n}에 세션 열기`);
    };
    this.element.onclick = () => {
      p.group.api.setActive();
      this.element.dispatchEvent(new CustomEvent('loom:uart-add-tab', { bubbles: true, detail: { group: p.group.id } }));
    };
    this.offs.push(p.group.api.onDidActivePanelChange(sync), p.containerApi.onDidAddPanel(sync), p.containerApi.onDidRemovePanel(sync));
    sync();
  }
  dispose(): void {
    for (const o of this.offs) o.dispose();
    this.element.onclick = null;
  }
}

/** REV6.1 .pane-acts: split right · split down · maximize/restore · close pane (all real Dockview operations) */
class PaneActions implements IHeaderActionsRenderer {
  readonly element: HTMLElement;
  private offs: { dispose(): void }[] = [];
  constructor() {
    this.element = document.createElement('div');
    this.element.className = 'pane-acts';
  }
  init(p: IGroupHeaderProps): void {
    const mk = (act: string, ico: string, title: string, run: () => void, danger = false) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = `ibtn${danger ? ' danger' : ''}`;
      b.dataset.act = act;
      b.title = title;
      b.innerHTML = svgIcon(ico);
      b.onclick = (e) => { e.stopPropagation(); run(); };
      this.element.append(b);
      return b;
    };
    const n = () => paneNoOf(p.containerApi, p.group);
    mk('split-right', 'split-r', '오른쪽으로 분할 (Alt+Shift+=)', () => splitGroup(p.containerApi, p.group as DockviewGroupPanel, 'right'));
    mk('split-down', 'split-d', '아래로 분할 (Alt+Shift+-)', () => splitGroup(p.containerApi, p.group as DockviewGroupPanel, 'below'));
    const max = mk('max', 'max', '최대화 (Alt+Shift+Enter)', () => {
      if (p.api.isMaximized()) p.api.exitMaximized();
      else p.api.maximize();
      sync();
    });
    max.classList.add('max-btn');
    mk('close-pane', 'pane-x', 'pane 닫기 (Alt+Shift+W)', () => closeGroup(p.containerApi, p.group as DockviewGroupPanel), true);
    const sync = () => {
      const m = p.api.isMaximized();
      max.innerHTML = svgIcon(m ? 'restore' : 'max');
      max.setAttribute('aria-pressed', String(m));
      max.title = `${m ? '복원' : '최대화'} (Alt+Shift+Enter)`;
      max.setAttribute('aria-label', `pane ${n()} ${m ? '복원' : '최대화'}`);
      for (const b of this.element.querySelectorAll<HTMLButtonElement>('[data-act]')) if (b !== max) b.setAttribute('aria-label', `pane ${n()} ${b.title.split(' (')[0]}`);
    };
    this.offs.push(p.containerApi.onDidMaximizedGroupChange(sync), p.containerApi.onDidAddGroup(sync), p.containerApi.onDidRemoveGroup(sync));
    sync();
  }
  dispose(): void {
    for (const o of this.offs) o.dispose();
  }
}

/** Split: move the active tab into a new group on that side (a single-tab group splits by duplicating nothing —
 * it creates an empty pane like REV6.1, which the user fills with "+" or by dragging a tab). */
export function splitGroup(api: DockviewApi, group: DockviewGroupPanel, direction: 'right' | 'below'): void {
  const active = group.activePanel;
  if (active && group.panels.length > 1) {
    active.api.moveTo({ group, position: direction === 'right' ? 'right' : 'bottom' });
    return;
  }
  const g = api.addGroup({ referenceGroup: group, direction });
  g.api.setActive();
}
export function closeGroup(api: DockviewApi, group: DockviewGroupPanel): void {
  // closing a pane closes its views only — the device channel stays open (as before: closing a tab never closes a port)
  if (api.groups.length <= 1 && group.panels.length === 0) return;
  for (const panel of [...group.panels]) panel.api.close();
  if (api.groups.includes(group) && api.groups.length > 1) api.removeGroup(group);
}

export interface DockHostOptions {
  raf?: Raf;
  cancelRaf?: (id: number) => void;
}

export class DockHost implements DockController {
  readonly api: DockviewApi;
  private name = '';
  lifecycleManaged = false;
  private desired = new Set<string>();
  private readonly renderers = new Set<PanelRenderer>();
  private shown = true;
  /** Which slots this dock's workspace may hold (a module workspace: its service's domains). Used to scope panel
   * `ctx.ui.openPanel` calls that complete while this dock is hidden (e.g. a Connect resolved after the user switched
   * tabs): they land in THIS dock, never in whichever dock is in front, and never navigate. */
  accepts: ((slot: Slot) => boolean) | null = null;
  /** module docks: a panel's `openPanel` for an accepted slot opens in this dock instead of navigating */
  openInPlace = false;
  /** slots opened on request whose channel the shell does not know yet → not pruned until known or expired */
  private pending = new Map<string, number>();
  holdUntilKnown(slot: Slot, ms = 10_000): void { this.pending.set(slotKey(slot), Date.now() + ms); }
  private readonly scopedUi: ShellUiHooks;

  constructor(
    private readonly el: HTMLElement,
    private readonly services: ShellServices,
    o: DockHostOptions = {},
  ) {
    const raf = o.raf ?? ((cb: () => void) => requestAnimationFrame(cb));
    const cancelRaf = o.cancelRaf ?? ((id: number) => cancelAnimationFrame(id));
    const base = services.uiHooks;
    this.scopedUi = {
      ...base,
      openPanel: (id, props) => {
        const slot = { panel: id, props: { ...props } };
        // integrated/link docks in front: the normal deep-link behaviour (hash navigation)
        if (this.shown && !this.openInPlace) { base.openPanel(id, props); return; }
        if (this.accepts?.(slot) && (this.find(slot) || this.api.panels.length < MAX_PANELS)) {
          // a module's own Connect asking for its bound view: open it right here (front or hidden), and give the
          // daemon's channel state a moment to arrive before the projection would prune it as "unknown"
          this.holdUntilKnown(slot);
          this.open(slot, { activate: true });
        } else if (this.shown) base.openPanel(id, props); // another module's view: routed like any deep link
        else base.toast(`${id}: 다른 탭에서 끝난 작업의 보기는 열지 않았습니다 — 해당 탭에서 인터페이스 목록으로 여세요.`);
      },
    };
    this.api = createDockview(el, {
      theme: themeLight,
      disableFloatingGroups: true, // M2: one window; popouts/floating later
      createComponent: () => new PanelRenderer(services, raf, cancelRaf, this.renderers, this.shown, this.scopedUi),
      defaultTabComponent: 'loom-tab',
      createTabComponent: () => new PaneTab(services),
      createPrefixHeaderActionComponent: () => new PaneNumber(),
      createLeftHeaderActionComponent: () => new PaneAdd(),
      createRightHeaderActionComponent: () => new PaneActions(),
    });
  }

  private dims(): { w: number; h: number } {
    return { w: this.el.clientWidth || 1200, h: this.el.clientHeight || 700 };
  }

  /** Empty default workbench; no placeholder device targets or subscriptions. */
  clear(name = ''): void {
    this.name = name;
    this.api.clear();
  }

  /** replace the dock with a layout (vendor state when it matches, else rebuilt from root) */
  load(l: LoomLayout): LayoutIssue[] {
    const issues: LayoutIssue[] = [];
    this.name = l.name;
    const { w, h } = this.dims();
    const rebuilt = () => toDockview(l.root, w, h, (x: LayoutLeaf) => panelTitle(x));
    const useVendor = vendorMatches(l);
    if (l.vendor?.dockview && !useVendor) issues.push({ level: 'warn', text: `도킹 상태(${l.vendor.dockview.version})를 쓰지 않고 root로 다시 배치함` });
    this.api.clear();
    try {
      this.api.fromJSON((useVendor ? l.vendor?.dockview?.state : rebuilt()) as Parameters<DockviewApi['fromJSON']>[0]);
    } catch (e) {
      this.api.clear();
      if (useVendor) {
        issues.push({ level: 'warn', text: `도킹 상태 복원 실패 — root로 다시 배치 (${e instanceof Error ? e.message : String(e)})` });
        this.api.fromJSON(rebuilt() as Parameters<DockviewApi['fromJSON']>[0]);
      } else throw e;
    }
    this.api.layout(w, h, true);
    return issues;
  }

  /** current arrangement as an app layout (vendor state as a sub-field) */
  snapshot(name = this.name): LoomLayout {
    const state = this.api.toJSON();
    const root = fromDockview(state) ?? { split: 'v' as const, children: [] };
    return { loom_layout: 1, name, env_hint: this.services.shell.env, root, pinned: [], vendor: { dockview: { version: DOCKVIEW_VERSION, state } } };
  }

  find(slot: Slot): IDockviewPanel | null {
    for (const p of this.api.panels) {
      const prm = p.params as { panel?: string; props?: Props } | undefined;
      if (prm?.panel && sameSlot({ panel: prm.panel, props: prm.props ?? {} }, slot)) return p;
    }
    return null;
  }

  open(slot: Slot, options: { activate?: boolean } = {}): 'shown' | 'added' {
    const have = this.find(slot);
    if (have) {
      if (options.activate !== false) have.api.setActive();
      return 'shown';
    }
    // Background arrivals do not activate a group. Reuse the first group when
    // none is active, so a snapshot of five devices does not create five columns.
    const domain = slot.panel.split('.')[0];
    const near = this.api.panels.find((p) => String((p.params as { panel?: string } | undefined)?.panel ?? '').startsWith(`${domain}.`));
    const group = near?.group ?? this.api.activeGroup ?? this.api.groups[0];
    const stableId = `slot:${slotKey(slot)}`;
    let id = stableId;
    let suffix = 1;
    while (this.api.getPanel(id)) id = `${stableId}:${suffix++}`;
    this.api.addPanel({
      id,
      component: PANEL_COMPONENT,
      inactive: options.activate === false,
      title: panelTitle(slot),
      params: { panel: slot.panel, props: { ...slot.props } },
      ...(group ? { position: { referenceGroup: group, direction: 'within' as const } } : {}),
    });
    return 'added';
  }

  /** Incremental updates retain surviving panel instances, input and group geometry. */
  reconcile(slots: Slot[], channels: Iterable<WorkspaceChannel>, hasTestSession: boolean, byDevice = false, autoAdd = true): void {
    const known = [...channels];
    const active = this.api.activePanel;
    for (const panel of [...this.api.panels]) {
      const params = panel.params as { panel: string; props: Props };
      const slot = { panel: params.panel, props: params.props ?? {} };
      const next = availableSlot(slot, known, hasTestSession);
      const k = slotKey(slot);
      const until = this.pending.get(k);
      if (until !== undefined) {
        const dev = deviceKey(slot);
        if (next || known.some((c) => `${c.domain}/${c.channel}` === dev) || Date.now() >= until) this.pending.delete(k);
        else continue; // requested view, channel state not here yet
      }
      if (!next || !sameSlot(next, slot)) this.api.removePanel(panel);
    }
    const nextDesired = new Set(slots.map(slotKey));
    for (const slot of slots) {
      if (!autoAdd) continue;
      if (this.desired.has(slotKey(slot))) continue;
      if (byDevice && this.api.panels.some((p) => {
        const prm = p.params as { panel: string; props: Props };
        return deviceKey(prm) === deviceKey(slot);
      })) continue;
      if (this.api.panels.length >= MAX_PANELS) break;
      this.open(slot, { activate: false });
    }
    // Remember arrivals while auto-open is off so a later heartbeat/settings change
    // does not retroactively open channels the user chose to leave in the explorer.
    this.desired = nextDesired;
    if (active && this.api.getPanel(active.id) === active && this.api.activePanel !== active) active.api.setActive();
  }

  /** Start tracking the slots loaded on this route, including an empty projection. */
  trackDesired(slots: Slot[]): void { this.desired = new Set(slots.map(slotKey)); }

  layout(): void {
    if (!this.shown) return; // hidden dock: sized again when shown
    const { w, h } = this.dims();
    this.api.layout(w, h);
  }

  /** Show/hide this whole dock (per-module workspaces stay mounted while another tab is in front). */
  setShown(shown: boolean): void {
    if (this.shown === shown) return;
    this.shown = shown;
    for (const r of this.renderers) r.setShown(shown);
    if (shown) this.layout();
  }
  get isShown(): boolean { return this.shown; }

  dispose(): void {
    this.api.dispose();
  }
}
