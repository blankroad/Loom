// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { LoomEvent } from '../../sdk';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { registerCanPanels } from './index';
import { DRY_RUN_DEBOUNCE_MS } from './canTrace';

registerCanPanels();

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
});

let seq = 1;
const frame = (id: number, data: string, ms: number, o: Partial<LoomEvent> = {}, p: Record<string, unknown> = {}) =>
  ev({
    kind: 'can.frame', channel: 'ch0', dir: 'rx', origin: 'device', seq: String(seq++),
    ts: { mono_ns: String(ms * 1e6) } as LoomEvent['ts'],
    payload: { id, ext: false, fd: false, dlc: data.length / 2, bytes: { encoding: 'hex', data }, ...p }, ...o,
  });
const q = <T extends HTMLElement = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const qa = (sel: string) => [...h!.host.querySelectorAll<HTMLElement>(sel)];
const tid = <T extends HTMLElement = HTMLElement>(id: string) => q<T>(`[data-testid="${id}"]`);
const key = (target: HTMLElement, k: string) => target.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));
const flush = () => new Promise((r) => setTimeout(r, 0));

describe('can.trace views: Receive/Transmit vs Trace', () => {
  it('default stays the chronological trace; view:"rxtx" opens Receive/Transmit; the trace grid is the first grid', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    expect(tid('can-trace').dataset.view).toBe('trace');
    expect(tid('can-rxtx').hidden).toBe(true);
    expect(tid('can-view-trace').getAttribute('aria-selected')).toBe('true');
    expect(q('[data-testid=can-trace] [role=grid]').getAttribute('aria-label')).toBe('CAN 트레이스');
    h.dispose();
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    expect(tid('can-trace').dataset.view).toBe('rxtx');
    expect(tid('can-rxtx').hidden).toBe(false);
    tid('can-view-trace').click();
    expect(tid('can-trace').dataset.view).toBe('trace');
    expect(tid('can-rxtx').hidden).toBe(true);
  });

  it('Receive list: one row per id with latest bytes / count / flags, while the trace keeps every frame', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.batch([frame(0x310, '0001', 1000), frame(0x310, '0002', 1010), frame(0x18daf110, 'aa'.repeat(12), 1012, {}, { ext: true, fd: true, brs: true, dlc: 9, len: 12 }), frame(0x310, '0003', 1020)]);
    const rows = qa('[data-testid=can-rx-row]');
    expect(rows.map((r) => r.dataset.id)).toEqual(['0x310', '18DAF110x']);
    expect(rows[0]!.dataset).toMatchObject({ count: '3', data: '0003', flags: 'STD', lowerBound: 'false', cycle: '10.000' });
    expect(rows[0]!.textContent).toContain('00 03');
    expect(rows[1]!.dataset.flags).toBe('EXT,FD,BRS');
    expect(rows[1]!.children[2]!.textContent).toBe('12');
    expect(tid('can-rx-window').dataset).toMatchObject({ frames: '4', gaps: '0' });
    // the raw trace is unchanged: four frame rows
    expect(qa('.ct-row[data-seq]').filter((r) => !r.dataset.gap)).toHaveLength(4);
  });

  it('gap event: existing rows become lower bounds ("≥"), window shows the gap separately from frames', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.batch([
      frame(0x100, '00', 0),
      ev({ kind: 'source.gap', domain: 'can', channel: 'ch0', payload: { seq_range: { from: '5', to: '9' }, count: '5', cause: 'driver_overrun' } }),
      frame(0x200, '00', 5),
    ]);
    const [a, b] = qa('[data-testid=can-rx-row]');
    expect(a!.dataset.lowerBound).toBe('true');
    expect(a!.children[5]!.textContent).toBe('≥1');
    expect(b!.dataset.lowerBound).toBe('false');
    expect(tid('can-rx-window').dataset.gaps).toBe('1');
    expect(tid('can-rx-window').textContent).toMatch(/gap 1건 \(알려진 유실 5\)/);
  });

  it('Receive row → Enter opens the trace filtered to that id; the filter also narrows the summary', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.batch([frame(0x100, '00', 0), frame(0x200, '00', 1)]);
    const body = q('[aria-label="CAN 수신 요약"]');
    key(body, 'ArrowDown');
    key(body, 'ArrowDown');
    expect(qa('[data-testid=can-rx-row]')[1]!.getAttribute('aria-selected')).toBe('true');
    key(body, 'Enter');
    expect(tid('can-trace').dataset.view).toBe('trace');
    expect(q<HTMLInputElement>('.ct-filter-ids').value).toBe('0x200');
    h.frame();
    tid('can-view-rxtx').click();
    h.frame();
    expect(qa('[data-testid=can-rx-row]').map((r) => r.dataset.id)).toEqual(['0x200']);
  });

  it('reset clears only the summary window, not the trace ring', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.batch([frame(0x100, '00', 0)]);
    tid('can-rx-reset').click();
    expect(qa('[data-testid=can-rx-row]')).toHaveLength(0);
    expect(qa('.ct-row[data-seq]')).toHaveLength(1);
  });
});

