// Wires one NetClient into the shell stores. Created once per tab in main.tsx.

import type { NetClient } from '../net/api';
import { ActionGate } from '../sdk/actionGate';
import { parseEvidenceRef, type EvidenceRef } from '../sdk/evidenceRef';
import { SimpleCursorBus, type ShellUiHooks } from '../sdk/context';
import { ActivityStore } from '../store/activityStore';
import { defaultPanelFor, ORIGIN_SUBJECT } from '../store/channels';
import { FocusController, type FocusRequest } from '../store/focus';
import { TabSettings } from '../store/tabSettings';
import { Emitter } from '../store/emitter';
import { RenderMetrics } from '../store/renderMetrics';
import { RenderScheduler, type FrameDeps } from '../store/renderScheduler';
import { RowStore } from '../store/rowStore';
import { ShellControlStore } from '../store/shellControl';
import { StopController, type Timers } from '../store/stopController';
import { TestSessionStore } from '../store/testSession';
import { ViewerReporter, type ReporterTimers } from '../store/viewerReport';
import { ConfirmHost } from './confirmHost';
import type { DockController } from './dock/dockHost';
import { LocalLayoutStore, type LayoutBackend } from './dock/layoutStore';
import { parseRoute, sameSlot, slotHash, type Slot } from './routes';

export const TICK_MS = 250;
const MAX_TOASTS = 5; // ring: oldest removed
export const TOAST_MS = 4_000;
const MAX_EXTRA_SLOTS = 4; // auto/explicitly opened panels beside the layout; oldest dropped
/** typing = focus in an input/terminal and a key within this window (focus requests wait) */
export const INPUT_IDLE_MS = 3_000;

export interface ToastAction {
  label: string;
  run(): void;
}

export interface Toast {
  id: number;
  text: string;
  actions: ToastAction[];
  ttlMs: number;
  /** same key replaces the earlier toast instead of stacking */
  key: string | null;
}

export class ToastStore extends Emitter {
  items: Toast[] = [];
  private next = 1;
  push(text: string, o: { actions?: ToastAction[]; ttlMs?: number; key?: string } = {}): number {
    const id = this.next++;
    const t: Toast = { id, text, actions: o.actions ?? [], ttlMs: o.ttlMs ?? TOAST_MS, key: o.key ?? null };
    const rest = t.key ? this.items.filter((x) => x.key !== t.key) : this.items;
    this.items = [...rest, t].slice(-MAX_TOASTS);
    this.changed();
    return id;
  }
  remove(id: number): void {
    this.items = this.items.filter((t) => t.id !== id);
    this.changed();
  }
}

/** Shell-level UI flags (overlays) shared by keys, top bar and panels. */
export class UiStateStore extends Emitter {
  approvalsOpen = false;
  helpOpen = false;
  /** no-dock fallback only (tests, embedding without a dock): panels opened beside the layout.
   * With the dock mounted, showSlot opens dock tabs instead (DockHost.open). */
  extraSlots: Slot[] = [];
  addExtraSlot(slot: Slot): boolean {
    if (this.extraSlots.some((x) => sameSlot(x, slot))) return false;
    this.extraSlots = [...this.extraSlots, slot].slice(-MAX_EXTRA_SLOTS);
    this.changed();
    return true;
  }
  removeExtraSlot(i: number): void {
    this.extraSlots = this.extraSlots.filter((_, k) => k !== i);
    this.changed();
  }
  setApprovalsOpen(v: boolean): void {
    this.approvalsOpen = v;
    this.changed();
  }
  setHelpOpen(v: boolean): void {
    this.helpOpen = v;
    this.changed();
  }
}

export interface ShellServices {
  net: NetClient;
  shell: ShellControlStore;
  rows: RowStore;
  activity: ActivityStore;
  testSession: TestSessionStore;
  scheduler: RenderScheduler;
  metrics: RenderMetrics;
  reporter: ViewerReporter;
  stop: StopController;
  confirm: ConfirmHost;
  gate: ActionGate;
  cursor: SimpleCursorBus;
  toasts: ToastStore;
  uiState: UiStateStore;
  uiHooks: ShellUiHooks;
  settings: TabSettings;
  focus: FocusController;
  /** make a panel visible: already in the layout → 'shown', else added as a dock tab → 'added' */
  showSlot(slot: Slot): 'shown' | 'added' | 'routed';
  /** the mounted dock (null on #/need-auth or in tests without a dock) */
  dock: { current: DockController | null };
  /** the dock registers itself while mounted; returns the detach function */
  attachDock(c: DockController): () => void;
  /** saved layouts (localStorage now; core.ui.layout.* later) */
  layouts: LayoutBackend;
  now: () => number;
  /** start listening to the net client; returns teardown */
  bind(): () => void;
}

