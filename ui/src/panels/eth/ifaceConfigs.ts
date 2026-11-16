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

/** (B4a, ETH-04) replay speeds offered in the form; numbers are pace factors (CaptureStartParams.replay.speed) */
export const REPLAY_SPEEDS = ['max', 1, 2, 10] as const;
export type ReplaySpeed = (typeof REPLAY_SPEEDS)[number];
/** longest daemon-side replay path accepted (worker validate_start bound) */
export const MAX_REPLAY_PATH = 4096;

export interface ReplayOptions {
  /** file path on the DAEMON machine (not the browser) */
  path: string;
  speed: ReplaySpeed;
  repeat: boolean;
}

export interface IfaceConfig {
  /** daemon channel alias (eth.capture.start `ch`) */
  alias: string;
  /** NIC name (eth.capture.start `iface`); for replay the file label (replayLabel) */
  iface: string;
  backend: 'live' | 'sim' | 'replay';
  /** (B4a) present iff backend === 'replay' */
  replay?: ReplayOptions;
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
    (o.backend === 'live' ||
      o.backend === 'sim' ||
      (o.backend === 'replay' &&
        typeof o.replay === 'object' &&
        o.replay !== null &&
        typeof o.replay.path === 'string' &&
        o.replay.path.trim().length >= 1 &&
        o.replay.path.trim().length <= MAX_REPLAY_PATH &&
        (REPLAY_SPEEDS as readonly unknown[]).includes(o.replay.speed) &&
        typeof o.replay.repeat === 'boolean')) &&
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
    out.push({
      alias: c.alias,
      iface: c.iface.trim(),
      backend: c.backend,
      bpf: c.backend === 'live' ? c.bpf : '',
      snaplen: c.snaplen,
      promisc: c.promisc,
      bufferMib: c.bufferMib,
      ...(c.backend === 'replay' && c.replay ? { replay: { path: c.replay.path.trim(), speed: c.replay.speed, repeat: c.replay.repeat } } : {}),
    });
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
  if (c.backend === 'replay' && c.replay) {
    return {
      iface: c.iface,
      ch: c.alias,
      backend: 'replay',
      snaplen: c.snaplen,
      promisc: c.promisc,
      buffer_mib: c.bufferMib,
      replay: { path: c.replay.path, speed: c.replay.speed, repeat: c.replay.repeat },
    };
  }
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

/** F4.6 — capture `iface` label for a replay file: its base name (≤ 128 chars), `replay` when empty */
export function replayLabel(path: string): string {
  const t = path.trim();
  const base = t.slice(Math.max(t.lastIndexOf('/'), t.lastIndexOf('\\')) + 1);
  return base === '' ? 'replay' : base.slice(0, 128);
}
