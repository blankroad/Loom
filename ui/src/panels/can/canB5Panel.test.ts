// @vitest-environment jsdom
// B5 spec (Opus): CAN-5 DBC in the panel — chip, load, Receive names, on-demand decode. FUNCTIONS.md §B5-PANEL.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { LoomEvent } from '../../sdk';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { CAN_TRACE_MANIFEST } from './canTrace';
import { registerCanPanels } from './index';

registerCanPanels();

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
});

const SHA = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90';
const loaded = { channel: 'ch0', loaded: true, path: '/data/body.dbc', db_sha256: SHA, messages: 2, encoding: 'utf8', encoding_guessed: false, warnings_total: 0, blocked_total: 0 };
const catalog = { channel: 'ch0', db_sha256: SHA, messages: [{ index: 0, name: 'EngineData', id: '0x100', id_raw: 0x100, ext: false, size: 8 }], total_messages: 1, next_cursor: null, truncated: false };
const tid = <T extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<T>(`[data-testid="${id}"]`);
const calls = (m: string) => h!.net.calls.filter((c) => c.method === m).map((c) => c.params);
const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
  h!.frame();
};
const frame = (id: number, seq: number) =>
  ev({
    kind: 'can.frame', channel: 'ch0', dir: 'rx', origin: 'device', seq: String(seq),
    ts: { mono_ns: String(seq * 1_000_000), wall_ns: '1794742414000000000', clock_id: 'c', source: 'synthetic' } as LoomEvent['ts'],
    payload: { id, ext: false, fd: false, len: 2, bytes: { encoding: 'hex', data: 'dc05' } },
  });
const key = (t: HTMLElement, k: string) => {
  t.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
  h!.frame();
};

const mountWithDb = async (props: Record<string, unknown> = { channel: 'ch0', view: 'rxtx' }) => {
  h = mountPanel('can.trace', props);
  // the mount read went out before responders existed → unknown; the chip re-reads on click
  h.net.responders.set('can.db.info', () => ({ channels: [loaded] }));
  h.net.responders.set('can.db.catalog', () => catalog);
  await flush();
};

