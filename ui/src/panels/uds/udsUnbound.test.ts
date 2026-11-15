// @vitest-environment jsdom
// Unbound UDS module application: uds.console mounted with props {} (the UDS service tab before any
// connection). Existing editable presets + [연결], DID/Raw composer and history with a real empty state;
// a preset-only alias the daemon never reported is not a target (no send, no dry-run against it).
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { PREVIEW_DEBOUNCE_MS } from './consolePanel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
  localStorage.clear();
  document.body.replaceChildren();
});
const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const flush = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
  h?.frame();
};
const sends = () => h!.net.calls.filter((c) => c.method === 'uds.request' && (c.params as { dry_run?: boolean }).dry_run !== true);
const dryRuns = () => h!.net.calls.filter((c) => c.method === 'uds.request' && (c.params as { dry_run?: boolean }).dry_run === true);
const up = (target: string) => ev({ kind: 'uds.state', channel: target, payload: { target, backend: 'sim', addr: 'DoIP 0x1001', link: { state: 'up' } } });
const typeDid = (v: string) => {
  const i = q<HTMLInputElement>('[aria-label="DID"]');
  i.value = v;
  i.dispatchEvent(new Event('input'));
};

describe('uds.console unbound (props {})', () => {
  it('zero targets: full workbench, editable composer, no send / dry-run / connect on mount or edit', () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', {});
    h.frame();
    for (const t of ['uds-targets', 'uds-current', 'uds-last-values', 'uds-catalog']) expect(q(`[data-testid="${t}"]`), t).not.toBeNull();
    expect(q('[data-cmd="connect"]')).not.toBeNull();
    expect(q<HTMLSelectElement>('[aria-label="대상"]').value).toBe('ecu1'); // editable SIM preset, not connected
    expect(q('.link-st').textContent).toBe('연결 안 됨');
    expect(q('.conn-ov').textContent).toContain('활성 연결 없음');
    expect(h.host.querySelectorAll('.hist .vt-row:not([hidden])')).toHaveLength(0);
    // composer editable, sending waits for [연결]
    const did = q<HTMLInputElement>('[aria-label="DID"]');
    const send = q<HTMLButtonElement>('[data-cmd="read"]');
    expect(did.disabled).toBe(false);
    expect(send.disabled).toBe(true);
    expect(send.title).toContain('[연결]');
    expect(q('.req-msg').textContent).toContain('[연결]');
    typeDid('F18C');
    vi.advanceTimersByTime(PREVIEW_DEBOUNCE_MS + 10);
    h.frame();
    did.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' })); // Enter must not send either
    send.click();
    q<HTMLElement>('.didlist li[data-did]')?.click(); // DID quick-pick only fills
    h.frame();
    expect(h.net.calls).toEqual([]);
    expect(q('[data-testid="uds-preview"]').textContent).toContain('연결 후');
  });

  it('explicit [연결] → uds.connect only; once uds.state reports the alias the composer sends to it', async () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', {});
    h.frame();
    q<HTMLButtonElement>('[data-cmd="connect"]').click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'uds.connect', params: { name: 'ecu1', sim: 'bench', timeout_ms: 5000 } }]);
    expect(q<HTMLButtonElement>('[data-cmd="read"]').disabled).toBe(true); // RPC result alone is not a link
    h.batch([up('ecu1')]);
    vi.advanceTimersByTime(PREVIEW_DEBOUNCE_MS + 10);
    await flush();
    expect(dryRuns().map((c) => (c.params as { target: string }).target)).toEqual(['ecu1']); // fresh dry-run after the target appears
    const send = q<HTMLButtonElement>('[data-cmd="read"]');
    expect(send.disabled).toBe(false);
    send.click();
    await flush();
    expect(sends().map((c) => c.params)).toEqual([{ target: 'ecu1', bytes_hex: '22F190' }]);
  });

  it('a target already connected in the daemon is selected and writable without a preset connect (multi target)', async () => {
    h = mountPanel('uds.console', {});
    h.batch([up('bcm'), up('gw')]);
    await flush();
    const sel = q<HTMLSelectElement>('[aria-label="대상"]');
    expect(sel.value).toBe('bcm');
    expect([...sel.options].map((o) => o.value)).toEqual(['bcm', 'gw', 'ecu1', 'ecu']);
    expect(q<HTMLButtonElement>('[data-cmd="read"]').disabled).toBe(false);
    sel.value = 'ecu'; // preset-only DoIP alias (no host): not a target
    sel.dispatchEvent(new Event('change'));
    h.frame();
    expect(q<HTMLButtonElement>('[data-cmd="read"]').disabled).toBe(true);
    sel.value = 'gw';
    sel.dispatchEvent(new Event('change'));
    h.frame();
    expect(q<HTMLButtonElement>('[data-cmd="read"]').disabled).toBe(false);
    expect(h.net.calls.filter((c) => c.method !== 'uds.request')).toEqual([]);
    expect(sends()).toEqual([]);
  });

  it('configured targets keep the existing gate/daemon-decides projection (no regression)', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.frame();
    expect(q<HTMLSelectElement>('[aria-label="대상"]').value).toBe('ecu1');
    expect(q<HTMLButtonElement>('[data-cmd="read"]').disabled).toBe(false);
  });
});