describe('Transmit list: structured editor, selection, gated send', () => {
  const addMessage = (fill: (ed: HTMLElement) => void) => {
    tid('can-tx-new').click();
    const ed = tid('can-tx-editor');
    expect(ed.hidden).toBe(false);
    fill(ed);
    tid<HTMLButtonElement>('can-tx-ok').click();
  };
  const setVal = (sel: string, v: string, evt = 'input') => {
    const i = q<HTMLInputElement | HTMLSelectElement>(sel);
    i.value = v;
    i.dispatchEvent(new Event(evt, { bubbles: true }));
  };

  it('New message → list row → Space sends the selected entry via can.send (same params as the send line)', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.net.responders.set('can.send', () => ({ request_id: 'u-7', write_state: 'requested' }));
    addMessage(() => {
      setVal('[data-testid=can-tx-id]', '18DAF110');
      const ext = tid<HTMLInputElement>('can-tx-ext');
      ext.checked = true;
      ext.dispatchEvent(new Event('change', { bubbles: true }));
      setVal('[data-testid=can-tx-len]', '3', 'change');
      const cells = qa('[data-testid=can-tx-byte]') as HTMLInputElement[];
      expect(cells).toHaveLength(3);
      ['02', '10', '03'].forEach((v, i) => {
        cells[i]!.value = v;
        cells[i]!.dispatchEvent(new Event('input', { bubbles: true }));
      });
      expect(tid('can-tx-error').textContent).toBe('→ 18DAF110x#02 10 03');
    });
    expect(tid('can-tx-editor').hidden).toBe(true);
    const row = tid('can-tx-row');
    expect(row.dataset).toMatchObject({ id: '18DAF110x', count: '0' });
    expect(row.getAttribute('aria-selected')).toBe('true');
    key(q('[aria-label="CAN 송신 목록"]'), ' ');
    const call = h.net.calls.filter((c) => c.method === 'can.send' && !(c.params as { dry_run?: boolean }).dry_run);
    expect(call.map((c) => c.params)).toEqual([{ channel: 'ch0', id: 0x18daf110, ext: true, fd: false, data: '021003' }]);
    // Space in the list did not toggle the trace render pause
    expect(tid('can-pause').getAttribute('aria-pressed')).not.toBe('true');
    await flush();
    h.frame();
    expect(tid('can-tx-row').dataset.count).toBe('1');
    expect(tid('can-tx-msg').textContent).toContain('u-7');
  });

  it('editor validation blocks OK with a reason; typing space in the editor neither sends nor pauses', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    tid('can-tx-new').click();
    setVal('[data-testid=can-tx-id]', '800');
    expect(tid<HTMLButtonElement>('can-tx-ok').disabled).toBe(true);
    expect(tid('can-tx-error').textContent).toMatch(/Extended/);
    key(tid('can-tx-id'), ' ');
    expect(h.net.calls.some((c) => c.method === 'can.send')).toBe(false);
    expect(tid('can-pause').getAttribute('aria-pressed')).not.toBe('true');
  });

  it('FD toggles the length list (64 allowed), BRS only with FD; RTR and cycle time are present but disabled', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    tid('can-tx-new').click();
    const lens = () => [...q<HTMLSelectElement>('[data-testid=can-tx-len]').options].map((o) => Number(o.value));
    expect(lens()).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
    expect(tid<HTMLInputElement>('can-tx-brs').disabled).toBe(true);
    const fd = tid<HTMLInputElement>('can-tx-fd');
    fd.checked = true;
    fd.dispatchEvent(new Event('change', { bubbles: true }));
    expect(lens()).toContain(64);
    expect(tid<HTMLInputElement>('can-tx-brs').disabled).toBe(false);
    expect(tid<HTMLInputElement>('can-tx-rtr').disabled).toBe(true);
    expect(tid<HTMLInputElement>('can-tx-cycle').disabled).toBe(true);
    expect(tid('can-tx-periodic-deferred').textContent).toMatch(/미지원/);
  });

  it('editor draft drives the same dry-run risk preview as the send line', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.net.responders.set('can.send', () => ({ data: { risk: 'R1', reason: 'sim channel', bytes: 1, policy: 'auto' } }));
    tid('can-tx-new').click();
    setVal('[data-testid=can-tx-id]', '300');
    setVal('[data-testid=can-tx-len]', '1', 'change');
    vi.advanceTimersByTime(DRY_RUN_DEBOUNCE_MS);
    const dry = h.net.calls.filter((c) => c.method === 'can.send');
    expect(dry.at(-1)!.params).toEqual({ channel: 'ch0', id: 0x300, ext: false, fd: false, data: '00', dry_run: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(tid('can-risk').textContent).toBe('R1');
  });

  it('edit (Enter) replaces the entry; Delete removes it', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    addMessage(() => setVal('[data-testid=can-tx-id]', '100'));
    const list = q('[aria-label="CAN 송신 목록"]');
    key(list, 'Enter');
    expect(q<HTMLInputElement>('[data-testid=can-tx-id]').value).toBe('100');
    setVal('[data-testid=can-tx-id]', '101');
    tid<HTMLButtonElement>('can-tx-ok').click();
    expect(qa('[data-testid=can-tx-row]').map((r) => r.dataset.id)).toEqual(['0x101']);
    key(list, 'Delete');
    expect(qa('[data-testid=can-tx-row]')).toHaveLength(0);
  });

  it('watch right: list send disabled, nothing reaches can.send', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' }, { hello: { rights: 'watch' } });
    addMessage(() => setVal('[data-testid=can-tx-id]', '100'));
    h.frame();
    expect(tid<HTMLButtonElement>('can-tx-send').disabled).toBe(true);
    key(q('[aria-label="CAN 송신 목록"]'), ' ');
    expect(h.net.calls.some((c) => c.method === 'can.send' && !(c.params as { dry_run?: boolean }).dry_run)).toBe(false);
  });
});
