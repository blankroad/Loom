// @vitest-environment jsdom
// B1 spec (Opus): CAN-3 in the panel — the transmit list survives close/reopen per channel (this
// browser), JSON export/import by copy/paste, explicit storage status. FUNCTIONS.md §B1-RXTX / §B1-PANEL.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mountPanel, type Harness } from '../../sdk/testing/harness';
import { registerCanPanels } from './index';
import { storeKey, TX_DOC_FORMAT } from './txStore';

registerCanPanels();

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
  vi.restoreAllMocks();
});

const tid = <T extends HTMLElement = HTMLElement>(id: string) => h!.host.querySelector<T>(`[data-testid="${id}"]`)!;
const txRows = () => [...h!.host.querySelectorAll<HTMLElement>('[data-testid=can-tx-row]')];
const flush = async () => {
  for (let k = 0; k < 6; k++) await Promise.resolve();
};
const saved = (ch = 'ch0') => {
  const v = localStorage.getItem(storeKey(ch));
  return v === null ? null : (JSON.parse(v) as { channel: string; entries: { frame: string; comment: string; cycle_ms: number }[] });
};
const doc = (entries: { frame: string; comment?: string; cycle_ms?: number }[], channel = 'ch0') =>
  JSON.stringify({ format: TX_DOC_FORMAT, version: 1, channel, saved_at: '2026-11-15T00:00:00.000Z', entries: entries.map((e) => ({ comment: '', cycle_ms: 0, ...e })) });
/** add one message through the real editor */
const addViaEditor = (idHex: string, bytes: string[], comment = '') => {
  tid('can-tx-new').click();
  const id = tid<HTMLInputElement>('can-tx-id');
  id.value = idHex;
  id.dispatchEvent(new Event('input', { bubbles: true }));
  const len = tid<HTMLSelectElement>('can-tx-len');
  len.value = String(bytes.length);
  len.dispatchEvent(new Event('change', { bubbles: true }));
  const cells = [...h!.host.querySelectorAll<HTMLInputElement>('[data-testid=can-tx-byte]')];
  bytes.forEach((b, i) => {
    cells[i]!.value = b;
    cells[i]!.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const c = tid<HTMLInputElement>('can-tx-comment');
  c.value = comment;
  c.dispatchEvent(new Event('input', { bubbles: true }));
  tid('can-tx-editor').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  h!.frame();
};
const type = (id: string, v: string) => {
  const t = tid<HTMLTextAreaElement>(id);
  t.value = v;
  t.dispatchEvent(new Event('input', { bubbles: true }));
};

describe('CAN-3 transmit list persistence (this browser, per channel)', () => {
  it('restores the saved list on mount; another channel stays empty', () => {
    localStorage.setItem(storeKey('ch0'), doc([{ frame: '300#01', comment: 'wake' }, { frame: '18DAF110#021003' }]));
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    expect(txRows().map((r) => r.dataset.id)).toEqual(['0x300', '18DAF110x']);
    expect(txRows()[0]!.textContent).toContain('wake');
    expect(txRows().every((r) => r.dataset.count === '0')).toBe(true);
    expect(tid('can-tx-persist').dataset.state).toBe('ok');
    expect(tid('can-tx-persist').textContent).toContain('2개 불러옴');
    h.dispose();
    h = mountPanel('can.trace', { channel: 'ch1', view: 'rxtx' });
    expect(txRows()).toHaveLength(0);
  });

  it('editor changes are saved without a timer (next frame), and survive close → reopen', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    addViaEditor('300', ['01', '02'], 'nm');
    expect(saved()).toMatchObject({ channel: 'ch0', entries: [{ frame: '300#0102', comment: 'nm', cycle_ms: 0 }] });
    expect(tid('can-tx-persist').textContent).toContain('이 브라우저에 저장됨');
    h.dispose();
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    expect(txRows().map((r) => r.dataset.id)).toEqual(['0x300']);
  });

  it('send counters are not persisted and do not cause a save', () => {
    localStorage.setItem(storeKey('ch0'), doc([{ frame: '300#01' }]));
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    const before = localStorage.getItem(storeKey('ch0'));
    txRows()[0]!.click();
    tid('can-tx-send').click();
    h.frame();
    expect(localStorage.getItem(storeKey('ch0'))).toBe(before);
  });

  it('deleting the last entry removes the saved list', () => {
    localStorage.setItem(storeKey('ch0'), doc([{ frame: '300#01' }]));
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    txRows()[0]!.click();
    tid('can-tx-delete').click();
    h.frame();
    expect(localStorage.getItem(storeKey('ch0'))).toBeNull();
  });

  it('unbound module: list is session-only and says so', () => {
    h = mountPanel('can.trace', { view: 'rxtx' });
    addViaEditor('300', ['01']);
    expect(Object.keys(localStorage).filter((k) => k.startsWith('loom.can.txlist'))).toEqual([]);
    expect(tid('can-tx-persist').dataset.state).toBe('off');
    expect(tid('can-tx-persist').textContent).toContain('채널 미선택');
  });

  it('a corrupt saved list is reported and NOT overwritten until the user changes the list', () => {
    localStorage.setItem(storeKey('ch0'), 'not json');
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    h.frame();
    expect(tid('can-tx-persist').dataset.state).toBe('err');
    expect(tid('can-tx-persist').textContent).toContain('JSON');
    expect(localStorage.getItem(storeKey('ch0'))).toBe('not json');
    expect(txRows()).toHaveLength(0);
  });

  it('storage write failure is visible; the list keeps working in memory', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError');
    });
    addViaEditor('300', ['01']);
    expect(txRows()).toHaveLength(1);
    expect(tid('can-tx-persist').dataset.state).toBe('err');
    expect(tid('can-tx-persist').textContent).toContain('저장 안 됨');
  });

  it('closing the panel flushes a change made since the last frame', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    tid('can-tx-new').click();
    const id = tid<HTMLInputElement>('can-tx-id');
    id.value = '301';
    id.dispatchEvent(new Event('input', { bubbles: true }));
    tid('can-tx-editor').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    h.dispose(); // no frame in between
    h = null;
    expect(saved()?.entries.map((e) => e.frame)).toEqual(['301#0000000000000000']);
  });
});

