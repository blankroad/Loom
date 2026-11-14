// Live projection only. Never mutate the saved source or its vendor serialization.
import { EXPLORER_DOMAINS } from '../../store/channels';
import type { Slot, WorkspaceChannel } from '../routes';
import { isSplit, isTabs, type LayoutNode, type LoomLayout } from './layoutModel';

/** Domains whose closed channels keep queryable history (ETH capture stop retains packets for query/show).
 * Their already-open or explicitly reopened views survive a normal close; presets never auto-open them. */
const HISTORY_AFTER_CLOSE: ReadonlySet<string> = new Set(['eth']);
export const retainsHistoryAfterClose = (domain: string): boolean => HISTORY_AFTER_CLOSE.has(domain);

export function availableSlot(slot: Slot, channels: Iterable<WorkspaceChannel>, hasTestSession = false): Slot | null {
  if (slot.panel === 'shell.test') return hasTestSession ? slot : null;
  const domain = slot.panel.split('.')[0] ?? '';
  if (!(EXPLORER_DOMAINS as readonly string[]).includes(domain)) return slot;
  const keep = (c: WorkspaceChannel) => c.state === 'open' || (c.state === 'closed' && HISTORY_AFTER_CLOSE.has(domain));
  const active = new Set([...channels].filter((c) => c.domain === domain && keep(c)).map((c) => c.channel));
  if (domain === 'uds') {
    const targets = String(slot.props.targets ?? '').split(',').map((s) => s.trim()).filter((s) => active.has(s));
    return targets.length ? { ...slot, props: { ...slot.props, targets: targets.join(',') } } : null;
  }
  const channel = slot.props.channel ?? (domain === 'can' ? slot.props.ch : undefined);
  return typeof channel === 'string' && active.has(channel) ? slot : null;
}

/** Collapse empty groups/splits while retaining surviving tab selection and relative sizes. */
export function activeLayout(layout: LoomLayout, channels: Iterable<WorkspaceChannel>, hasTestSession = false): LoomLayout | null {
  const known = [...channels];
  const walk = (node: LayoutNode): LayoutNode | null => {
    if (isSplit(node)) {
      const children: LayoutNode[] = [];
      const sizes: number[] = [];
      node.children.forEach((child, i) => {
        const next = walk(child);
        if (next) { children.push(next); sizes.push(node.sizes?.[i] ?? 1); }
      });
      return children.length === 0 ? null : children.length === 1 ? children[0]! : { ...node, children, ...(node.sizes ? { sizes } : {}) };
    }
    if (isTabs(node)) {
      const tabs = node.tabs.flatMap((t) => { const slot = availableSlot(t, known, hasTestSession); return slot ? [slot] : []; });
      const selected = node.tabs[node.active ?? 0];
      const projected = selected && availableSlot(selected, known, hasTestSession);
      const active = projected ? tabs.findIndex((t) => t === projected || (t.panel === projected.panel && JSON.stringify(t.props) === JSON.stringify(projected.props))) : 0;
      return tabs.length === 0 ? null : tabs.length === 1 ? tabs[0]! : { tabs, active: Math.max(0, active) };
    }
    return availableSlot(node, known, hasTestSession);
  };
  const root = walk(layout.root);
  if (!root) return null;
  // Keep vendor geometry only when no leaf or group was removed/changed.
  return JSON.stringify(root) === JSON.stringify(layout.root) ? { ...layout, root } : { ...layout, root, vendor: undefined };
}

/** Identity of a device independent of the chosen view (e.g. UART trace vs terminal). */
export function deviceKey(slot: Slot): string {
  return `${slot.panel.split('.')[0]}/${String(slot.props.channel ?? slot.props.ch ?? slot.props.targets ?? '')}`;
}
