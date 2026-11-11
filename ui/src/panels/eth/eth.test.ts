// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TreeHex } from '../../sdk';
import { ev, mountPanel, writeEv, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { gapRow, MAX_BYTES, MAX_ROWS, PacketStore, readPkt, samplingText } from './ethModel';
import { DIAG_REQ_HEX, DIAG_REQ_TREE, fixtureDetail, fixtureEvents, fixtureQueryRows, fixtureRecord } from './fixtures';

const hexBytes = (h: string) => new Uint8Array(h.match(/../g)!.map((x) => parseInt(x, 16)));
const flush = async (h: Harness) => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  h.frame();
};
const pktRows = (h: Harness) => [...h.host.querySelectorAll<HTMLElement>('.eth-list .vt-row')].filter((e) => !e.hidden);

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
  document.body.replaceChildren();
});

describe('TreeHex two-way sync', () => {
  const setup = () => {
    const host = document.createElement('div');
    document.body.append(host);
    const th = new TreeHex(host);
    th.set(DIAG_REQ_TREE, hexBytes(DIAG_REQ_HEX));
    const hl = () => [...host.querySelectorAll<HTMLElement>('.th-b.hl')].map((b) => Number(b.dataset.off));
    return { th, host, hl };
  };

  it('tree field → its byte range is highlighted', () => {
    const { host, hl } = setup();
    const uds = [...host.querySelectorAll<HTMLElement>('.th-node')].find((n) => n.textContent?.includes('UDS 0x22'))!;
    uds.click();
    expect(hl()).toEqual([66, 67, 68]);
  });

  it('byte → deepest field containing it is selected (ancestors expanded)', () => {
    const { th, host, hl } = setup();
    host.querySelector<HTMLElement>('.th-b[data-off="62"]')!.click();
    expect(th.selectedLabel).toBe('Source address: 0x0E80');
    expect(hl()).toEqual([62, 63]);
    host.querySelector<HTMLElement>('.th-b[data-off="68"]')!.click();
    expect(th.selectedLabel).toBe('DID: 0xF190 VIN');
    const did = [...host.querySelectorAll<HTMLElement>('.th-node')].find((n) => n.textContent?.includes('DID: 0xF190'))!;
    expect((did.closest('.th-kids') as HTMLElement).hidden).toBe(false);
  });
});

