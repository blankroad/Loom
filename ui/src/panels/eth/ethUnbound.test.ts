// @vitest-environment jsdom
// Unbound Ethernet module application: eth.packets mounted with props {} (the Ethernet service tab before any
// capture). Full actual workbench, real empty data, explicit interface configuration / [연결] only — never an
// implicit lan1 subscription, capture, journal query, export or send.
import { afterEach, describe, expect, it } from 'vitest';
import { getPanel } from '../../sdk/registry';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { IFACE_CFG_KEY, loadConfigs, sanitizeConfigs, type IfaceConfig } from './ifaceConfigs';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
  document.body.replaceChildren();
});
const flush = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
  h?.frame();
};
const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const cmd = (c: string) => q<HTMLButtonElement>(`[data-cmd="${c}"]`);
/** every non read-only call the panel made (capture.status / filter.check / ifaces are read-only) */
const READ_ONLY = new Set(['eth.capture.status', 'eth.ifaces', 'eth.filter.check']);
const writes = () => h!.net.calls.filter((c) => !READ_ONLY.has(c.method));
const input = (el: HTMLInputElement, v: string) => {
  el.value = v;
  el.dispatchEvent(new Event('input'));
};
const CFG = (alias: string, iface: string, backend: 'live' | 'sim' = 'live'): IfaceConfig => ({ alias, iface, backend, bpf: '', snaplen: 262144, promisc: false, bufferMib: 64 });

