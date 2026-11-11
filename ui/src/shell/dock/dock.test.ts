// @vitest-environment jsdom
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { CHANNEL_PROP } from '../../sdk/propsSchema';
import { registerPanel } from '../../sdk/registry';
import { TabSettings, type KV } from '../../store/tabSettings';
import { ev, FakeNet, TEST_HELLO } from '../../store/testing/fakeNet';
import { type Preset, type Slot } from '../routes';
import { createShellServices } from '../services';
import { DockHost } from './dockHost';
import {
  DOCKVIEW_VERSION,
  fromDockview,
  leaves,
  MAX_LAYOUT_BYTES,
  parseLayout,
  presetToLayout,
  serializeLayout,
  slotLayout,
  toDockview,
  uniqueName,
  vendorMatches,
  type LayoutNode,
  type LoomLayout,
} from './layoutModel';
import { LocalLayoutStore, MAX_SAVED } from './layoutStore';

// Explicit model fixtures; production presets resolve actual devices at runtime.
const PRESETS: readonly Preset[] = [
  { name: 'm1-default', label: 'fixture', grid: 'two-top-one-bottom', slots: [
    { panel: 'uart.terminal', props: { channel: 'dut' } },
    { panel: 'can.trace', props: { channel: 'ch0' } },
    { panel: 'uds.console', props: { targets: 'bcm' } },
  ] },
  { name: 'uart-only', label: 'fixture', grid: 'single', slots: [{ panel: 'uart.terminal', props: { channel: 'dut' } }] },
];

// jsdom has no ResizeObserver (dockview watches its container)
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// probe panels: count mounts, draws, resizes, disposes
const log: string[] = [];
const draws = new Map<string, number>();
const resizes: string[] = [];
const invalidators = new Map<string, () => void>();
beforeAll(() => {
  for (const id of ['uart.terminal', 'can.trace', 'uds.console', 'eth.packets', 'psu.gauge']) {
    registerPanel(
      {
        id,
        domain: id.split('.')[0] as string,
        title: id.toUpperCase(),
        version: 1,
        milestone: 'M1',
        topics: [],
        queries: [],
        actions: [],
        memory: { maxBytes: 1 },
        render: { maxHz: 60, budgetMs: 8 },
        propsSchema: { type: 'object', properties: { channel: CHANNEL_PROP, targets: CHANNEL_PROP, rows: { type: 'integer', minimum: 1 } } },
      },
      (ctx) => {
        const key = `${id}:${String(ctx.props.channel ?? ctx.props.targets ?? '')}`;
        ctx.scheduler.onFrame(() => {
          draws.set(key, (draws.get(key) ?? 0) + 1);
        });
        invalidators.set(key, () => ctx.scheduler.invalidate());
        return {
          mount: () => void log.push(`mount ${key}`),
          resize: (w, h) => void resizes.push(`${key} ${w}x${h}`),
          dispose: () => void log.push(`dispose ${key}`),
        };
      },
    );
  }
});

const memKV = (): KV & { removeItem(k: string): void; m: Map<string, string> } => {
  const m = new Map<string, string>();
  return { m, getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v), removeItem: (k) => void m.delete(k) };
};

