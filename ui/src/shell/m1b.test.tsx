// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { HelloResult, LoomEvent } from '../proto/provisional';
import { MERGE_MS } from '../store/focus';
import { JOIN_GRACE_MS } from '../store/shellControl';
import { TabSettings, type KV } from '../store/tabSettings';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { Banners, Toasts } from './Banners';
import { BOOTSTRAP_PATH, NEED_AUTH_TEXT, readBootFragment, RETURN_KEY, routeAfterBoot, runBootstrap, type BootDeps } from './bootstrap';
import { ChannelBar } from './ChannelBar';
import { FocusPrompt } from './FocusPrompt';
import { parseRoute } from './routes';
import { createShellServices, type ServiceOptions } from './services';
import { NeedAuth } from './ShellApp';
import { ShellCtx } from './ShellContext';
import { StopButton } from './TopBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CODE = 'Zk3q9V_x-71aa0c4e5b6d8f20';

// ---------------------------------------------------------------- bootstrap
function bootDeps(hash: string, fetchImpl: BootDeps['fetch'], session: Record<string, string> = {}) {
  const loc = { hash, pathname: '/ui/', search: '' };
  const urls: (string | undefined)[] = [];
  const order: string[] = [];
  const d: BootDeps = {
    location: loc,
    history: {
      replaceState: (_d, _u, url) => {
        order.push('replace');
        urls.push(url);
        loc.hash = '';
      },
    },
    fetch: (u, i) => {
      order.push(`fetch:${loc.hash === '' ? 'stripped' : 'NOT-stripped'}`);
      return fetchImpl(u, i);
    },
    session: { getItem: (k) => session[k] ?? null, removeItem: (k) => void delete session[k] },
  };
  return { d, urls, order };
}

describe('bootstrap exchange (contract 01 §1.3)', () => {
  it('strips the fragment before the POST, posts {code}, returns to the deep link', async () => {
    const seen: { url: string; init: RequestInit }[] = [];
    const next = encodeURIComponent('/p/can/trace?channel=ch0');
    const { d, urls, order } = bootDeps(`#boot=${CODE}&next=${next}`, async (url, init) => {
      seen.push({ url, init });
      return { ok: true, status: 204 };
    });
    const r = await runBootstrap(d);
    expect(order).toEqual(['replace', 'fetch:stripped']);
    expect(urls).toEqual(['/ui/']); // no fragment, no secret in the address bar or history entry
    expect(seen[0]!.url).toBe(BOOTSTRAP_PATH);
    expect(seen[0]!.init).toMatchObject({ method: 'POST', credentials: 'same-origin', cache: 'no-store', referrerPolicy: 'no-referrer' });
    expect(JSON.parse(seen[0]!.init.body as string)).toEqual({ code: CODE });
    expect(r).toEqual({ kind: 'ok', next: '/p/can/trace?channel=ch0' });
    const hashes: string[] = [];
    routeAfterBoot(r, (h) => hashes.push(h));
    expect(hashes).toEqual(['#/p/can/trace?channel=ch0']);
  });

  it('without next: the deep link remembered before #/need-auth, else the default layout', async () => {
    const ok = async () => ({ ok: true, status: 200 });
    const s = { [RETURN_KEY]: '/l/can-uds' };
    expect(await runBootstrap(bootDeps(`#boot=${CODE}`, ok, s).d)).toEqual({ kind: 'ok', next: '/l/can-uds' });
    expect(s[RETURN_KEY]).toBeUndefined(); // used once
    expect(await runBootstrap(bootDeps(`#boot=${CODE}`, ok).d)).toEqual({ kind: 'ok', next: '/l/m1-default' });
  });

  it('expired / reused (4xx) → #/need-auth?reason=expired; the code appears nowhere in the result', async () => {
    const { d, urls } = bootDeps(`#boot=${CODE}`, async () => ({ ok: false, status: 410 }));
    const r = await runBootstrap(d);
    expect(r).toEqual({ kind: 'failed', reason: 'expired' });
    expect(urls).toEqual(['/ui/']); // stripped on failure too
    const hashes: string[] = [];
    routeAfterBoot(r, (h) => hashes.push(h));
    expect(hashes).toEqual(['#/need-auth?reason=expired']);
    expect(JSON.stringify([r, hashes])).not.toContain(CODE);
    expect(parseRoute(hashes[0]!)).toEqual({ kind: 'need-auth', reason: 'expired' });
  });

  it('network error → reason network; malformed code → no request at all', async () => {
    expect(await runBootstrap(bootDeps(`#boot=${CODE}`, () => Promise.reject(new Error('down'))).d)).toEqual({ kind: 'failed', reason: 'network' });
    const f = vi.fn();
    const { d, urls } = bootDeps('#boot=short<script>', f);
    expect(await runBootstrap(d)).toEqual({ kind: 'failed', reason: 'malformed' });
    expect(f).not.toHaveBeenCalled();
    expect(urls).toEqual(['/ui/']);
  });

  it('no fragment / ordinary routes → nothing happens; next must be an app route', async () => {
    const f = vi.fn();
    expect(await runBootstrap(bootDeps('#/l/eth', f).d)).toEqual({ kind: 'none' });
    expect(f).not.toHaveBeenCalled();
    expect(readBootFragment(`#boot=${CODE}&next=${encodeURIComponent('https://evil.example/')}`)?.next).toBeNull();
  });
});

