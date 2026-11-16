// Per-viewer browser key/value storage for CAN panel conveniences (time mode, transmit list).
// Same precedent as panels/eth/filterBar.ts and store/tabSettings.ts: localStorage, every access
// guarded — a private window / blocked storage / quota error never breaks the panel.
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B1-KV.
//
// SCAFFOLD (Opus): signatures only. Bodies are Sonnet batch-1 work.

export interface KV {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** `globalThis.localStorage` when reachable, else null (access itself may throw). */
export function defaultKV(): KV | null {
  try {
    return typeof localStorage !== 'undefined' ? globalThis.localStorage : null;
  } catch {
    return null;
  }
}
