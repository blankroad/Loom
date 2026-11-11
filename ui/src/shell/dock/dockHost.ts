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

import { createDockview, themeAbyss, type DockviewApi, type GroupPanelPartInitParameters, type IContentRenderer, type IDockviewPanel, type IHeaderActionsRenderer, type IGroupHeaderProps } from 'dockview-core';
import { createRoot, type Root } from 'react-dom/client';
import './dockview.css';
import { createElement } from 'react';
import { createPanelContext, type PanelContextHandle } from '../../sdk/context';
import type { Props } from '../../sdk/manifest';
import { validateProps } from '../../sdk/propsSchema';
import { getPanel, type PanelInstance } from '../../sdk/registry';
import { sameSlot, slotKey, type Slot } from '../routes';
import type { ShellServices } from '../services';
import { ShellCtx } from '../ShellContext';
import { TestScreen } from '../test/TestScreen';
import {
  DOCKVIEW_VERSION,
  fromDockview,
  PANEL_COMPONENT,
  toDockview,
  vendorMatches,
  type LayoutIssue,
  type LayoutLeaf,
  type LoomLayout,
} from './layoutModel';

export interface DockController {
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

  constructor(
    private readonly services: ShellServices,
    private readonly raf: Raf,
    private readonly cancelRaf: (id: number) => void,
  ) {
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
        ui: s.uiHooks,
        metrics: s.metrics,
        now: s.now,
      },
      `${panel}#d${++instanceSeq}`,
    );
    this.inst = reg.factory(this.handle.ctx);
    this.inst.mount(this.body);
    this.handle.setVisible(p.api.isVisible);
    this.offVis = p.api.onDidVisibilityChange((e) => this.handle?.setVisible(e.isVisible));
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

/** [최대화]/[복원] in each group header */
class MaximizeAction implements IHeaderActionsRenderer {
  readonly element: HTMLButtonElement;
  private offs: { dispose(): void }[] = [];
  constructor() {
    this.element = document.createElement('button');
    this.element.type = 'button';
    this.element.className = 'max-btn';
  }
  init(p: IGroupHeaderProps): void {
    const sync = () => {
      const max = p.api.isMaximized();
      this.element.textContent = max ? '복원' : '최대화';
      this.element.setAttribute('aria-pressed', String(max));
    };
    this.element.onclick = () => {
      if (p.api.isMaximized()) p.api.exitMaximized();
      else p.api.maximize();
      sync();
    };
    this.offs.push(p.containerApi.onDidMaximizedGroupChange(sync));
    sync();
  }
  dispose(): void {
    for (const o of this.offs) o.dispose();
    this.element.onclick = null;
  }
}

export interface DockHostOptions {
  raf?: Raf;
  cancelRaf?: (id: number) => void;
}

export class DockHost implements DockController {
  readonly api: DockviewApi;
  private name = '';

  constructor(
    private readonly el: HTMLElement,
    private readonly services: ShellServices,
    o: DockHostOptions = {},
  ) {
    const raf = o.raf ?? ((cb: () => void) => requestAnimationFrame(cb));
    const cancelRaf = o.cancelRaf ?? ((id: number) => cancelAnimationFrame(id));
    this.api = createDockview(el, {
      theme: themeAbyss,
      disableFloatingGroups: true, // M2: one window; popouts/floating later
      createComponent: () => new PanelRenderer(services, raf, cancelRaf),
      createRightHeaderActionComponent: () => new MaximizeAction(),
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
    const root = fromDockview(state) ?? { panel: 'demo.echo', props: {} };
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
    // next to a panel of the same domain if there is one, else in the active group
    const domain = slot.panel.split('.')[0];
    const near = this.api.panels.find((p) => String((p.params as { panel?: string } | undefined)?.panel ?? '').startsWith(`${domain}.`));
    const group = near?.group ?? this.api.activeGroup;
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

  layout(): void {
    const { w, h } = this.dims();
    this.api.layout(w, h);
  }

  dispose(): void {
    this.api.dispose();
  }
}
