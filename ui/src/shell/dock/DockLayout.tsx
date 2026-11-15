// React wrapper for the docks + the layout bar in the shell's sub bar. Routes (plan §4.1):
//   #/l/<name>  → the INTEGRATED workspace: saved layout <name>, else preset <name>, else the default preset
//   #/s/<svc>   → that service's MODULE workspace (one of seven permanent tabs): its open channels plus, while the
//                 service has no open channel, the unbound module application (eth.packets {}, uds.console {}, …)
//                 with its own Connect/configuration. UART's application is its module chrome (sessions, macros,
//                 panes); an empty UART workspace shows the saved-session pane picker. Navigation never opens a device.
//   #/p/<d>/<p> → deep link. Inside a module workspace whose module covers <d> (an agent focusing one session from
//                 #/s/<svc>), it is a live FOCUS: the existing tab is activated, or the panel is added as one tab —
//                 panes, tab order and surviving instances are kept. Added tabs count against MAX_PANELS: at the
//                 cap the addition is refused with a toast. Otherwise (from the integrated workspace or cross-module)
//                 the deep link opens as a single-target view in a separate LINK dock, so neither the integrated
//                 nor any module workspace is replaced; 100 deep-link hops never accumulate (UI-T10).
// Every workspace (integrated, each module, link) is its own DockHost, created on first visit and RETAINED while
// another tab is in front: switching tabs keeps panel instances, drafts, REC, running macros and split geometry.
// A hidden dock keeps syncing with the channels (closed views pruned, arrivals added) but does not draw.
// Selecting a different integrated layout (#/l/<other>) (re)applies it; returning to the same one resumes it.
// Layout warnings (unknown panels, dropped props, vendor state not reused) become one toast.

import { useEffect, useRef, useState } from 'react';
import { useStoreVersion } from '../../store/hooks';
import { activePresets, isUnboundModuleSlot, presetForChannels, serviceAllowsDomain, servicePresetForChannels, PRESETS, SERVICE_LABELS, unboundModuleSlot, type Route, type ServiceDomain, type Slot } from '../routes';
import { useServices } from '../ShellContext';
import type { ShellServices } from '../services';
import { UartEmptyPane } from '../../workbench/uart/UartWorkbench';
import { DockHost } from './dockHost';
import { activeLayout } from './activeWorkspace';
import { leaves, MAX_PANELS, presetToLayout, slotLayout, type LayoutIssue, type LoomLayout } from './layoutModel';

const PRESET_NAMES: ReadonlySet<string> = new Set(PRESETS.map((p) => p.name));

export function routeKey(r: Route): string {
  if (r.kind === 'panel') return `p:${r.slot.panel}?${JSON.stringify(r.slot.props)}`;
  if (r.kind === 'layout') return r.service ? `s:${r.service}` : `l:${r.name}`;
  return 'none';
}

type WorkspaceKey = 'integrated' | 'link' | ServiceDomain;
const isModuleKey = (k: WorkspaceKey): k is ServiceDomain => k !== 'integrated' && k !== 'link';

interface Workspace {
  key: WorkspaceKey;
  el: HTMLElement;
  host: DockHost;
  /** integrated: the loaded layout/preset name; null until first load */
  name: string | null;
  source: LoomLayout | null;
  warnedLimit: boolean;
  /** module: the projection currently asks for the unbound application */
  seedWant: boolean;
  /** module: the unbound application in the dock was seeded by the shell (handed off to the first bound panel) */
  seedManaged: boolean;
  /** a load is mounting panels (which emit shell freshness synchronously): no reconcile until it is tracked */
  loading: boolean;
  /** an in-module deep link (live focus) is showing: hold the projection (a history link to an unknown/closed
   * channel stays visible) until the service route resumes, as with the single dock before */
  focusHold: boolean;
  sync: () => void;
  dispose: () => void;
}

const panelSlot = (p: { params?: unknown }): Slot => {
  const prm = (p.params ?? {}) as { panel?: string; props?: Slot['props'] };
  return { panel: prm.panel ?? '', props: prm.props ?? {} };
};