describe('eth.packets', () => {
  it('lists fixture packets with colours, data-seq, origin badge (attributed) and ↔UDS chip → reveal', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch(fixtureEvents());
    const rows = pktRows(h);
    expect(rows.map((r) => r.dataset.seq)).toEqual(['41230', '41240', '41242', '41250', '41260', '41270', '41280']);
    const req = rows[1]!;
    expect(req.className).toContain('c-doip');
    expect(req.dataset.origin).toBe('agent'); // attributed_origin over record origin=device
    expect(req.dataset.requestId).toBe('t-000418');
    expect(rows[3]!.className).toContain('c-nrc');
    expect(rows[4]!.className).toContain('c-arp');
    expect(rows[5]!.className).toContain('c-tcpwarn');
    expect(rows[6]!.className).toContain('c-sd');
    const chips = [...h.host.querySelectorAll<HTMLElement>('.chip')].filter((c) => !c.closest('.vt-row')!.hasAttribute('hidden'));
    expect(chips.map((c) => c.className)).toEqual(['vt-td chip chip-exact', 'vt-td chip chip-probable']);
    chips[0]!.click();
    expect(h.reveals).toEqual(['uds:bcm@req:t-000418']);
    expect(h.host.querySelector('[data-testid="eth-status"]')!.textContent).toContain('LIVE [SIM tap]');
  });

  it('filter: error → underline at pos + message; valid → pushdown resubscribe + complete query', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.filter.check', (p) =>
      (p as { expr: string }).expr === 'doip.type ==' ? { ok: false, pos: 12, expected: '값' } : { ok: true },
    );
    h.net.responders.set('eth.query', () => ({ rows: fixtureQueryRows(3), range: { from: '50000', to: '50002' } }));
    const input = h.host.querySelector<HTMLInputElement>('[data-testid="eth-filter-input"]')!;
    input.value = 'doip.type ==';
    input.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(160);
    const err = h.host.querySelector<HTMLElement>('[data-testid="eth-filter-err"]')!;
    expect(err.hidden).toBe(false);
    expect(err.textContent).toContain('위치 12: 값 필요');
    expect(h.host.querySelector('[data-testid="eth-filter-err-mark"]')!.textContent).toBe('␣');
    // Enter on an invalid filter does not apply
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.advanceTimersByTimeAsync(10);
    expect(h.net.subs).toHaveLength(1);

    input.value = 'doip';
    input.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(160);
    expect(err.hidden).toBe(true);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.advanceTimersByTimeAsync(10);
    expect(h.net.subs[0]!.closed).toBe(true);
    expect(h.net.subs[1]!.filter.ext).toEqual({ eth_filter: 'doip' });
    const q = h.net.calls.find((c) => c.method === 'eth.query')!;
    expect(q.params).toMatchObject({ ch: 'lan1', filter: 'doip', max: 2000, detail: false });
    h.frame();
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toContain('완전(저널 기준) · 3행 seq 50000–50002');
  });

  it('sampling badge while live; Space → complete journal query replaces the sampled rows', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', (p) => {
      expect((p as { range: { to: string } }).range.to).toBe('41280');
      return { rows: fixtureQueryRows(5), range: { from: '50000', to: '50004' } };
    });
    h.batch([...fixtureEvents(), fixtureRecord('eth.batch', 41281, { covered_first_seq: '41230', covered_last_seq: '41281', total: 1000, shown: 10 })]);
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toBe('라이브 · 표시 1/100 샘플링 · 기록 100 %');
    h.host.querySelector<HTMLElement>('.eth-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    await flush(h);
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toContain('완전(저널 기준) · 5행');
    expect(pktRows(h).map((r) => r.dataset.seq)).toEqual(['50000', '50001', '50002', '50003', '50004']);
    // Space again → back to live
    h.host.querySelector<HTMLElement>('.eth-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    h.frame();
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toContain('라이브');
  });

  it('kernel drops: red gap rows, badge total = sum of source.gap counts', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([
      fixtureRecord('eth.capture_state', 1, { state: 'LIVE', drop_stats: 'available' }),
      ev({ kind: 'source.gap', domain: 'eth', channel: 'lan1', payload: { producer_range: { producer_id: 'eth', producer_epoch: '1', from: '1', to: '37' }, count: '37', cause: 'kernel_drop' } }),
      ev({ kind: 'source.gap', domain: 'eth', channel: 'lan1', payload: { seq_range: { from: '90', to: '94' }, count: '5', cause: 'kernel_drop' } }),
    ]);
    const gaps = h.host.querySelectorAll('.eth-list .vt-row[data-gap="source.gap"]');
    expect(gaps).toHaveLength(2);
    expect(gaps[0]!.textContent).toContain('수신 전 손실 37');
    expect(h.host.querySelector('[data-testid="eth-status"]')!.textContent).toContain('커널 드롭 42');
  });

  it('kernel drop stats unavailable → "커널 드롭: 알 수 없음", never "0"', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([fixtureRecord('eth.capture_state', 1, { state: 'LIVE', drop_stats: 'unavailable' })]);
    expect(h.host.querySelector('[data-testid="eth-status"]')!.textContent).toContain('커널 드롭: 알 수 없음');
  });

  it('eth.tx row progresses by stage and shows "NIC 관찰 ✓" once a captured copy links to it', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([writeEv('tx1', 'requested', { seq: 10, kind: 'eth.tx', payload: { kind: 'raw', summary: 'DoIP diag 22 F190' } })]);
    h.batch([writeEv('tx1', 'sent', { seq: 11, kind: 'eth.tx', payload: { kind: 'raw', summary: 'DoIP diag 22 F190' } })]);
    const txRow = () => h!.host.querySelector<HTMLElement>('.eth-tx-host .vt-row[data-request-id="tx1"]')!;
    expect(txRow().dataset.state).toBe('sent');
    expect(txRow().textContent).toContain('NIC 관찰 —');
    h.batch([fixtureRecord('eth.pkt', 12, { proto: 'DoIP/UDS', len: 69, info: 'Diag 22 F1 90', tx_request_id: 'tx1', attributed_origin: 'user' }, { dir: 'tx' })]);
    expect(txRow().textContent).toContain('NIC 관찰 ✓');
  });

  it('select → detail query → tree/hex; retransmit sends the exact bytes', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', (p) => fixtureDetail((p as { ref: string }).ref));
    h.batch(fixtureEvents());
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    expect(h.net.calls.find((c) => c.method === 'eth.query')!.params).toEqual({ ref: 'eth:lan1@seq:41240', detail: true });
    expect(h.host.querySelectorAll('.th-b')).toHaveLength(69);
    expect(h.host.querySelector('[data-testid="eth-detail-info"]')!.textContent).toContain('상세');
    const resend = [...h.host.querySelectorAll('button')].find((b) => b.textContent === '재전송')!;
    expect(resend.disabled).toBe(false);
    resend.click();
    await flush(h);
    expect(h.net.calls.find((c) => c.method === 'eth.send')!.params).toEqual({ kind: 'raw', ch: 'lan1', hex: DIAG_REQ_HEX });
  });

  it('watch: capture stop works (safety direction), retransmit / capture start are not available', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' }, { hello: { rights: 'watch' } });
    h.net.responders.set('eth.query', (p) => fixtureDetail((p as { ref: string }).ref));
    h.batch(fixtureEvents());
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    const btn = (t: string) => [...h!.host.querySelectorAll('button')].find((b) => b.textContent === t)!;
    expect(btn('재전송').disabled).toBe(true);
    expect(btn('캡처 시작…').disabled).toBe(true);
    btn('■ 캡처 정지').click();
    await flush(h);
    expect(h.net.calls.map((c) => c.method)).toContain('eth.capture.stop');
    expect(h.net.calls.map((c) => c.method)).not.toContain('eth.send');
  });

  it('edit-and-send preview queries eth.send with dry_run:true only', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', (p) => fixtureDetail((p as { ref: string }).ref));
    h.net.responders.set('eth.send', (p) => ((p as { dry_run?: boolean }).dry_run ? { proto: 'DoIP/UDS', info: '22 F1 8C', risk: 'R1' } : { request_id: 'tx9' }));
    h.batch(fixtureEvents());
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await vi.advanceTimersByTimeAsync(5);
    h.frame();
    [...h.host.querySelectorAll('button')].find((b) => b.textContent === '편집 후 송신…')!.click();
    await vi.advanceTimersByTimeAsync(5);
    const dlg = h.host.querySelector<HTMLElement>('[data-testid="eth-edit-dialog"]')!;
    expect(dlg.querySelector('[data-testid="eth-edit-preview"]')!.textContent).toBe('DoIP/UDS · 22 F1 8C');
    const sends = h.net.calls.filter((c) => c.method === 'eth.send');
    expect(sends.every((c) => (c.params as { dry_run?: boolean }).dry_run === true)).toBe(true);
    [...dlg.querySelectorAll('button')].find((b) => b.textContent === '보내기')!.click();
    await vi.advanceTimersByTimeAsync(5);
    expect(h.net.calls.filter((c) => c.method === 'eth.send' && !(c.params as { dry_run?: boolean }).dry_run)).toHaveLength(1);
  });
});

