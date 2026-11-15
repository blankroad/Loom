// @vitest-environment jsdom
import { act, StrictMode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { DEFAULT_ROUTE_KEY, routeAfterBoot, runBootstrap } from './bootstrap';
import { ShellApp } from './ShellApp';
import { createShellServices } from './services';

// Test shell behavior with the dock mounted, without depending on browser geometry.
vi.mock('./dock/DockLayout', () => ({
  DockLayout: () => <main data-testid="layout" />,
  LayoutBar: () => <span>레이아웃 도구</span>,
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  localStorage.clear();
  location.hash = '#/l/m1-default';
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});
function mount(rights: 'watch' | 'operate' = 'operate') {
  const net = new FakeNet();
  net.open({ ...TEST_HELLO, rights });
  const services = createShellServices(net, { autoTick: false });
  act(() => root.render(<ShellApp services={services} />));
  return { net, services };
}
const find = <T extends Element = HTMLElement>(selector: string) => host.querySelector<T>(selector)!;

describe('developer workbench shell', () => {
  it('keeps the dock attached under a useful default welcome, with explorer beside main', () => {
    const { net } = mount();
    // Bench Light frame: rail | toolbar / side (interface explorer) | work (dock)
    expect(find('.bl-body > main.mod-uart > aside.side .channel-bar')).toBeTruthy();
    expect(find('.mod-uart > .mtbar .channel-bar')).toBeNull();
    expect(find('.bl-body > main.mod-uart > section.work [data-testid="layout"]')).toBeTruthy();
    expect(find('[data-shell="header"] .svcnav')).toBeTruthy();
    expect(find('[data-shell="status"]')).toBeTruthy();
    expect(find('[data-testid="workbench-welcome"]').textContent).toContain('인터페이스 추가');
    expect(find('.workspace-dock').hasAttribute('inert')).toBe(true);
    expect(net.calls.some((c) => /(?:open|connect|start)$/.test(c.method))).toBe(false);
  });

  it('reveals the attached dock as soon as the first known interface arrives', () => {
    const { net } = mount();
    expect(find('[data-testid="workbench-welcome"]')).toBeTruthy();
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel: 'serial-a', payload: { state: 'opened' } })));
    expect(find('[data-testid="workbench-welcome"]')).toBeNull();
    expect(find('.workspace-dock').hasAttribute('inert')).toBe(false);
    expect(find('[data-chan="uart/serial-a"]')).toBeTruthy();
  });

  it('returns to the welcome when the last active interface closes without losing history', () => {
    const { net, services } = mount();
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel: 'serial-a', payload: { state: 'opened' } })));
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel: 'serial-a', payload: { state: 'closed' } })));
    expect(find('[data-testid="workbench-welcome"]')).toBeTruthy();
    expect(find('[data-chan="uart/serial-a"]')).toBeNull();
    expect(services.shell.channels.get('uart/serial-a')?.state).toBe('closed');
    expect(find('.explorer-inactive-toggle').textContent).toContain('1개');
    act(() => net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan-b', payload: { state: 'live' } })));
    expect(find('[data-testid="workbench-welcome"]')).toBeNull();
    expect([...host.querySelectorAll('[data-chan]')].map((node) => node.getAttribute('data-chan'))).toEqual(['eth/lan-b']);
  });

  it('uses the REV6.1 module frame per route: UART chrome on service and deep links, bare frame for ETH/UDS', () => {
    const { net } = mount();
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel: 'serial-a', payload: { state: 'opened' } })));
    act(() => { location.hash = '#/p/uart/terminal?channel=serial-a'; window.dispatchEvent(new HashChangeEvent('hashchange')); });
    // an agent deep link keeps the UART workbench chrome (rail 세션/매크로, module toolbar, saved sessions)
    expect(find('main.mod-uart').dataset.module).toBe('uart');
    expect(find('#railSessions')).toBeTruthy();
    expect(find('#railMacros')).toBeTruthy();
    expect([...host.querySelectorAll('.mod-uart > .mtbar .tbtn')].map((b) => b.textContent)).toEqual(['세션 목록 접기', '새 세션', '오른쪽 분할', '아래 분할', '최대화', 'pane 닫기', '새 매크로', '내보내기', '가져오기', '레이아웃 초기화', '단축키']);
    expect(find('.mod-uart > .side .side-head h2').textContent).toBe('저장된 UART 세션');
    act(() => { location.hash = '#/s/uds'; window.dispatchEvent(new HashChangeEvent('hashchange')); });
    // ETH/UDS render their own module toolbar inside the dock: no outer rail/side/toolbar, same Dockview element
    expect(find('main.mod-uart').classList.contains('mod-bare')).toBe(true);
    expect(find('.mod-uart > .rail').hidden).toBe(true);
    expect(find('.mod-uart > .side').hidden).toBe(true);
    expect(find('.mod-uart > .mtbar').hidden).toBe(true);
    expect(net.calls.some((c) => /(?:open|connect|start)$/.test(c.method))).toBe(false);
  });

  it('lands a default (non-explicit) start with UART as the only live service on the UART workbench', () => {
    location.hash = '';
    const { net } = mount();
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel: 'serial-a', payload: { state: 'opened' } })));
    expect(location.hash).toBe('#/s/uart');
  });

  it('never overrides an explicit integrated route', () => {
    location.hash = '#/l/m1-default';
    sessionStorage.clear();
    const { net } = mount();
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel: 'serial-a', payload: { state: 'opened' } })));
    expect(location.hash).toBe('#/l/m1-default');
  });

  it('describes automation rights as daemon policy on the auth screen', () => {
    location.hash = '#/need-auth';
    mount();
    expect(find('[data-testid="need-auth"]').textContent).toContain('데몬 정책에 따라');
    expect(find('[data-testid="need-auth"]').textContent).not.toContain('보기(watch) 자격만');
  });

  it('does not cover a saved layout bearing the default name', () => {
    localStorage.setItem('loom.layouts.index', JSON.stringify(['m1-default']));
    mount();
    expect(find('[data-testid="workbench-welcome"]')).toBeNull();
  });

  it('does not cover explicit deep links or nondefault saved layouts', () => {
    location.hash = '#/p/uart/terminal?channel=my-device';
    mount();
    expect(find('[data-testid="workbench-welcome"]')).toBeNull();
    act(() => {
      location.hash = '#/l/my-layout';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    expect(find('[data-testid="workbench-welcome"]')).toBeNull();
    expect(find('.workspace-dock').hasAttribute('inert')).toBe(false);
  });

  it('shows truthful connection, rights, REC and watch STOP; preserves HTTP fallback', async () => {
    const { net } = mount('watch');
    expect(find('[data-testid="rights"]').textContent).toContain('watch');
    expect(find('[data-testid="connection-status"]').textContent).toContain('연결됨');
    expect(find('[data-shell="header"] .rec').textContent).toContain('REC');
    const stop = find<HTMLButtonElement>('[data-testid="stop-button"]');
    expect(stop.disabled).toBe(false);
    await act(async () => stop.click());
    expect(net.calls.some((c) => c.method === 'core.stop')).toBe(true);
    act(() => net.setState({ status: 'closed', reason: 'transport lost' }));
    expect(find('[data-testid="connection-status"]').textContent).toContain('연결 끊김');
    await act(async () => stop.click());
    expect(net.httpStops).toBe(1);
    act(() => net.emitControl(ev({ kind: 'core.latch', payload: { on: true, missing: [] } })));
    expect(find('.stop-panel')).toBeTruthy();
    expect(find('[data-testid="stop-button"]').getAttribute('data-phase')).toBe('locked');
  });

  it('discloses working tab settings and expands activity without opening any interface', () => {
    const { services, net } = mount();
    const details = find<HTMLDetailsElement>('.tab-settings');
    expect(details.open).toBe(false);
    act(() => find<HTMLElement>('.tab-settings summary').click());
    expect(details.open).toBe(true);
    const follow = find<HTMLSelectElement>('select[aria-label="follow_agent"]');
    act(() => { follow.value = 'off'; follow.dispatchEvent(new Event('change', { bubbles: true })); });
    expect(services.settings.followAgent).toBe('off');
    const auto = find<HTMLSelectElement>('select[aria-label="auto_open_panels"]');
    act(() => { auto.value = 'off'; auto.dispatchEvent(new Event('change', { bubbles: true })); });
    expect(services.settings.autoOpenPanels).toBe('off');
    expect(find('.act-toggle').getAttribute('aria-expanded')).toBe('false');
    act(() => find<HTMLButtonElement>('.act-toggle').click());
    expect(find('.act-toggle').getAttribute('aria-expanded')).toBe('true');
    expect(find('#workbench-activity-entries').textContent).toContain('아직 기록된 활동');
    expect(net.calls.some((c) => /(?:open|connect|start)$/.test(c.method))).toBe(false);
  });
});

