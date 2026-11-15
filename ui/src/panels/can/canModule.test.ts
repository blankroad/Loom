// @vitest-environment jsdom
// can.trace without a channel = the CAN module application before any interface is bound
// (work/reports/module-apps/CONTRACT.md): full tools, empty data, no subscription, no RPC on mount,
// explicit can.open with exactly the selected backend params, sends disabled until bound.
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { PanelContext } from '../../sdk';
import { mountPanel, type Harness } from '../../sdk/testing/harness';
import { canOpenCli, canOpenParams, createCanConnect, readCanStatus } from './canConnect';
import { registerCanPanels } from './index';

registerCanPanels();

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
});
const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
const flush = async () => {
  for (let k = 0; k < 6; k++) await Promise.resolve();
};
const type = (id: string, v: string) => {
  const i = q<HTMLInputElement>(id);
  i.value = v;
  i.dispatchEvent(new Event('input', { bubbles: true }));
};

describe('can.trace unbound module application', () => {
  it('renders the full trace / rx-tx tools with a connect section, opens no subscription and sends no RPC', async () => {
    vi.useFakeTimers();
    h = mountPanel('can.trace', {});
    await vi.advanceTimersByTimeAsync(3_000);
    h.frame();
    expect(h.net.subs).toHaveLength(0);
    expect(h.net.calls).toHaveLength(0);
    const root = q('can-trace');
    expect(root.dataset.bound).toBe('false');
    expect(root.dataset.channel).toBe('');
    expect(root.textContent).toContain('채널 미선택');
    expect(root.textContent).not.toContain('ch0');
    // real tools are there (not a welcome card): trace grid, view tabs, send line, connect section
    expect(root.querySelector('[role=grid]')).not.toBeNull();
    expect(q('can-view-tabs').querySelectorAll('button')).toHaveLength(2);
    expect(q('can-connect')).toBeTruthy();
    expect(q<HTMLSelectElement>('can-connect-backend').value).toBe('sim');
    // alias-dependent operations disabled; typing a frame never asks for a dry-run
    const input = q<HTMLInputElement>('can-send-input');
    expect(input.disabled).toBe(true);
    input.disabled = false;
    type('can-send-input', '0x300#01');
    await vi.advanceTimersByTimeAsync(1_000);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(h.net.calls).toHaveLength(0);
    // switching views is UI only
    q('can-view-rxtx').click();
    h.frame();
    expect(h.net.calls).toHaveLength(0);
  });

  it('only implemented backend is selectable; unimplemented adapters are listed disabled, not mapped', () => {
    h = mountPanel('can.trace', {});
    const opts = [...q<HTMLSelectElement>('can-connect-backend').options];
    expect(opts.filter((o) => !o.disabled).map((o) => o.value)).toEqual(['sim']);
    expect(opts.filter((o) => o.disabled).map((o) => o.textContent)).toEqual(expect.arrayContaining([expect.stringContaining('미구현')]));
  });

  it('connect is explicit: disabled until a valid name, then exactly can.open{channel, bus?, nodes}', async () => {
    h = mountPanel('can.trace', {});
    const btn = q<HTMLButtonElement>('can-connect-open');
    expect(btn.disabled).toBe(true);
    type('can-connect-channel', 'Bad Name');
    h.frame();
    expect(btn.disabled).toBe(true);
    expect(q('can-connect-error').textContent).toContain('채널 이름');
    type('can-connect-channel', 'body-a');
    const nodes = q<HTMLSelectElement>('can-connect-nodes');
    nodes.value = 'none';
    nodes.dispatchEvent(new Event('change', { bubbles: true }));
    h.frame();
    expect(btn.disabled).toBe(false);
    expect(q('can-connect-cli').textContent).toBe('loom can open --channel body-a --nodes none');
    expect(h.net.calls).toHaveLength(0);
    btn.click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'can.open', params: { channel: 'body-a', nodes: 'none' } }]);
    expect(h.toasts.some((t) => t.includes('body-a'))).toBe(true);
  });

  it('watch right: connect disabled (send also disabled)', () => {
    h = mountPanel('can.trace', {}, { hello: { rights: 'watch' } });
    type('can-connect-channel', 'ch1');
    h.frame();
    expect(q<HTMLButtonElement>('can-connect-open').disabled).toBe(true);
    expect(q<HTMLInputElement>('can-send-input').disabled).toBe(true);
  });

  it('listing open channels is an explicit read-only can.status{} query', async () => {
    h = mountPanel('can.trace', {});
    h.net.responders.set('can.status', () => ({ data: { backend: 'sim', channels: [{ channel: 'ch0', bus: 'ch0', state: 'open' }, { channel: 'pt', bus: 'shared', state: 'open' }] } }));
    expect(h.net.calls).toHaveLength(0);
    q('can-list-open').click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'can.status', params: {} }]);
    const items = [...h.host.querySelectorAll<HTMLButtonElement>('[data-testid="can-open-channel"]')];
    expect(items.map((b) => b.dataset.channel)).toEqual(['ch0', 'pt']);
    expect(items[1]!.textContent).toContain('bus shared');
    items[0]!.click(); // observation only: no further RPC
    await flush();
    expect(h.net.calls).toHaveLength(1);
  });

  it('bound panel keeps its channel subscription and has no connect section', () => {
    h = mountPanel('can.trace', { channel: 'ch1' });
    expect(h.net.subs[0]!.filter).toEqual({ domains: ['can'], channels: ['ch1'] });
    expect(h.host.querySelector('[data-testid="can-connect"]')).toBeNull();
    expect(q('can-trace').dataset.bound).toBe('true');
    expect(q<HTMLInputElement>('can-send-input').disabled).toBe(false);
  });
});