export interface ServiceOptions {
  now?: () => number;
  timers?: Timers;
  frames?: FrameDeps;
  /** set false in tests to drive tick() manually */
  autoTick?: boolean;
  /** absolute browser ms on the net clock's base (default performance.timeOrigin + now) */
  absNowMs?: () => number;
  /** timers for the 10 s render report (tests) */
  reportTimers?: ReporterTimers;
  /** ui.render.sample mode (test harness); default: `?render_sample=1` or localStorage loom.renderSample = '1' */
  renderSample?: boolean;
  settings?: TabSettings;
  /** navigator.webdriver (default) — automation browser, V5 */
  automation?: boolean;
  /** user is typing (default: focus in input/textarea/contenteditable + key within INPUT_IDLE_MS) */
  inputBusy?: () => boolean;
  /** hash routing (tests) */
  navigate?: (hash: string) => void;
  currentHash?: () => string;
  /** reveal retry timer (tests) */
  setTimeout?: (cb: () => void, ms: number) => unknown;
  layouts?: LayoutBackend;
}

const REVEAL_TRIES = 20;
const REVEAL_RETRY_MS = 100;

/** REF for a focus request's `at=seq:N` (null when the panel has no channel to address) */
function focusRef(req: FocusRequest): string | null {
  if (!req.at?.startsWith('seq:')) return null;
  const domain = req.panel.split('.')[0] ?? '';
  const ch = (req.props.channel ?? req.props.targets ?? '').split(',')[0]?.trim() ?? '';
  return ch ? `${domain}:${ch}@seq:${req.at.slice(4)}` : null;
}

function sampleModeFromEnv(): boolean {
  try {
    if (typeof location !== 'undefined' && new URLSearchParams(location.search).get('render_sample') === '1') return true;
    return typeof localStorage !== 'undefined' && localStorage.getItem('loom.renderSample') === '1';
  } catch {
    return false;
  }
}