// ---------------------------------------------------------------- model
describe('layout model', () => {
  it('every M1 preset becomes a dock layout and survives dockview JSON round-trip', () => {
    for (const p of PRESETS as readonly Preset[]) {
      const l = presetToLayout(p);
      expect(leaves(l.root).map((x) => x.panel)).toEqual(p.slots.map((s) => s.panel));
      const back = fromDockview(toDockview(l.root, 1200, 800, (x) => x.panel));
      expect(back).toEqual(l.root);
    }
  });

  it('tabs nodes and nested splits round-trip (sizes as relative weights)', () => {
    const root: LayoutNode = {
      split: 'h',
      sizes: [70, 30],
      children: [
        { tabs: [{ panel: 'can.trace', props: { channel: 'ch0' } }, { panel: 'can.trace', props: { channel: 'ch1' } }], active: 1 },
        { split: 'v', children: [{ panel: 'uds.console', props: { targets: 'bcm' } }, { panel: 'uart.terminal', props: { channel: 'dut' } }] },
      ],
    };
    expect(fromDockview(toDockview(root, 1000, 600, () => 't'))).toEqual(root);
    // a vertical root split nested under nothing: orientation follows the root
    const v: LayoutNode = { split: 'v', sizes: [25, 75], children: [{ panel: 'uart.terminal', props: {} }, { panel: 'can.trace', props: {} }] };
    expect(fromDockview(toDockview(v, 1000, 800, () => 't'))).toEqual(v);
  });

  it('parse: v1 accepted; props validated (bad value / unknown prop dropped, `at` kept); unknown panel kept as placeholder', () => {
    const { layout, issues } = parseLayout({
      loom_layout: 1,
      name: 'bench-a',
      env_hint: 'bench',
      root: {
        split: 'h',
        children: [
          { panel: 'can.trace', props: { channel: 'ch0', rows: '5000', bogus: 1, at: 'seq:12' } },
          { panel: 'uart.terminal', props: { channel: 'bad channel!' } },
          { panel: 'viz.plot', props: { signals: 'x' } },
        ],
      },
      pinned: ['core.activity'],
    });
    expect(layout).not.toBeNull();
    const ls = leaves(layout!.root);
    expect(ls[0]).toEqual({ panel: 'can.trace', props: { channel: 'ch0', rows: 5000, at: 'seq:12' } });
    expect(ls[1]).toEqual({ panel: 'uart.terminal', props: {} });
    expect(ls[2]).toEqual({ panel: 'viz.plot', props: { signals: 'x' } }); // kept for a newer build
    expect(issues.map((i) => i.text)).toEqual(
      expect.arrayContaining([expect.stringContaining('모르는 prop "bogus"'), expect.stringContaining('"channel" 형식 불일치'), expect.stringContaining('이 버전에 없는 패널 "viz.plot"')]),
    );
    expect(layout).toMatchObject({ name: 'bench-a', env_hint: 'bench', pinned: ['core.activity'] });
  });

  it('migration: M1 {slots[]} → v1; a newer loom_layout is refused; preset refs and junk refused', () => {
    const m = parseLayout({ name: 'old', slots: [{ panel: 'uart.terminal', props: { channel: 'dut' } }, { panel: 'can.trace', props: { channel: 'ch0' } }] });
    expect(m.layout?.root).toEqual({ split: 'v', children: [{ panel: 'uart.terminal', props: { channel: 'dut' } }, { panel: 'can.trace', props: { channel: 'ch0' } }] });
    expect(m.issues[0]?.text).toContain('M1 slots');
    expect(parseLayout({ loom_layout: 2, root: { panel: 'can.trace' } })).toMatchObject({ layout: null, issues: [{ level: 'error', text: expect.stringContaining('더 새 레이아웃 형식') }] });
    expect(parseLayout({ preset: 'm1-default' }).layout).toBeNull();
    expect(parseLayout('{nope').layout).toBeNull();
    expect(parseLayout({ loom_layout: 1, root: { split: 'h', children: [] } }).layout).toBeNull();
  });

  it('1 MiB cap on read and on save', () => {
    const big = JSON.stringify({ loom_layout: 1, name: 'big', root: { panel: 'can.trace', props: {} }, pinned: [], pad: 'x'.repeat(MAX_LAYOUT_BYTES) });
    expect(parseLayout(big)).toMatchObject({ layout: null, issues: [{ level: 'error', text: expect.stringContaining('1 MiB') }] });
    const l: LoomLayout = { ...presetToLayout(PRESETS[0] as Preset), vendor: { dockview: { version: DOCKVIEW_VERSION, state: 'y'.repeat(MAX_LAYOUT_BYTES) } } };
    expect(serializeLayout(l)).toMatchObject({ ok: false });
    expect(serializeLayout(presetToLayout(PRESETS[0] as Preset)).ok).toBe(true);
  });

  it('vendor state is reused only for the same dockview version and the same panels', () => {
    const base = presetToLayout(PRESETS[0] as Preset);
    const state = toDockview(base.root, 1200, 800, () => 't');
    expect(vendorMatches({ ...base, vendor: { dockview: { version: DOCKVIEW_VERSION, state } } })).toBe(true);
    expect(vendorMatches({ ...base, vendor: { dockview: { version: '9.0.0', state } } })).toBe(false);
    const other = toDockview(presetToLayout(PRESETS[1] as Preset).root, 1200, 800, () => 't');
    expect(vendorMatches({ ...base, vendor: { dockview: { version: DOCKVIEW_VERSION, state: other } } })).toBe(false);
  });

  it('unique names never overwrite (presets reserved)', () => {
    expect(uniqueName('m1-default', new Set(['m1-default']))).toBe('m1-default-2');
    expect(uniqueName('My Bench!', new Set())).toBe('my-bench');
    expect(uniqueName('a', new Set(['a', 'a-2']))).toBe('a-3');
  });
});

