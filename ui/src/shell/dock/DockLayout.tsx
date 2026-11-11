// React wrapper for the dock (one DockHost per tab, created once) + the layout bar in the
// shell's sub bar. Routes (plan §4.1):
//   #/l/<name>  → saved layout <name>, else preset <name>, else the default preset
//   #/p/<d>/<p> → a dock with just that panel (deep link; replaces the dock, so 100 deep-link
//                 hops never accumulate panels — UI-T10)
// Layout warnings (unknown panels, dropped props, vendor state not reused) become one toast.

import { useEffect, useRef, useState } from 'react';
import { useStoreVersion } from '../../store/hooks';
import { PRESETS, type Route } from '../routes';
import { useServices } from '../ShellContext';
import { DockHost } from './dockHost';
import { presetToLayout, slotLayout, type LayoutIssue, type LoomLayout } from './layoutModel';

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
  const key = routeKey(route);

  // one dockview instance for the lifetime of the shell
  useEffect(() => {
    if (!el.current) return;
    const h = new DockHost(el.current, services);
    host.current = h;
    const detach = services.attachDock(h);
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => h.layout()) : null;
    ro?.observe(el.current);
    return () => {
      ro?.disconnect();
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
    if (route.kind === 'panel') layout = slotLayout(route.slot);
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

  return <main ref={el} className="dock-host" data-testid="layout" data-layout={route.kind === 'layout' ? route.name : ''} />;
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