function warnIssues(services: ShellServices, issues: LayoutIssue[]): void {
  const warn = issues.filter((i) => i.level === 'warn' || i.level === 'error');
  if (warn.length) services.toasts.push(`레이아웃: ${warn.map((i) => i.text).slice(0, 3).join(' · ')}${warn.length > 3 ? ` 외 ${warn.length - 3}건` : ''}`, { ttlMs: 8_000 });
}

function createWorkspace(key: WorkspaceKey, parent: HTMLElement, services: ShellServices, onPanels: () => void): Workspace {
  const el = document.createElement('main');
  el.className = 'dock-host';
  el.hidden = true;
  el.dataset.workspace = key;
  parent.append(el);
  const host = new DockHost(el, services);
  host.lifecycleManaged = true;
  host.setShown(false);
  // a hidden module dock keeps late panel openPanel calls to its own domains (UDS: uds + CAN/Ethernet observations)
  host.accepts = isModuleKey(key) ? (slot) => serviceAllowsDomain(key, slot.panel.split('.')[0] ?? '') : key === 'integrated' ? () => true : null;
  host.openInPlace = isModuleKey(key);
  const hasTest = () => services.testSession.sessionId !== null;
  const ws: Workspace = { key, el, host, name: null, source: null, warnedLimit: false, seedWant: false, seedManaged: false, loading: false, focusHold: false, sync: () => {}, dispose: () => {} };
  const ownPanel = (service: ServiceDomain) => host.api.panels.find((p) => { const slot = panelSlot(p); return slot.panel.split('.')[0] === service && !isUnboundModuleSlot(slot); });
  // the user closing the seeded application is a choice: it is not re-seeded until the projection asks again
  const removed = host.api.onDidRemovePanel((p) => {
    const seed = isModuleKey(key) ? unboundModuleSlot(key) : null;
    if (seed && isUnboundModuleSlot(panelSlot(p)) && panelSlot(p).panel === seed.panel && !host.find(seed)) ws.seedManaged = false;
    onPanels();
  });
  const added = host.api.onDidAddPanel(() => onPanels());
  let syncing = false;
  ws.sync = () => {
    // Mount/dispose registers freshness with the shell, which emits synchronously.
    if (syncing || ws.loading || ws.focusHold || ws.name === null || key === 'link') return;
    syncing = true;
    try {
      const channels = [...services.shell.channels.values()];
      const autoAdd = services.settings.autoOpenPanels === 'on';
      if (isModuleKey(key)) {
        const preset = servicePresetForChannels(key, channels);
        const bound = preset.slots.filter((slot) => !isUnboundModuleSlot(slot));
        // byDevice: a channel already shown in some view of this module (e.g. CAN Trace vs Rx/Tx chosen in the panel)
        // is not auto-added a second time
        host.reconcile(bound.slice(0, MAX_PANELS), channels, hasTest(), true, autoAdd);
        const seed = unboundModuleSlot(key);
        const want = preset.slots.some(isUnboundModuleSlot);
        if (seed && want && !ws.seedWant && !host.find(seed) && host.api.panels.length < MAX_PANELS) {
          host.open(seed, { activate: host.api.panels.length === 0 });
          ws.seedManaged = true;
        }
        if (seed && !want && ws.seedManaged) {
          // hand off: the module's first connected interface replaces the shell-seeded application in its pane
          const seeded = host.find(seed);
          const own = ownPanel(key);
          if (seeded && own) {
            const wasActive = host.api.activePanel === seeded;
            if (own.group !== seeded.group) own.api.moveTo({ group: seeded.group, position: 'center' });
            host.api.removePanel(seeded);
            if (wasActive) own.api.setActive();
            ws.seedManaged = false;
          } else if (!seeded) ws.seedManaged = false;
        }
        ws.seedWant = want;
        return;
      }
      const next = projectIntegrated(ws, services, hasTest());
      host.reconcile(next ? leaves(next.root) : [], channels, hasTest() || ws.name === 'test', ws.name === 'm1-default', autoAdd);
    } finally { syncing = false; }
  };
  const offShell = services.shell.subscribe(() => ws.sync());
  const offTest = services.testSession.subscribe(() => ws.sync());
  ws.dispose = () => {
    offShell(); offTest(); removed.dispose(); added.dispose();
    host.dispose(); // disposes every panel (subscriptions released)
    el.remove();
  };
  return ws;
}

