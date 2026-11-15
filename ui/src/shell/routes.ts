// Hash routes (plan §4.1): `#/l/<preset>`, `#/p/<domain>/<panel>?<props>[&at=<cursor>]`,
// `#/need-auth[?reason=expired|network|malformed]`. Links never carry rights or tokens.

import { defaultPanelFor } from '../store/channels';
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

/** Definitions carry no device aliases. Resolve against actual OPEN channels before use. */
export const PRESETS: readonly Preset[] = [
  { name: 'm1-default', label: '기본 작업 공간', grid: 'stack', slots: [] },
  { name: 'uart-only', label: 'UART 터미널', grid: 'single', slots: [] },
  { name: 'can-only', label: 'CAN 트레이스', grid: 'stack', slots: [] },
  { name: 'uds-only', label: 'UDS 진단', grid: 'stack', slots: [] },
  { name: 'psu-only', label: 'PSU 전압·전류', grid: 'stack', slots: [] },
  { name: 'can-uds', label: 'CAN·UDS 분석', grid: 'stack', slots: [] },
  { name: 'test', label: '테스트 실행', grid: 'stack', slots: [] },
  { name: 'eth', label: 'Ethernet 캡처', grid: 'stack', slots: [] },
  { name: 'logic', label: '로직 분석기', grid: 'stack', slots: [] },
  { name: 'video', label: '비디오 캡처', grid: 'stack', slots: [] },
];

export interface WorkspaceChannel {
  domain: string;
  channel: string;
  state: 'open' | 'closed' | 'unknown';
}

export const SERVICE_DOMAINS = ['uart', 'can', 'eth', 'psu', 'uds', 'logic', 'video'] as const;
export type ServiceDomain = typeof SERVICE_DOMAINS[number];
/** the permanent top-tab order (UART, CAN, Ethernet, UDS, PSU, logic, video) */
export const SERVICE_TAB_ORDER: readonly ServiceDomain[] = ['uart', 'can', 'eth', 'uds', 'psu', 'logic', 'video'];
export const SERVICE_LABELS: Record<ServiceDomain, string> = { uart: 'UART', can: 'CAN', eth: 'Ethernet', uds: 'UDS', psu: 'PSU', logic: '로직 분석기', video: '비디오 캡처' };
export const DIAGNOSTIC_OBSERVATION_DOMAINS = ['uds', 'can', 'eth'] as const;
export function serviceAllowsDomain(service: ServiceDomain, domain: string): boolean {
  return service === 'uds' ? DIAGNOSTIC_OBSERVATION_DOMAINS.some((d) => d === domain) : service === domain;
}
const SERVICE_PRESET: Record<ServiceDomain, string> = { uart: 'uart-only', can: 'can-only', eth: 'eth', uds: 'uds-only', psu: 'psu-only', logic: 'logic', video: 'video' };

const PRESET_DOMAINS: Record<string, readonly string[]> = {
  'uart-only': ['uart'], 'can-only': ['can'], 'uds-only': ['uds'], 'psu-only': ['psu'], 'can-uds': ['can', 'uds'], eth: ['eth'], logic: ['logic'], video: ['video'],
};

/** Resolve even a legacy direct route against available aliases, with no fallback devices. */
export function presetForChannels(name: string, channels: Iterable<WorkspaceChannel>, hasTestSession = false, explicitTest = false): Preset {
  const definition = PRESETS.find((p) => p.name === name) ?? PRESETS[0]!;
  const required = PRESET_DOMAINS[definition.name];
  const slots: Slot[] = definition.name === 'test'
    ? (hasTestSession || explicitTest ? [{ panel: 'shell.test', props: {} }] : [])
    : [...channels].filter((c) => c.state === 'open' && (!required || required.includes(c.domain))).flatMap((c) => {
      const slot = defaultPanelFor(c.domain, c.channel);
      return slot ? [slot] : [];
    });
  return { ...definition, slots };
}

/** The module application each service tab embeds before (or without) a connection: the existing rich panel with
 * NO channel/targets prop. Unbound means "module application, nothing selected" — never an implicit default alias.
 * UART has no unbound panel: its module chrome (saved sessions, macros, panes) is the application. */
