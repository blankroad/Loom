// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { LoomEvent } from '../../sdk';
import { ev, mountPanel, writeEv, type Harness } from '../../sdk/testing/harness';
import { registerCanPanels } from './index';
import { DRY_RUN_DEBOUNCE_MS, parseIdFilter } from './canTrace';

registerCanPanels();
registerCanPanels(); // idempotent

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
});

const CAN = 0;
const ACT = 1;
const rows = (host: HTMLElement) => [...host.querySelectorAll<HTMLElement>('.ct-row')].filter((e) => !e.hidden);
const frame = (id: number, data: string, o: Partial<LoomEvent> = {}) =>
  ev({ kind: 'can.frame', channel: 'ch0', dir: 'rx', origin: 'device', payload: { id, ext: false, fd: false, dlc: data.length / 2, bytes: { encoding: 'hex', data } }, ...o });
const tx = (rid: string, state: Parameters<typeof writeEv>[1], origin: 'agent' | 'user' = 'agent', extra: Record<string, unknown> = {}) => ({
  ...writeEv(rid, state, { kind: 'can.tx', origin, payload: { id: 0x300, ext: false, fd: false, dlc: 1, bytes: { encoding: 'hex', data: '01' }, ...extra } }),
  channel: 'ch0',
  dir: 'tx' as const,
  client_id: origin === 'agent' ? 'agent:claude-1' : 'viewer:web',
});

