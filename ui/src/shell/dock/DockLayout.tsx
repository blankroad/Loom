// React wrapper for the dock (one DockHost per tab, created once) + the layout bar in the
// shell's sub bar. Routes (plan §4.1):
//   #/l/<name>  → saved layout <name>, else preset <name>, else the default preset
//   #/p/<d>/<p> → a dock with just that panel (deep link; replaces the dock, so 100 deep-link
//                 hops never accumulate panels — UI-T10)
// Layout warnings (unknown panels, dropped props, vendor state not reused) become one toast.

import { useEffect, useRef, useState } from 'react';
import { defaultPanelFor } from '../../store/channels';
import { useStoreVersion } from '../../store/hooks';
import { PRESETS, type Route } from '../routes';
import { useServices } from '../ShellContext';
import { DockHost } from './dockHost';
import { MAX_PANELS, presetToLayout, slotLayout, type LayoutIssue, type LoomLayout } from './layoutModel';

const PRESET_NAMES: ReadonlySet<string> = new Set(PRESETS.map((p) => p.name));

export function routeKey(r: Route): string {
  if (r.kind === 'panel') return `p:${r.slot.panel}?${JSON.stringify(r.slot.props)}`;
  if (r.kind === 'layout') return `l:${r.name}`;
  return 'none';
}

export function DockLayout({ route }: { route: Route }) {
  const services = useServices();
  const el = useRef<HTMLDivElement>(null);
  const host = useRef<DockHost | null>(null);
  const [panelCount, setPanelCount] = useState(0);
  const key = routeKey(route);

  // one dockview instance for the lifetime of the shell
  useEffect(() => {
    if (!el.current) return;
    const h = new DockHost(el.current, services);
    host.current = h;
    const added = h.api.onDidAddPanel(() => setPanelCount(h.api.panels.length));
    const removed = h.api.onDidRemovePanel(() => setPanelCount(h.api.panels.length));
    const detach = services.attachDock(h);
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => h.layout()) : null;
    ro?.observe(el.current);
    return () => {
      ro?.disconnect();
      added.dispose();
      removed.dispose();
      detach();
      h.dispose(); // disposes every panel (subscriptions released)
      host.current = null;
    };
  }, [services]);

  // (re)load on route change
  useEffect(() => {
    const h = host.current;
    if (!h || route.kind === 'need-auth') return;
    let layout: LoomLayout;
    const issues: LayoutIssue[] = [];
    if (route.kind === 'layout' && route.name === 'm1-default') {
      // Initialize once per route entry, never on channel/store ticks. Newly opened
      // interfaces are added by services.showSlot, preserving the user's open tabs.
      const slots = [...services.shell.channels.values()].flatMap((c) => {
        const slot = defaultPanelFor(c.domain, c.channel);
        return slot ? [slot] : [];
      });
      if (!slots.length) { h.clear('m1-default'); return; }
      if (slots.length > MAX_PANELS) services.toasts.push(`기본 작업 공간: ${MAX_PANELS}개 보기만 열었습니다. 나머지 채널은 인터페이스 탐색기에서 선택하세요.`, { ttlMs: 8_000 });
      layout = { loom_layout: 1, name: 'm1-default', env_hint: services.shell.env, pinned: [], root: { tabs: slots.slice(0, MAX_PANELS), active: 0 } };
    } else if (route.kind === 'panel') layout = slotLayout(route.slot);
    else if (!PRESET_NAMES.has(route.name) && services.layouts.list().includes(route.name)) {
      const r = services.layouts.get(route.name);
      issues.push(...r.issues);
      layout = r.layout ?? presetToLayout(route.preset);
    } else layout = presetToLayout(route.preset);
    issues.push(...h.load(layout));
    const warn = issues.filter((i) => i.level === 'warn' || i.level === 'error');
    if (warn.length) services.toasts.push(`레이아웃: ${warn.map((i) => i.text).slice(0, 3).join(' · ')}${warn.length > 3 ? ` 외 ${warn.length - 3}건` : ''}`, { ttlMs: 8_000 });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload only when the route itself changes
  }, [key, services]);

  return <div className="dock-container">
    <main ref={el} className="dock-host" data-testid="layout" data-layout={route.kind === 'layout' ? route.name : ''} />
    {panelCount === 0 && <div className="dock-empty" data-testid="dock-empty"><span aria-hidden="true">▤</span><h2>관찰할 인터페이스를 선택하세요</h2><p>왼쪽에서 채널과 보기를 선택하면 이곳에 열립니다.</p><p>여러 탭을 열고 끌어서 나란히 배치할 수 있습니다.</p></div>}
  </div>;
}

/** layout picker + save (sub bar) */
export function LayoutBar({ route }: { route: Route }) {
  const { layouts, dock, toasts, uiState } = useServices();
  useStoreVersion(uiState);
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
          value={route.kind === 'layout' ? current : ''}
          onChange={(e) => {
            location.hash = `#/l/${e.target.value}`;
          }}
        >
          {route.kind !== 'layout' && <option value="">(딥링크)</option>}
          <optgroup label="프리셋">
            {PRESETS.map((p) => (
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