function projectIntegrated(ws: Workspace, services: ShellServices, hasTest: boolean): LoomLayout | null {
  const name = ws.name ?? 'm1-default';
  if (ws.source) return activeLayout(ws.source, services.shell.channels.values(), hasTest);
  // The explicit test route may open the test screen before a session exists.
  const preset = presetForChannels(name, services.shell.channels.values(), hasTest, name === 'test');
  if (!preset.slots.length) return null;
  if (preset.slots.length > MAX_PANELS && !ws.warnedLimit) {
    ws.warnedLimit = true;
    services.toasts.push(`기본 작업 공간: ${MAX_PANELS}개 보기만 열었습니다. 나머지 채널은 인터페이스 탐색기에서 선택하세요.`, { key: 'workspace-limit', ttlMs: 8_000 });
  }
  const bounded = { ...preset, slots: preset.slots.slice(0, MAX_PANELS) };
  const layout = presetToLayout(bounded);
  if (name === 'm1-default') layout.root = { tabs: bounded.slots, active: 0 };
  return layout;
}

function loadIntegrated(ws: Workspace, services: ShellServices, name: string): void {
  const issues: LayoutIssue[] = [];
  ws.name = name;
  ws.source = null;
  ws.warnedLimit = false;
  if (!PRESET_NAMES.has(name) && services.layouts.list().includes(name)) {
    const stored = services.layouts.get(name);
    issues.push(...stored.issues);
    ws.source = stored.layout;
  }
  const initial = projectIntegrated(ws, services, services.testSession.sessionId !== null);
  ws.loading = true;
  try {
    if (initial) issues.push(...ws.host.load(initial));
    else ws.host.clear(name);
    ws.host.trackDesired(initial ? leaves(initial.root) : []);
  } finally { ws.loading = false; }
  ws.sync();
  warnIssues(services, issues);
}

function loadModule(ws: Workspace & { key: ServiceDomain }, services: ShellServices): void {
  const issues: LayoutIssue[] = [];
  const preset = servicePresetForChannels(ws.key, services.shell.channels.values());
  if (preset.slots.length > MAX_PANELS) services.toasts.push(`${SERVICE_LABELS[ws.key]}: ${MAX_PANELS}개 보기만 열었습니다. 나머지 채널은 인터페이스 탐색기에서 선택하세요.`, { key: 'workspace-limit', ttlMs: 8_000 });
  const bounded = { ...preset, slots: preset.slots.slice(0, MAX_PANELS) };
  ws.name = preset.name;
  ws.loading = true;
  try {
    if (bounded.slots.length) {
      const layout = presetToLayout(bounded);
      layout.root = { tabs: bounded.slots, active: 0 };
      issues.push(...ws.host.load(layout));
    } else ws.host.clear(preset.name);
    ws.host.trackDesired(bounded.slots.filter((slot) => !isUnboundModuleSlot(slot)));
  } finally { ws.loading = false; }
  ws.seedWant = bounded.slots.some(isUnboundModuleSlot);
  ws.seedManaged = ws.seedWant;
  ws.sync();
  warnIssues(services, issues);
}