describe('eth.packets unbound (props {})', () => {
  it('mounts the full workbench with zero channels: no subscription, no lan1, no writes', async () => {
    h = mountPanel('eth.packets', {});
    await flush();
    // full actual tool, not a launcher card
    for (const c of ['add-channel', 'start-all', 'stop-all', 'pause-view', 'autoscroll', 'colorize', 'follow', 'export', 'clear-view', 'toggle-socket', 'keys']) {
      expect(cmd(c), c).not.toBeNull();
    }
    for (const t of ['eth-toolbar', 'eth-status', 'eth-filter-host', 'eth-panes', 'eth-statusbar', 'eth-iface-config']) expect(q(`[data-testid="${t}"]`), t).not.toBeNull();
    expect(h.host.querySelector('.eth-list')).not.toBeNull();
    expect(h.host.querySelector('.eth-sock')).not.toBeNull();
    expect(q('[data-testid="eth-status"]').dataset.state).toBe('UNBOUND');
    expect(q('[data-testid="eth-status"] .alias').textContent).toBe('채널 미선택');
    // no server subscription at all, and lan1 never appears anywhere
    expect(h.net.subs).toHaveLength(0);
    expect(JSON.stringify(h.net.calls)).not.toContain('lan1');
    expect(h.host.textContent).not.toContain('lan1');
    expect(h.net.calls.map((c) => c.method)).toEqual(['eth.capture.status']);
    // alias-dependent operations disabled until a channel is connected
    expect(cmd('stop-all').disabled).toBe(true);
    expect(cmd('export').disabled).toBe(true);
    expect(cmd('toggle-socket').disabled).toBe(true);
    expect(cmd('iface-connect').disabled).toBe(false);
    expect(q<HTMLSelectElement>('[data-testid="eth-iface-config"]').value).toBe('\u0000new');
    // pause / scroll-away / Space / export / stop do nothing against a channel
    cmd('pause-view').click();
    cmd('autoscroll').click();
    q('.eth-panel').dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    cmd('export').click();
    cmd('stop-all').click();
    q('.eth-panel').dispatchEvent(new KeyboardEvent('keydown', { key: 's', altKey: true, bubbles: true }));
    await flush();
    expect(writes()).toEqual([]);
    expect(h.net.calls.some((c) => c.method === 'eth.query')).toBe(false);
    expect((h.host.querySelector('.eth-sock') as HTMLElement).hidden).toBe(true);
  });

  it('lists every existing daemon channel as a card (multi alias) without binding or writing', async () => {
    const status = () => ({ channels: [{ ch: 'lan1', state: 'running', backend: 'live', iface: 'en5' }, { ch: 'lan2', state: 'stopped', backend: 'sim', iface: 'sim' }] });
    h = mountPanel('eth.packets', {});
    h.net.responders.set('eth.capture.status', status);
    // another channel's capture state on the control path → read-only refresh of the cards
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan2', payload: { ch: 'lan2', state: 'stopped' } }));
    await flush();
    const cards = [...h.host.querySelectorAll<HTMLElement>('[data-testid="eth-other-channels"] .eth-chcard')];
    expect(cards.map((c) => c.dataset.channel)).toEqual(['lan1', 'lan2']);
    cards[1]!.click(); // opens that channel's bound panel (shell navigation) — never a write
    await flush();
    expect(h.net.subs).toHaveLength(0);
    expect(writes()).toEqual([]);
  });

  it('[연결…] with a new configuration: explicit dialog → exactly one eth.capture.start for the chosen alias; config saved', async () => {
    h = mountPanel('eth.packets', {});
    h.net.responders.set('eth.capture.status', () => ({ channels: [{ ch: 'lan1', state: 'running' }] }));
    h.net.responders.set('eth.ifaces', () => ({ ifaces: [{ name: 'en5' }, { name: 'sim' }] }));
    await flush();
    cmd('iface-connect').click();
    await flush();
    const dlg = q('[data-testid="eth-connect-dialog"]');
    expect(dlg).not.toBeNull();
    expect(writes()).toEqual([]); // opening the dialog starts nothing
    const [alias, iface] = [...dlg.querySelectorAll<HTMLInputElement>('input')];
    input(alias!, 'lan7');
    input(iface!, 'sim');
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-add-channel-start"]')!.click();
    await flush();
    expect(writes()).toEqual([{ method: 'eth.capture.start', params: { iface: 'sim', ch: 'lan7', backend: 'sim', snaplen: 262144, promisc: false, buffer_mib: 64 } }]);
    expect(h.toasts.at(-1)).toContain('lan7');
    expect(loadConfigs().map((c) => [c.alias, c.iface, c.backend])).toEqual([['lan7', 'sim', 'sim']]);
    expect(q<HTMLSelectElement>('[data-testid="eth-iface-config"]').value).toBe('lan7');
    expect(h.net.subs).toHaveLength(0); // this panel stays unbound; the bound channel is its own panel
  });

  it('refuses an alias that already exists in the daemon (never restarts another channel)', async () => {
    localStorage.setItem(IFACE_CFG_KEY, JSON.stringify([CFG('lan1', 'en5')]));
    h = mountPanel('eth.packets', {});
    h.net.responders.set('eth.capture.status', () => ({ channels: [{ ch: 'lan1', state: 'stopped' }] }));
    await flush();
    cmd('iface-connect').click();
    await flush();
    q<HTMLButtonElement>('[data-testid="eth-add-channel-start"]').click();
    await flush();
    expect(h.net.calls.some((c) => c.method === 'eth.capture.start')).toBe(false);
    expect(q('[data-testid="eth-capture-msg"]').textContent).toContain('이미 있는 채널');
  });

  it('saved configurations: choose / edit / save / delete are local only (no RPC)', async () => {
    localStorage.setItem(IFACE_CFG_KEY, JSON.stringify([CFG('lanA', 'en5'), CFG('lanB', 'en6')]));
    h = mountPanel('eth.packets', {});
    await flush();
    const sel = q<HTMLSelectElement>('[data-testid="eth-iface-config"]');
    expect([...sel.options].map((o) => o.value)).toEqual(['lanA', 'lanB', '\u0000new']);
    sel.value = 'lanB';
    sel.dispatchEvent(new Event('change'));
    cmd('iface-edit').click();
    await flush();
    const dlg = q('[data-testid="eth-connect-dialog"]');
    const [alias, iface] = [...dlg.querySelectorAll<HTMLInputElement>('input')];
    expect([alias!.value, iface!.value]).toEqual(['lanB', 'en6']);
    input(iface!, 'en9');
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-iface-save"]')!.click();
    await flush();
    expect(loadConfigs().map((c) => `${c.alias}:${c.iface}`)).toEqual(['lanB:en9', 'lanA:en5']);
    sel.value = 'lanA';
    sel.dispatchEvent(new Event('change'));
    h.frame();
    cmd('iface-delete').click();
    await flush();
    expect(loadConfigs().map((c) => c.alias)).toEqual(['lanB']);
    expect(writes()).toEqual([]);
    expect(h.net.calls.some((c) => c.method === 'eth.capture.start')).toBe(false);
  });

  it('corrupt saved configs are dropped (no fake interfaces)', () => {
    expect(sanitizeConfigs([CFG('ok', 'en0'), { alias: 'bad name', iface: 'x' }, CFG('ok', 'dup'), { ...CFG('s', 'sim', 'sim'), bpf: 'port 1' }, 7])).toEqual([CFG('ok', 'en0'), CFG('s', 'sim', 'sim')]);
    localStorage.setItem(IFACE_CFG_KEY, '{not json');
    expect(loadConfigs()).toEqual([]);
  });

  it('topic filter: no channel → no implicit lan1; a bound panel still subscribes only its alias', () => {
    const spec = getPanel('eth.packets')!.manifest.topics[0]!;
    expect(spec.filter({}).channels).toBeUndefined();
    expect(spec.filter({ channel: 'lan2' }).channels).toEqual(['lan2']);
    h = mountPanel('eth.packets', { channel: 'lan2' });
    expect(h.net.subs).toHaveLength(1);
    expect(h.net.subs[0]!.filter.channels).toEqual(['lan2']);
    expect(q('[data-testid="eth-status"] .alias').textContent).toBe('lan2');
    expect(h.host.querySelector('[data-testid="eth-iface-config"]')?.isConnected ?? false).toBe(false);
  });
});