describe('CAN-5 DBC in the panel', () => {
  it('manifest declares the DB reads', () => {
    for (const q of ['can.db.info', 'can.db.catalog', 'can.db.load']) expect(CAN_TRACE_MANIFEST.queries).toContain(q);
  });

  it('unbound: no chip, no DB reads', () => {
    h = mountPanel('can.trace', { view: 'rxtx' });
    expect(tid('can-db')).toBeNull();
    expect(calls('can.db.info')).toEqual([]);
  });

  it('bound mount: one can.db.info; none → "DB: 없음"', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    await flush();
    expect(calls('can.db.info')).toEqual([{ channel: 'ch0' }]);
    // default FakeNet reply {} has no channels → unknown, not "none"
    expect(tid('can-db')!.textContent).toBe('DB: ?');
    h.net.responders.set('can.db.info', () => ({ channels: [{ channel: 'ch0', loaded: false }] }));
    tid('can-db')!.click();
    await flush();
    expect(tid('can-db-box')!.hidden).toBe(false);
    expect(calls('can.db.info').length).toBe(2);
    expect(tid('can-db')!.textContent).toBe('DB: 없음');
    expect(calls('can.db.catalog')).toEqual([]);
  });

  it('loaded DB: chip shows file · sha8; catalog read once; Receive rows show the message name', async () => {
    await mountWithDb();
    tid('can-db')!.click();
    await flush();
    expect(tid('can-db')!.textContent).toBe('DB: body.dbc · a1b2c3d4');
    expect(tid('can-db')!.className).toContain('ct-db-ok');
    expect(calls('can.db.catalog')).toEqual([{ channel: 'ch0', cursor: 0, max: 1000 }]);
    h!.batch([frame(0x100, 5), frame(0x101, 6)]);
    const rx = [...h!.host.querySelectorAll<HTMLElement>('[data-testid="can-rx-row"]')];
    const name = (id: string) => rx.find((r) => r.children[0]!.textContent === id)!.children[7]!.textContent;
    expect(name('0x100')).toBe('EngineData');
    expect(name('0x101')).toBe('');
    // same sha again → no second catalog read
    tid('can-db')!.click();
    await flush();
    expect(calls('can.db.catalog').length).toBe(1);
  });

  it('load box: absolute daemon path → can.db.load (query); needs operate; errors shown', async () => {
    await mountWithDb();
    tid('can-db')!.click();
    await flush();
    h!.net.responders.set('can.db.load', () => ({ ...loaded, path: '/data/new.dbc', db_sha256: 'ff'.repeat(32), changed: true }));
    h!.net.responders.set('can.db.catalog', () => ({ ...catalog, db_sha256: 'ff'.repeat(32) }));
    const path = tid<HTMLInputElement>('can-db-path')!;
    path.value = 'rel.dbc';
    tid('can-db-load')!.click();
    await flush();
    expect(calls('can.db.load')).toEqual([]);
    expect(tid('can-db-msg')!.textContent).toBe('경로는 데몬 컴퓨터의 절대 경로여야 합니다');
    path.value = '/data/new.dbc';
    const enc = tid<HTMLSelectElement>('can-db-encoding')!;
    enc.value = 'windows1252';
    tid('can-db-load')!.click();
    await flush();
    expect(calls('can.db.load')).toEqual([{ channel: 'ch0', path: '/data/new.dbc', encoding: 'windows1252' }]);
    expect(tid('can-db')!.textContent).toBe('DB: new.dbc · ffffffff');
    expect(tid('can-db-msg')!.textContent).toBe('불러옴: new.dbc · 메시지 2개');
    expect(calls('can.db.catalog').length).toBe(2);
    h!.net.responders.set('can.db.load', () => {
      throw new Error('E_USAGE path must be absolute');
    });
    tid('can-db-load')!.click();
    await flush();
    expect(tid('can-db-msg')!.textContent).toBe('✗ 불러오기 실패: E_USAGE path must be absolute');
  });

  it('can.db_changed in the stream → one re-read of can.db.info', async () => {
    await mountWithDb({ channel: 'ch0', view: 'trace' });
    const n = calls('can.db.info').length;
    h!.batch([ev({ kind: 'can.db_changed', channel: 'ch0', payload: { channel: 'ch0', db_sha256: SHA, previous_sha256: null, path: '/data/body.dbc' } })]);
    await flush();
    expect(calls('can.db.info').length).toBe(n + 1);
    expect(tid('can-db')!.textContent).toBe('DB: body.dbc · a1b2c3d4');
  });

  it('row detail 디코드: one trace query for that seq with decode; lines shown as text', async () => {
    await mountWithDb({ channel: 'ch0', view: 'trace' });
    tid('can-db')!.click();
    await flush();
    tid('can-db-close')!.click();
    h!.batch([frame(0x100, 41)]);
    h!.net.responders.set('can.trace.query', () => ({
      rows: [{ seq: '41', kind: 'can.frame', decoded: { message: 'EngineData', signals: [{ name: 'Rpm', value: 1500, unit: 'rpm', raw: 1500 }] } }],
    }));
    const grid = h!.host.querySelector<HTMLElement>('.ct-scroll')!;
    key(grid, 'ArrowDown');
    key(grid, 'Enter');
    const btn = tid<HTMLButtonElement>('can-row-detail-decode')!;
    expect(btn.disabled).toBe(false);
    btn.click();
    await flush();
    expect(calls('can.trace.query')).toEqual([{ channel: 'ch0', since: 'seq:41', until: 'seq:41', max: 1, decode: true }]);
    const lines = [...tid('can-row-detail-decoded')!.children].map((e) => e.textContent);
    expect(lines).toEqual(['EngineData', '  Rpm = 1500 rpm · raw 1500']);
  });

  it('decode is disabled without a loaded DB', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    h.net.responders.set('can.db.info', () => ({ channels: [{ channel: 'ch0', loaded: false }] }));
    tid('can-db')!.click();
    await flush();
    h.batch([frame(0x100, 3)]);
    const grid = h.host.querySelector<HTMLElement>('.ct-scroll')!;
    key(grid, 'ArrowDown');
    key(grid, 'Enter');
    expect(tid<HTMLButtonElement>('can-row-detail-decode')!.disabled).toBe(true);
  });
});
