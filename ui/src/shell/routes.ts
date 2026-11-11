// Hash routes (plan §4.1): `#/l/<preset>`, `#/p/<domain>/<panel>?<props>[&at=<cursor>]`,
// `#/need-auth[?reason=expired|network|malformed]`. Links never carry rights or tokens.

import type { Props } from '../sdk/manifest';

export interface Slot {
  panel: string;
  props: Props;
}

export interface Preset {
  name: string;
  label: string;
  /** CSS grid template areas name */
  grid: 'two-top-one-bottom' | 'single' | 'stack';
  slots: Slot[];
}

export const PRESETS: readonly Preset[] = [
  {
    name: 'm1-default',
    label: 'm1-default',
    grid: 'two-top-one-bottom',
    slots: [
      { panel: 'uart.terminal', props: { channel: 'dut' } },
      { panel: 'can.trace', props: { channel: 'ch0' } },
      { panel: 'uds.console', props: { targets: 'bcm' } },
    ],
  },
  { name: 'uart-only', label: 'uart-only', grid: 'single', slots: [{ panel: 'uart.terminal', props: { channel: 'dut' } }] },
  {
    name: 'can-uds',
    label: 'can-uds',
    grid: 'stack',
    slots: [
      { panel: 'can.trace', props: { channel: 'ch0' } },
      { panel: 'uds.console', props: { targets: 'bcm' } },
    ],
  },
  {
    // M1b: test execution screen (shell-owned) + the bench PSU used by TC-1
    name: 'test',
    label: 'test',
    grid: 'stack',
    slots: [
      { panel: 'shell.test', props: {} },
      { panel: 'psu.gauge', props: { channel: 'bench' } },
    ],
  },
  {
    // F2: DoIP over Ethernet — packets next to the UDS console (↔UDS chips reveal there)
    name: 'eth',
    label: 'eth',
    grid: 'stack',
    slots: [
      { panel: 'eth.packets', props: { channel: 'lan1' } },
      { panel: 'uds.console', props: { targets: 'bcm' } },
    ],
  },
];

export type Route =
  /** `name` = what the hash asked for (a saved layout or a preset); `preset` = that preset or the default */
  | { kind: 'layout'; preset: Preset; name: string }
  | { kind: 'panel'; slot: Slot }
  | { kind: 'need-auth'; reason: 'expired' | 'network' | 'malformed' | null };

export function parseRoute(hash: string): Route {
  const h = hash.replace(/^#/, '');
  const na = /^\/need-auth(?:\?reason=([a-z]+))?$/.exec(h);
  if (na) {
    const reason = na[1] === 'expired' || na[1] === 'network' || na[1] === 'malformed' ? na[1] : null;
    return { kind: 'need-auth', reason };
  }
  const p = /^\/p\/([a-z][a-z0-9_]*)\/([a-z][a-z0-9_]*)(?:\?(.*))?$/.exec(h);
  if (p) {
    const props: Props = {};
    for (const [k, v] of new URLSearchParams(p[3] ?? '')) props[k] = v;
    return { kind: 'panel', slot: { panel: `${p[1]}.${p[2]}`, props } };
  }
  const l = /^\/l\/([a-z0-9-]+)$/.exec(h);
  const preset = PRESETS.find((x) => x.name === l?.[1]) ?? (PRESETS[0] as Preset);
  return { kind: 'layout', preset, name: l?.[1] ?? preset.name };
}


/** deep link for one panel: `#/p/<domain>/<name>?props[&at=…]` */
export function slotHash(slot: Slot, at: string | null = null): string {
  const [domain = '', name = ''] = slot.panel.split('.');
  const q = new URLSearchParams(Object.entries(slot.props).map(([k, v]) => [k, String(v)]));
  if (at) q.set('at', at);
  const qs = q.toString();
  return `#/p/${domain}/${name}${qs ? `?${qs}` : ''}`;
}

/** same panel type and the same props (ignoring the cursor `at`) */
export function sameSlot(a: Slot, b: Slot): boolean {
  if (a.panel !== b.panel) return false;
  const strip = (p: Props) => JSON.stringify(Object.entries(p).filter(([k]) => k !== 'at').sort(([x], [y]) => (x < y ? -1 : 1)).map(([k, v]) => [k, String(v)]));
  return strip(a.props) === strip(b.props);
}
