// Saved layouts (plan §5.7). Backend today: localStorage (per browser profile), every access
// in try/catch — storage blocked or full → the dock keeps working, saving reports why.
// When CORE ships `core.ui.layout.{list,get,put}` (request already filed), a backend with the
// same interface replaces this one (daemon `~/.loom/config/layouts/<name>.json`).
// Names: preset names are reserved; a save never overwrites — a taken name gets `-2`, `-3`, ….
// C2: ≤ MAX_SAVED layouts, each ≤ 1 MiB (layoutModel.serializeLayout).

import type { KV } from '../../store/tabSettings';
import { parseLayout, serializeLayout, uniqueName, type LayoutIssue, type LoomLayout } from './layoutModel';

export const MAX_SAVED = 32;
const INDEX_KEY = 'loom.layouts.index';
const KEY = (name: string) => `loom.layout.${name}`;

export interface LayoutBackend {
  list(): string[];
  get(name: string): { layout: LoomLayout | null; issues: LayoutIssue[] };
  /** saves under a free name (never overwrites); returns the name used */
  save(l: LoomLayout, reserved: ReadonlySet<string>): { ok: true; name: string } | { ok: false; error: string };
  remove(name: string): void;
}

function defaultKV(): KV | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null;
  } catch {
    return null;
  }
}

export class LocalLayoutStore implements LayoutBackend {
  constructor(private readonly kv: KV | null = defaultKV()) {}

  list(): string[] {
    try {
      const raw = this.kv?.getItem(INDEX_KEY);
      const arr: unknown = raw ? JSON.parse(raw) : [];
      return Array.isArray(arr) ? arr.filter((x): x is string => typeof x === 'string').slice(0, MAX_SAVED) : [];
    } catch {
      return [];
    }
  }

  get(name: string): { layout: LoomLayout | null; issues: LayoutIssue[] } {
    let text: string | null;
    try {
      text = this.kv?.getItem(KEY(name)) ?? null;
    } catch {
      text = null;
    }
    if (text === null) return { layout: null, issues: [{ level: 'error', text: `저장된 레이아웃 "${name}" 없음` }] };
    const r = parseLayout(text);
    if (r.layout) r.layout.name = name;
    return r;
  }

  save(l: LoomLayout, reserved: ReadonlySet<string>): { ok: true; name: string } | { ok: false; error: string } {
    const names = this.list();
    if (names.length >= MAX_SAVED) return { ok: false, error: `저장 레이아웃 ${MAX_SAVED}개 상한 — 하나를 지우세요` };
    const name = uniqueName(l.name || 'layout', new Set([...names, ...reserved]));
    const ser = serializeLayout({ ...l, name });
    if (!ser.ok) return ser;
    try {
      this.kv?.setItem(KEY(name), ser.text);
      this.kv?.setItem(INDEX_KEY, JSON.stringify([...names, name]));
    } catch (e) {
      return { ok: false, error: `브라우저 저장소에 저장 못 함 (${e instanceof Error ? e.message : String(e)})` };
    }
    if (!this.kv) return { ok: false, error: '브라우저 저장소를 쓸 수 없음 (이 페이지 동안만 유지)' };
    return { ok: true, name };
  }

  remove(name: string): void {
    try {
      this.kv?.setItem(INDEX_KEY, JSON.stringify(this.list().filter((n) => n !== name)));
      (this.kv as (KV & { removeItem?(k: string): void }) | null)?.removeItem?.(KEY(name));
    } catch {
      /* storage blocked: nothing to remove */
    }
  }
}
