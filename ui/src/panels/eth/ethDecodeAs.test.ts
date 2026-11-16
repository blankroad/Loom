// @vitest-environment jsdom
// operator-upgrade B4b (ETH-06) — read-time Decode As in the UI. Spec tests for FUNCTIONS.md F6.1–F6.16.
import { afterEach, describe, expect, it } from 'vitest';
import { mountPanel, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { DECODE_AS_KEY, decodeAsNotes, loadRules, MAX_DECODE_AS, MAX_DECODE_AS_CHANNELS, parseRuleRow, rulesText, sanitizeRules, saveRules, validateRules, type DecodeAsRule } from './decodeAs';
import { readPkt } from './ethModel';
import { fixtureQueryRows, fixtureRecord } from './fixtures';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
  document.body.replaceChildren();
});
const flush = async (hh: Harness) => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
  hh.frame();
};
const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const cmd = (c: string) => q<HTMLButtonElement>(`[data-cmd="${c}"]`);
const pktRows = (hh: Harness) => [...hh.host.querySelectorAll<HTMLElement>('.eth-list .vt-row')].filter((e) => !e.hidden);
const params = (m: string) => h!.net.calls.filter((c) => c.method === m).map((c) => c.params as Record<string, unknown>);
const input = (el: HTMLInputElement, v: string) => {
  el.value = v;
  el.dispatchEvent(new Event('input'));
};
const R = (transport: 'tcp' | 'udp', port: number, proto: 'doip' | 'someip'): DecodeAsRule => ({ transport, port, proto });
const UDP = R('udp', 64681, 'doip');

describe('decodeAs model (F6.1–F6.8)', () => {
  it('sanitizeRules keeps valid, de-duplicated, conflict-free rules (first wins), at most 8', () => {
    const many = Array.from({ length: 10 }, (_, i) => R('tcp', 40000 + i, 'someip'));
    expect(
      sanitizeRules([UDP, { ...UDP }, R('udp', 64681, 'someip'), R('tcp', 0, 'doip'), R('tcp', 70000, 'doip'), { transport: 'sctp', port: 1, proto: 'doip' }, { transport: 'udp', port: '1', proto: 'doip' }, { transport: 'udp', port: 1.5, proto: 'doip' }, 7, ...many]),
    ).toEqual([UDP, ...many.slice(0, MAX_DECODE_AS - 1)]);
    expect(sanitizeRules('nope')).toEqual([]);
  });
  it('row parsing and validation give user-facing errors with the row number', () => {
    expect(parseRuleRow('udp', ' 64681 ', 'doip')).toEqual(UDP);
    for (const p of ['', '0', '65536', 'x', '1.5', '-1']) expect(parseRuleRow('udp', p, 'doip')).toEqual({ error: '포트는 1–65535' });
    expect(parseRuleRow('sctp', '1', 'doip')).toEqual({ error: '전송 계층은 tcp 또는 udp' });
    expect(parseRuleRow('udp', '1', 'uds')).toEqual({ error: '프로토콜은 doip 또는 someip' });
    expect(validateRules([])).toEqual([]);
    expect(validateRules([{ transport: 'udp', port: '64681', proto: 'doip' }, { transport: 'udp', port: '64681', proto: 'doip' }])).toEqual([UDP]);
    expect(validateRules([{ transport: 'udp', port: '1', proto: 'doip' }, { transport: 'udp', port: '', proto: 'doip' }])).toEqual({ error: '2번째 규칙: 포트는 1–65535' });
    expect(validateRules([{ transport: 'udp', port: '5', proto: 'doip' }, { transport: 'udp', port: '5', proto: 'someip' }])).toEqual({ error: '2번째 규칙: 같은 포트(udp 5)가 다른 프로토콜로 이미 있습니다' });
    expect(validateRules(Array.from({ length: 9 }, (_, i) => ({ transport: 'udp', port: String(100 + i), proto: 'doip' })))).toEqual({ error: `규칙은 최대 ${MAX_DECODE_AS}개입니다` });
  });
  it('per-channel storage: save/load/remove, corrupt storage, bounded channel count (oldest dropped, re-save refreshes)', () => {
    saveRules('lan1', [UDP]);
    expect(loadRules('lan1')).toEqual([UDP]);
    expect(loadRules('lan2')).toEqual([]);
    saveRules('lan1', []);
    expect(loadRules('lan1')).toEqual([]);
    expect(JSON.parse(localStorage.getItem(DECODE_AS_KEY) ?? '{}')).not.toHaveProperty('lan1');
    localStorage.setItem(DECODE_AS_KEY, '{bad');
    expect(loadRules('lan1')).toEqual([]);
    localStorage.clear();
    for (let i = 0; i < MAX_DECODE_AS_CHANNELS; i++) saveRules(`c${i}`, [UDP]);
    saveRules('c0', [UDP]); // refresh: c0 becomes the newest
    saveRules('cX', [UDP]);
    expect(loadRules('c0')).toEqual([UDP]);
    expect(loadRules('c1')).toEqual([]); // the oldest after the refresh is dropped
    expect(Object.keys(JSON.parse(localStorage.getItem(DECODE_AS_KEY)!))).toHaveLength(MAX_DECODE_AS_CHANNELS);
  });
  it('text, show notes, row marker', () => {
    expect(rulesText([UDP, R('tcp', 30509, 'someip')])).toBe('udp:64681=doip, tcp:30509=someip');
    expect(rulesText([])).toBe('');
    expect(decodeAsNotes({ warnings: [{ code: 'decode_as', layer: 'udp', detail: 'UDP port 64681 decoded as doip (Decode As)' }, { code: 'malformed', layer: 'udp', detail: 'x' }] })).toEqual(['UDP port 64681 decoded as doip (Decode As)']);
    expect(decodeAsNotes(null)).toEqual([]);
    expect(readPkt({ seq: '1', decode_as_applied: true }, 'lan1')!.decodeAsApplied).toBe(true);
    expect(readPkt({ seq: '1', decode_as_applied: 'yes' }, 'lan1')!.decodeAsApplied).toBe(false);
  });
});