export function createShellServices(net: NetClient, opts: ServiceOptions = {}): ShellServices {
  const now = opts.now ?? (() => performance.now());
  const shell = new ShellControlStore();
  const rows = new RowStore();
  const activity = new ActivityStore();
  const testSession = new TestSessionStore();
  const scheduler = new RenderScheduler(opts.frames);
  const metrics = new RenderMetrics();
  metrics.sampleMode = opts.renderSample ?? sampleModeFromEnv();
  scheduler.sink = metrics;
  const reporter = new ViewerReporter(net, metrics, {
    now,
    ...(opts.absNowMs ? { absNowMs: opts.absNowMs } : {}),
    ...(opts.reportTimers ? { timers: opts.reportTimers } : {}),
  });
  const stop = new StopController(net, shell, opts.timers);
  const confirm = new ConfirmHost();
  const toasts = new ToastStore();
  const uiState = new UiStateStore();
  const cursor = new SimpleCursorBus();
  const settings = opts.settings ?? new TabSettings();
  const navigate = opts.navigate ?? ((h: string) => void (location.hash = h));
  const currentHash = opts.currentHash ?? (() => (typeof location !== 'undefined' ? location.hash : ''));
  const later = opts.setTimeout ?? ((cb: () => void, ms: number) => setTimeout(cb, ms));
  shell.setAutomation(opts.automation ?? (typeof navigator !== 'undefined' && navigator.webdriver === true));
  let lastKeyMs = Number.NEGATIVE_INFINITY;
  const inputBusy =
    opts.inputBusy ??
    (() => {
      if (typeof document === 'undefined') return false;
      const a = document.activeElement;
      const typing = a instanceof HTMLElement && (a.tagName === 'INPUT' || a.tagName === 'TEXTAREA' || a.isContentEditable);
      return typing && now() - lastKeyMs < INPUT_IDLE_MS;
    });
  const gate = new ActionGate({
    net,
    rights: () => shell.rights,
    writeLocked: () => shell.status.writeLocked,
    confirmUi: confirm,
  });
  // C2: listeners = mounted panels (bounded by the layout)
  const revealListeners = new Set<(ref: EvidenceRef) => boolean>();
  const uiHooks: ShellUiHooks = {
    reveal: (ref) => {
      const p = parseEvidenceRef(ref);
      if (!p) {
        toasts.push(`알 수 없는 증거 REF: ${ref}`);
        return;
      }
      let handled = false;
      for (const cb of [...revealListeners]) if (cb(p)) handled = true;
      if (!handled) {
        const where = p.requestId ? `request ${p.requestId}` : `seq ${p.seqA}${p.seqB !== null ? `..${p.seqB}` : ''}`;
        toasts.push(`${p.domain}:${p.channel} ${where} — 이 화면에 해당 패널이 없음`);
      }
    },
    onReveal: (cb) => {
      revealListeners.add(cb);
      return () => revealListeners.delete(cb);
    },
    openPanel: (id, props) => {
      const [domain, name] = id.split('.');
      const q = new URLSearchParams(Object.entries(props).map(([k, v]) => [k, String(v)])).toString();
      location.hash = `#/p/${domain ?? ''}/${name ?? ''}${q ? `?${q}` : ''}`;
    },
    toast: (msg) => {
      toasts.push(msg);
    },
  };
  /** reveal without the "not on this screen" toast; true when a panel took it */
  const revealQuiet = (ref: string): boolean => {
    const p = parseEvidenceRef(ref);
    if (!p) return false;
    let handled = false;
    for (const cb of [...revealListeners]) if (cb(p)) handled = true;
    return handled;
  };
  /** after routing, the panel mounts asynchronously — retry for ~2 s */
  const revealSoon = (ref: string): void => {
    let n = 0;
    const attempt = () => {
      if (revealQuiet(ref)) return;
      if (++n < REVEAL_TRIES) later(attempt, REVEAL_RETRY_MS);
      else toasts.push(`${ref} — 열린 패널에서 찾지 못함 (보관 범위 밖일 수 있음)`);
    };
    later(attempt, 0);
  };
  const dock: { current: DockController | null } = { current: null };
  const layouts = opts.layouts ?? new LocalLayoutStore();
  const attachDock = (c: DockController): (() => void) => {
    dock.current = c;
    return () => {
      if (dock.current === c) dock.current = null;
    };
  };
  const showSlot = (slot: Slot): 'shown' | 'added' | 'routed' => {
    // docking (M2): an open panel's tab is activated, a new one becomes a tab
    if (dock.current) return dock.current.open(slot);
    // no dock mounted (tests / need-auth): the M1 fallback below
    const route = parseRoute(currentHash());
    const shown =
      (route.kind === 'layout' && route.preset.slots.some((x) => sameSlot(x, slot))) ||
      (route.kind === 'panel' && sameSlot(route.slot, slot)) ||
      uiState.extraSlots.some((x) => sameSlot(x, slot));
    if (shown) return 'shown';
    if (route.kind === 'need-auth') {
      navigate(slotHash(slot));
      return 'routed';
    }
    uiState.addExtraSlot(slot);
    return 'added';
  };
  // agent "look here": routes + reveals only (never presses anything)
  const focus = new FocusController({
    settings,
    inputBusy,
    now,
    open: (req) => {
      const slot: Slot = { panel: req.panel, props: req.props };
      const ref = focusRef(req);
      if (dock.current) {
        // docking: activate the panel's tab (or add it as a tab), then reveal
        dock.current.open(slot);
        if (ref) revealSoon(ref);
        return;
      }
      const route = parseRoute(currentHash());
      const visible =
        (route.kind === 'layout' && route.preset.slots.some((x) => sameSlot(x, slot))) ||
        (route.kind === 'panel' && sameSlot(route.slot, slot)) ||
        uiState.extraSlots.some((x) => sameSlot(x, slot));
      if (!visible) navigate(slotHash(slot, req.at));
      if (ref) revealSoon(ref);
    },
  });

  const bind = (): (() => void) => {
    const offs: (() => void)[] = [];
    shell.setConn(net.state, now());
    offs.push(net.onState((s) => shell.setConn(s, now())));
    offs.push(
      net.onControl((ev) => {
        const t = now();
        shell.applyControl(ev, t);
        if (ev.request_id) rows.apply(ev);
        if (ev.kind === 'core.agent_cursor' || ev.kind === 'core.activity') activity.applyEvent(ev);
        if (ev.kind.startsWith('test.')) testSession.apply(ev);
        if (ev.kind === 'ui.focus_request') focus.receive(ev);
      }),
    );
    // UD-31: a channel opened while watching → bar (shell store) + toast + optional auto-open
    offs.push(
      shell.onChannelOpened(({ entry, signal, isNew }) => {
        const dp = defaultPanelFor(entry.domain, entry.channel);
        const subj = ORIGIN_SUBJECT[signal.origin] ?? `${signal.origin}가`;
        const what = `${entry.domain} ${entry.channel}`;
        const verb = isNew ? '열었습니다' : '다시 열었습니다';
        const key = `chan:${entry.domain}/${entry.channel}`;
        if (dp && settings.autoOpenPanels === 'on') {
          const r = showSlot(dp);
          toasts.push(`${subj} ${what}를 ${verb}${r === 'added' ? ' — 패널을 열었습니다' : ''}`, { key, ttlMs: 8_000 });
        } else {
          toasts.push(`${subj} ${what}를 ${verb}`, {
            key,
            ttlMs: 12_000,
            actions: dp ? [{ label: '패널 열기', run: () => void showSlot(dp) }] : [],
          });
        }
      }),
    );
    if (typeof window !== 'undefined') {
      const onKey = () => {
        lastKeyMs = now();
      };
      window.addEventListener('keydown', onKey, true);
      offs.push(() => window.removeEventListener('keydown', onKey, true));
    }
    // Activity is not a control kind: the shell holds one subscription for it.
    const sub = net.subscribe(
      { kinds: ['core.activity'] },
      { snapshot: true },
      {
        onSnapshotPart: (p) => {
          shell.noteSnapshotItems(p.items ?? []);
          p.events?.forEach((e) => activity.applyEvent(e));
        },
        onBatch: (b) => {
          b.events.forEach((e) => activity.applyEvent(e));
        },
      },
    );
    offs.push(() => sub.close());
    // test.* are priority events (control path); this subscription adds the late-join
    // snapshot (reducer state + backlog). Duplicates are dropped by seq in the store.
    const testSub = net.subscribe(
      { domains: ['test'] },
      { snapshot: true },
      {
        onSnapshotPart: (p) => {
          for (const it of p.items ?? []) if (it.domain === 'test') testSession.applySnapshotState(it.state);
          testSession.applyMany(p.events ?? []);
        },
        onBatch: (b) => testSession.applyMany(b.events),
      },
    );
    offs.push(() => testSub.close());
    offs.push(scheduler.onDegradedChange(() => shell.tick(now())));
    if (opts.autoTick !== false) {
      const h = setInterval(() => {
        shell.tick(now());
        focus.retryHeld();
      }, TICK_MS);
      offs.push(() => clearInterval(h));
    }
    // presence + 10 s render report (reporter calls net.setVisible first, then the full presence)
    if (opts.autoTick !== false || opts.reportTimers) reporter.start();
    offs.push(() => reporter.stop());
    if (typeof document !== 'undefined') {
      const vis = () => reporter.setVisible(!document.hidden);
      document.addEventListener('visibilitychange', vis);
      offs.push(() => document.removeEventListener('visibilitychange', vis));
    }
    return () => {
      offs.forEach((f) => f());
      stop.dispose();
    };
  };

  return { net, shell, rows, activity, testSession, scheduler, metrics, reporter, stop, confirm, gate, cursor, toasts, uiState, uiHooks, settings, focus, showSlot, dock, attachDock, layouts, now, bind };
}

// evidence/reveal REF parsing lives in the SDK (panels use it too)
export { parseEvidenceRef, type EvidenceRef } from '../sdk/evidenceRef';
