// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
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
    expect(find('.workbench-body > aside .channel-bar')).toBeTruthy();
    expect(find('.sub-bar .channel-bar')).toBeNull();
    expect(find('.workbench-body > .workbench-main [data-testid="layout"]')).toBeTruthy();
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
    expect(find('.top-bar .rec').textContent).toContain('REC');
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