export function DockLayout({ route }: { route: Route }) {
  const services = useServices();
  const stack = useRef<HTMLDivElement>(null);
  const spaces = useRef(new Map<WorkspaceKey, Workspace>());
  const visible = useRef<Workspace | null>(null);
  const detachDock = useRef<(() => void) | null>(null);
  // what the front dock shows (mirrored into state: refs are not read during render)
  const [front, setFront] = useState<{ key: WorkspaceKey | null; panels: number }>({ key: null, panels: 0 });
  const key = routeKey(route);
  const refresh = () => setFront({ key: visible.current?.key ?? null, panels: visible.current?.host.api.panels.length ?? 0 });

  // the docks live as long as the shell; window/page resizes re-layout only the visible one
  useEffect(() => {
    const map = spaces.current;
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => visible.current?.host.layout()) : null;
    if (stack.current) ro?.observe(stack.current);
    // Belt and braces: a hidden/background page gets no rendering steps (no ResizeObserver callbacks), so a banner
    // inserted or a window resized while hidden must re-layout when the page is shown/resized again.
    const relayout = () => visible.current?.host.layout();
    const onVis = () => { if (document.visibilityState === 'visible') relayout(); };
    window.addEventListener('resize', relayout);
    document.addEventListener('visibilitychange', onVis);
    return () => {
      window.removeEventListener('resize', relayout);
      document.removeEventListener('visibilitychange', onVis);
      ro?.disconnect();
      detachDock.current?.();
      detachDock.current = null;
      for (const ws of map.values()) ws.dispose();
      map.clear();
      visible.current = null;
    };
  }, [services]);

  // select (and on first visit create/load) the route's workspace
  useEffect(() => {
    const parent = stack.current;
    if (!parent || route.kind === 'need-auth') return;
    const map = spaces.current;
    const obtain = (k: WorkspaceKey): Workspace => {
      let ws = map.get(k);
      if (!ws) { ws = createWorkspace(k, parent, services, refresh); map.set(k, ws); }
      return ws;
    };
    const show = (ws: Workspace) => {
      const prev = visible.current;
      if (prev !== ws) {
        for (const other of map.values()) {
          if (other === ws) continue;
          // leaving a held live focus for another tab: that workspace resumes projecting while hidden
          if (other.focusHold) { other.focusHold = false; other.sync(); }
          other.el.hidden = true;
          other.el.setAttribute('data-testid', 'layout-retained');
          other.host.setShown(false);
        }
        ws.el.hidden = false;
        ws.host.setShown(true);
        visible.current = ws;
        detachDock.current?.();
        detachDock.current = services.attachDock(ws.host);
        services.uiState.noteDockChange();
      }
      ws.el.setAttribute('data-testid', 'layout');
      ws.el.dataset.layout = route.kind === 'layout' ? route.name : '';
      if (route.kind === 'layout' && route.service) ws.el.dataset.service = route.service;
      else delete ws.el.dataset.service;
      ws.host.layout();
      refresh();
    };
    if (route.kind === 'panel') {
      const cur = visible.current;
      const domain = route.slot.panel.split('.')[0] ?? '';
      if (cur && isModuleKey(cur.key) && serviceAllowsDomain(cur.key, domain)) {
        // live focus: activate the tab or add one; no reload, no remount
        const h = cur.host;
        cur.focusHold = true;
        if (h.find(route.slot) || h.api.panels.length < MAX_PANELS) h.open(route.slot);
        // at the cap the user's workspace and instances stay; the addition is refused, not a replacement
        else services.toasts.push(`보기가 ${MAX_PANELS}개로 가득 차 ${route.slot.panel} · ${String(route.slot.props.channel ?? route.slot.props.targets ?? '')}을(를) 열지 않았습니다. 기존 탭을 닫은 뒤 다시 여세요.`, { key: 'deeplink-limit', ttlMs: 8_000 });
        show(cur);
        return;
      }
      const link = obtain('link');
      show(link);
      // Explicit history/diagnostic links remain single-target views.
      warnIssues(services, link.host.load(slotLayout(route.slot)));
      return;
    }
    const ws = obtain(route.service ?? 'integrated');
    ws.focusHold = false;
    show(ws);
    if (route.service) {
      if (ws.name === null) loadModule(ws as Workspace & { key: ServiceDomain }, services);
      else ws.sync(); // retained module workspace: resume (channels may have changed while it was hidden)
    } else if (ws.name !== route.name) loadIntegrated(ws, services, route.name);
    else ws.sync();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload only when the route itself changes
  }, [key, services]);

  const empty = front.key !== null && front.panels === 0;
  const service = route.kind === 'layout' ? route.service : undefined;
  const reopen = () => {
    const ws = visible.current;
    if (!ws || !isModuleKey(ws.key)) return;
    const seed = unboundModuleSlot(ws.key);
    if (!seed) return;
    ws.host.open(seed);
    ws.seedManaged = true;
  };
  return <div className="dock-container">
    <div ref={stack} className="dock-stack" />
    {empty && <div className={`dock-empty${service ? ' dock-empty-module' : ''}`} data-testid="dock-empty" data-service={service}>
      {service === 'uart' ? <UartEmptyPane />
        : service ? <><span aria-hidden="true">▤</span><h2>{SERVICE_LABELS[service]} 작업대가 닫혀 있습니다</h2><p>탭만 닫혔습니다. 연결과 장치 상태는 바뀌지 않았습니다.</p><button type="button" className="dock-empty-action" data-act="reopen-module" onClick={reopen}>{SERVICE_LABELS[service]} 작업대 다시 열기</button></>
          : <><span aria-hidden="true">▤</span><h2>관찰할 인터페이스를 선택하세요</h2><p>왼쪽에서 채널과 보기를 선택하면 이곳에 열립니다.</p><p>여러 탭을 열고 끌어서 나란히 배치할 수 있습니다.</p></>}
    </div>}
  </div>;
}