describe('saved layouts (localStorage)', () => {
  it('save → list → get; a taken or preset name gets a new name; blocked storage reports, never throws', () => {
    const kv = memKV();
    const st = new LocalLayoutStore(kv);
    const reserved = new Set(PRESETS.map((p) => p.name));
    const l = presetToLayout(PRESETS[0] as Preset);
    expect(st.save(l, reserved)).toEqual({ ok: true, name: 'm1-default-2' });
    expect(st.save({ ...l, name: 'bench' }, reserved)).toEqual({ ok: true, name: 'bench' });
    expect(st.save({ ...l, name: 'bench' }, reserved)).toEqual({ ok: true, name: 'bench-2' });
    expect(st.list()).toEqual(['m1-default-2', 'bench', 'bench-2']);
    expect(st.get('bench').layout?.root).toEqual(l.root);
    st.remove('bench');
    expect(st.list()).toEqual(['m1-default-2', 'bench-2']);
    const broken: KV = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('quota');
      },
    };
    const b = new LocalLayoutStore(broken);
    expect(b.list()).toEqual([]);
    expect(b.save(l, reserved)).toMatchObject({ ok: false, error: expect.stringContaining('quota') });
  });

  it(`at most ${MAX_SAVED} saved layouts`, () => {
    const st = new LocalLayoutStore(memKV());
    const l = slotLayout({ panel: 'can.trace', props: { channel: 'ch0' } });
    for (let i = 0; i < MAX_SAVED; i++) expect(st.save({ ...l, name: `l${i}` }, new Set()).ok).toBe(true);
    expect(st.save({ ...l, name: 'one-more' }, new Set())).toMatchObject({ ok: false });
  });
});

// ---------------------------------------------------------------- DockHost
let hostEl: HTMLDivElement;
let dock: DockHost | null = null;
let unbind: (() => void) | null = null;
afterEach(() => {
  dock?.dispose();
  dock = null;
  unbind?.();
  unbind = null;
  hostEl?.remove();
  log.length = 0;
  draws.clear();
  resizes.length = 0;
});

function mountDock() {
  const net = new FakeNet();
  net.open(TEST_HELLO);
  let frame: (() => void) | null = null;
  let t = 0;
  const rafQ: (() => void)[] = [];
  const services = createShellServices(net, {
    autoTick: false,
    settings: new TabSettings(memKV()),
    frames: { raf: (cb) => ((frame = () => cb(t)), 1), cancelRaf: () => (frame = null), now: () => t },
    setTimeout: (cb) => (cb(), 0),
  });
  unbind = services.bind();
  hostEl = document.createElement('div');
  document.body.append(hostEl);
  dock = new DockHost(hostEl, services, { raf: (cb) => (rafQ.push(cb), rafQ.length), cancelRaf: () => {} });
  const detach = services.attachDock(dock);
  const prevUnbind = unbind;
  unbind = () => {
    detach();
    prevUnbind();
  };
  const runFrame = () => {
    t += 20; // one 50 Hz frame later (panels are capped at 60 Hz)
    const f = frame;
    frame = null;
    f?.();
  };
  const runRaf = () => rafQ.splice(0).forEach((cb) => cb());
  return { net, services, dock, runFrame, runRaf };
}

