// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApprovalView } from '../store/adapter';
import { ev, FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { ApprovalCard, ARM_DELAY_MS } from './Approvals';
import { installPrefixKeys } from './prefixKeys';
import { createShellServices } from './services';
import { ShellCtx } from './ShellContext';
import { StopButton } from './TopBar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const key = (k: string, init: KeyboardEventInit = {}) =>
  new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });

describe('prefix keys Ctrl-]', () => {
  const setup = () => {
    const calls: string[] = [];
    const off = installPrefixKeys(window, {
      stop: () => calls.push('stop'),
      mark: () => calls.push('mark'),
      approvals: () => calls.push('approvals'),
      help: () => calls.push('help'),
      literal: () => calls.push('literal'),
    });
    return { calls, off };
  };

  it('x → STOP, m → mark, A → approvals, ? → help', () => {
    const { calls, off } = setup();
    for (const k of ['x', 'm', 'A', '?']) {
      window.dispatchEvent(key(']', { ctrlKey: true, code: 'BracketRight' }));
      window.dispatchEvent(key(k));
    }
    expect(calls).toEqual(['stop', 'mark', 'approvals', 'help']);
    off();
  });

  it('Ctrl-] Ctrl-] → literal; modifiers alone keep the prefix armed', () => {
    const { calls, off } = setup();
    window.dispatchEvent(key(']', { ctrlKey: true }));
    window.dispatchEvent(key('Shift', { shiftKey: true }));
    window.dispatchEvent(key(']', { ctrlKey: true }));
    expect(calls).toEqual(['literal']);
    off();
  });

  it('Ctrl-X is NOT STOP on the web, and keys without prefix pass through', () => {
    const { calls, off } = setup();
    const e = key('x', { ctrlKey: true });
    window.dispatchEvent(e);
    window.dispatchEvent(key('x'));
    expect(calls).toEqual([]);
    expect(e.defaultPrevented).toBe(false);
    off();
  });

  it('the prefix keystroke itself is swallowed (not sent to the terminal)', () => {
    const { off } = setup();
    const e = key(']', { ctrlKey: true });
    window.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
    off();
  });
});

describe('STOP button', () => {
  it('idle → STOP 요청 중… → STOP — 잠김 on server ack', async () => {
    const net = new FakeNet();
    net.open();
    const services = createShellServices(net, { autoTick: false });
    const unbind = services.bind();
    act(() =>
      root.render(
        <ShellCtx.Provider value={services}>
          <StopButton />
        </ShellCtx.Provider>,
      ),
    );
    const btn = () => host.querySelector('[data-testid="stop-button"]') as HTMLButtonElement;
    expect(btn().dataset.phase).toBe('idle');
    await act(async () => btn().click());
    expect(btn().dataset.phase).toBe('requesting');
    expect(btn().textContent).toBe('STOP 요청 중…');
    expect(net.calls.some((c) => c.method === 'core.stop')).toBe(true);
    act(() => net.emitControl(ev({ kind: 'core.latch', payload: { on: true, missing: [] } })));
    expect(btn().dataset.phase).toBe('locked');
    expect(btn().textContent).toBe('STOP — 잠김');
    unbind();
  });

  it('shows "확인 안 됨 — 재시도" after 2 s without ack', async () => {
    vi.useFakeTimers();
    const net = new FakeNet();
    net.open();
    const services = createShellServices(net, { autoTick: false });
    const unbind = services.bind();
    act(() =>
      root.render(
        <ShellCtx.Provider value={services}>
          <StopButton />
        </ShellCtx.Provider>,
      ),
    );
    const btn = host.querySelector('[data-testid="stop-button"]') as HTMLButtonElement;
    await act(async () => btn.click());
    await act(async () => {
      vi.advanceTimersByTime(2_000);
    });
    expect(btn.dataset.phase).toBe('unconfirmed');
    unbind();
  });
});

describe('approval card key rules', () => {
  const approval: ApprovalView = {
    approvalId: 'a17',
    risk: 'R2',
    requester: 'claude-1',
    origin: 'agent',
    domain: 'uds',
    channel: 'bcm',
    requestId: 'r-77',
    display: { service: 'WriteDataByIdentifier', did: '0101', diff: 'KR→EU', html: '<img src=x onerror=alert(1)>' },
    expiresAtWallMs: null,
    presenceRequired: false,
    codeSuffix: null,
  };

  const mount = (a: ApprovalView = approval) => {
    vi.useFakeTimers();
    const decided: string[] = [];
    let deferred = 0;
    act(() =>
      root.render(<ApprovalCard approval={a} onDecide={(d) => decided.push(d)} onDefer={() => deferred++} />),
    );
    const card = host.querySelector('[data-testid="approval-card"]') as HTMLDivElement;
    return { decided, card, deferred: () => deferred };
  };

  it('default focus is 거부, buttons disabled and Y ignored during the 1 s arming delay', () => {
    const { decided, card } = mount();
    expect(document.activeElement?.className).toBe('deny');
    expect((host.querySelector('[data-testid="approve-btn"]') as HTMLButtonElement).disabled).toBe(true);
    act(() => void card.dispatchEvent(key('Y')));
    expect(decided).toEqual([]);
    act(() => void vi.advanceTimersByTime(ARM_DELAY_MS));
    expect(card.dataset.armed).toBe('true');
    act(() => void card.dispatchEvent(key('Y')));
    expect(decided).toEqual(['approve']);
  });

  it('Enter and Esc never approve; Esc defers; auto-repeat Y ignored', () => {
    const { decided, card, deferred } = mount();
    act(() => void vi.advanceTimersByTime(ARM_DELAY_MS));
    act(() => void card.dispatchEvent(key('Enter')));
    act(() => void card.dispatchEvent(key('Y', { repeat: true })));
    expect(decided).toEqual([]);
    act(() => void card.dispatchEvent(key('Escape')));
    expect(deferred()).toBe(1);
    expect(decided).toEqual([]);
  });

  it('renders display as text only and exposes data-request-id / data-origin', () => {
    const { card } = mount();
    expect(host.querySelector('img')).toBeNull();
    expect(card.textContent).toContain('<img src=x onerror=alert(1)>');
    expect(card.dataset.requestId).toBe('r-77');
    expect(card.dataset.origin).toBe('agent');
  });

  it('a required code suffix blocks approval until it matches', () => {
    const { decided, card } = mount({ ...approval, codeSuffix: '7K' });
    act(() => void vi.advanceTimersByTime(ARM_DELAY_MS));
    act(() => void card.dispatchEvent(key('Y')));
    expect(decided).toEqual([]);
  });
});

describe('STOP rights (contract v1.1: every right)', () => {
  it('watch can STOP: button enabled, click sends core.stop', async () => {
    const net = new FakeNet();
    net.open({ ...TEST_HELLO, rights: 'watch' });
    const services = createShellServices(net, { autoTick: false });
    const unbind = services.bind();
    act(() =>
      root.render(
        <ShellCtx.Provider value={services}>
          <StopButton />
        </ShellCtx.Provider>,
      ),
    );
    const btn = host.querySelector('[data-testid="stop-button"]') as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    await act(async () => btn.click());
    expect(net.calls.map((c) => c.method)).toEqual(['core.stop']);
    expect(net.calls[0]!.params).toMatchObject({ scope: 'all' });
    unbind();
  });
});