describe('can.trace panel', () => {
  it('subscribes to can/<channel> and shares the activity topic shape with the shell', () => {
    h = mountPanel('can.trace', { channel: 'ch1' });
    expect(h.net.subs[CAN]!.filter).toEqual({ domains: ['can'], channels: ['ch1'] });
    expect(h.net.subs[CAN]!.opts.snapshot).toBe(true);
    expect(h.net.subs[ACT]!.filter).toEqual({ kinds: ['core.activity'] });
    expect(h.net.subs[ACT]!.opts).toEqual({ snapshot: true });
  });

  it('E-UI-3 rows: AGT tx row with request_id and stage, DEV rx 0x301', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([tx('r-1', 'requested'), tx('r-1', 'accepted'), tx('r-1', 'sent')]);
    let r = rows(h.host);
    expect(r).toHaveLength(1);
    expect(r[0]!.dataset).toMatchObject({ requestId: 'r-1', origin: 'agent', dir: 'tx', state: 'sent', id: '0x300' });
    expect(r[0]!.textContent).toContain('AGT');
    expect(r[0]!.textContent).toContain('◑보냄');
    h.batch([frame(0x300, '01', { dir: 'tx', origin: 'agent', request_id: 'r-1' }), frame(0x301, '01a5'), tx('r-1', 'confirmed')]);
    r = rows(h.host);
    expect(r).toHaveLength(2);
    expect(r[0]!.dataset.state).toBe('confirmed');
    expect(r[0]!.textContent).toContain('●확정');
    expect(r[0]!.textContent).toContain('agent:claude-1 r-1');
    expect(r[1]!.dataset).toMatchObject({ origin: 'device', dir: 'rx', id: '0x301' });
    expect(r[1]!.textContent).toContain('01 A5');
  });

  it('control-path final stage (RowStore) updates the row before the data batch', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([tx('r-5', 'sent')]);
    h.rows.apply({ ...tx('r-5', 'confirmed'), seq: '99999' });
    h.frame();
    expect(rows(h.host)[0]!.dataset.state).toBe('confirmed');
  });

  it('send line: Enter → can.send via ctx.act; USR(나) marks my row', async () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.net.responders.set('can.send', () => ({ request_id: 'u-1', write_state: 'requested' }));
    const input = h.host.querySelector<HTMLInputElement>('[data-testid="can-send-input"]')!;
    input.value = '0x300#00';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(h.net.calls.find((c) => c.method === 'can.send')?.params).toEqual({ channel: 'ch0', id: 0x300, ext: false, fd: false, data: '00' });
    h.batch([tx('u-1', 'requested', 'user', { bytes: { encoding: 'hex', data: '00' } })]);
    await new Promise((r) => setTimeout(r, 0));
    h.frame();
    const r = rows(h.host);
    expect(r[0]!.dataset).toMatchObject({ requestId: 'u-1', origin: 'user' });
    expect(r[0]!.textContent).toContain('USR(나)');
    expect(h.host.querySelector('[data-testid="can-send-msg"]')!.textContent).toContain('u-1');
  });

  it('send line: invalid input is not sent and says why', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    const input = h.host.querySelector<HTMLInputElement>('[data-testid="can-send-input"]')!;
    input.value = '0x300#010203040506070809';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(h.net.calls.some((c) => c.method === 'can.send')).toBe(false);
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(h.host.querySelector('[data-testid="can-send-msg"]')!.textContent).toMatch(/0–8바이트/);
  });

  it('watch right: send line disabled', () => {
    h = mountPanel('can.trace', { channel: 'ch0' }, { hello: { rights: 'watch' } });
    expect(h.host.querySelector<HTMLInputElement>('[data-testid="can-send-input"]')!.disabled).toBe(true);
  });

  it('gap rows: display.gap "기록 정상" only when record health is ok; loss rows red', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([
      frame(0x100, '00'),
      ev({ kind: 'display.gap', domain: 'can', channel: 'ch0', payload: { seq_range: { from: '9001', to: '12812' }, count: '3812', cause: 'slow_client' } }),
    ]);
    h.shell.recordHealth = 'degraded';
    h.batch([ev({ kind: 'display.gap', domain: 'can', channel: 'ch0', payload: { seq_range: { from: '13000', to: '13009' }, count: '10', cause: 'slow_client' } })]);
    h.batch([ev({ kind: 'record.gap', domain: 'can', channel: 'ch0', payload: { seq_range: { from: '14000', to: '14005' }, count: '6', cause: 'disk_full' } })]);
    const g = rows(h.host).filter((e) => e.dataset.gap);
    expect(g.map((e) => e.dataset.gap)).toEqual(['display.gap', 'display.gap', 'record.gap']);
    expect(g[0]!.textContent).toContain('⋯ 3,812 프레임 화면 생략 (seq 9001–12812) — 기록 정상');
    expect(g[1]!.textContent).toContain('기록 확인 안 됨');
    expect(g[2]!.className).toContain('ct-loss');
    expect(g[2]!.textContent).toContain('기록 손실 6');
  });

  it('batch display_gap summary without an event still shows a row; resync shows a row', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.net.emitBatch(CAN, [frame(0x100, '00')], { display_gap: { count: '42', seq_range: { from: '50', to: '91' }, causes: ['slow_client'] } });
    h.frame();
    h.net.subs[CAN]!.h.onResync?.('epoch_changed');
    h.frame();
    const g = rows(h.host).filter((e) => e.dataset.gap);
    expect(g.map((e) => e.dataset.gap)).toEqual(['display.gap', 'resync']);
    expect(g[0]!.textContent).toContain('42 프레임 화면 생략 (seq 50–91)');
    expect(g[1]!.textContent).toContain('재동기화 — epoch_changed');
    // after an epoch change, lower seqs are taken again (not skipped as old)
    h.batch([frame(0x222, '00', { epoch: '2', seq: '1' })]);
    expect(rows(h.host).some((e) => e.dataset.id === '0x222')).toBe(true);
  });

  it('agent read range bar from core.activity', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    const fs = [frame(0x100, '00'), frame(0x101, '00'), frame(0x102, '00')];
    h.batch(fs);
    h.batch(
      [ev({ kind: 'core.activity', domain: 'core', origin: 'agent', payload: { actor: 'claude-1', verb: 'read', domain: 'can', channel: 'ch0', range: { seq_a: fs[0]!.seq, seq_b: fs[1]!.seq }, summary: 'trace' } })],
      ACT,
    );
    const marked = rows(h.host).map((e) => e.dataset.agentRead === '1');
    expect(marked).toEqual([true, true, false]);
    expect(h.host.querySelector('[data-testid="can-agent-read"]')!.textContent).toContain(`seq ${fs[0]!.seq}–${fs[1]!.seq}`);
  });

  it('client filter by id', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([frame(0x100, '00'), frame(0x300, '00'), frame(0x301, '00')]);
    const f = h.host.querySelector<HTMLInputElement>('.ct-filter-ids')!;
    f.value = '0x300, 301';
    f.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    h.frame();
    expect(rows(h.host).map((e) => e.dataset.id)).toEqual(['0x300', '0x301']);
    expect(() => parseIdFilter('0xZZ')).toThrow();
  });

  it('pause: rendering freezes, collection continues, PAUSED +N', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([frame(0x100, '00')]);
    h.host.querySelector<HTMLButtonElement>('[data-testid="can-pause"]')!.click();
    h.batch([frame(0x101, '00'), frame(0x102, '00')]);
    expect(rows(h.host).map((e) => e.dataset.id)).toEqual(['0x100']);
    expect(h.host.querySelector('[data-testid="can-live"]')!.textContent).toMatch(/PAUSED .* \+2/);
    h.host.querySelector<HTMLButtonElement>('[data-testid="can-pause"]')!.click();
    h.frame();
    expect(rows(h.host).map((e) => e.dataset.id)).toEqual(['0x100', '0x101', '0x102']);
  });

  it('scrolled up: view stays put and offers "새 행 N"', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch(Array.from({ length: 100 }, (_, i) => frame(0x100 + i, '00')));
    const probe = (globalThis as unknown as { __loomCan: Record<string, { view: { scrollToIndex(i: number): void } }> }).__loomCan;
    probe['can.trace']!.view.scrollToIndex(10);
    h.frame();
    const firstBefore = rows(h.host)[0]!.dataset.id;
    h.batch(Array.from({ length: 30 }, () => frame(0x7ff, '00')));
    expect(rows(h.host)[0]!.dataset.id).toBe(firstBefore);
    const btn = h.host.querySelector<HTMLButtonElement>('[data-testid="can-new-rows"]')!;
    expect(btn.hidden).toBe(false);
    expect(btn.textContent).toContain('새 행 30');
    btn.click();
    h.frame();
    expect(btn.hidden).toBe(true);
  });
});

