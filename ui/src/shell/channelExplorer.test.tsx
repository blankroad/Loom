// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { viewsForChannel } from '../store/channels';
import { TabSettings } from '../store/tabSettings';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { ChannelBar } from './ChannelBar';
import { sameSlot, slotHash, parseRoute, type Slot } from './routes';
import { createShellServices } from './services';
import { ShellCtx } from './ShellContext';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
let unbind: () => void;
beforeEach(() => { host = document.createElement('div'); document.body.append(host); root = createRoot(host); });
afterEach(() => { act(() => root.unmount()); unbind(); host.remove(); });
function mount() {
  const net = new FakeNet(); net.open(TEST_HELLO);
  const settings = new TabSettings({ getItem: () => null, setItem: () => {} }); settings.setAutoOpen('off');
  const services = createShellServices(net, { autoTick: false, settings, now: () => 1000 });
  unbind = services.bind();
  act(() => root.render(<ShellCtx.Provider value={services}><ChannelBar /></ShellCtx.Provider>));
  const add = (domain: string, channel: string, state = 'opened') => act(() => net.emitControl(ev({ kind: `${domain}.channel_state`, domain, channel, payload: { state } })));
  const search = (value: string) => act(() => {
    const input = host.querySelector('input[type="search"]')!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  return { services, net, add, search };
}
const channel = (key: string) => host.querySelector<HTMLElement>(`[data-chan="${key}"]`);

describe('channel explorer', () => {
  it('renders exactly the active domains across all 31 combinations, including closes and reopen', () => {
    const { add, services, net } = mount();
    const domains = ['uart', 'can', 'uds', 'eth', 'psu'];
    for (let mask = 1; mask < 32; mask++) {
      for (const [index, domain] of domains.entries()) add(domain, `${domain}-a`, mask & (1 << index) ? 'open' : 'closed');
      const selected = domains.filter((_, index) => mask & (1 << index));
      expect([...host.querySelectorAll('section')].map((section) => section.getAttribute('aria-label'))).toEqual(selected.map((domain) => `${domain.toUpperCase()} 채널`));
      expect([...host.querySelectorAll('[data-chan]')].map((node) => node.getAttribute('data-chan'))).toEqual(selected.map((domain) => `${domain}/${domain}-a`));
      expect(host.querySelector('[aria-label="연결된 인터페이스 수"]')?.textContent).toBe(String(selected.length));
    }
    for (const domain of domains) add(domain, `${domain}-a`, 'closed');
    expect(host.querySelectorAll('[data-chan]')).toHaveLength(0);
    expect(services.shell.channels.size).toBe(5);
    expect(net.calls).toEqual([]);
  });

  it('shows only active channels by default and preserves inactive state behind disclosure', () => {
    const { add, net, services } = mount();
    expect(host.textContent).toContain('채널 없음');
    add('uart', 'dut-b'); add('can', 'dut-b', 'closed'); add('uart', 'dut-a'); act(() => services.shell.noteSnapshotItems([{ domain: 'custom', channel: 'probe', state: {} }]));
    expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['uart/dut-a', 'uart/dut-b']);
    expect(host.querySelector('[aria-label="연결된 인터페이스 수"]')?.textContent).toBe('2');
    expect(services.shell.channels.size).toBe(4);
    act(() => host.querySelector<HTMLButtonElement>('.explorer-inactive-toggle')!.click());
    expect([...host.querySelectorAll('section')].map((s) => s.getAttribute('aria-label'))).toEqual(['UART 채널', 'CAN 채널', 'CUSTOM 채널']);
    expect([...host.querySelectorAll('[data-chan]')].map((e) => e.getAttribute('data-chan'))).toEqual(['uart/dut-a', 'uart/dut-b', 'can/dut-b', 'custom/probe']);
    expect(channel('can/dut-b')!.textContent).toContain('(닫힘)');
    expect(channel('custom/probe')!.textContent).toContain('상태 미확인');
    expect(host.textContent).toContain('이 버전에 지원되는 보기 없음');
    expect(net.calls).toEqual([]);
  });

  it('searches across domain, identity and state, reveals collapsed matches and clears an empty result', () => {
    const { add, search } = mount(); add('uart', 'dut-a'); add('uart', 'dut-b', 'closed'); add('can', 'dut-a');
    act(() => host.querySelector<HTMLButtonElement>('.explorer-inactive-toggle')!.click());
    act(() => host.querySelector<HTMLButtonElement>('.explorer-group-toggle')!.click());
    expect(channel('uart/dut-a')).toBeNull();
    search('UART dut-b closed');
    expect(channel('uart/dut-b')).not.toBeNull(); expect(channel('can/dut-a')).toBeNull();
    search('missing'); expect(host.querySelector('[role="status"]')!.textContent).toContain('검색 결과 없음');
    act(() => host.querySelector<HTMLButtonElement>('.explorer-empty button')!.click());
    expect(channel('can/dut-a')).not.toBeNull();
    // Original collapsed preference survives clearing search.
    expect(channel('uart/dut-b')).toBeNull();
  });

  it('opens channel-specific implemented views without connecting, writing, or navigating', () => {
    const { add, services, net } = mount();
    const opened: Slot[] = [];
    services.attachDock({ find: () => null, open: (slot) => { opened.push(slot); return 'added'; } });
    for (const domain of ['uart', 'can', 'uds', 'eth', 'psu']) add(domain, 'bench-2');
    for (const button of host.querySelectorAll<HTMLButtonElement>('[data-view]')) act(() => button.click());
    const expected: Slot[] = ['uart', 'can', 'uds', 'eth', 'psu'].flatMap((d) => viewsForChannel(d, 'bench-2').map(({ panel, props }) => ({ panel, props })));
    expect(opened).toEqual(expected);
    for (const button of host.querySelectorAll<HTMLButtonElement>('[data-view]')) act(() => button.click());
    expect(opened).toEqual([...expected, ...expected]);
    expect(net.calls).toEqual([]);
  });
});

describe('interface view route identity', () => {
  it('retains deep links and distinguishes channel and view while normalizing CAN aliases and cursor', () => {
    for (const domain of ['uart', 'can', 'uds', 'eth', 'psu']) for (const { panel, props } of viewsForChannel(domain, 'bench-2')) {
      const slot = { panel, props };
      expect(parseRoute(slotHash(slot))).toEqual({ kind: 'panel', slot });
      expect(sameSlot(slot, { panel, props: { ...props, at: 'seq:12' } })).toBe(true);
    }
    expect(sameSlot({ panel: 'can.trace', props: { ch: 'bus' } }, { panel: 'can.trace', props: { channel: 'bus' } })).toBe(true);
    expect(sameSlot({ panel: 'uart.trace', props: { channel: 'a' } }, { panel: 'uart.terminal', props: { channel: 'a' } })).toBe(false);
    expect(sameSlot({ panel: 'uart.terminal', props: { channel: 'a' } }, { panel: 'uart.terminal', props: { channel: 'b' } })).toBe(false);
  });
});