/** layout picker + save (sub bar) */
export function LayoutBar({ route }: { route: Route }) {
  const { layouts, dock, toasts, uiState, shell, testSession } = useServices();
  useStoreVersion(uiState);
  useStoreVersion(shell);
  useStoreVersion(testSession);
  const eligible = activePresets(shell.channels.values(), testSession.sessionId !== null);
  const [saved, setSaved] = useState<string[]>(() => layouts.list());
  const [name, setName] = useState('');
  const current = route.kind === 'layout' ? route.name : '';
  const save = () => {
    const h = dock.current as DockHost | null;
    if (!h || !('snapshot' in h)) return;
    if (!h.api.panels.length) { toasts.push('먼저 인터페이스 보기를 연 뒤 레이아웃을 저장하세요.'); return; }
    const want = name.trim() || current || 'layout';
    const r = layouts.save(h.snapshot(want), PRESET_NAMES);
    if (r.ok) {
      setSaved(layouts.list());
      setName('');
      toasts.push(`레이아웃 저장: ${r.name}${r.name !== want ? ` ("${want}"은 이미 있어 새 이름)` : ''} — #/l/${r.name}`);
    } else toasts.push(`레이아웃 저장 실패: ${r.error}`);
  };
  return (
    <span className="layout-bar">
      <label className="preset-picker">
        레이아웃{' '}
        <select
          aria-label="레이아웃"
          value={route.kind === 'layout' && (eligible.some((p) => p.name === current) || saved.includes(current)) ? current : ''}
          onChange={(e) => {
            location.hash = `#/l/${e.target.value}`;
          }}
        >
          {(route.kind !== 'layout' || (!eligible.some((p) => p.name === current) && !saved.includes(current))) && <option value="">{route.kind === 'panel' ? '(딥링크)' : ([...shell.channels.values()].some((c) => c.state === 'open') ? '(현재 연결에 맞춰 표시)' : '(연결된 인터페이스 없음)')}</option>}
          <optgroup label="프리셋">
            {eligible.map((p) => (
              <option key={p.name} value={p.name}>
                {p.label}
              </option>
            ))}
          </optgroup>
          {saved.length > 0 && (
            <optgroup label="저장됨">
              {saved.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </optgroup>
          )}
        </select>
      </label>{' '}
      <input aria-label="저장할 레이아웃 이름" placeholder="이름" value={name} size={10} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && save()} />
      <button type="button" onClick={save}>
        저장
      </button>
      {saved.includes(current) && (
        <button
          type="button"
          onClick={() => {
            layouts.remove(current);
            setSaved(layouts.list());
            toasts.push(`레이아웃 삭제: ${current}`);
          }}
        >
          삭제
        </button>
      )}
    </span>
  );
}
