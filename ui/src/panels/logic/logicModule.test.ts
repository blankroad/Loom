// @vitest-environment jsdom
// logic.analyzer as the Logic module tab (work/reports/module-apps/CONTRACT.md): source / engine
// configuration visible before anything runs; opening the tab never starts a capture, never asks the
// engine; the engine check and the start are explicit user actions with the real backend params.
import { afterEach, describe, expect, it } from 'vitest';
import { mountPanel, type Harness } from '../../sdk/testing/harness';
import './logicPanel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  document.body.replaceChildren();
  try {
    localStorage.clear();
  } catch {
    /* ignore */
  }
});
const q = <E extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<E>(`[data-testid="${id}"]`)!;
const flush = async () => {
  for (let k = 0; k < 6; k++) await Promise.resolve();
};

describe('logic module tab (unpinned workbench)', () => {
  it('config is open with source controls; mount sends no RPC (observation subscription only)', async () => {
    h = mountPanel('logic.analyzer', {});
    h.frame();
    const cfg = q<HTMLDetailsElement>('logic-config');
    expect(cfg.open).toBe(true);
    expect(cfg.querySelector('select[name="source"]')).not.toBeNull();
    expect(q('logic-start')).toBeTruthy();
    // a toggle event (fired by browsers for the default open) must not query the engine
    cfg.dispatchEvent(new Event('toggle'));
    await flush();
    expect(h.net.calls).toEqual([]);
    expect(h.net.subs.map((s) => s.filter.domains)).toEqual([['logic']]);
    expect(q('logic-capability').textContent).toContain('엔진 확인');
  });

  it('[엔진 확인] is the explicit, read-only logic.status query and shows the truthful engine state', async () => {
    h = mountPanel('logic.analyzer', {});
    h.net.responders.set('logic.status', () => ({ data: { engine: { path: null, version: null, error: 'sigrok-cli not found' }, implemented: { sources: ['synthetic'], decoders: ['uart', 'spi'], device_drivers: ['fx2lafw'], device_verified: false } } }));
    q('logic-engine-check').click();
    await flush();
    h.frame();
    expect(h.net.calls).toEqual([{ method: 'logic.status', params: {} }]);
    const line = q('logic-capability').textContent!;
    expect(line).toContain('sigrok-cli 없음 (sigrok-cli not found)');
    expect(line).toContain('구현 소스 synthetic');
    expect(line).toContain('(실물 미검증)');
  });

  it('opening a collapsed config by its summary still checks the engine once (user gesture)', async () => {
    h = mountPanel('logic.analyzer', { channel: 'lc-9' }); // pinned capture: config starts collapsed
    const cfg = q<HTMLDetailsElement>('logic-config');
    expect(cfg.open).toBe(false);
    cfg.querySelector('summary')!.click();
    await flush();
    expect(h.net.calls.map((c) => c.method)).toEqual(['logic.status']);
  });

  it('start is explicit: only the click sends logic.start', async () => {
    h = mountPanel('logic.analyzer', {});
    h.frame();
    expect(h.net.calls).toEqual([]);
    q<HTMLButtonElement>('logic-start').click();
    await flush();
    expect(h.net.calls.map((c) => c.method)).toEqual(['logic.start']);
  });
});
