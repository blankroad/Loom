// Agent "look here" requests (plan §5.7 가리키기, contract 02 §2.5 `ui.focus_request`).
// payload {panel, props, at, reason, actor}. The agent may only CHANGE THE VIEW (with
// the tab's permission) — never press a button: open() only routes + reveals.
//
//   follow_agent = ask  (default) → prompt [열기 ⏎] [이 탭은 자동으로 따라가기 ☐] [무시 Esc]
//                = auto           → open at once, unless the user is typing (then held and
//                                   prompted; retried when input goes idle)
//                = off            → ignored (counted)
// Requests with the same key (panel+props+at) within MERGE_MS are merged (count++);
// a different request replaces the pending one (the latest wins, replaced counted).
//
// C2: one pending request; payload strings capped (reason 500, props 16 keys × 256).

import type { LoomEvent } from '../proto/provisional';
import { Emitter } from './emitter';
import type { TabSettings } from './tabSettings';

export const MERGE_MS = 2_000;
const PANEL_RE = /^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$/;
const AT_RE = /^(seq:\d{1,20}|mark:[^\s#]{1,64}|-\d{1,6}(\.\d{1,3})?s)$/;
const MAX_PROPS = 16;
const MAX_PROP_LEN = 256;
const MAX_REASON = 500;

export interface FocusRequest {
  key: string;
  panel: string;
  props: Record<string, string>;
  at: string | null;
  reason: string | null;
  actor: string;
  count: number;
  atMs: number;
  /** auto mode, but the user was typing → waiting */
  held: boolean;
}

export function readFocusRequest(ev: LoomEvent, nowMs: number): FocusRequest | null {
  if (ev.kind !== 'ui.focus_request') return null;
  const p = (typeof ev.payload === 'object' && ev.payload !== null ? ev.payload : {}) as Record<string, unknown>;
  const panel = typeof p.panel === 'string' && PANEL_RE.test(p.panel) ? p.panel : null;
  if (!panel) return null;
  const props: Record<string, string> = {};
  if (typeof p.props === 'object' && p.props !== null) {
    for (const [k, v] of Object.entries(p.props as Record<string, unknown>).slice(0, MAX_PROPS)) {
      if (!/^[a-z][a-z0-9_]{0,31}$/.test(k)) continue;
      if (typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') props[k] = String(v).slice(0, MAX_PROP_LEN);
    }
  }
  const at = typeof p.at === 'string' && AT_RE.test(p.at) ? p.at : null;
  const reason = typeof p.reason === 'string' && p.reason.trim() ? p.reason.trim().slice(0, MAX_REASON) : null;
  const actor = (typeof p.actor === 'string' && p.actor) || ev.actor_id || ev.client_id || 'agent';
  const key = `${panel}?${Object.entries(props)
    .sort(([a], [b]) => (a < b ? -1 : 1))
    .map(([k, v]) => `${k}=${v}`)
    .join('&')}@${at ?? ''}`;
  return { key, panel, props, at, reason, actor: actor.slice(0, 64), count: 1, atMs: nowMs, held: false };
}

export interface FocusDeps {
  settings: TabSettings;
  /** the user is typing (terminal / input with recent keystrokes) */
  inputBusy(): boolean;
  /** route + reveal; never presses anything */
  open(req: FocusRequest): void;
  now(): number;
}

export class FocusController extends Emitter {
  pending: FocusRequest | null = null;
  /** last request opened automatically (info line) */
  lastAuto: FocusRequest | null = null;
  ignored = 0;
  replaced = 0;
  private recentOpened: FocusRequest | null = null;

  constructor(private readonly d: FocusDeps) {
    super();
  }

  receive(ev: LoomEvent): void {
    const t = this.d.now();
    const req = readFocusRequest(ev, t);
    if (!req) return;
    const mode = this.d.settings.followAgent;
    if (mode === 'off') {
      this.ignored++;
      this.changed();
      return;
    }
    // merge duplicates within MERGE_MS
    if (this.pending && this.pending.key === req.key && t - this.pending.atMs < MERGE_MS) {
      this.pending.count++;
      this.changed();
      return;
    }
    if (this.recentOpened && this.recentOpened.key === req.key && t - this.recentOpened.atMs < MERGE_MS) {
      this.recentOpened.count++;
      return;
    }
    if (this.pending) this.replaced++;
    if (mode === 'auto' && !this.d.inputBusy()) {
      this.pending = null;
      this.doOpen(req, true);
      return;
    }
    req.held = mode === 'auto';
    this.pending = req;
    this.changed();
  }

  /** [열기 ⏎] */
  accept(): void {
    const p = this.pending;
    if (!p) return;
    this.pending = null;
    this.doOpen(p, false);
  }

  /** [이 탭은 자동으로 따라가기 ☐] then open */
  followAndOpen(): void {
    this.d.settings.setFollow('auto');
    this.accept();
  }

  /** [무시 Esc] */
  dismiss(): void {
    if (!this.pending) return;
    this.pending = null;
    this.ignored++;
    this.changed();
  }

  /** shell tick: a held auto request opens once the user stops typing */
  retryHeld(): void {
    const p = this.pending;
    if (p?.held && this.d.settings.followAgent === 'auto' && !this.d.inputBusy()) {
      this.pending = null;
      this.doOpen(p, true);
    }
  }

  dismissAutoInfo(): void {
    if (!this.lastAuto) return;
    this.lastAuto = null;
    this.changed();
  }

  private doOpen(req: FocusRequest, auto: boolean): void {
    this.recentOpened = { ...req, atMs: this.d.now() };
    this.lastAuto = auto ? req : null;
    this.d.open(req);
    this.changed();
  }
}

/** "CAN ch0 Trace · seq 5012로 이동" */
export function describeFocus(r: Pick<FocusRequest, 'panel' | 'props' | 'at'>): { where: string; at: string | null } {
  const [domain = '', name = ''] = r.panel.split('.');
  const ch = r.props.channel ?? r.props.targets ?? r.props.ch ?? '';
  const where = `${domain.toUpperCase()}${ch ? ` ${ch}` : ''} ${name.charAt(0).toUpperCase()}${name.slice(1)}`;
  let at: string | null = null;
  if (r.at?.startsWith('seq:')) at = `seq ${r.at.slice(4)}로 이동`;
  else if (r.at?.startsWith('mark:')) at = `마커 ${r.at.slice(5)}로 이동`;
  else if (r.at) at = `${r.at.slice(1, -1)} s 전으로 이동`;
  return { where, at };
}
