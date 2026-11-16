// @vitest-environment jsdom
// psu.gauge without a channel = PSU module application before any supply is chosen
// (work/reports/module-apps/CONTRACT.md). Safety: no implicit "bench", no psu.out/psu.set/psu.status
// while unbound; explicit psu.connect with only engine-supported backends; emergency OFF of an
// explicitly bound alias stays available even when the link is lost.
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.hoisted(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
});
import type { PanelContext } from '../../sdk';
import { mountPanel, type Harness } from '../../sdk/testing/harness';
import { rec } from './fixtures';
import './gaugePanel';
import { createPsuConnect, isLoopbackAddr, PSU_DEFAULT_LIMITS, psuConnectCli, psuConnectParams } from './psuConnect';

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
const type = (id: string, value: string) => {
  const i = q<HTMLInputElement>(id);
  i.value = value;
  i.dispatchEvent(new Event('input', { bubbles: true }));
};

describe('psu.gauge unbound module application', () => {
  it('full tile + connect section; no subscription, no status poll, no implicit bench', async () => {
    vi.useFakeTimers();
    h = mountPanel('psu.gauge', {});
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(h.net.subs).toHaveLength(0);
    expect(h.net.calls).toHaveLength(0);
    expect(q('psu-badge').textContent).toBe('별칭 미선택');
    expect(h.host.textContent).not.toContain('bench');
    // PSU-07: the connect form leads; the empty instrument tile is collapsed to one placeholder line
    expect(q('psu-connect').closest('[hidden]')).toBeNull();
    expect(q('psu-placeholder').closest('[hidden]')).toBeNull();
    expect(q('psu-placeholder').textContent).toBe('연결하면 실측·설정·추세가 여기에 표시됩니다');
    for (const id of ['psu-meter', 'psu-setpoints', 'psu-set-form', 'psu-trend', 'psu-foot', 'psu-out', 'psu-mode', 'psu-prot']) {
      expect(q(id).closest('[hidden]'), id).not.toBeNull();
    }
    // no disabled red emergency button on an unbound tile (bound aliases keep it always enabled)
    expect(q('psu-emergency').closest('[hidden]')).not.toBeNull();
    // connect form comes before the (collapsed) tile in document order
    expect(q('psu-connect').compareDocumentPosition(q('psu-placeholder')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('PSU-07: a bound tile has no placeholder and shows every block', () => {
    h = mountPanel('psu.gauge', { channel: 'rail' });
    h.frame();
    expect(h.host.querySelector('[data-testid="psu-placeholder"]')).toBeNull();
    for (const id of ['psu-meter', 'psu-setpoints', 'psu-set-form', 'psu-trend', 'psu-foot', 'psu-emergency']) {
      expect(q(id).closest('[hidden]'), id).toBeNull();
    }
  });

  it('no off / set / output RPC while unbound — buttons disabled and the "!" key is inert', async () => {
    h = mountPanel('psu.gauge', {});
    h.frame();
    const emergency = q<HTMLButtonElement>('psu-emergency');
    expect(emergency.disabled).toBe(true);
    expect(q<HTMLButtonElement>('psu-output-toggle').disabled).toBe(true);
    expect(q<HTMLButtonElement>('psu-apply').disabled).toBe(true);
    expect(q('psu-disconnect').hidden).toBe(true);
    // even if forced (devtools / a stale handler), nothing reaches the daemon
    for (const id of ['psu-emergency', 'psu-output-toggle', 'psu-apply', 'psu-disconnect']) {
      const b = q<HTMLButtonElement>(id);
      b.disabled = false;
      b.click();
    }
    h.host.querySelector<HTMLElement>('.psu-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: '!', bubbles: true }));
    await flush();
    expect(h.net.calls).toHaveLength(0);
  });

  it('only engine-supported backends are selectable; visa listed disabled', () => {
    h = mountPanel('psu.gauge', {});
    const opts = [...q<HTMLSelectElement>('psu-connect-backend').options];
    expect(opts.filter((o) => !o.disabled).map((o) => o.value)).toEqual(['sim', 'scpi-tcp']);
    expect(opts.find((o) => o.value === 'visa')!.disabled).toBe(true);
  });

  it('explicit connect: exact psu.connect params (sim), output never touched', async () => {
    h = mountPanel('psu.gauge', {});
    const btn = q<HTMLButtonElement>('psu-connect-open');
    expect(btn.disabled).toBe(true);
    type('psu-connect-ch', 'rail-a');
    type('psu-connect-vmax', '12');
    h.frame();
    expect(btn.disabled).toBe(false);
    expect(h.net.calls).toHaveLength(0);
    btn.click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'psu.connect', params: { ch: 'rail-a', backend: 'sim', vmax: 12 } }]);
    expect(h.net.calls.some((c) => c.method === 'psu.out' || c.method === 'psu.off' || c.method === 'psu.set')).toBe(false);
  });

  it('scpi-tcp needs a loopback address; LAN address refused before any RPC', async () => {
    h = mountPanel('psu.gauge', {});
    const sel = q<HTMLSelectElement>('psu-connect-backend');
    sel.value = 'scpi-tcp';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    type('psu-connect-ch', 'ext');
    type('psu-connect-address', '192.168.1.5:5025');
    h.frame();
    expect(q<HTMLButtonElement>('psu-connect-open').disabled).toBe(true);
    expect(q('psu-connect-error').textContent).toContain('루프백');
    type('psu-connect-address', '127.0.0.1:5025');
    h.frame();
    expect(q('psu-connect-cli').textContent).toBe('loom psu connect ext --scpi-tcp 127.0.0.1:5025');
    q<HTMLButtonElement>('psu-connect-open').click();
    await flush();
    expect(h.net.calls).toEqual([{ method: 'psu.connect', params: { ch: 'ext', backend: 'scpi-tcp', address: '127.0.0.1:5025' } }]);
  });

  it('PSU-01/PSU-06: limit fields show the profile defaults; the note states one worker per alias (≤ 4), not one supply', () => {
    h = mountPanel('psu.gauge', {});
    expect(q<HTMLInputElement>('psu-connect-vmax').placeholder).toBe('기본 16');
    expect(q<HTMLInputElement>('psu-connect-imax').placeholder).toBe('기본 5');
    const note = q('psu-connect-note').textContent!;
    expect(note).toContain('별칭마다 별도 PSU 작업자 (최대 4대)');
    expect(note).toContain('다출력 계측기의 출력 번호는 미지원');
    expect(note).not.toContain('1대만');
    type('psu-connect-ch', 'rail');
    type('psu-connect-vmax', '20');
    h.frame();
    expect(q('psu-connect-error').textContent).toBe('전압 한도는 기본 16 V 이하로만 낮출 수 있습니다');
    expect(q<HTMLButtonElement>('psu-connect-open').disabled).toBe(true);
  });

  it('watch right: connect disabled', () => {
    h = mountPanel('psu.gauge', {}, { hello: { rights: 'watch' } });
    type('psu-connect-ch', 'x');
    h.frame();
    expect(q<HTMLButtonElement>('psu-connect-open').disabled).toBe(true);
  });
});

describe('psu.gauge bound alias safety', () => {
  it('emergency OFF stays enabled for the bound alias when the link is lost / state unknown', async () => {
    h = mountPanel('psu.gauge', { channel: 'rail' });
    h.frame();
    expect(q<HTMLButtonElement>('psu-emergency').disabled).toBe(false); // state unknown
    h.batch([{ ...rec('psu.state', 1, 0, { ch: 'rail', out: true, connected: false }), channel: 'rail' }]);
    expect(q<HTMLButtonElement>('psu-emergency').disabled).toBe(false);
    expect(q<HTMLButtonElement>('psu-output-toggle').disabled).toBe(true);
    expect(q<HTMLButtonElement>('psu-apply').disabled).toBe(true);
    q('psu-emergency').click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'psu.off' || c.method === 'psu.out')).toEqual([{ method: 'psu.off', params: { channel: 'rail' } }]);
  });

  it('set needs a device-reported state first (no blind write); then targets the bound alias', async () => {
    h = mountPanel('psu.gauge', { channel: 'rail' });
    h.frame();
    expect(q<HTMLButtonElement>('psu-apply').disabled).toBe(true);
    h.batch([{ ...rec('psu.state', 1, 0, { ch: 'rail', set_v: 5, set_i: 1, out: false }), channel: 'rail' }]);
    expect(q<HTMLButtonElement>('psu-apply').disabled).toBe(false);
    type('psu-set-v', '6');
    q('psu-apply').click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'psu.set')).toEqual([{ method: 'psu.set', params: { channel: 'rail', v: 6 } }]);
  });

  it('disconnect is explicit and only closes the link of the bound alias', async () => {
    h = mountPanel('psu.gauge', { channel: 'rail' });
    h.frame();
    expect(q('psu-disconnect').hidden).toBe(false);
    q('psu-disconnect').click();
    await flush();
    expect(h.net.calls.filter((c) => c.method !== 'psu.status')).toEqual([{ method: 'psu.disconnect', params: { ch: 'rail' } }]);
  });
});