describe('Decode As in the panel (F6.9–F6.16)', () => {
  const rowsWith = () => [{ ...fixtureQueryRows(1, 1380)[0], proto: 'DoIP/UDS', decode_as_applied: true }, fixtureQueryRows(1, 1381)[0]];
  const showRes = (p: unknown) => ({ ref: (p as { ref: string }).ref, len: 4, bytes: { encoding: 'hex', data: '00010203' }, tree: [], warnings: [{ code: 'decode_as', layer: 'udp', detail: 'UDP port 64681 decoded as doip (Decode As)' }] });

  it('dialog edits per-channel rules (validated, ≤ 8); saved rules go to query/show/export but never to follow', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', () => ({ rows: rowsWith(), range: { from: '1380', to: '1381' }, partial_reasons: [], complete: true }));
    h.net.responders.set('eth.show', showRes);
    h.net.responders.set('eth.export', () => ({ path: '/tmp/x.pcapng', packets: 1, bytes: 1, truncated: false, gaps: [] }));
    h.net.responders.set('eth.follow', () => ({ segments: [] }));
    await flush(h);
    const btn = cmd('decode-as');
    expect([btn.textContent, btn.dataset.active]).toEqual(['Decode As', 'false']);
    btn.click();
    const dlg = q('[data-testid="eth-decodeas-dialog"]');
    expect(dlg.querySelector('header')!.textContent).toBe('Decode As — 읽기 해석 규칙 (lan1)');
    expect(dlg.textContent).toContain('저장된 기록은 바꾸지 않습니다');
    expect(dlg.querySelectorAll('.eth-da-row')).toHaveLength(0);
    dlg.querySelector<HTMLButtonElement>('[data-cmd="da-add"]')!.click();
    const row = dlg.querySelector('.eth-da-row')!;
    expect([row.querySelector<HTMLSelectElement>('select[aria-label="전송 계층"]')!.value, row.querySelector<HTMLSelectElement>('select[aria-label="해석"]')!.value]).toEqual(['udp', 'doip']);
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-decodeas-apply"]')!.click();
    expect(dlg.querySelector('.eth-da-msg')!.textContent).toBe('1번째 규칙: 포트는 1–65535');
    expect(loadRules('lan1')).toEqual([]);
    input(row.querySelector<HTMLInputElement>('input[aria-label="포트"]')!, '64681');
    // at most 8 rows
    for (let i = 0; i < 7; i++) dlg.querySelector<HTMLButtonElement>('[data-cmd="da-add"]')!.click();
    expect(dlg.querySelector<HTMLButtonElement>('[data-cmd="da-add"]')!.disabled).toBe(true);
    for (const r of [...dlg.querySelectorAll('.eth-da-row')].slice(1)) r.querySelector<HTMLButtonElement>('[data-cmd="da-remove"]')!.click();
    expect(dlg.querySelector<HTMLButtonElement>('[data-cmd="da-add"]')!.disabled).toBe(false);
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-decodeas-apply"]')!.click();
    await flush(h);
    expect(h.host.querySelector('[data-testid="eth-decodeas-dialog"]')).toBeNull();
    expect(loadRules('lan1')).toEqual([UDP]);
    expect([btn.textContent, btn.dataset.active, btn.title]).toEqual(['Decode As (1)', 'true', 'Decode As: udp:64681=doip — 완전 조회·상세·내보내기 필터에 적용 (라이브 목록·기록은 그대로)']);
    expect(q('[data-testid="eth-mode"]').textContent).toContain(' · Decode As는 완전 조회·상세에만 적용');
    // complete query carries the rules; a rule-decoded row is marked
    q('.eth-panel').dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    await flush(h);
    expect(params('eth.query').at(-1)!.decode_as).toEqual([UDP]);
    expect(q('[data-testid="eth-mode"]').textContent).toMatch(/ · Decode As udp:64681=doip$/);
    const proto = pktRows(h)[0]!.querySelectorAll<HTMLElement>('.vt-td')[5]!;
    expect([proto.textContent, proto.classList.contains('eth-da'), proto.title]).toEqual(['DoIP/UDS', true, 'Decode As 규칙으로 해석됨 (읽기 해석, 기록은 그대로)']);
    expect(pktRows(h)[1]!.querySelectorAll<HTMLElement>('.vt-td')[5]!.classList.contains('eth-da')).toBe(false);
    // detail
    pktRows(h)[0]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    expect(params('eth.show').at(-1)).toEqual({ ref: 'eth:lan1@seq:1380', decode_as: [UDP] });
    expect(q('[data-testid="eth-detail-info"]').textContent).toContain('Decode As: UDP port 64681 decoded as doip (Decode As)');
    // export
    cmd('export').click();
    await flush(h);
    q<HTMLButtonElement>('[data-testid="eth-export-run"]').click();
    await flush(h);
    expect(params('eth.export').at(-1)!.decode_as).toEqual([UDP]);
    // follow: raw transport payload of the conversation — never re-interpreted, so no rules are sent
    q<HTMLElement>('[data-testid="eth-export-result"]')?.remove();
    q('.eth-panel').dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true })); // back to live
    h.batch([fixtureRecord('eth.pkt', 1390, { proto: 'UDP', info: 'x', len: 60, stream: 2, stream_proto: 'udp' })]);
    pktRows(h).at(-1)!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    cmd('follow').click();
    await flush(h);
    expect(params('eth.follow').at(-1)).not.toHaveProperty('decode_as');
    expect(params('eth.follow').at(-1)).toMatchObject({ proto: 'udp', stream: 2 });
  });

  it('applying rules while viewing re-runs the complete query and the selected detail; removing all rules omits decode_as', async () => {
    saveRules('lan1', [UDP]);
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', () => ({ rows: rowsWith(), range: { from: '1380', to: '1381' }, partial_reasons: [], complete: true }));
    h.net.responders.set('eth.show', showRes);
    await flush(h);
    expect(cmd('decode-as').textContent).toBe('Decode As (1)'); // loaded for this channel
    q('.eth-panel').dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    await flush(h);
    pktRows(h)[0]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    const [nq, ns] = [params('eth.query').length, params('eth.show').length];
    cmd('decode-as').click();
    const dlg = q('[data-testid="eth-decodeas-dialog"]');
    expect(dlg.querySelector<HTMLInputElement>('.eth-da-row input[aria-label="포트"]')!.value).toBe('64681');
    dlg.querySelector<HTMLButtonElement>('[data-cmd="da-remove"]')!.click();
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-decodeas-apply"]')!.click();
    await flush(h);
    expect(loadRules('lan1')).toEqual([]);
    expect(params('eth.query')).toHaveLength(nq + 1);
    expect(params('eth.query').at(-1)).not.toHaveProperty('decode_as');
    expect(params('eth.show')).toHaveLength(ns + 1);
    expect(params('eth.show').at(-1)).toEqual({ ref: 'eth:lan1@seq:1380' });
    expect([cmd('decode-as').textContent, cmd('decode-as').dataset.active]).toEqual(['Decode As', 'false']);
    // cancel changes nothing
    cmd('decode-as').click();
    q('[data-testid="eth-decodeas-dialog"] [data-cmd="da-add"]').click();
    q<HTMLButtonElement>('[data-testid="eth-decodeas-dialog"] [data-cmd="da-cancel"]').click();
    expect(h.host.querySelector('[data-testid="eth-decodeas-dialog"]')).toBeNull();
    expect(loadRules('lan1')).toEqual([]);
  });

  it('rules are per channel; the unbound workbench has no Decode As button', async () => {
    saveRules('lan1', [UDP]);
    h = mountPanel('eth.packets', { channel: 'lan2' });
    h.net.responders.set('eth.query', () => ({ rows: [], partial_reasons: [], complete: true }));
    q('.eth-panel').dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    await flush(h);
    expect(cmd('decode-as').textContent).toBe('Decode As');
    expect(params('eth.query').at(-1)).not.toHaveProperty('decode_as');
    h.dispose();
    h = mountPanel('eth.packets', {});
    expect(cmd('decode-as').hidden).toBe(true);
  });
});