describe('CAN-3 JSON export / import (copy & paste, no download claim)', () => {
  it('export shows the versioned document; copy failure is reported, not faked', async () => {
    localStorage.setItem(storeKey('ch0'), doc([{ frame: '300#01', comment: 'a' }]));
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    tid('can-tx-export').click();
    expect(tid('can-tx-io').hidden).toBe(false);
    expect(tid('can-tx-io').dataset.mode).toBe('export');
    const box = tid<HTMLTextAreaElement>('can-tx-io-text');
    expect(box.readOnly).toBe(true);
    const d = JSON.parse(box.value) as { format: string; channel: string; entries: { frame: string }[] };
    expect(d).toMatchObject({ format: TX_DOC_FORMAT, channel: 'ch0', entries: [{ frame: '300#01', comment: 'a', cycle_ms: 0 }] });
    tid('can-tx-io-copy').click(); // jsdom: no navigator.clipboard
    await flush();
    expect(tid('can-tx-io-msg').textContent).toContain('복사 실패');
    tid('can-tx-io-close').click();
    expect(tid('can-tx-io').hidden).toBe(true);
  });

  it('import append: valid lines added, bad ones reported with index; saved', () => {
    localStorage.setItem(storeKey('ch0'), doc([{ frame: '100#00' }]));
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    tid('can-tx-import').click();
    expect(tid('can-tx-io').dataset.mode).toBe('import');
    expect(tid<HTMLTextAreaElement>('can-tx-io-text').readOnly).toBe(false);
    type('can-tx-io-text', '["300#01", "zz"]');
    tid('can-tx-io-append').click();
    h.frame();
    expect(txRows().map((r) => r.dataset.id)).toEqual(['0x100', '0x300']);
    expect(tid('can-tx-io-msg').textContent).toContain('1개 추가');
    expect(tid('can-tx-io-msg').textContent).toContain('#2');
    expect(saved()?.entries.map((e) => e.frame)).toEqual(['100#00', '300#01']);
  });

  it('import replace swaps the list; a broken document changes nothing', () => {
    localStorage.setItem(storeKey('ch0'), doc([{ frame: '100#00' }]));
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    tid('can-tx-import').click();
    type('can-tx-io-text', '{"format":"other"}');
    tid('can-tx-io-replace').click();
    h.frame();
    expect(txRows().map((r) => r.dataset.id)).toEqual(['0x100']);
    expect(tid('can-tx-io-msg').className).toContain('err');
    type('can-tx-io-text', doc([{ frame: '301#01' }, { frame: '302#02' }], 'other-bench'));
    tid('can-tx-io-replace').click();
    h.frame();
    expect(txRows().map((r) => r.dataset.id)).toEqual(['0x301', '0x302']);
    expect(tid('can-tx-io-msg').textContent).toContain('2개로 바꿈');
  });

  it('typing a space in the import box neither pauses the trace nor sends', () => {
    h = mountPanel('can.trace', { channel: 'ch0', view: 'rxtx' });
    tid('can-tx-import').click();
    const box = tid<HTMLTextAreaElement>('can-tx-io-text');
    box.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    h.frame();
    expect(tid('can-pause').getAttribute('aria-pressed')).not.toBe('true');
    expect(h.net.calls.filter((c) => c.method === 'can.send')).toHaveLength(0);
  });
});