describe('psuConnect helpers', () => {
  it('loopback address rule mirrors scpi::parse_loopback (no host names, port 1..65535)', () => {
    for (const ok of ['127.0.0.1:5025', '127.1.2.3:1', '[::1]:5025']) expect(isLoopbackAddr(ok)).toBe(true);
    for (const bad of ['localhost:5025', '10.0.0.2:5025', '127.0.0.1:0', '127.0.0.1:70000', '127.0.0.1', '127.0.0.256:1']) expect(isLoopbackAddr(bad)).toBe(false);
  });
  it('limits only as numbers > 0; empty = profile default; address only for scpi-tcp', () => {
    expect(psuConnectParams({ ch: 'a', backend: 'sim', address: '127.0.0.1:1', vmax: '', imax: '' })).toEqual({ ok: true, params: { ch: 'a', backend: 'sim' } });
    expect(psuConnectParams({ ch: 'a', backend: 'sim', address: '', vmax: '0', imax: '' }).ok).toBe(false);
    expect(psuConnectParams({ ch: 'A', backend: 'sim', address: '', vmax: '', imax: '' }).ok).toBe(false);
    expect(psuConnectCli({ ch: 'a', backend: 'sim', address: '', vmax: '', imax: '' })).toBe('loom psu connect a');
  });
  it('PSU-01: CLI preview carries --vmax/--imax exactly as sent; a limit above the profile default is refused, not clamped', () => {
    expect(psuConnectCli({ ch: 'a', backend: 'sim', address: '', vmax: '9', imax: '' })).toBe('loom psu connect a --vmax 9');
    expect(psuConnectCli({ ch: 'ext', backend: 'scpi-tcp', address: '127.0.0.1:5025', vmax: '12.5', imax: '2' })).toBe(
      'loom psu connect ext --scpi-tcp 127.0.0.1:5025 --vmax 12.5 --imax 2',
    );
    expect(psuConnectParams({ ch: 'a', backend: 'sim', address: '', vmax: '16', imax: '5' })).toEqual({ ok: true, params: { ch: 'a', backend: 'sim', vmax: 16, imax: 5 } });
    expect(psuConnectParams({ ch: 'a', backend: 'sim', address: '', vmax: '16.5', imax: '' })).toEqual({ ok: false, error: '전압 한도는 기본 16 V 이하로만 낮출 수 있습니다' });
    expect(psuConnectParams({ ch: 'a', backend: 'sim', address: '', vmax: '', imax: '6' })).toEqual({ ok: false, error: '전류 한도는 기본 5 A 이하로만 낮출 수 있습니다' });
    expect(PSU_DEFAULT_LIMITS).toEqual({ vmax: 16, imax: 5 });
  });
  it('after connect the bound gauge opens for that alias; failure keeps the form with the error', async () => {
    const opened: [string, Record<string, unknown>][] = [];
    let fail = false;
    const ctx = {
      act: async () => {
        if (fail) throw Object.assign(new Error('this worker already drives bench'), { code: 'E_PORT_BUSY' });
        return { requestId: null };
      },
      ui: { openPanel: (id: string, p: Record<string, unknown>) => void opened.push([id, p]), toast: () => {} },
      scheduler: { invalidate: () => {} },
    } as unknown as PanelContext;
    const c = createPsuConnect(ctx);
    const ch = c.root.querySelector<HTMLInputElement>('[data-testid="psu-connect-ch"]')!;
    ch.value = 'rail';
    ch.dispatchEvent(new Event('input'));
    c.update(true, true);
    const btn = c.root.querySelector<HTMLButtonElement>('[data-testid="psu-connect-open"]')!;
    btn.click();
    await flush();
    expect(opened).toEqual([['psu.gauge', { channel: 'rail' }]]);
    fail = true;
    c.update(true, true);
    btn.click();
    await flush();
    expect(opened).toHaveLength(1);
    expect(c.root.querySelector('[data-testid="psu-connect-error"]')!.textContent).toContain('E_PORT_BUSY');
    // observe an already-connected alias: navigation only
    const v = c.root.querySelector<HTMLInputElement>('[data-testid="psu-view-ch"]')!;
    v.value = 'bench';
    v.dispatchEvent(new Event('input'));
    c.root.querySelector<HTMLButtonElement>('[data-testid="psu-view-open"]')!.click();
    expect(opened[1]).toEqual(['psu.gauge', { channel: 'bench' }]);
    c.dispose();
  });
});