// ---------------------------------------------------------------- CAN schema v1
describe('can.trace — schema v1', () => {
  const echo = (rid: string, idx: number) =>
    ({ ...frame(0x300, '01', { dir: 'tx', origin: 'user', request_id: rid }), payload: { id: 0x300, ext: false, fd: false, dlc: 1, len: 1, bytes: { encoding: 'hex', data: '01' }, frame_index: idx } }) as LoomEvent;
  const txv1 = (rid: string, state: Parameters<typeof writeEv>[1]) => ({
    ...writeEv(rid, state, { kind: 'can.tx', origin: 'user', payload: { echo_of: { channel: 'ch0', frame: '300#01', count: 2 } } }),
    channel: 'ch0',
  });

  it('len column = bytes, DLC code in the tooltip; RTR; 29-bit id tooltip', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    const twelve = 'AB'.repeat(12);
    h.batch([
      frame(0x123, twelve, { payload: { id: 0x123, ext: false, fd: true, brs: true, dlc: 9, len: 12, bytes: { encoding: 'hex', data: twelve } } }),
      frame(0x301, '', { payload: { id: 0x301, ext: false, fd: false, rtr: true, dlc: 2, len: 2, bytes: { encoding: 'hex', data: '' } } }),
      frame(0x18daf110, '02', { payload: { id: 0x18daf110, ext: true, fd: false, dlc: 1, len: 1, bytes: { encoding: 'hex', data: '02' } } }),
    ]);
    const [fd, rtr, ext] = rows(h.host);
    expect(fd!.dataset).toMatchObject({ len: '12', dlc: '9' });
    const cells = (r: HTMLElement) => [...r.querySelectorAll<HTMLElement>('.ct-td')];
    expect(cells(fd!)[5]!.textContent).toBe('FB 12');
    expect(cells(fd!)[5]!.title).toContain('12 B · DLC 코드 9 · CAN FD BRS');
    expect(cells(rtr!)[5]!.textContent).toBe('R 2');
    expect(cells(rtr!)[6]!.textContent).toBe('RTR');
    expect(cells(ext!)[4]!.textContent).toBe('18DAF110x');
    expect(cells(ext!)[4]!.title).toBe('29bit (ext) id');
  });

  it('one send = one row per frame_index; state on every row; #k in the note', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.batch([txv1('u-7', 'sent'), echo('u-7', 0), echo('u-7', 1), txv1('u-7', 'confirmed')]);
    const r = rows(h.host);
    expect(r).toHaveLength(2);
    expect(r.map((x) => x.dataset.requestId)).toEqual(['u-7', 'u-7']);
    expect(r.map((x) => x.dataset.state)).toEqual(['confirmed', 'confirmed']);
    expect(r.map((x) => x.dataset.frameIndex)).toEqual(['0', '1']);
    expect(r[1]!.textContent).toContain('#1');
  });

  it('send: can.send structured params; D5 envelope result gives the request_id', async () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.net.responders.set('can.send', () => ({ data: { request_id: 'u-9', write_state: 'requested', risk: 'R1' } }));
    const input = h.host.querySelector<HTMLInputElement>('[data-testid="can-send-input"]')!;
    input.value = 'fd 0x123#' + '00'.repeat(12);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(h.net.calls.find((c) => c.method === 'can.send')?.params).toEqual({ channel: 'ch0', id: 0x123, ext: false, fd: true, brs: false, data: '00'.repeat(12) });
    await new Promise((r) => setTimeout(r, 0));
    expect(h.host.querySelector('[data-testid="can-send-msg"]')!.textContent).toContain('u-9');
  });
});

