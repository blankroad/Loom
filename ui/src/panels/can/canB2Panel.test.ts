// @vitest-environment jsdom
// B2 spec (Opus): CAN-8 filter chip, CAN-7 detail/keyboard/data title, CAN-6 close in the bound panel.
// FUNCTIONS.md §B2-VIEW / §B2-PANEL.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LoomEvent } from '../../sdk';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { CAN_TRACE_MANIFEST, parseIdFilter } from './canTrace';
import { registerCanPanels } from './index';
import { VIEW_CLOSE_NOTE } from './closeChannel';

registerCanPanels();

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
  vi.unstubAllGlobals();
});

let seq = 1;
const frame = (id: number, data: string, o: { ext?: boolean; fd?: boolean } = {}) =>
  ev({
    kind: 'can.frame', channel: 'ch0', dir: 'rx', origin: 'device', seq: String(seq++),
    ts: { mono_ns: String(seq * 1_000_000), wall_ns: String(1_794_742_414_000_000_000n + BigInt(seq) * 1_000_000n), clock_id: 'proc-1', source: 'synthetic' } as LoomEvent['ts'],
    payload: { id, ext: o.ext ?? false, fd: o.fd ?? false, len: data.length / 2, bytes: { encoding: 'hex', data } },
  });
const tid = <T extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<T>(`[data-testid="${id}"]`);
const rows = () => [...h!.host.querySelectorAll<HTMLElement>('.ct-row')].filter((r) => !r.hidden && r.dataset.gap === undefined);
const grid = () => h!.host.querySelector<HTMLElement>('.ct-scroll')!;
const key = (t: HTMLElement, k: string) => {
  t.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
  h!.frame();
};
const setFilter = (v: string) => {
  const i = h!.host.querySelector<HTMLInputElement>('.ct-filter-ids')!;
  i.value = v;
  i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  h!.frame();
};
const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  h!.frame();
};

describe('CAN-8 id filter in the panel', () => {
  it('ranges filter the trace; the applied filter is shown normalised; errors keep the old filter', () => {
    expect(parseIdFilter('7e0-7ef')!.has(0x7e8)).toBe(true); // canTrace still exports the parser
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    h.batch([frame(0x300, '01'), frame(0x301, '02'), frame(0x7e8, '03')]);
    const chip = tid('can-filter-norm')!;
    expect(chip.hidden).toBe(true);
    setFilter('7E0-7EF, 300');
    expect(rows().map((r) => r.dataset.id)).toEqual(['0x300', '0x7E8']);
    expect(chip.hidden).toBe(false);
    expect(chip.textContent).toBe('적용: 0x300, 0x7E0-0x7EF');
    setFilter('0x3FF-0x300');
    expect(h.host.querySelector('.ct-filter-ids')!.getAttribute('aria-invalid')).toBe('true');
    expect(tid('can-send-msg')!.textContent).toContain('범위 "0x3FF-0x300": 시작 > 끝');
    expect(rows().map((r) => r.dataset.id)).toEqual(['0x300', '0x7E8']);
    expect(chip.textContent).toBe('적용: 0x300, 0x7E0-0x7EF');
    setFilter('');
    expect(rows().length).toBe(3);
    expect(chip.hidden).toBe(true);
  });
});

describe('CAN-7 full data and row detail', () => {
  it('data cell title carries the full bytes (FD 64)', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    const hex = Array.from({ length: 64 }, (_, i) => i.toString(16).padStart(2, '0')).join('');
    h.batch([frame(0x123, hex, { fd: true })]);
    const dataCell = rows()[0]!.children[6] as HTMLElement;
    expect(dataCell.title).toBe(hex.toUpperCase().match(/../g)!.join(' '));
  });

  it('↑/↓ move the selection over frame rows; Enter opens the detail; Escape closes it', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    h.batch([frame(0x300, '01'), frame(0x301, '0203'), frame(0x302, '04')]);
    const g = grid();
    key(g, 'ArrowDown');
    expect(rows()[0]!.getAttribute('aria-selected')).toBe('true');
    key(g, 'ArrowDown');
    expect(rows()[1]!.getAttribute('aria-selected')).toBe('true');
    key(g, 'ArrowDown');
    key(g, 'ArrowDown'); // stays on the last row
    expect(rows()[2]!.getAttribute('aria-selected')).toBe('true');
    key(g, 'ArrowUp');
    expect(rows()[1]!.getAttribute('aria-selected')).toBe('true');
    const box = tid('can-row-detail')!;
    expect(box.hidden).toBe(true);
    key(g, 'Enter');
    expect(box.hidden).toBe(false);
    const f = (k: string) => box.querySelector<HTMLElement>(`[data-k="${k}"] .ct-detail-v`)!.textContent;
    expect(f('id')).toBe('0x301 (11-bit)');
    expect(f('data')).toBe('02 03');
    expect(f('seq')).toBe(rows()[1]!.dataset.seq);
    key(box, 'Escape');
    expect(box.hidden).toBe(true);
  });

  it('ArrowUp with no selection picks the last shown row; double-click opens that row', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    h.batch([frame(0x300, '01'), frame(0x301, '02')]);
    key(grid(), 'ArrowUp');
    expect(rows()[1]!.getAttribute('aria-selected')).toBe('true');
    rows()[0]!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    h.frame();
    expect(rows()[0]!.getAttribute('aria-selected')).toBe('true');
    expect(tid('can-row-detail')!.querySelector('[data-k="id"] .ct-detail-v')!.textContent).toBe('0x300 (11-bit)');
  });

  it('copy puts the detail text on the clipboard; failure says so', async () => {
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    h.batch([frame(0x300, 'A5')]);
    key(grid(), 'ArrowDown');
    key(grid(), 'Enter');
    tid('can-row-detail-copy')!.click();
    await flush();
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(String((writeText.mock.calls[0] as unknown[])[0])).toContain('데이터: A5');
    expect(tid('can-row-detail-msg')!.textContent).toBe('복사함');
    writeText.mockImplementationOnce(() => Promise.reject(new Error('denied')));
    tid('can-row-detail-copy')!.click();
    await flush();
    expect(tid('can-row-detail-msg')!.textContent).toBe('복사 실패 — 상세 줄에서 직접 선택해 복사하세요');
    tid('can-row-detail-close')!.click();
    h.frame();
    expect(tid('can-row-detail')!.hidden).toBe(true);
  });
});

