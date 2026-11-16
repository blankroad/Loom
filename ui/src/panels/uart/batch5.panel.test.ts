// @vitest-environment jsdom
// U1-B batch 5 — the REC dialog's "지난 구간 내보내기 (데몬 저널)" (FUNCTIONS.md §B5.2). Opus-owned specification; Sonnet must not edit.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import { EXPORT_QUERY } from './journalExport';
import './terminalPanel';

beforeAll(() => {
  if (!window.matchMedia) {
    window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
  }
  (URL as unknown as { createObjectURL: () => string }).createObjectURL = vi.fn(() => 'blob:loom-test');
  (URL as unknown as { revokeObjectURL: () => void }).revokeObjectURL = vi.fn();
});

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  document.querySelectorAll('.ub-layer').forEach((n) => n.remove());
});

const flush = () => new Promise((r) => setTimeout(r, 0));
const settle = async () => { for (let i = 0; i < 10; i++) await flush(); h!.frame(); };
const hex = (s: string) => Array.from(new TextEncoder().encode(s), (b) => b.toString(16).padStart(2, '0')).join('');
const dlg = () => document.querySelector<HTMLDialogElement>('dialog[aria-label="UART 기록"]')!;
const inDlg = <T extends HTMLElement = HTMLElement>(sel: string) => dlg().querySelector<T>(sel);

function openPanel() {
  h = mountPanel('uart.terminal', { channel: 'dut' });
  h.batch([
    ev({ kind: 'uart.state', channel: 'dut', generation: 2, payload: { state: 'open', path: '/dev/ttyUSB0', transport: 'serial' } }),
    ev({ kind: 'uart.config', channel: 'dut', generation: 2, payload: { baud: 115200, data_bits: 8, parity: 'none', stop_bits: '1', flow: 'none' } }),
  ]);
  h.host.querySelector<HTMLButtonElement>('[data-act="rec"]')!.click();
  return h;
}
const page = (events: unknown[], o: Record<string, unknown> = {}) => ({ events, count: events.length, truncated: false, next_cursor: null, stream_epoch: '7', stream_seq_hw: '2', ...o });
const rec = (seq: number) => ({
  schema_version: 1, session_id: 's', epoch: '7', seq: String(seq), domain: 'uart', channel: 'dut', kind: 'uart.data', dir: 'rx', origin: 'device',
  producer_id: 'p', producer_epoch: '1', source_seq: String(seq), generation: 2,
  ts: { mono_ns: '1', wall_ns: `${Date.UTC(2026, 10, 16, 0, 0, seq)}000000`, clock_id: 'host', source: 'host_read_complete' },
  payload: { bytes: { encoding: 'hex', data: hex(`line ${seq}\n`) }, len: 7, rx_offset: (seq - 1) * 7 },
});

describe('REC dialog: journal export (U1-B)', () => {
  it('declares core.events.query and offers the three ranges with the live generation', () => {
    openPanel();
    expect(inDlg('fieldset[data-role="jx"]')).not.toBeNull();
    const values = [...dlg().querySelectorAll<HTMLInputElement>('input[name="jxrange"]')].map((i) => i.value);
    expect(values).toEqual(['retained', 'recent', 'generation']);
    expect(inDlg<HTMLInputElement>('input[name="jxrange"][value="retained"]')!.checked).toBe(true);
    expect([...dlg().querySelectorAll<HTMLOptionElement>('[data-act="jx-minutes"] option')].map((o) => o.value)).toEqual(['5', '15', '60', '240']);
    expect(inDlg('input[name="jxrange"][value="generation"]')!.closest('label')!.textContent).toContain('gen 2');
    expect(inDlg('[data-act="jx-start"]')).not.toBeNull();
    expect(dlg().textContent).toContain('이전 데몬 실행의 기록은 포함하지 않');
  });

  it('exports with peek, requests a download and reports completeness', async () => {
    openPanel();
    h!.net.responders.set(EXPORT_QUERY, (p) => ((p as { since?: string }).since ? page([rec(1), rec(2)]) : page([rec(2)])));
    inDlg<HTMLButtonElement>('[data-act="jx-start"]')!.click();
    await settle();
    const calls = h!.net.calls.filter((c) => c.method === EXPORT_QUERY);
    expect(calls.length).toBeGreaterThan(0);
    expect(calls.every((c) => (c.params as { peek?: boolean }).peek === true)).toBe(true);
    const toast = h!.toasts.at(-1) ?? '';
    expect(toast).toMatch(/^다운로드 요청: dut-journal-\d{8}-\d{6}\.log/);
    expect(toast).toContain('complete');
    expect(toast).toContain('저장 완료 여부는 브라우저/앱의 다운로드에서 확인하세요');
    expect(inDlg('[data-role="jxstat"]')!.textContent).toContain('마지막 내보내기');
    expect(inDlg('[data-act="jx-start"]')).not.toBeNull();
  });

  it('a recent range sends a since/until probe and keeps the selection across re-renders', async () => {
    openPanel();
    h!.net.responders.set(EXPORT_QUERY, (p) => ((p as { since?: string }).since ? page([rec(2)]) : page([rec(2)])));
    inDlg<HTMLInputElement>('input[name="jxrange"][value="recent"]')!.click();
    inDlg<HTMLInputElement>('input[name="jxrange"][value="recent"]')!.dispatchEvent(new Event('change', { bubbles: true }));
    const sel = inDlg<HTMLSelectElement>('[data-act="jx-minutes"]')!;
    sel.value = '60';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    inDlg<HTMLButtonElement>('[data-act="jx-start"]')!.click();
    await settle();
    expect(inDlg<HTMLInputElement>('input[name="jxrange"][value="recent"]')!.checked).toBe(true);
    expect(inDlg<HTMLSelectElement>('[data-act="jx-minutes"]')!.value).toBe('60');
    expect(h!.toasts.at(-1)).toMatch(/^다운로드 요청/);
  });

  it('cancel stops the export and makes no file', async () => {
    (URL.createObjectURL as unknown as { mockClear(): void }).mockClear();
    openPanel();
    let release: (() => void) | null = null;
    h!.net.responders.set(EXPORT_QUERY, (p) => {
      const q = p as { since?: string; max?: number };
      if (!q.since) return page([rec(2)]);
      if ((q.max ?? 0) <= 1) return page([rec(1)]);
      return new Promise((r) => { release = () => r(page([rec(1)], { truncated: true, next_cursor: 'e7:seq:2' })); });
    });
    inDlg<HTMLButtonElement>('[data-act="jx-start"]')!.click();
    await settle();
    expect(inDlg('[data-act="jx-cancel"]')).not.toBeNull();
    expect(inDlg('[data-role="jxstat"]')!.textContent).not.toBe('');
    inDlg<HTMLButtonElement>('[data-act="jx-cancel"]')!.click();
    release!();
    await settle();
    expect(h!.toasts.at(-1)).toBe('저널 내보내기 취소 — 파일을 만들지 않았습니다');
    expect((URL.createObjectURL as unknown as { mock: { calls: unknown[] } }).mock.calls.length).toBe(0);
  });

  it('a daemon failure still produces a partial file and says partial', async () => {
    openPanel();
    h!.net.responders.set(EXPORT_QUERY, () => { throw Object.assign(new Error('hub stopped'), { dataCode: 'E_DAEMON' }); });
    inDlg<HTMLButtonElement>('[data-act="jx-start"]')!.click();
    await settle();
    expect(h!.toasts.at(-1)).toContain('partial');
    expect(h!.toasts.at(-1)).toContain('query_failed');
  });
});