describe('can.trace — dry-run risk preview (debounced)', () => {
  afterEach(() => vi.useRealTimers());
  const type = (v: string) => {
    const input = h!.host.querySelector<HTMLInputElement>('[data-testid="can-send-input"]')!;
    input.value = v;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const badge = () => h!.host.querySelector<HTMLElement>('[data-testid="can-risk"]')!;
  const dryCalls = () => h!.net.calls.filter((c) => c.method === 'can.send');

  it(`one can.send{dry_run:true} ${DRY_RUN_DEBOUNCE_MS} ms after the last keystroke; badge = daemon risk + reason`, async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    h = mountPanel('can.trace', { channel: 'ch0' });
    h.net.responders.set('can.send', () => ({ data: { risk: 'R1', reason: 'sim channel', bytes: 1, policy: 'auto' } }));
    type('0x3');
    type('0x300#');
    type('0x300#01');
    expect(badge().textContent).toBe('R…');
    vi.advanceTimersByTime(DRY_RUN_DEBOUNCE_MS - 1);
    expect(dryCalls()).toHaveLength(0);
    vi.advanceTimersByTime(1);
    expect(dryCalls()).toEqual([{ method: 'can.send', params: { channel: 'ch0', id: 0x300, ext: false, fd: false, data: '01', dry_run: true } }]);
    await vi.advanceTimersByTimeAsync(0);
    expect(badge().textContent).toBe('R1');
    expect(badge().title).toContain('R1 — sim channel · 정책 auto · 1 B');
    h.frame();
    expect(badge().textContent).toBe('R1');
    expect(badge().title).toContain('sim channel');
  });

  it('invalid or empty input: no dry-run, badge back to R?; a stale answer is ignored', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    h = mountPanel('can.trace', { channel: 'ch0' });
    let release: ((v: unknown) => void) | null = null;
    h.net.responders.set('can.send', () => new Promise((r) => (release = r)));
    type('0x300#0');
    vi.advanceTimersByTime(DRY_RUN_DEBOUNCE_MS);
    expect(dryCalls()).toHaveLength(0);
    expect(badge().textContent).toBe('R?');
    type('0x300#01');
    vi.advanceTimersByTime(DRY_RUN_DEBOUNCE_MS);
    expect(dryCalls()).toHaveLength(1);
    type(''); // input changed while the answer is in flight
    release!({ risk: 'R3', reason: 'old' });
    await vi.advanceTimersByTimeAsync(0);
    expect(badge().textContent).toBe('R?');
  });
});

describe('U-3: STALLED locks the send line like the shell write lock', () => {
  it('disabled while STALLED, enabled again on the next heartbeat; UDS/UART follow the same lock', () => {
    h = mountPanel('can.trace', { channel: 'ch0' });
    const input = h.host.querySelector<HTMLInputElement>('[data-testid="can-send-input"]')!;
    expect(input.disabled).toBe(false);
    h.shell.tick(5_000); // no heartbeat for 5 s → STALLED
    h.frame();
    expect(h.shell.status.badge).toBe('STALLED');
    expect(input.disabled).toBe(true);
    expect(input.placeholder).toBe('연결 STALLED — 쓰기 잠김');
    h.shell.applyControl(ev({ kind: 'core.heartbeat', payload: { latch: false } }), 5_000);
    h.frame();
    expect(input.disabled).toBe(false);
  });
});