// ---------------------------------------------------------------- shell rendering helpers
let host: HTMLDivElement;
let root: Root;
let unbind: (() => void) | null = null;
beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  unbind?.();
  unbind = null;
  host.remove();
  vi.useRealTimers();
});

const memKV = (): KV => {
  const m = new Map<string, string>();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};

function mount(o: Partial<ServiceOptions> & { hello?: Partial<HelloResult>; hash?: string } = {}) {
  let now = 1_000;
  let hash = o.hash ?? '#/l/m1-default';
  const navs: string[] = [];
  const net = new FakeNet();
  net.open({ ...TEST_HELLO, ...o.hello });
  const settings = o.settings ?? new TabSettings(memKV());
  let busy = false;
  const services = createShellServices(net, {
    autoTick: false,
    now: () => now,
    settings,
    automation: o.automation ?? false,
    inputBusy: () => busy,
    navigate: (h) => {
      navs.push(h);
      hash = h;
    },
    currentHash: () => hash,
    setTimeout: (cb) => {
      cb();
      return 0;
    },
  });
  unbind = services.bind();
  act(() =>
    root.render(
      <ShellCtx.Provider value={services}>
        <Banners />
        <ChannelBar />
        <StopButton />
        <FocusPrompt />
        <Toasts />
      </ShellCtx.Provider>,
    ),
  );
  const emit = (e: LoomEvent) => act(() => net.emitControl(e));
  return {
    net,
    services,
    settings,
    navs,
    emit,
    setBusy: (b: boolean) => (busy = b),
    advance: (ms: number) => (now += ms),
  };
}
const toastTexts = () => [...host.querySelectorAll('.toast')].map((t) => t.textContent);
const chip = (key: string) => host.querySelector<HTMLElement>(`[data-chan="${key}"]`);
const opened = (domain: string, channel: string, extra: Partial<LoomEvent> = {}) =>
  ev({ kind: `${domain}.channel_state`, domain, channel, origin: 'agent', actor_id: 'claude-1', payload: { state: 'opened' }, ...extra });