describe('CAN-6 channel close', () => {
  const jobs = { jobs: [{ job: 'p1', channel: 'ch0', id: 0x100, ext: false, owner: 'cli-a', origin: 'agent', period_ms: 100, state: 'running' }] };
  const status = { channels: [{ channel: 'ch0', isotp_endpoints: 1, pending_bursts: 0, nodes: 0 }] };

  it('manifest declares the close action (operate, card) and the periodic list query', () => {
    const a = CAN_TRACE_MANIFEST.actions.find((x) => x.id === 'close')!;
    expect(a).toMatchObject({ rpc: 'can.close', class: 'request', right: 'operate', confirm: 'card' });
    expect(CAN_TRACE_MANIFEST.queries).toContain('can.periodic.list');
  });

  it('unbound panel has no close button and makes no call', () => {
    h = mountPanel('can.trace', {});
    expect(tid('can-close')).toBeNull();
    expect(h.net.calls.some((c) => c.method === 'can.periodic.list' || c.method === 'can.close')).toBe(false);
  });

  it('preview (one read each) → confirm → can.close → toast + closed badge', async () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.net.responders.set('can.periodic.list', () => jobs);
    h.net.responders.set('can.status', () => status);
    h.net.responders.set('can.close', () => ({ channel: 'ch0', state: 'closed', generation: 3 }));
    const btn = tid<HTMLButtonElement>('can-close')!;
    const before = h.net.calls.length; // B3: the bound panel reads can.periodic.list once at mount
    expect(btn.textContent).toBe('채널 닫기…');
    expect(btn.title).toContain('탭 ×는 보기만 닫음');
    btn.click();
    await flush();
    const reads = h.net.calls.slice(before).filter((c) => c.method === 'can.periodic.list' || c.method === 'can.status');
    expect(reads.map((c) => [c.method, c.params])).toEqual([
      ['can.periodic.list', { channel: 'ch0' }],
      ['can.status', { channel: 'ch0' }],
    ]);
    const box = tid('can-close-box')!;
    expect(box.hidden).toBe(false);
    const lines = [...tid('can-close-lines')!.children].map((e) => e.textContent);
    expect(lines).toContain('· p1 0x100 100 ms — agent cli-a (running)');
    expect(lines).toContain('ISO-TP 엔드포인트: 1개 닫힘');
    expect(lines[lines.length - 1]).toBe(VIEW_CLOSE_NOTE);
    expect(h.net.calls.some((c) => c.method === 'can.close')).toBe(false);
    tid<HTMLButtonElement>('can-close-confirm')!.click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'can.close').map((c) => c.params)).toEqual([{ channel: 'ch0' }]);
    expect(h.toasts).toContain('CAN ch0 닫힘 (generation 3) · 주기 송신 1개 정지');
    expect(box.hidden).toBe(true);
    expect(tid('can-closed')!.hidden).toBe(false);
    expect(btn.disabled).toBe(true);
  });

  it('cancel makes no close call; a failed read is unknown, not none', async () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.net.responders.set('can.periodic.list', () => {
      throw new Error('E_TIMEOUT');
    });
    h.net.responders.set('can.status', () => status);
    tid('can-close')!.click();
    await flush();
    const lines = [...tid('can-close-lines')!.children].map((e) => e.textContent);
    expect(lines).toContain('주기 송신: 알 수 없음');
    expect(lines).toContain('⚠ 주기 송신 목록 조회 실패: E_TIMEOUT');
    tid('can-close-cancel')!.click();
    h.frame();
    expect(tid('can-close-box')!.hidden).toBe(true);
    expect(h.net.calls.some((c) => c.method === 'can.close')).toBe(false);
    expect(tid<HTMLButtonElement>('can-close')!.disabled).toBe(false);
  });

  it('a failed close keeps the preview open with the error', async () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.net.responders.set('can.periodic.list', () => ({ jobs: [] }));
    h.net.responders.set('can.status', () => status);
    h.net.responders.set('can.close', () => {
      throw new Error('E_NOT_OPEN');
    });
    tid('can-close')!.click();
    await flush();
    tid('can-close-confirm')!.click();
    await flush();
    expect(tid('can-close-box')!.hidden).toBe(false);
    expect(tid('can-close-msg')!.textContent).toBe('✗ 닫기 실패: E_NOT_OPEN');
    expect(tid('can-closed')!.hidden).toBe(true);
    expect(tid<HTMLButtonElement>('can-close-confirm')!.disabled).toBe(false);
  });

  it('can.channel_state from the control path shows / clears the closed badge (closed by an agent)', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    const st = (state: string) => ev({ kind: 'can.channel_state', channel: 'ch0', origin: 'agent', payload: { state, channel: 'ch0' } });
    h.net.emitControl(st('closed'));
    h.frame();
    expect(tid('can-closed')!.hidden).toBe(false);
    expect(tid<HTMLButtonElement>('can-close')!.disabled).toBe(true);
    h.net.emitControl(st('open'));
    h.frame();
    expect(tid('can-closed')!.hidden).toBe(true);
    expect(tid<HTMLButtonElement>('can-close')!.disabled).toBe(false);
  });
});