// The real entry (main.tsx): runBootstrap → routeAfterBoot(location.hash = …) → StrictMode-wrapped ShellApp,
// channels arrive afterwards from the daemon. The bootstrap fallback marks DEFAULT_ROUTE_KEY in sessionStorage.
describe('startup module route via the real bootstrap entry', () => {
  const CODE = 'Zk3q9V_x-71aa0c4e5b6d8f20';
  beforeEach(() => sessionStorage.clear());
  async function bootLikeMain(fragment: string, strict = true) {
    location.hash = fragment;
    const boot = await runBootstrap({ location, history, fetch: async () => ({ ok: true, status: 200 }), session: sessionStorage });
    routeAfterBoot(boot, (h) => { location.hash = h; });
    const net = new FakeNet();
    net.open(TEST_HELLO);
    const services = createShellServices(net, { autoTick: false });
    const render = () => root.render(strict ? <StrictMode><ShellApp services={services} /></StrictMode> : <ShellApp services={services} />);
    act(render);
    return { net, render };
  }
  const uartOpens = (net: FakeNet, channel = 'serial-a') =>
    act(() => net.emitControl(ev({ kind: 'uart.channel_state', channel, payload: { state: 'opened' } })));
  const tick = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

  it.each([true, false])('fallback #/l/m1-default + UART arriving later lands on the UART workbench (StrictMode=%s)', async (strict) => {
    const { net, render } = await bootLikeMain(`#boot=${CODE}`, strict);
    expect(location.hash).toBe('#/l/m1-default');
    expect(sessionStorage.getItem(DEFAULT_ROUTE_KEY)).toBe('1');
    await tick(); // bootstrap's own hashchange dispatches late: must not cancel
    act(render); // intervening shell re-render before any channel
    act(() => net.emitControl(ev({ kind: 'core.latch', payload: { on: false, missing: [] } }))); // unrelated store change
    expect(location.hash).toBe('#/l/m1-default');
    uartOpens(net);
    expect(location.hash).toBe('#/s/uart');
    expect(sessionStorage.getItem(DEFAULT_ROUTE_KEY)).toBeNull(); // consumed at the completion boundary only
  });

  it('a UART-only snapshot delivered right on connect (before the late bootstrap hashchange) lands on UART', async () => {
    const { net } = await bootLikeMain(`#boot=${CODE}`);
    uartOpens(net); // no tick: the bootstrap's own hashchange is still queued
    expect(location.hash).toBe('#/s/uart');
    await tick();
    expect(location.hash).toBe('#/s/uart');
  });

  it('explicit user navigation before channels arrive cancels the automatic route', async () => {
    const { net } = await bootLikeMain(`#boot=${CODE}`);
    act(() => { location.hash = '#/l/my-layout'; window.dispatchEvent(new HashChangeEvent('hashchange')); });
    uartOpens(net);
    expect(location.hash).toBe('#/l/my-layout');
    expect(sessionStorage.getItem(DEFAULT_ROUTE_KEY)).toBeNull();
  });

  it('an explicit next=/l/m1-default (no marker) is never overridden', async () => {
    const { net } = await bootLikeMain(`#boot=${CODE}&next=/l/m1-default`);
    expect(sessionStorage.getItem(DEFAULT_ROUTE_KEY)).toBeNull();
    uartOpens(net);
    expect(location.hash).toBe('#/l/m1-default');
  });

  it('a mixed live set keeps the integrated default', async () => {
    const { net } = await bootLikeMain(`#boot=${CODE}`);
    act(() => {
      net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan-b', payload: { state: 'live' } }));
    });
    uartOpens(net);
    expect(location.hash).toBe('#/l/m1-default');
  });
});
