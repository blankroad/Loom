// Per-tab settings (plan §5.7): auto_open_panels on|off, follow_agent ask|auto|off.
// Stored in localStorage (per browser profile); storage failures fall back to defaults
// and the page keeps working (§5.7 "탭 로컬 조정은 localStorage(실패해도 동작)").

import { Emitter } from './emitter';

export type AutoOpen = 'on' | 'off';
export type FollowAgent = 'ask' | 'auto' | 'off';

export interface KV {
  getItem(k: string): string | null;
  setItem(k: string, v: string): void;
}

const K_AUTO = 'loom.tab.auto_open_panels';
const K_FOLLOW = 'loom.tab.follow_agent';

function defaultStorage(): KV | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null;
  } catch {
    return null;
  }
}

export class TabSettings extends Emitter {
  autoOpenPanels: AutoOpen = 'on';
  followAgent: FollowAgent = 'ask';
  private kv: KV | null;

  constructor(kv: KV | null = defaultStorage()) {
    super();
    this.kv = kv;
    const a = this.read(K_AUTO);
    if (a === 'on' || a === 'off') this.autoOpenPanels = a;
    const f = this.read(K_FOLLOW);
    if (f === 'ask' || f === 'auto' || f === 'off') this.followAgent = f;
  }

  setAutoOpen(v: AutoOpen): void {
    this.autoOpenPanels = v;
    this.write(K_AUTO, v);
    this.changed();
  }
  setFollow(v: FollowAgent): void {
    this.followAgent = v;
    this.write(K_FOLLOW, v);
    this.changed();
  }

  private read(k: string): string | null {
    try {
      return this.kv?.getItem(k) ?? null;
    } catch {
      return null;
    }
  }
  private write(k: string, v: string): void {
    try {
      this.kv?.setItem(k, v);
    } catch {
      /* private mode / quota: setting lasts for this page only */
    }
  }
}