describe('canConnect helpers', () => {
  it('params follow the worker contract (bus omitted when equal / empty)', () => {
    expect(canOpenParams({ channel: 'ch0', bus: '', nodes: 'default' })).toEqual({ ok: true, params: { channel: 'ch0', nodes: 'default' } });
    expect(canOpenParams({ channel: 'ch0', bus: 'ch0', nodes: 'default' })).toEqual({ ok: true, params: { channel: 'ch0', nodes: 'default' } });
    expect(canOpenParams({ channel: 'a', bus: 'shared', nodes: 'none' })).toEqual({ ok: true, params: { channel: 'a', bus: 'shared', nodes: 'none' } });
    expect(canOpenParams({ channel: '', bus: '', nodes: 'default' }).ok).toBe(false);
    expect(canOpenParams({ channel: 'a', bus: 'Bad', nodes: 'default' }).ok).toBe(false);
    expect(canOpenCli({ channel: 'a', bus: 'shared', nodes: 'default' })).toBe('loom can open --channel a --bus shared --nodes default');
  });
  it('status rows never invent fields', () => {
    expect(readCanStatus({ channels: [{ channel: 'x' }, { bus: 'no-name' }] })).toEqual([{ channel: 'x', bus: null, backend: null, state: null }]);
    expect(readCanStatus(null)).toEqual([]);
  });
  it('after a successful open the bound trace opens for that channel and current view', async () => {
    const opened: [string, Record<string, unknown>][] = [];
    const acts: [string, object][] = [];
    const ctx = {
      act: async (id: string, p: object) => {
        acts.push([id, p]);
        return { requestId: 'r1' };
      },
      query: async () => ({}),
      ui: { openPanel: (id: string, p: Record<string, unknown>) => void opened.push([id, p]), toast: () => {} },
      scheduler: { invalidate: () => {} },
    } as unknown as PanelContext;
    const c = createCanConnect(ctx, () => 'rxtx');
    document.body.append(c.root);
    const ch = c.root.querySelector<HTMLInputElement>('[data-testid="can-connect-channel"]')!;
    ch.value = 'pt';
    ch.dispatchEvent(new Event('input'));
    c.update(true, true);
    c.root.querySelector<HTMLButtonElement>('[data-testid="can-connect-open"]')!.click();
    await flush();
    expect(acts).toEqual([['open', { channel: 'pt', nodes: 'default' }]]);
    expect(opened).toEqual([['can.trace', { channel: 'pt', view: 'rxtx' }]]);
    // failed open: no navigation, error shown
    ctx.act = async () => {
      throw Object.assign(new Error('channel pt is already open'), { code: 'E_BUSY' });
    };
    c.update(true, true);
    c.root.querySelector<HTMLButtonElement>('[data-testid="can-connect-open"]')!.click();
    await flush();
    expect(opened).toHaveLength(1);
    expect(c.root.querySelector('[data-testid="can-connect-error"]')!.textContent).toContain('E_BUSY');
    c.dispose();
    c.root.remove();
  });
});