describe('new channel rule (UD-31)', () => {
  it('auto_open on: bar + toast + the panel opened beside the layout', () => {
    const { emit, services } = mount();
    emit(opened('uart', 'dut2'));
    expect(chip('uart/dut2')!.dataset.state).toBe('open');
    expect(chip('uart/dut2')!.className).toContain('chan-new');
    expect(toastTexts()).toEqual(['에이전트가 uart dut2를 열었습니다 — 패널을 열었습니다']);
    expect(services.uiState.extraSlots).toEqual([{ panel: 'uart.terminal', props: { channel: 'dut2' } }]);
  });

  it('auto_open off: bar still shows it; toast offers [패널 열기]', () => {
    const settings = new TabSettings(memKV());
    settings.setAutoOpen('off');
    const { emit, services } = mount({ settings });
    emit(opened('can', 'ch3'));
    expect(chip('can/ch3')).not.toBeNull();
    expect(services.uiState.extraSlots).toEqual([]);
    expect(toastTexts()).toEqual(['에이전트가 can ch3를 열었습니다패널 열기']);
    act(() => host.querySelector<HTMLButtonElement>('.toast button')!.click());
    expect(services.uiState.extraSlots).toEqual([{ panel: 'can.trace', props: { channel: 'ch3' } }]);
    expect(toastTexts()).toEqual([]);
  });

  it('a channel already in the layout is not opened twice; uds.state target / eth.capture_state count too', () => {
    const { emit, services, advance } = mount();
    emit(opened('uart', 'dut')); // m1-default already has uart.terminal dut
    expect(services.uiState.extraSlots).toEqual([]);
    expect(toastTexts()).toEqual(['에이전트가 uart dut를 열었습니다']);
    advance(JOIN_GRACE_MS);
    emit(ev({ kind: 'uds.state', domain: 'uds', origin: 'agent', payload: { target: 'ecm', link: 'up' } }));
    expect(chip('uds/ecm')).not.toBeNull();
    emit(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan2', origin: 'agent', payload: { state: 'running' } }));
    expect(services.uiState.extraSlots.map((s) => s.panel)).toEqual(['uds.console', 'eth.packets']);
  });

  it('close and generation change are reflected; re-open toasts again; repeated state reports do not', () => {
    const { emit } = mount();
    emit(opened('uart', 'dut2'));
    emit(opened('uart', 'dut2')); // still open → no new toast
    expect(toastTexts()).toHaveLength(1);
    emit(ev({ kind: 'uart.channel_state', domain: 'uart', channel: 'dut2', origin: 'agent', generation: 1, payload: { state: 'closed' } }));
    expect(chip('uart/dut2')!.dataset.state).toBe('closed');
    expect(chip('uart/dut2')!.textContent).toContain('(닫힘)');
    emit(opened('uart', 'dut2', { generation: 2 }));
    expect(chip('uart/dut2')!.textContent).toContain('gen2 ↻');
    expect(toastTexts()[0]).toBe('에이전트가 uart dut2를 다시 열었습니다');
  });

  it('join grace: implicit state reports right after connecting are bar-only (they predate the tab)', () => {
    const { emit, services, advance } = mount();
    emit(ev({ kind: 'psu.state', domain: 'psu', channel: 'bench', origin: 'device', payload: { set_v: 13.5, out: true } }));
    expect(chip('psu/bench')).not.toBeNull();
    expect(toastTexts()).toEqual([]);
    expect(services.uiState.extraSlots).toEqual([]);
    advance(JOIN_GRACE_MS);
    emit(ev({ kind: 'psu.state', domain: 'psu', channel: 'aux', origin: 'agent', payload: { out: false } }));
    expect(toastTexts()).toEqual([]); // output state alone does not confirm a connection
    expect(chip('psu/aux')!.dataset.state).toBe('unknown');
    emit(ev({ kind: 'psu.state', domain: 'psu', channel: 'aux', origin: 'agent', payload: { connected: true, out: false } }));
    expect(toastTexts()).toEqual(['에이전트가 psu aux를 다시 열었습니다 — 패널을 열었습니다']);
  });

  it('late join: channels from snapshot items appear silently', () => {
    const { services } = mount();
    act(() => services.shell.noteSnapshotItems([{ domain: 'uart', channel: 'dut3', kind: 'uart.snapshot', state: { state: 'open' } }, { domain: 'test', state: {} }]));
    expect(chip('uart/dut3')!.dataset.state).toBe('open');
    expect(toastTexts()).toEqual([]);
  });

  it('auto_open setting persists per browser profile', () => {
    const kv = memKV();
    new TabSettings(kv).setAutoOpen('off');
    expect(new TabSettings(kv).autoOpenPanels).toBe('off');
    const broken: KV = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    const s = new TabSettings(broken);
    s.setFollow('auto'); // storage failure → still works for this page
    expect(s.followAgent).toBe('auto');
  });
});

// ---------------------------------------------------------------- focus
const focusEv = (o: Record<string, unknown> = {}) =>
  ev({ kind: 'ui.focus_request', origin: 'agent', payload: { panel: 'can.trace', props: { channel: 'ch0' }, at: 'seq:5012', reason: 'PowerMode 전이', actor: 'claude-1', ...o } });

describe('agent focus request (가리키기)', () => {
  it('ask (default): prompt text; [열기] routes to the deep link and reveals the seq', () => {
    const { emit, navs, services } = mount({ hash: '#/l/uart-only' });
    const reveals: string[] = [];
    services.uiHooks.onReveal?.((r) => {
      reveals.push(`${r.domain}:${r.channel}@${r.seqA}`);
      return true;
    });
    emit(focusEv());
    const p = host.querySelector('[data-testid="focus-prompt"]')!;
    expect(p.textContent).toContain('에이전트 claude-1이 보여 주려 함 · CAN ch0 Trace · seq 5012로 이동 · 이유 PowerMode 전이');
    expect(navs).toEqual([]); // nothing moves without the user
    act(() => [...p.querySelectorAll('button')].find((b) => b.textContent === '열기 ⏎')!.click());
    expect(navs).toEqual(['#/p/can/trace?channel=ch0&at=seq%3A5012']);
    expect(reveals).toEqual(['can:ch0@5012']);
    expect(host.querySelector('[data-testid="focus-prompt"]')).toBeNull();
  });

  it('Enter opens, Esc ignores (when not typing in an input)', () => {
    const { emit, navs, services } = mount({ hash: '#/l/uart-only' });
    emit(focusEv());
    act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })));
    expect(services.focus.pending).toBeNull();
    expect(services.focus.ignored).toBe(1);
    emit(focusEv({ at: 'seq:6000' }));
    act(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' })));
    expect(navs).toEqual(['#/p/can/trace?channel=ch0&at=seq%3A6000']);
  });

  it('panel already on screen → no navigation, just reveal', () => {
    const { emit, navs, services } = mount(); // m1-default has can.trace ch0
    const reveals: string[] = [];
    services.uiHooks.onReveal?.((r) => {
      reveals.push(String(r.seqA));
      return true;
    });
    emit(focusEv());
    act(() => services.focus.accept());
    expect(navs).toEqual([]);
    expect(reveals).toEqual(['5012']);
  });

  it('auto: opens at once; while typing it is held and opens when input goes idle', () => {
    const settings = new TabSettings(memKV());
    settings.setFollow('auto');
    const { emit, navs, services, setBusy } = mount({ settings, hash: '#/l/uart-only' });
    emit(focusEv());
    expect(navs).toHaveLength(1);
    expect(host.querySelector('[data-testid="focus-auto"]')!.textContent).toContain('따라 이동함');
    setBusy(true);
    emit(focusEv({ at: 'seq:7000' }));
    expect(navs).toHaveLength(1);
    expect(host.querySelector('[data-testid="focus-prompt"]')!.textContent).toContain('입력 중이라 자동 따라가기를 보류함');
    act(() => services.focus.retryHeld());
    expect(navs).toHaveLength(1);
    const reveals: string[] = [];
    services.uiHooks.onReveal?.((r) => {
      reveals.push(String(r.seqA));
      return true;
    });
    setBusy(false);
    act(() => services.focus.retryHeld());
    // the CAN panel is already on screen after the first follow → reveal only, no second route
    expect(navs).toEqual(['#/p/can/trace?channel=ch0&at=seq%3A5012']);
    expect(reveals).toEqual(['7000']);
    expect(services.focus.pending).toBeNull();
  });

  it('off: ignored (counted), nothing shown', () => {
    const settings = new TabSettings(memKV());
    settings.setFollow('off');
    const { emit, navs, services } = mount({ settings });
    emit(focusEv());
    expect(host.querySelector('[data-testid="focus-prompt"]')).toBeNull();
    expect(navs).toEqual([]);
    expect(services.focus.ignored).toBe(1);
  });

  it(`duplicates within ${MERGE_MS} ms are merged; later ones prompt again`, () => {
    const { emit, services, advance } = mount({ hash: '#/l/uart-only' });
    emit(focusEv());
    advance(500);
    emit(focusEv());
    expect(host.querySelector('[data-testid="focus-prompt"]')!.textContent).toContain('(×2)');
    act(() => services.focus.accept());
    advance(500);
    emit(focusEv()); // same request right after opening → merged, no new prompt
    expect(services.focus.pending).toBeNull();
    advance(MERGE_MS);
    emit(focusEv());
    expect(services.focus.pending?.count).toBe(1);
  });

  it('[이 탭은 자동으로 따라가기] switches the tab to auto and opens; invalid payloads are ignored', () => {
    const { emit, settings, navs, services } = mount({ hash: '#/l/uart-only' });
    emit(focusEv({ panel: 'not a panel' }));
    expect(services.focus.pending).toBeNull();
    emit(focusEv());
    act(() => host.querySelector<HTMLInputElement>('[data-testid="focus-prompt"] input[type="checkbox"]')!.click());
    expect(settings.followAgent).toBe('auto');
    expect(navs).toHaveLength(1);
  });
});

