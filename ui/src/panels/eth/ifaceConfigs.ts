// Saved Ethernet capture-interface configurations (per viewer, local only).
//
// The unbound Ethernet module application (eth.packets without props.channel — the Ethernet service tab
// before any capture) lets the user pick / edit a saved configuration or a new one and press [연결]
// explicitly: that is the only path to `eth.capture.start`. A saved config is NOT a channel and never
// implies a live capture; loading, editing or deleting one sends nothing. The fields mirror
// crates/loom-eth/src/model.rs CaptureStartParams (iface + backend required; BPF live-only).
import { loadJson, saveJson } from '../../workbench/eth/bench';
import { CHANNEL_RE } from './dialogs';

export const IFACE_CFG_KEY = 'loom.eth.ifaces.v1';
export const MAX_IFACE_CFGS = 16;

export interface IfaceConfig {
  /** daemon channel alias (eth.capture.start `ch`) */
  alias: string;
  /** NIC name (eth.capture.start `iface`) */
  iface: string;
  backend: 'live' | 'sim';
  bpf: string;
  snaplen: number;
  promisc: boolean;
  bufferMib: number;
}

const isCfg = (v: unknown): v is IfaceConfig => {
  const o = v as Partial<IfaceConfig> | null;
  return (
    !!o &&
    typeof o.alias === 'string' &&
    CHANNEL_RE.test(o.alias) &&
    typeof o.iface === 'string' &&
    o.iface.trim().length > 0 &&
    o.iface.length <= 128 &&
    (o.backend === 'live' || o.backend === 'sim') &&
    typeof o.bpf === 'string' &&
    o.bpf.length <= 1024 &&
    typeof o.snaplen === 'number' &&
    Number.isInteger(o.snaplen) &&
    o.snaplen > 0 &&
    typeof o.promisc === 'boolean' &&
    typeof o.bufferMib === 'number' &&
    Number.isInteger(o.bufferMib) &&
    o.bufferMib > 0
  );
};

/** validated list; corrupt entries are dropped, duplicate aliases keep the first */
export function sanitizeConfigs(v: unknown): IfaceConfig[] {
  if (!Array.isArray(v)) return [];
  const out: IfaceConfig[] = [];
  for (const c of v) {
    if (!isCfg(c) || out.some((x) => x.alias === c.alias)) continue;
    out.push({ alias: c.alias, iface: c.iface.trim(), backend: c.backend, bpf: c.backend === 'live' ? c.bpf : '', snaplen: c.snaplen, promisc: c.promisc, bufferMib: c.bufferMib });
    if (out.length >= MAX_IFACE_CFGS) break;
  }
  return out;
}

export function loadConfigs(): IfaceConfig[] {
  return sanitizeConfigs(loadJson<unknown[]>(IFACE_CFG_KEY, Array.isArray, []));
}

export function saveConfigs(list: readonly IfaceConfig[]): void {
  saveJson(IFACE_CFG_KEY, sanitizeConfigs(list));
}

/** insert or replace by alias (most recent first), bounded */
export function upsertConfig(list: readonly IfaceConfig[], c: IfaceConfig, previousAlias?: string): IfaceConfig[] {
  return sanitizeConfigs([c, ...list.filter((x) => x.alias !== c.alias && x.alias !== previousAlias)]);
}

/** eth.capture.start params for a config (same shape as the capture dialog) */
export function captureParams(c: IfaceConfig): Record<string, unknown> {
  return {
    iface: c.iface,
    ch: c.alias,
    backend: c.backend,
    ...(c.backend === 'live' && c.bpf.trim() ? { bpf: c.bpf.trim() } : {}),
    snaplen: c.snaplen,
    promisc: c.promisc,
    buffer_mib: c.bufferMib,
  };
}