describe('ethModel', () => {
  it('PacketStore evicts oldest rows past 20,000 and counts them', () => {
    const s = new PacketStore();
    for (let i = 0; i < MAX_ROWS + 500; i++) s.push(readPkt({ seq: String(i), proto: 'UDP', len: 60, info: 'x' }, 'lan1')!);
    expect(s.rows.length).toBeLessThanOrEqual(MAX_ROWS);
    expect(s.evicted).toBeGreaterThan(0);
    expect(s.rows[s.rows.length - 1]!.seq).toBe(BigInt(MAX_ROWS + 499));
    expect(s.get(0n)).toBeUndefined();
  });

  it('PacketStore keeps the 4 MiB byte budget', () => {
    const s = new PacketStore();
    const info = 'i'.repeat(2_000); // ≈ 4 KB per row estimate
    for (let i = 0; i < 2_000; i++) s.push(readPkt({ seq: String(i), proto: 'UDP', len: 60, info }, 'lan1')!);
    expect(s.bytes).toBeLessThanOrEqual(MAX_BYTES);
    expect(s.evicted).toBeGreaterThan(0);
  });

  it('sampling text is honest about record health', () => {
    expect(samplingText({ total: 1000, shown: 10 }, 'ok')).toBe('표시 1/100 샘플링 · 기록 100 %');
    expect(samplingText({ total: 10, shown: 10 }, 'degraded')).toBe('표시 1/1 · 기록 degraded');
    expect(gapRow('source.gap', 'unknown', 'kernel_drop', null, null, 5n).gap!.count).toBe('unknown');
  });
});