// ---------------------------------------------------------------- automation browser (V5)
describe('automation browser (navigator.webdriver)', () => {
  it('real daemon: band "보기 전용", rights capped to watch (writes off), STOP stays enabled', async () => {
    const { services, net } = mount({ automation: true });
    expect(host.querySelector('[data-banner="automation"]')!.textContent).toContain('자동화 브라우저 — 보기 전용');
    expect([...services.shell.rights]).toEqual(['watch']);
    expect(services.shell.can('operate')).toBe(false);
    await expect(services.gate.run({ id: 'send', rpc: 'uart.write', class: 'request', right: 'operate', confirm: 'none' }, {})).rejects.toThrow();
    expect(net.calls.map((c) => c.method)).not.toContain('uart.write');
    expect(host.querySelector<HTMLButtonElement>('[data-testid="stop-button"]')!.disabled).toBe(false);
  });

  it('dev mock server (capabilities mock): band says writes are allowed here only', () => {
    const { services } = mount({ automation: true, hello: { capabilities: ['mock'] } });
    expect(host.querySelector('[data-banner="automation"]')!.textContent).toContain('mock 서버라 쓰기 허용');
    expect(services.shell.can('operate')).toBe(true);
  });

  it('ordinary browser: no band, rights as granted', () => {
    const { services } = mount();
    expect(host.querySelector('[data-banner="automation"]')).toBeNull();
    expect(services.shell.can('operate')).toBe(true);
  });
});

// ---------------------------------------------------------------- need-auth page
describe('#/need-auth', () => {
  it('shows the reason sentence, never a code', () => {
    const net = new FakeNet();
    const services = createShellServices(net, { autoTick: false, settings: new TabSettings(memKV()) });
    act(() =>
      root.render(
        <ShellCtx.Provider value={services}>
          <NeedAuth reason="expired" />
        </ShellCtx.Provider>,
      ),
    );
    expect(host.querySelector('[data-testid="need-auth-reason"]')!.textContent).toBe(NEED_AUTH_TEXT.expired);
    expect(host.textContent).toContain('loom ui');
    expect(host.innerHTML).not.toContain(CODE);
  });
});