export const UNBOUND_MODULE_PANEL: Readonly<Partial<Record<ServiceDomain, string>>> = {
  can: 'can.trace', eth: 'eth.packets', uds: 'uds.console', psu: 'psu.gauge', logic: 'logic.analyzer', video: 'video.workbench',
};
const BINDING_PROPS = ['channel', 'ch', 'targets'] as const;
/** true for a module panel that names no channel/capture/target (the unbound module application) */
export function isUnboundModuleSlot(slot: Slot): boolean {
  return Object.values(UNBOUND_MODULE_PANEL).includes(slot.panel) && BINDING_PROPS.every((k) => slot.props[k] === undefined || slot.props[k] === '');
}
/** the unbound module application slot of a service (a fresh object), or null for UART */
export function unboundModuleSlot(service: ServiceDomain): Slot | null {
  const panel = UNBOUND_MODULE_PANEL[service];
  return panel ? { panel, props: {} } : null;
}

/** Service tab projection: the service's OPEN channels (UDS adds raw CAN/Ethernet observations without inferring an
 * ECU/transport binding). When the service itself has no open channel, the unbound module application is seeded
 * first so the tab is a usable application with its own Connect/configuration — never a phantom channel. */
export function servicePresetForChannels(service: ServiceDomain, channels: Iterable<WorkspaceChannel>): Preset {
  const name = SERVICE_PRESET[service];
  const known = [...channels];
  let preset: Preset;
  if (service === 'can') {
    const base = presetForChannels(name, known);
    preset = { ...base, slots: base.slots.map((slot) => ({ ...slot, props: { ...slot.props, view: 'rxtx' } })) };
  } else if (service !== 'uds') preset = presetForChannels(name, known);
  else {
    const definition = PRESETS.find((p) => p.name === name)!;
    const active = known.filter((c) => c.state === 'open');
    const slots = DIAGNOSTIC_OBSERVATION_DOMAINS.flatMap((domain) => active.filter((c) => c.domain === domain).flatMap((c) => {
      const slot = defaultPanelFor(c.domain, c.channel);
      return slot ? [slot] : [];
    }));
    preset = { ...definition, slots };
  }
  const seed = unboundModuleSlot(service);
  const ownOpen = known.some((c) => c.state === 'open' && c.domain === service);
  return seed && !ownOpen ? { ...preset, slots: [seed, ...preset.slots] } : preset;
}

/** Only complete, currently usable choices appear in the picker. */
export function activePresets(channels: Iterable<WorkspaceChannel>, hasTestSession = false): Preset[] {
  const active = [...channels].filter((c) => c.state === 'open');
  return PRESETS.filter((p) => {
    if (p.name === 'test') return hasTestSession;
    const required = PRESET_DOMAINS[p.name];
    return !required || required.every((d) => active.some((c) => c.domain === d));
  }).map((p) => presetForChannels(p.name, active, hasTestSession));
}

export type Route =
  /** `name` = what the hash asked for (a saved layout or a preset); `preset` = that preset or the default */
  | { kind: 'layout'; preset: Preset; name: string; service?: ServiceDomain }
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
  const s = /^\/s\/(uart|can|eth|uds|psu|logic|video)$/.exec(h);
  if (s) {
    const service = s[1] as ServiceDomain;
    const name = SERVICE_PRESET[service];
    return { kind: 'layout', preset: PRESETS.find((x) => x.name === name)!, name, service };
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

/** Canonical slot identity; `at` is a reveal cursor, never a new panel instance.
 * CAN's legacy `ch` prop is equivalent to `channel` (the panel gives channel priority).
 */
export function slotKey(slot: Slot): string {
  const props = { ...slot.props };
  delete props.at;
  if (slot.panel === 'can.trace' && props.ch !== undefined) {
    props.channel ??= props.ch;
    delete props.ch;
  }
  const entries = Object.entries(props).sort(([a], [b]) => a.localeCompare(b)).map(([k, v]) => [k, String(v)]);
  return JSON.stringify([slot.panel, entries]);
}

/** same view and configuration, ignoring a reveal cursor and equivalent aliases */
export function sameSlot(a: Slot, b: Slot): boolean {
  return slotKey(a) === slotKey(b);
}

/** The service module a route belongs to: `#/s/<svc>` and deep links `#/p/<domain>/…` keep their module chrome
 * (an agent's `#/p/uart/terminal?channel=x` link opens inside the UART workbench, not a generic frame). */
export function routeModule(r: Route): ServiceDomain | null {
  if (r.kind === 'layout') return r.service ?? null;
  if (r.kind === 'panel') {
    const d = r.slot.panel.split('.')[0] ?? '';
    return (SERVICE_DOMAINS as readonly string[]).includes(d) ? (d as ServiceDomain) : null;
  }
  return null;
}
