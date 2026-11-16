// @vitest-environment jsdom
// B4 spec (Opus): CAN-4 export from the bound panel. FUNCTIONS.md §B4-PANEL.
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

const frame = (id: number, seq: number, dir: 'rx' | 'tx' = 'rx') =>
  ev({
    kind: 'can.frame', channel: 'ch0', dir, origin: dir === 'rx' ? 'device' : 'agent', seq: String(seq),
    ts: { mono_ns: String(seq * 1_000_000), wall_ns: String(1_794_742_414_000_000_000n + BigInt(seq) * 1_000_000n), clock_id: 'c', source: 'synthetic' } as LoomEvent['ts'],
    payload: { id, ext: false, fd: false, len: 1, bytes: { encoding: 'hex', data: '01' } },
  });
const tid = <T extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<T>(`[data-testid="${id}"]`);
const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  h!.frame();
};
const result = {
  channel: 'ch0', format: 'blf', location: 'daemon', path: '/tmp/loom-can-ch0-r1.blf', sidecar: '/tmp/loom-can-ch0-r1.blf.loom.jsonl',
  frames: 3, gaps: 0, gaps_dropped: 0, truncated: false, truncated_reason: null, next_cursor: null, complete: true,
  range: { from: '101', to: '103' }, clock_id: 'c', wall_anchor_known: true,
};
const exportCalls = () => h!.net.calls.filter((c) => c.method === 'can.export').map((c) => c.params);

describe('CAN-4 export in the panel', () => {
  it('manifest: export action is a watch-right job without confirmation', () => {
    expect(CAN_TRACE_MANIFEST.actions.find((a) => a.id === 'export')).toMatchObject({ rpc: 'can.export', class: 'job', right: 'watch', confirm: 'none' });
  });

  it('unbound panel has no export button', () => {
    h = mountPanel('can.trace', {});
    expect(tid('can-export')).toBeNull();
  });

  it('ring scope → one can.export with the ring seq bounds; result says daemon machine, not a download', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    h.batch([frame(0x300, 101), frame(0x301, 102), frame(0x302, 103, 'tx')]);
    h.net.responders.set('can.export', () => result);
    const btn = tid<HTMLButtonElement>('can-export')!;
    expect(btn.textContent).toBe('내보내기…');
    expect(btn.title).toContain('데몬');
    expect(tid('can-export-box')!.hidden).toBe(true);
    btn.click();
    h.frame();
    expect(tid('can-export-box')!.hidden).toBe(false);
    expect(tid<HTMLSelectElement>('can-export-format')!.value).toBe('blf');
    expect(tid<HTMLSelectElement>('can-export-scope')!.value).toBe('ring');
    expect(tid<HTMLButtonElement>('can-export-copy')!.hidden).toBe(true);
    tid('can-export-go')!.click();
    await flush();
    expect(exportCalls()).toEqual([{ channel: 'ch0', format: 'blf', since: 'seq:101', until: 'seq:103' }]);
    const lines = [...tid('can-export-result')!.children].map((e) => e.textContent);
    expect(lines[0]).toBe('데몬 컴퓨터에 저장됨 (브라우저 다운로드 아님): /tmp/loom-can-ch0-r1.blf');
    expect(lines).toContain('3프레임 · BLF · seq 101–103 · 갭 0');
    expect(tid('can-export-result')!.classList.contains('warn')).toBe(false);
    expect(tid<HTMLButtonElement>('can-export-copy')!.hidden).toBe(false);
    expect(h.host.querySelector('a[download]')).toBeNull();
  });

  it('current filter + asc + recent scope + explicit path', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    h.batch([frame(0x300, 101)]);
    h.net.responders.set('can.export', () => result);
    const ids = h.host.querySelector<HTMLInputElement>('.ct-filter-ids')!;
    ids.value = '300, 7e0-7ef';
    ids.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    h.frame();
    tid('can-export')!.click();
    const fmt = tid<HTMLSelectElement>('can-export-format')!;
    fmt.value = 'asc';
    fmt.dispatchEvent(new Event('change', { bubbles: true }));
    const scope = tid<HTMLSelectElement>('can-export-scope')!;
    scope.value = 'recent';
    scope.dispatchEvent(new Event('change', { bubbles: true }));
    tid<HTMLInputElement>('can-export-usefilter')!.click();
    tid<HTMLInputElement>('can-export-path')!.value = '/data/cap.asc';
    tid('can-export-go')!.click();
    await flush();
    expect(exportCalls()).toEqual([{ channel: 'ch0', format: 'asc', since: '-10m', path: '/data/cap.asc', ids: ['0x300', '0x7E0-0x7EF'] }]);
  });

  it('a bad request makes no call; a daemon error is shown', async () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'trace' });
    tid('can-export')!.click();
    tid('can-export-go')!.click(); // ring scope, empty ring
    await flush();
    expect(exportCalls()).toEqual([]);
    expect(tid('can-export-msg')!.textContent).toBe('이 패널에 보관된 프레임이 없습니다 — "최근 10분"을 고르세요');
    expect(tid('can-export-msg')!.classList.contains('err')).toBe(true);

    h.batch([frame(0x300, 7)]);
    tid<HTMLInputElement>('can-export-path')!.value = 'rel.blf';
    tid('can-export-go')!.click();
    await flush();
    expect(exportCalls()).toEqual([]);

    tid<HTMLInputElement>('can-export-path')!.value = '';
    h.net.responders.set('can.export', () => {
      throw new Error('E_INTERNAL cannot create');
    });
    tid('can-export-go')!.click();
    await flush();
    expect(exportCalls().length).toBe(1);
    expect(tid('can-export-msg')!.textContent).toBe('✗ 내보내기 실패: E_INTERNAL cannot create');
    expect(tid<HTMLButtonElement>('can-export-go')!.disabled).toBe(false);
    tid('can-export-close')!.click();
    expect(tid('can-export-box')!.hidden).toBe(true);
  });
});