describe('DockHost', () => {
  it('loads a preset: every panel mounts; clear/dispose disposes every panel', () => {
    const { dock: d } = mountDock();
    expect(d.load(presetToLayout(PRESETS[0] as Preset))).toEqual([]);
    expect(log).toEqual(['mount uart.terminal:dut', 'mount can.trace:ch0', 'mount uds.console:bcm']);
    expect(d.snapshot('x').root).toEqual(presetToLayout(PRESETS[0] as Preset).root);
    d.load(slotLayout({ panel: 'can.trace', props: { channel: 'ch1' } })); // deep link replaces the dock
    expect(log.slice(3)).toEqual(['dispose uart.terminal:dut', 'dispose can.trace:ch0', 'dispose uds.console:bcm', 'mount can.trace:ch1']);
    expect(d.api.panels).toHaveLength(1);
  });

  it('a background tab is not drawn; it draws once with the latest data when shown', () => {
    const { dock: d, runFrame } = mountDock();
    d.load({ loom_layout: 1, name: 't', env_hint: null, pinned: [], root: { tabs: [{ panel: 'can.trace', props: { channel: 'ch0' } }, { panel: 'can.trace', props: { channel: 'ch1' } }], active: 0 } });
    runFrame();
    draws.clear();
    invalidators.get('can.trace:ch0')!();
    invalidators.get('can.trace:ch1')!(); // hidden tab: data arrived
    runFrame();
    expect(draws.get('can.trace:ch0')).toBe(1);
    expect(draws.get('can.trace:ch1')).toBeUndefined();
    d.find({ panel: 'can.trace', props: { channel: 'ch1' } })!.api.setActive();
    runFrame();
    expect(draws.get('can.trace:ch1')).toBe(1);
  });

  it('resize is coalesced to one call per animation frame', () => {
    const { dock: d, runRaf } = mountDock();
    d.load(slotLayout({ panel: 'uart.terminal', props: { channel: 'dut' } }));
    runRaf();
    resizes.length = 0;
    for (let i = 0; i < 5; i++) d.api.layout(800 + i, 600, true);
    runRaf();
    expect(resizes.length).toBeLessThanOrEqual(1);
  });

  it('an unknown panel id renders the "이 버전에 없는 패널" placeholder (and is kept in the snapshot)', () => {
    const { dock: d } = mountDock();
    d.load({ loom_layout: 1, name: 'n', env_hint: null, pinned: [], root: { split: 'h', children: [{ panel: 'viz.plot', props: { signals: 'a' } }, { panel: 'can.trace', props: { channel: 'ch0' } }] } });
    expect(hostEl.querySelector('[data-testid="panel-unknown"]')!.textContent).toContain('이 버전에 없는 패널: viz.plot');
    expect(leaves(d.snapshot().root).map((x) => x.panel)).toEqual(['viz.plot', 'can.trace']);
  });

  it('closing a tab disposes that panel only', () => {
    const { dock: d } = mountDock();
    d.load(presetToLayout(PRESETS[0] as Preset));
    d.find({ panel: 'can.trace', props: { channel: 'ch0' } })!.api.close();
    expect(log.slice(3)).toEqual(['dispose can.trace:ch0']);
  });

  it('new channel (auto_open on) → a dock tab next to the same domain; already open → activated, no duplicate', () => {
    const { dock: d, net, services } = mountDock();
    d.load(presetToLayout(PRESETS[0] as Preset));
    net.emitControl(ev({ kind: 'uart.channel_state', domain: 'uart', channel: 'dut2', origin: 'agent', payload: { state: 'opened' } }));
    const p = d.find({ panel: 'uart.terminal', props: { channel: 'dut2' } })!;
    expect(p).not.toBeNull();
    expect(p.group).toBe(d.find({ panel: 'uart.terminal', props: { channel: 'dut' } })!.group); // tab beside uart dut
    expect(services.toasts.items.at(-1)?.text).toBe('에이전트가 uart dut2를 열었습니다 — 패널을 열었습니다');
    expect(services.showSlot({ panel: 'uart.terminal', props: { channel: 'dut2' } })).toBe('shown');
    expect(d.api.panels).toHaveLength(4);
  });

  it('keeps background multi-domain arrivals in one readable tab group before any group is active', () => {
    const { dock: d } = mountDock();
    d.clear('m1-default');
    for (const panel of ['uart.terminal', 'can.trace', 'uds.console', 'eth.packets', 'psu.gauge']) {
      d.open({ panel, props: panel === 'uds.console' ? { targets: 'real-ecu' } : { channel: `real-${panel.split('.')[0]}` } }, { activate: false });
    }
    expect(d.api.panels).toHaveLength(5);
    expect(d.api.groups).toHaveLength(1);
    expect(d.api.groups[0]!.panels).toHaveLength(5);
  });

  it('keeps more than four added interfaces, deduplicates views and persists stable identities/titles', () => {
    const { dock: d, services } = mountDock();
    d.load(slotLayout({ panel: 'uart.terminal', props: { channel: 'base' } }));
    const slots: Slot[] = [
      ...Array.from({ length: 6 }, (_, i) => ({ panel: 'uart.terminal', props: { channel: `dut-${i}` } })),
      { panel: 'can.trace', props: { channel: 'bus-2' } },
      { panel: 'uds.console', props: { targets: 'ecu-2' } },
      { panel: 'eth.packets', props: { channel: 'lan-2' } },
      { panel: 'psu.gauge', props: { channel: 'supply-2' } },
    ];
    for (const slot of slots) expect(services.showSlot(slot)).toBe('added');
    const ids = slots.map((slot) => d.find(slot)!.id);
    for (const slot of slots) {
      expect(services.showSlot({ ...slot, props: { ...slot.props, at: 'seq:20' } })).toBe('shown');
      expect(d.api.activePanel).toBe(d.find(slot));
      expect(d.find(slot)!.title).toContain(String(slot.props.channel ?? slot.props.targets));
    }
    expect(services.showSlot({ panel: 'can.trace', props: { ch: 'bus-2' } })).toBe('shown');
    expect(d.api.panels).toHaveLength(11);
    expect(services.uiState.extraSlots).toEqual([]); // mounted dock bypasses the four-slot fallback
    const saved = d.snapshot('interfaces');
    const parsed = parseLayout(serializeLayout(saved).ok ? JSON.stringify(saved) : '').layout!;
    expect(parsed).not.toBeNull();
    d.load(parsed);
    expect(slots.map((slot) => d.find(slot)!.id)).toEqual(ids);
    const removedId = d.find(slots[0]!)!.id;
    d.find(slots[0]!)!.api.close();
    d.open(slots[0]!);
    expect(d.find(slots[0]!)!.id).toBe(removedId);
    expect(d.api.panels).toHaveLength(11);
  });

  it('focus request in a dock: activate the open tab and reveal (no route change)', () => {
    const { dock: d, net, services } = mountDock();
    d.load({ loom_layout: 1, name: 't', env_hint: null, pinned: [], root: { tabs: [{ panel: 'can.trace', props: { channel: 'ch0' } }, { panel: 'uds.console', props: { targets: 'bcm' } }], active: 1 } });
    const seen: string[] = [];
    services.uiHooks.onReveal?.((r) => {
      seen.push(`${r.domain}:${r.channel}@${r.seqA}`);
      return true;
    });
    net.emitControl(ev({ kind: 'ui.focus_request', origin: 'agent', payload: { panel: 'can.trace', props: { channel: 'ch0' }, at: 'seq:5012', actor: 'claude-1' } }));
    services.focus.accept();
    expect(d.api.activePanel?.id).toBe(d.find({ panel: 'can.trace', props: { channel: 'ch0' } })!.id);
    expect(seen).toEqual(['can:ch0@5012']);
  });
});
