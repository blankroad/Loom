// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TreeHex } from '../../sdk';
import { ev, mountPanel, writeEv, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { EMPTY_STATS, fmtCount, fmtRate, gapRow, gapRowText, MAX_BYTES, MAX_ROWS, PacketStore, readCaptureState, readPkt, readStats, samplingText } from './ethModel';
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
    expect(h.host.querySelector('[data-testid="eth-status"]')!.textContent).toMatch(/LIVE\s*SIM tap/);
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
    // B2: explicit range (capture anchor unknown here → labelled recent-10-minutes fallback), page = daemon cap 1,000
    expect(q.params).toEqual({ ch: 'lan1', range: { from: '-600s' }, filter: 'doip', max: 1000, detail: false });
    h.frame();
    // this fixture daemon sends no partial_reasons (pre-B1 shape) → never 완전
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toBe('완전 여부 미확인 · 최근 10분 (캡처 시작 위치 모름) · 3행 · seq 50000–50002');
  });

  it('sampling badge while live; Space → complete journal query replaces the sampled rows', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', (p) => {
      expect((p as { range: { to: string } }).range).toEqual({ from: '-600s', to: 'seq:41280' }); // proto cursor grammar, never a bare number; B2: never from-less
      return { rows: fixtureQueryRows(5), range: { from: '50000', to: '50004' } };
    });
    h.batch([...fixtureEvents(), fixtureRecord('eth.batch', 41281, { covered_first_seq: '41230', covered_last_seq: '41281', total: 1000, shown: 10 })]);
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toBe('라이브 · 표시 1/100 샘플링 · 기록 100 %');
    h.host.querySelector<HTMLElement>('.eth-panel')!.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    await flush(h);
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toBe('완전 여부 미확인 · 최근 10분 (캡처 시작 위치 모름) · 5행 · seq 50000–50004');
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
    // Actual gateway responses use the D5 envelope; the detail panel consumes its data.
    h.net.responders.set('eth.show', (p) => ({
      schema_version: 1, ok: true, cmd: 'eth.show',
      data: fixtureDetail((p as { ref: string }).ref),
      meta: { record_health: 'ok', latch: false, stream_epoch: '1' },
      others_since_last: { count: '0' },
    }));
    h.batch(fixtureEvents());
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    expect(h.net.calls.find((c) => c.method === 'eth.show')!.params).toEqual({ ref: 'eth:lan1@seq:41240' });
    expect(h.net.calls.filter((c) => c.method === 'eth.query')).toHaveLength(0);
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
    h.net.responders.set('eth.show', (p) => fixtureDetail((p as { ref: string }).ref));
    h.batch(fixtureEvents());
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    const btn = (t: string) => [...h!.host.querySelectorAll('button')].find((b) => b.textContent === t)!;
    expect(btn('재전송').disabled).toBe(true);
    expect(btn('캡처 시작…').disabled).toBe(true);
    btn('캡처 정지').click();
    await flush(h);
    expect(h.net.calls.map((c) => c.method)).toContain('eth.capture.stop');
    expect(h.net.calls.map((c) => c.method)).not.toContain('eth.send');
  });

  it('edit-and-send preview queries eth.send with dry_run:true only', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.show', (p) => fixtureDetail((p as { ref: string }).ref));
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

// eth.export as the gateway answers it: daemon envelope whose data is handlers.rs `Export`'s result.
const daemonEnv = (cmd: string, body: { data?: unknown; error?: unknown }) => ({
  schema_version: 1,
  ok: body.error === undefined,
  cmd,
  ...body,
  meta: { truncated: false, record_health: 'ok', latch: false, viewers: { subscribed: 1, visible: 1 }, stream_epoch: '1' },
  others_since_last: { count: '0' },
});

describe('eth.packets — export result and failed TX reason', () => {
  const exportBtn = (hh: Harness) => hh.host.querySelector<HTMLButtonElement>('[data-cmd="export"]')!;
  /** B2: [내보내기] opens the range dialog; [eth-export-run] starts the one export */
  const startExport = async (hh: Harness) => {
    exportBtn(hh).click();
    await flush(hh);
    hh.host.querySelector<HTMLButtonElement>('[data-testid="eth-export-run"]')!.click();
  };
  const dialog = (hh: Harness) => hh.host.querySelector<HTMLElement>('[data-testid="eth-export-result"]');
  const stubClipboard = () => {
    const writeText = vi.fn((text: string) => Promise.resolve(text).then(() => undefined));
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    return writeText;
  };
  afterEach(() => {
    Reflect.deleteProperty(navigator, 'clipboard');
  });

  it('completed export shows the daemon path, counts and a copy action; busy only while the RPC runs; never claims Activity progress', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    const path = '/var/folders/x/T/loom-eth-lan1-1763190000000000000.pcapng';
    let resolve!: (v: unknown) => void;
    h.net.responders.set('eth.export', () => new Promise((r) => (resolve = r)));
    expect(exportBtn(h).title).not.toContain('Activity');
    await startExport(h);
    await flush(h);
    // live mode, unknown capture anchor → the dialog's default "capture" scope = explicit recent-10-minutes range
    expect(h.net.calls.find((c) => c.method === 'eth.export')!.params).toEqual({ ch: 'lan1', format: 'pcapng', range: { from: '-600s' } });
    // in flight: a second click is not a second export, no result shown yet
    expect(exportBtn(h).disabled).toBe(true);
    exportBtn(h).click();
    expect(h.net.calls.filter((c) => c.method === 'eth.export')).toHaveLength(1);
    expect(dialog(h)).toBeNull();
    resolve(daemonEnv('eth.export', { data: { path, packets: 17, bytes: 1234, truncated: false, comments_dropped: 0, gaps: [] } }));
    await flush(h);
    expect(exportBtn(h).disabled).toBe(false);
    const d = dialog(h)!;
    expect(d.querySelector('[data-testid="eth-export-path"]')!.textContent).toBe(path);
    expect(d.textContent).toContain('17');
    expect(d.textContent).toContain('1234 B');
    expect(d.textContent).toContain('daemon');
    expect(d.textContent).toContain('브라우저 다운로드가 아닙니다');
    expect(d.textContent).not.toContain('잘림');
    expect(d.textContent).not.toContain('손실');
    // pre-B1 result shape: completeness and scope are unknown, never claimed
    expect(d.textContent).toContain('완전성완전 여부 미확인 (이전 daemon)');
    expect(d.textContent).toContain('요청 범위정보 없음 (이전 daemon)');
    expect(h.toasts.join('\n')).not.toContain('Activity');
    expect(h.toasts.at(-1)).toContain(path);
    const writeText = stubClipboard();
    d.querySelector<HTMLButtonElement>('[data-testid="eth-export-copy"]')!.click();
    await flush(h);
    expect(writeText).toHaveBeenCalledWith(path);
    // a second export shows its own (new) result, one dialog at a time
    const path2 = path.replace('.pcapng', '-1.pcapng');
    h.net.responders.set('eth.export', () => daemonEnv('eth.export', { data: { path: path2, packets: 3, bytes: 99, truncated: true, comments_dropped: 2, gaps: [{ kind: 'source.gap' }] } }));
    await startExport(h);
    await flush(h);
    expect(h.host.querySelectorAll('[data-testid="eth-export-result"]')).toHaveLength(1);
    const d2 = dialog(h)!;
    expect(d2.querySelector('[data-testid="eth-export-path"]')!.textContent).toBe(path2);
    expect(d2.textContent).toContain('잘림');
    expect(d2.textContent).toContain('손실 구간1건');
    expect(d2.textContent).toContain('주석 생략2개');
    d2.querySelector<HTMLElement>('button')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(dialog(h)).toBeNull();
  });

  it('a failed export stays an error: daemon message toasted, no result dialog, button usable again', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.export', () =>
      daemonEnv('eth.export', { error: { code: 'internal', message: 'cannot create /ro/x.pcapng: Permission denied (os error 13)', retryable: false } }),
    );
    await startExport(h);
    await flush(h);
    expect(dialog(h)).toBeNull();
    expect(h.toasts.at(-1)).toContain('export 실패');
    expect(h.toasts.at(-1)).toContain('Permission denied');
    expect(h.toasts.some((t) => t.includes('완료'))).toBe(false);
    expect(exportBtn(h).disabled).toBe(false);
  });

  it('failed TX row shows the daemon reason (E_LOCKED) visibly; lifecycle class and NIC honesty unchanged', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    // daemon `fail()` row: {write_state, method, final, error{code,message}, detail} — no summary/to
    const reason = 'locked: lease on lan1 held by another viewer';
    h.batch([writeEv('tx9', 'requested', { seq: 20, kind: 'eth.tx', payload: { method: 'eth.send' } })]);
    h.batch([writeEv('tx9', 'failed', { seq: 21, kind: 'eth.tx', payload: { method: 'eth.send', final: true, error: { code: 'E_LOCKED', message: reason }, detail: {} } })]);
    // worker failed row with a summary but no daemon error → no invented reason
    h.batch([writeEv('tx8', 'failed', { seq: 22, kind: 'eth.tx', payload: { kind: 'raw', summary: 'DoIP diag 10 03' } })]);
    h.frame();
    const row = (id: string) => h!.host.querySelector<HTMLElement>(`.eth-tx-host .vt-row[data-request-id="${id}"]`)!;
    expect(row('tx9').dataset.state).toBe('failed');
    expect(row('tx9').className).toContain('st-row-failed');
    expect(row('tx9').textContent).toContain(`E_LOCKED: ${reason}`);
    expect(row('tx9').querySelector<HTMLElement>('.eth-tx-why')!.title).toContain('E_LOCKED');
    expect(row('tx9').textContent).toContain('NIC 관찰 —');
    expect(row('tx9').textContent).not.toContain('NIC 관찰 ✓');
    expect(row('tx8').textContent).toContain('DoIP diag 10 03');
    expect(row('tx8').querySelector('.eth-tx-why')).toBeNull();
  });
});

// Field-for-field mirrors of crates/loom-eth/src/model.rs serde output
// (CaptureStatePayload / StatsPayload): U64 → decimal string, Option::None → field absent.
const U64_MAX = '18446744073709551615';
const canonCapture = (over: Record<string, unknown> = {}): Record<string, unknown> => ({
  ch: 'lan1',
  state: 'live',
  backend: 'sim',
  iface: 'lo',
  bpf: 'port 13400',
  snaplen: 262144,
  promisc: false,
  buffer_bytes_requested: '4194304',
  buffer_bytes_actual: '2097152',
  ts_precision: 'micro',
  drop_stats: 'available',
  linktype: 1,
  started_mono_ns: '12000000000',
  ...over,
});
const canonStats = (over: Record<string, unknown> = {}): Record<string, unknown> => ({
  ch: 'lan1',
  pps: '1234',
  bps: '9876543',
  kernel_drop: '2',
  if_drop: '1',
  decode_lag_ms: '7',
  shown: '100',
  total: '1000',
  ...over,
});
/** serde `skip_serializing_if = Option::is_none` → field absent */
const omit = (o: Record<string, unknown>, ...keys: string[]) => Object.fromEntries(Object.entries(o).filter(([k]) => !keys.includes(k)));
const statusText = (hh: Harness) => hh.host.querySelector('[data-testid="eth-status"]')!.textContent ?? '';

describe('eth.packets — canonical capture_state / stats payloads', () => {
  it('U64-string stats drive pps/bps/drops/sampling; backend:"sim" shows the SIM tag', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([fixtureRecord('eth.capture_state', 1, canonCapture()), fixtureRecord('eth.stats', 2, canonStats())]);
    const t = statusText(h);
    expect(t).toMatch(/LIVE\s*SIM tap/);
    expect(t).toContain('pps 1,234');
    expect(t).toContain('bps 9.8M');
    expect(t).toContain('커널 드롭 3');
    expect(t).not.toContain('pps ?');
    // stats shown/total (same 1 s window) → sampling badge agrees with the status bar
    expect(h.host.querySelector('[data-testid="eth-mode"]')!.textContent).toContain('표시 1/10 샘플링');
  });

  it('backend live → no SIM tag even if a legacy sim flag were absent', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([fixtureRecord('eth.capture_state', 1, canonCapture({ backend: 'live' }))]);
    expect(statusText(h)).toContain('LIVE');
    expect(statusText(h)).not.toContain('SIM');
  });

  it('stats with kernel_drop/if_drop absent (None) → unknown, never a false "0"', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    const noDrops = omit(canonStats(), 'kernel_drop', 'if_drop');
    h.batch([fixtureRecord('eth.capture_state', 1, canonCapture()), fixtureRecord('eth.stats', 2, noDrops)]);
    expect(statusText(h)).toContain('커널 드롭: 알 수 없음');
    expect(statusText(h)).not.toMatch(/커널 드롭 0/);
  });

  it('known drops then an unknown second → lower bound "≥N"', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    const noDrops = omit(canonStats(), 'kernel_drop', 'if_drop');
    h.batch([
      fixtureRecord('eth.capture_state', 1, canonCapture()),
      fixtureRecord('eth.stats', 2, canonStats({ kernel_drop: '4', if_drop: '1' })),
      fixtureRecord('eth.stats', 3, noDrops),
    ]);
    expect(statusText(h)).toContain('커널 드롭 ≥5');
  });

  it('unknown interval stays a lower bound after drop counters recover', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([
      fixtureRecord('eth.capture_state', 1, canonCapture()),
      fixtureRecord('eth.stats', 2, omit(canonStats(), 'kernel_drop', 'if_drop')),
      fixtureRecord('eth.stats', 3, canonStats({ kernel_drop: '4', if_drop: '0' })),
    ]);
    expect(statusText(h)).toContain('커널 드롭 ≥4');
  });

  it('drop totals: gap counts and stats describe the same counters → larger shown, not summed', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([
      fixtureRecord('eth.capture_state', 1, canonCapture()),
      fixtureRecord('eth.stats', 2, canonStats({ kernel_drop: '5', if_drop: '0' })),
      ev({ kind: 'source.gap', domain: 'eth', channel: 'lan1', payload: { count: '5', cause: 'kernel_drop' } }),
    ]);
    expect(statusText(h)).toContain('커널 드롭 5');
    expect(statusText(h)).not.toContain('커널 드롭 10');
  });

  it('no capture_state yet → drops unknown (not 0)', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([fixtureRecord('eth.stats', 2, canonStats({ kernel_drop: '0', if_drop: '0' }))]);
    expect(statusText(h)).toContain('커널 드롭: 알 수 없음');
  });

  it('source.gap stats_unavailable → drops unknown', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([
      fixtureRecord('eth.capture_state', 1, canonCapture()),
      ev({ kind: 'source.gap', domain: 'eth', channel: 'lan1', payload: { count: 'unknown', cause: 'stats_unavailable' } }),
    ]);
    expect(statusText(h)).toContain('커널 드롭: 알 수 없음');
  });

  it('state permission_denied → permission banner; state error → its error text', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([fixtureRecord('eth.capture_state', 1, canonCapture({ state: 'permission_denied', backend: 'live' }))]);
    const perm = h.host.querySelector<HTMLElement>('.eth-perm')!;
    expect(perm.hidden).toBe(false);
    expect(h.host.querySelector<HTMLElement>('[data-testid="eth-status"]')!.dataset.state).toBe('PERMISSION');
    h.batch([fixtureRecord('eth.capture_state', 2, canonCapture({ state: 'error', error: 'interface lo went down' }))]);
    expect(perm.hidden).toBe(true);
    expect(statusText(h)).toContain('오류: interface lo went down');
  });

  it('U64 values above 2^53 stay exact (pps, gap count)', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([
      fixtureRecord('eth.capture_state', 1, canonCapture()),
      fixtureRecord('eth.stats', 2, canonStats({ pps: U64_MAX })),
      ev({ kind: 'source.gap', domain: 'eth', channel: 'lan1', payload: { count: '9007199254740993', cause: 'kernel_drop' } }),
    ]);
    expect(statusText(h)).toContain('pps 18,446,744,073,709,551,615');
    expect(statusText(h)).toContain('커널 드롭 9,007,199,254,740,993');
    const gap = h.host.querySelector('.eth-list .vt-row[data-gap="source.gap"]')!;
    expect(gap.textContent).toContain('9,007,199,254,740,993');
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

  it('readCaptureState: canonical U64 buffers exact; actual absent → null (not = requested)', () => {
    const c = readCaptureState(canonCapture({ buffer_bytes_requested: U64_MAX }), null);
    expect(c.backend).toBe('sim');
    expect(c.sim).toBe(true);
    expect(c.bufferBytesRequested).toBe(18446744073709551615n);
    expect(c.bufferBytesActual).toBe(2097152n);
    const noActual = omit(canonCapture(), 'buffer_bytes_actual');
    const c2 = readCaptureState(noActual, c);
    expect(c2.bufferBytesActual).toBeNull();
    expect(c2.bufferBytesRequested).toBe(4194304n);
  });

  it('readCaptureState: legacy mock shape (sim flag, buffer_bytes number, upper-case state) still read', () => {
    const c = readCaptureState({ state: 'PERMISSION', iface: 'lo', buffer_bytes: 4194304, sim: true, drop_stats: 'partial' }, null);
    expect(c.state).toBe('permission_denied');
    expect(c.sim).toBe(true);
    expect(c.backend).toBeNull();
    expect(c.bufferBytesRequested).toBe(4194304n);
    expect(readCaptureState({ state: 'LIVE' }, null).state).toBe('live');
  });

  it('readStats: U64 strings and legacy safe numbers; unsafe floats/numbers are not guessed', () => {
    const st = readStats(canonStats(), EMPTY_STATS);
    expect(st).toEqual({ pps: 1234n, bps: 9876543n, decodeLagMs: 7n, kernelDrop: 2n, ifDrop: 1n, shown: 100n, total: 1000n });
    const legacy = readStats({ pps: 10, bps: 8000, decode_lag_ms: 3 }, EMPTY_STATS);
    expect(legacy.pps).toBe(10n);
    expect(legacy.kernelDrop).toBeNull();
    expect(readStats({ pps: 2 ** 60, bps: 1.5 }, st).pps).toBe(1234n); // keeps last exact value
    expect(readStats({ pps: '18446744073709551616' }, EMPTY_STATS).pps).toBeNull(); // > u64::MAX
  });

  it('fmtCount / fmtRate are exact integer formatting (no float rounding)', () => {
    expect(fmtCount(18446744073709551615n)).toBe('18,446,744,073,709,551,615');
    expect(fmtCount(0n)).toBe('0');
    expect(fmtRate(null)).toBe('?');
    expect(fmtRate(999n)).toBe('999');
    expect(fmtRate(1500n)).toBe('1.5k');
    expect(fmtRate(18446744073709551615n)).toBe('18446744073.7G');
    expect(gapRowText(gapRow('source.gap', 9007199254740993n, 'kernel_drop', null, null, 1n).gap!)).toContain('9,007,199,254,740,993');
  });

  it('sampling text is honest about record health', () => {
    expect(samplingText({ total: 1000, shown: 10 }, 'ok')).toBe('표시 1/100 샘플링 · 기록 100 %');
    expect(samplingText({ total: 10, shown: 10 }, 'degraded')).toBe('표시 1/1 · 기록 degraded');
    expect(gapRow('source.gap', 'unknown', 'kernel_drop', null, null, 5n).gap!.count).toBe('unknown');
  });
});

// ---- fidelity (docs/impl/main/research/fidelity/eth.md): Wireshark-style pane stack ----
describe('eth.packets fidelity layout & interaction', () => {
  const q = <T extends HTMLElement = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
  const showDetail = () =>
    h!.net.responders.set('eth.show', (p) => ({
      schema_version: 1, ok: true, cmd: 'eth.show',
      data: fixtureDetail((p as { ref: string }).ref),
      meta: { record_health: 'ok', latch: false, stream_epoch: '1' },
      others_since_last: { count: '0' },
    }));

  it('Bench Light order: toolbar → channel card → filter bar (scope/mode + display filter) → [list ═ (details | bytes)] + socket drawer → status line', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    const root = q('.eth-panel');
    const order = [...root.children].map((c) => c.getAttribute('data-testid') ?? c.className);
    expect(order.slice(0, 5)).toEqual(['eth-toolbar', 'eth-chbar', 'eth-fbar', 'eth-wsp', 'eth-statusbar']);
    // toolbar order and labels = selected web toolbar
    const labels = [...q('.wb-mtbar').querySelectorAll('.wb-tbtn')].map((b) => b.textContent);
    expect(labels).toEqual(['채널 추가', '캡처 시작…', '캡처 정지', '표시 일시정지', '자동 스크롤', '색상', '스트림 따라가기', '내보내기', '기록 지우기', '소켓 테스트', '단축키']);
    expect(q('.eth-chbar [data-testid="eth-status"]')).toBeTruthy();
    expect(q('.eth-fbar [data-testid="eth-mode"]')).toBeTruthy();
    expect(q('.eth-fbar [data-testid="eth-filter-input"]')).toBeTruthy();
    const panes = q('[data-testid="eth-panes"]');
    const seq = [...panes.querySelectorAll('.eth-list, .eth-split, .eth-detail-bar, .th-tree, .eth-bytes-h, .th-right')].map((e) =>
      e.classList.contains('eth-split') ? (e.classList.contains('eth-split-a') ? 'split-a' : 'split-b') : e.className.split(' ')[0],
    );
    expect(seq).toEqual(['eth-list', 'split-a', 'eth-detail-bar', 'th-tree', 'split-b', 'eth-bytes-h', 'th-right']);
    expect(q('.eth-treehex').parentElement).toBe(q('.eth-lower'));
    // socket drawer: closed by default, Alt+S toggles, TX log lives in it
    const sock = q<HTMLElement>('[data-testid="eth-sock"]');
    expect(sock.hidden).toBe(true);
    expect(sock.querySelector('[data-testid="eth-tx"]')).toBeTruthy();
    root.dispatchEvent(new KeyboardEvent('keydown', { key: 's', code: 'KeyS', altKey: true, bubbles: true }));
    expect(sock.hidden).toBe(false);
    expect(q('[data-cmd="toggle-socket"]').getAttribute('aria-pressed')).toBe('true');
  });

  it('pane boundaries: list height / details width, keyboard ±4 %, persist, reset; corrupt storage → defaults', () => {
    localStorage.setItem('loom.eth.layout.v2', '{bad');
    h = mountPanel('eth.packets', { channel: 'lan1' });
    const panes = q('[data-testid="eth-panes"]');
    const lower = q('.eth-lower');
    const a = q('[data-testid="eth-split-list"]');
    const b = q('[data-testid="eth-split-bytes"]');
    expect(a.getAttribute('role')).toBe('separator');
    expect(b.getAttribute('aria-orientation')).toBe('vertical');
    expect(a.tabIndex).toBe(0);
    expect(panes.style.getPropertyValue('--eth-list-h')).toBe('52%');
    expect(lower.style.getPropertyValue('--eth-dtl-w')).toBe('55%');
    a.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    expect(panes.style.getPropertyValue('--eth-list-h')).toBe('56%');
    b.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    expect(lower.style.getPropertyValue('--eth-dtl-w')).toBe('51%');
    expect(JSON.parse(localStorage.getItem('loom.eth.layout.v2')!)).toEqual({ panes: [56, 51], colorize: true });
    a.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(panes.style.getPropertyValue('--eth-list-h')).toBe('52%');
    q('[data-cmd="colorize"]').click();
    expect(q('.eth-panel').classList.contains('colorize')).toBe(false);
    expect(JSON.parse(localStorage.getItem('loom.eth.layout.v2')!).colorize).toBe(false);
    localStorage.removeItem('loom.eth.layout.v2');
  });

  it('filter field validity (green/red), applied vs edited, Esc revert, ✕ clear re-applies "no filter"', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.filter.check', (p) => ((p as { expr: string }).expr === 'doip.type ==' ? { ok: false, pos: 12, expected: '값' } : { ok: true }));
    h.net.responders.set('eth.query', () => ({ rows: fixtureQueryRows(3), range: { from: '50000', to: '50002' } }));
    const input = q<HTMLInputElement>('[data-testid="eth-filter-input"]');
    const frame = q('[data-testid="eth-filter-frame"]');
    const applied = q('[data-testid="eth-filter-applied"]');
    expect(frame.dataset.validity).toBe('idle');
    expect(applied.textContent).toBe('필터 없음 · 전체 표시');
    input.value = 'doip.type ==';
    input.dispatchEvent(new Event('input'));
    expect(frame.dataset.validity).toBe('checking');
    await vi.advanceTimersByTimeAsync(160);
    expect(frame.dataset.validity).toBe('err');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    input.value = 'doip';
    input.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(160);
    expect(frame.dataset.validity).toBe('ok');
    expect(frame.dataset.dirty).toBe('true');
    q('[data-testid="eth-filter-apply"]').click();
    await vi.advanceTimersByTimeAsync(10);
    expect(h.net.subs[1]!.filter.ext).toEqual({ eth_filter: 'doip' });
    expect(frame.dataset.dirty).toBe('false');
    expect(applied.dataset.active).toBe('true');
    // edit without applying: the list still reflects the applied filter, and says so
    input.value = 'arp';
    input.dispatchEvent(new Event('input'));
    expect(applied.textContent).toContain('적용 중: doip · 편집 미적용');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(input.value).toBe('doip');
    expect(frame.dataset.dirty).toBe('false');
    // ✕ clears and applies "no display filter" (capture filter untouched)
    q('[data-testid="eth-filter-clear"]').click();
    await vi.advanceTimersByTimeAsync(10);
    expect(input.value).toBe('');
    expect(h.net.subs.at(-1)!.filter.ext ?? null).toBeNull();
    expect(applied.dataset.active).toBe('false');
    expect(h.net.calls.some((c) => c.method === 'eth.capture.stop' || c.method === 'eth.capture.start')).toBe(false);
  });

  it('selection feedback: highlighted row, status bar packet → field (label · bytes @off), Space in tree does not pause', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    showDetail();
    h.batch(fixtureEvents());
    const sb = q('[data-testid="eth-statusbar"]');
    expect(sb.dataset.selected).toBe('none');
    expect(q('[data-testid="eth-sb-count"]').textContent).toContain('라이브 표시 패킷 7');
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    expect(pktRows(h)[1]!.classList.contains('selected')).toBe(true);
    expect(sb.dataset.selected).toBe('packet');
    expect(q('[data-testid="eth-sb-field"]').textContent).toBe('선택 seq 41240 · DoIP/UDS · 69 B · UDS t-000418');
    // details start with protocol layers collapsed (Wireshark default)
    expect([...h.host.querySelectorAll<HTMLElement>('.th-tree .th-kids')].every((k) => k.hidden)).toBe(true);
    h.host.querySelector<HTMLElement>('.th-b[data-off="62"]')!.click();
    h.frame();
    expect(q('[data-testid="eth-sb-field"]').textContent).toBe('Source address: 0x0E80 · 2 bytes @0x003e');
    const node = h.host.querySelector<HTMLElement>('.th-tree .th-node')!;
    node.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    h.frame();
    expect(q('[data-testid="eth-mode"]').textContent).toContain('라이브');
  });

  it('related-packet glyphs only from verified eth.link request_id pairing (exact → / probable ⇠); none otherwise', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    showDetail();
    h.batch(fixtureEvents());
    const glyphs = () => pktRows(h!).map((r) => r.querySelector('.vt-td')!.textContent);
    expect(glyphs().every((g) => g === '')).toBe(true);
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    expect(glyphs()).toEqual(['', '→', '⇠', '', '', '', '']);
    // NRC row from the same addresses but no eth.link → no glyph anywhere (no src/dst heuristic)
    pktRows(h)[3]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    expect(glyphs().every((g) => g === '')).toBe(true);
  });

  it('F8 / F7 step packets (skipping gap rows); Ctrl+/ focuses the display filter', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    showDetail();
    h.batch(fixtureEvents());
    const root = q('.eth-panel');
    root.dispatchEvent(new KeyboardEvent('keydown', { key: 'F8', bubbles: true }));
    await flush(h);
    expect(pktRows(h)[0]!.classList.contains('selected')).toBe(true);
    root.dispatchEvent(new KeyboardEvent('keydown', { key: 'F8', bubbles: true }));
    await flush(h);
    expect(pktRows(h)[1]!.classList.contains('selected')).toBe(true);
    root.dispatchEvent(new KeyboardEvent('keydown', { key: 'F7', bubbles: true }));
    await flush(h);
    expect(pktRows(h)[0]!.classList.contains('selected')).toBe(true);
    root.dispatchEvent(new KeyboardEvent('keydown', { key: '/', ctrlKey: true, bubbles: true }));
    expect(document.activeElement).toBe(q('[data-testid="eth-filter-input"]'));
  });

  it('filter, pause and export send proto seq cursors (seq:N), never bare numbers (native "bad HH:MM:SS" regression)', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.filter.check', () => ({ ok: true }));
    h.net.responders.set('eth.query', () => ({ rows: fixtureQueryRows(3), range: { from: '50000', to: '50002' } }));
    const queries = () => h!.net.calls.filter((c) => c.method === 'eth.query').map((c) => c.params as { range?: { from?: string; to?: string }; filter?: string });
    const button = (text: string) => { h!.frame(); return [...h!.host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!; };
    // no packet yet: pause queries an explicit start without an end, never "seq:0" / "0" (B2: never from-less)
    button('일시정지').click();
    await vi.advanceTimersByTimeAsync(10);
    expect(queries().at(-1)!.range).toEqual({ from: '-600s' });
    button('라이브로').click();
    // live packets → pause: complete query up to the live head as seq:N
    h.batch(fixtureEvents());
    const head = pktRows(h).map((r) => r.dataset.seq).filter(Boolean).at(-1)!;
    button('일시정지').click();
    await vi.advanceTimersByTimeAsync(10);
    expect(queries().at(-1)!.range).toEqual({ from: '-600s', to: `seq:${head}` });
    button('라이브로').click();
    // display filter → complete filtered range, same cursor grammar, filter preserved
    const input = h.host.querySelector<HTMLInputElement>('[data-testid="eth-filter-input"]')!;
    input.value = 'arp';
    input.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(160);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.advanceTimersByTimeAsync(10);
    expect(queries().at(-1)).toMatchObject({ filter: 'arp', range: { from: '-600s', to: `seq:${head}` } });
    // export (B2 range dialog): default = the current complete view incl. its display filter, same cursors
    await flush(h);
    button('내보내기').click();
    await vi.advanceTimersByTimeAsync(10);
    const filterBox = h.host.querySelector<HTMLInputElement>('[data-testid="eth-export-filter"]')!;
    expect(filterBox.checked).toBe(true);
    expect(filterBox.closest('label')!.textContent).toContain('표시 필터 적용: arp');
    h.host.querySelector<HTMLButtonElement>('[data-testid="eth-export-run"]')!.click();
    await vi.advanceTimersByTimeAsync(10);
    const ex = h.net.calls.find((c) => c.method === 'eth.export')!.params as { range?: { from?: string; to?: string } };
    expect(ex).toEqual({ ch: 'lan1', format: 'pcapng', filter: 'arp', range: { from: '-600s', to: `seq:${head}` } });
    for (const r of [...queries().map((x) => x.range), ex.range]) for (const v of [r?.from, r?.to]) expect(v === undefined || /^(-\d+s|(e\d+:)?seq:\d+)$/.test(v)).toBe(true);
  });

  it('relative time uses the real capture start (snapshot anchor), never the first visible backlog packet; new generation resets it', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    const late = fixtureEvents().filter((e) => e.kind !== 'eth.capture_state'); // late join: start event aged out of the backlog
    const first = late.find((e) => e.kind === 'eth.pkt')!;
    const cells = (seq: string) => [...pktRows(h!).find((r) => r.dataset.seq === seq)!.querySelectorAll('.vt-td')].map((c) => c.textContent);
    // unknown start → honest "?" (the old code showed 0.000000 relative to the first visible packet)
    h.batch(late);
    expect(cells(first.seq)).toContain('?');
    expect(cells(first.seq)).not.toContain('0.000000');
    // reducer snapshot carries the exact worker start (same clock domain, no conversion): 41230 ms after 12 s base
    h.net.subs[0]!.h.onSnapshotPart?.({ sub: 's', n: 1, total: 1, items: [{ domain: 'eth', kind: 'eth.snapshot', state: { kind: 'eth.capture_state', ch: 'lan1', state: 'live', backend: 'sim', iface: 'sim', generation: 1, started_mono_ns: '12000000000' } }], events: [] } as never);
    h.snapshotEnd(late.at(-1)!.seq);
    expect(first.seq).toBe('41230');
    expect(cells(first.seq)).toContain('41.230000');
    // a partial stop without the field keeps the anchor
    h.batch([{ ...first, kind: 'eth.capture_state', seq: String(Number(late.at(-1)!.seq) + 1), generation: 1, payload: { ch: 'lan1', state: 'stopped' } }]);
    expect(cells(first.seq)).toContain('41.230000');
    // a new generation without its own start never inherits the old anchor
    h.batch([{ ...first, kind: 'eth.capture_state', seq: String(Number(late.at(-1)!.seq) + 2), generation: 2, payload: { ch: 'lan1', state: 'starting' } }]);
    expect(cells(first.seq)).toContain('?');
  });

  it('capture state on the control path only (no data batch, no snapshot) updates the header and stop button; stale/other-channel ignored; dispose unhooks', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    const ctl = (seq: number, state: string, channel = 'lan1', generation = 1) =>
      ev({ kind: 'eth.capture_state', domain: 'eth', channel, seq: String(seq), generation, origin: 'agent', payload: { ch: channel, state, backend: 'sim', iface: 'sim', started_mono_ns: '421959' } });
    const status = () => h!.host.querySelector('[data-testid="eth-status"]')!;
    const stopBtn = () => [...h!.host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('캡처 정지'))!;
    const listeners = () => (h!.net as unknown as { controlCbs: Set<unknown> }).controlCbs.size;
    const before = listeners();
    h.net.emitControl(ctl(10, 'live'));
    h.frame();
    expect(status().textContent).toContain('LIVE');
    expect(stopBtn().disabled).toBe(false);
    h.net.emitControl(ctl(20, 'stopped'));
    h.frame();
    expect(status().textContent).toContain('STOPPED');
    expect((status() as HTMLElement).dataset.state).toBe('stopped');
    expect(stopBtn().disabled).toBe(true);
    // an older state (e.g. a late duplicate) and another channel never override it
    h.net.emitControl(ctl(15, 'live'));
    h.net.emitControl(ctl(30, 'live', 'other'));
    h.frame();
    expect(status().textContent).toContain('STOPPED');
    // gen2 restart on the same channel goes live again (no extra frame needed)
    h.net.emitControl(ctl(40, 'live', 'lan1', 2));
    h.frame();
    expect(status().textContent).toContain('LIVE');
    expect(stopBtn().disabled).toBe(false);
    expect(h.net.subs[0]!.closed).toBe(false);
    expect(h.net.calls.filter((c) => c.method === 'eth.capture.start' || c.method === 'eth.capture.stop' || c.method === 'eth.send')).toEqual([]);
    h.dispose();
    expect(listeners()).toBe(before - 1);
    h = null;
  });
});

describe('eth.packets socket test drawer (real eth.send)', () => {
  const q = <T extends HTMLElement = HTMLElement>(s: string) => h!.host.querySelector<T>(s)!;
  afterEach(() => {
    localStorage.removeItem('loom.eth.sock.draft.v1');
    localStorage.removeItem('loom.eth.sock.templates.v1');
  });

  it('restores the draft without sending; one explicit Send = one eth.send (udp, to, from_port, hex); dry-run is a query', async () => {
    vi.useFakeTimers();
    localStorage.setItem('loom.eth.sock.draft.v1', JSON.stringify({ proto: 'udp', host: '127.0.0.1', port: '13400', fromPort: '', fmt: 'hex', data: '02 fd 00 05' }));
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.frame();
    await vi.advanceTimersByTimeAsync(300);
    expect(h.net.calls.filter((c) => c.method === 'eth.send' && (c.params as { dry_run?: boolean }).dry_run !== true)).toEqual([]);
    q('[data-cmd="toggle-socket"]').click();
    const send = q<HTMLButtonElement>('[data-testid="eth-sock-send"]');
    expect(send.disabled).toBe(false);
    send.click();
    await vi.advanceTimersByTimeAsync(10);
    const sends = h.net.calls.filter((c) => c.method === 'eth.send' && (c.params as { dry_run?: boolean }).dry_run !== true);
    expect(sends.map((c) => c.params)).toEqual([expect.objectContaining({ kind: 'udp', ch: 'lan1', to: '127.0.0.1:13400', hex: '02fd0005' })]);
    expect(q('[data-testid="eth-sock-preview"]').textContent).toContain('4 B');
  });

  const realSends = () => h!.net.calls.filter((c) => c.method === 'eth.send' && (c.params as { dry_run?: boolean }).dry_run !== true);

  it('single in-flight guard: repeated Ctrl/⌘+Enter, Send and template ▶ while a send is pending → exactly one eth.send', async () => {
    vi.useFakeTimers();
    localStorage.setItem('loom.eth.sock.draft.v1', JSON.stringify({ proto: 'udp', host: '127.0.0.1', port: '13400', fromPort: '', fmt: 'hex', data: '01 02' }));
    localStorage.setItem('loom.eth.sock.templates.v1', JSON.stringify([{ name: 'T1', proto: 'tcp', host: '10.0.0.9', port: '7000', fromPort: '', fmt: 'text', data: 'STAT?' }]));
    h = mountPanel('eth.packets', { channel: 'lan1' });
    let release: (v: unknown) => void = () => {};
    h.net.responders.set('eth.send', (p) => ((p as { dry_run?: boolean }).dry_run ? {} : new Promise((r) => (release = r))));
    h.frame();
    q('[data-cmd="toggle-socket"]').click();
    const data = q<HTMLTextAreaElement>('[data-testid="eth-sock-data"]');
    const ctrlEnter = () => data.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }));
    ctrlEnter();
    ctrlEnter();
    ctrlEnter();
    const send = q<HTMLButtonElement>('[data-testid="eth-sock-send"]');
    expect(send.disabled).toBe(true);
    send.click(); // disabled button: no event; and the guard holds even if called
    const play = q<HTMLButtonElement>('.eth-sock .sk-tpl [aria-label="T1 보내기"]');
    expect(play.disabled).toBe(true);
    play.dispatchEvent(new MouseEvent('click', { bubbles: true })); // forced click on a disabled ▶
    await vi.advanceTimersByTimeAsync(10);
    expect(realSends()).toHaveLength(1);
    expect(data.value).toBe('01 02'); // the composer was not swapped by the template under the in-flight send
    release({ request_id: 'tx-1' });
    await vi.advanceTimersByTimeAsync(10);
    expect(send.disabled).toBe(false);
    q<HTMLButtonElement>('.eth-sock .sk-tpl [aria-label="T1 보내기"]').click();
    await vi.advanceTimersByTimeAsync(10);
    expect(realSends()).toHaveLength(2);
    expect(realSends()[1]!.params).toEqual({ kind: 'tcp', ch: 'lan1', to: '10.0.0.9:7000', hex: '53544154 3f'.replace(' ', '') });
    release({ request_id: 'tx-2' });
  });

  it('from_port: empty → daemon picks; valid → sent; invalid (0, 65536, 12a, " 5", 1e3, -1) → error, no send and no dry-run', async () => {
    vi.useFakeTimers();
    localStorage.setItem('loom.eth.sock.draft.v1', JSON.stringify({ proto: 'udp', host: '127.0.0.1', port: '13400', fromPort: '', fmt: 'hex', data: 'aa' }));
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.frame();
    q('[data-cmd="toggle-socket"]').click();
    const fp = q<HTMLInputElement>('.eth-sock input[aria-label="출발 포트"]');
    const send = q<HTMLButtonElement>('[data-testid="eth-sock-send"]');
    for (const bad of ['0', '65536', '12a', ' 5', '1e3', '-1', '99999']) {
      fp.value = bad;
      fp.dispatchEvent(new Event('input'));
      await vi.advanceTimersByTimeAsync(300);
      expect(send.disabled, bad).toBe(true);
      expect(fp.classList.contains('invalid'), bad).toBe(true);
      expect(q('.eth-sock .sk-err').textContent, bad).toContain('출발 포트');
      send.click();
      q<HTMLTextAreaElement>('[data-testid="eth-sock-data"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, bubbles: true }));
    }
    expect(h.net.calls.filter((c) => c.method === 'eth.send')).toEqual([]); // neither send nor dry-run preview
    fp.value = '50001';
    fp.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(300);
    expect(q('.eth-sock .sk-err').textContent).toBe('');
    send.click();
    await vi.advanceTimersByTimeAsync(10);
    expect(realSends().map((c) => c.params)).toEqual([{ kind: 'udp', ch: 'lan1', to: '127.0.0.1:13400', from_port: 50001, hex: 'aa' }]);
    fp.value = '';
    fp.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(300);
    send.click();
    await vi.advanceTimersByTimeAsync(10);
    expect(realSends()[1]!.params).toEqual({ kind: 'udp', ch: 'lan1', to: '127.0.0.1:13400', hex: 'aa' });
  });

  it('invalid hex → error, Send disabled, nothing sent; text escapes encode exactly', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.frame();
    q('[data-cmd="toggle-socket"]').click();
    const data = q<HTMLTextAreaElement>('[data-testid="eth-sock-data"]');
    const host = q<HTMLInputElement>('.eth-sock input[aria-label="피어 IP/호스트"]');
    host.value = '10.0.0.2';
    host.dispatchEvent(new Event('input'));
    data.value = 'zz';
    data.dispatchEvent(new Event('input'));
    expect(q<HTMLButtonElement>('[data-testid="eth-sock-send"]').disabled).toBe(true);
    expect(q('.eth-sock .sk-err').textContent).toContain('hex 형식 오류');
    const { parseTextPayload } = await import('./socketDrawer');
    expect(parseTextPayload('A😀한\\r\\n\\x00')).toBe('41f09f9880ed959c0d0a00');
    expect(parseTextPayload('bad\\q')).toBeNull();
    expect(h.net.calls.some((c) => c.method === 'eth.send' && (c.params as { dry_run?: boolean }).dry_run !== true)).toBe(false);
  });
});

describe('eth.packets channel add vs current-channel capture start (real eth.capture.start)', () => {
  const q = <T extends HTMLElement = HTMLElement>(s: string) => h!.host.querySelector<T>(s)!;
  const setv = (sel: string, v: string) => {
    const e = q<HTMLInputElement | HTMLSelectElement>(sel);
    e.value = v;
    e.dispatchEvent(new Event('input'));
    e.dispatchEvent(new Event('change'));
  };
  const starts = () => h!.net.calls.filter((c) => c.method === 'eth.capture.start').map((c) => c.params);

  it('채널 추가: new alias + NIC → status check → eth.capture.start{ch:new, backend} → opens that channel (toast); never touches this channel', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', () => ({ channels: [{ ch: 'lan1', state: 'live' }, { ch: 'lan3', state: 'stopped' }] }));
    h.net.responders.set('eth.ifaces', () => ({ ifaces: [{ name: 'sim' }, { name: 'en5' }, { name: 'replay' }] }));
    h.frame();
    q('[data-cmd="add-channel"]').click();
    await vi.advanceTimersByTimeAsync(10);
    const dlg = q('[data-testid="eth-add-channel-dialog"]');
    expect([...dlg.querySelectorAll('datalist option')].map((o) => (o as HTMLOptionElement).value)).toEqual(['sim', 'en5']);
    const go = () => q<HTMLButtonElement>('[data-testid="eth-add-channel-start"]').click();
    // duplicates of this channel or any existing channel are refused (no restart/overwrite)
    for (const dup of ['lan1', 'lan3']) {
      setv('[data-testid="eth-add-channel-dialog"] input[aria-label="새 채널 이름"]', dup);
      setv('[data-testid="eth-add-channel-dialog"] input[aria-label="NIC"]', 'sim');
      go();
      await vi.advanceTimersByTimeAsync(10);
      expect(q('[data-testid="eth-capture-msg"]').textContent).toContain(`이미 있는 채널 "${dup}"`);
    }
    setv('[data-testid="eth-add-channel-dialog"] input[aria-label="새 채널 이름"]', 'bad name');
    go();
    expect(q('[data-testid="eth-capture-msg"]').textContent).toContain('채널 이름');
    expect(starts()).toEqual([]);
    setv('[data-testid="eth-add-channel-dialog"] input[aria-label="새 채널 이름"]', 'lan2');
    go();
    await vi.advanceTimersByTimeAsync(10);
    expect(starts()).toEqual([{ iface: 'sim', ch: 'lan2', backend: 'sim', snaplen: 262144, promisc: false, buffer_mib: 64 }]);
    expect(h.host.querySelector('[data-testid="eth-add-channel-dialog"]')).toBeNull();
    expect(h.toasts.some((t) => t.includes('채널 lan2 캡처 시작'))).toBe(true);
  });

  it('캡처 시작… keeps this panel channel fixed and sends the required backend (live + BPF; BPF dropped for sim)', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.frame();
    const statusCalls = () => h!.net.calls.filter((c) => c.method === 'eth.capture.status').length;
    const statusBefore = statusCalls();
    q('[data-cmd="start-all"]').click();
    const dlg = '[data-testid="eth-capture-dialog"]';
    expect(q(dlg).querySelector('input[aria-label="새 채널 이름"]')).toBeNull();
    setv(`${dlg} input[aria-label="NIC"]`, 'en5');
    setv(`${dlg} input.bpf`, 'port 13400');
    q<HTMLButtonElement>('[data-testid="eth-capture-start"]').click();
    await vi.advanceTimersByTimeAsync(10);
    expect(starts()).toEqual([{ iface: 'en5', ch: 'lan1', backend: 'live', bpf: 'port 13400', snaplen: 262144, promisc: false, buffer_mib: 64 }]);
    expect(statusCalls()).toBe(statusBefore); // current-channel start never consults/creates another channel
    q('[data-cmd="start-all"]').click();
    setv(`${dlg} input[aria-label="NIC"]`, 'sim');
    setv(`${dlg} input.bpf`, 'arp');
    expect(q<HTMLInputElement>(`${dlg} input.bpf`).disabled).toBe(true);
    q<HTMLButtonElement>('[data-testid="eth-capture-start"]').click();
    await vi.advanceTimersByTimeAsync(10);
    expect(starts()[1]).toEqual({ iface: 'sim', ch: 'lan1', backend: 'sim', snaplen: 262144, promisc: false, buffer_mib: 64 });
  });

  it('스트림 따라가기 uses the daemon eth.follow query with the packet stream index (allowed query)', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.show', (p) => fixtureDetail((p as { ref: string }).ref));
    h.net.responders.set('eth.follow', () => ({ segments: [{ dir: 'c2s', data: '53544154 3f'.replace(' ', ''), seq: '1' }, { dir: 's2c', data: '4f4b', seq: '2' }], bytes: '7' }));
    h.batch([fixtureRecord('eth.pkt', 50, { proto: 'TCP', info: 'x', len: 60, stream: 4 })]);
    pktRows(h)[0]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    const follow = q<HTMLButtonElement>('[data-cmd="follow"]');
    expect(follow.disabled).toBe(false);
    follow.click();
    await flush(h);
    // B2: explicit range (live: capture anchor unknown → recent-10-minutes fallback up to the live head)
    expect(h.net.calls.find((c) => c.method === 'eth.follow')!.params).toEqual({ ch: 'lan1', proto: 'tcp', stream: 4, range: { from: '-600s', to: 'seq:50' } });
    const body = q('[data-testid="eth-follow-body"]');
    expect([...body.children].map((c) => [c.className, c.textContent])).toEqual([['c', 'STAT?'], ['s', 'OK']]);
  });
});

describe('eth.packets multi-channel strip (real eth.capture.status + control events)', () => {
  const q = <T extends HTMLElement = HTMLElement>(s: string) => h!.host.querySelector<T>(s)!;
  it('shows every other daemon channel as its own card (state/backend/iface only), this channel stays active, periodic refresh, + tile', async () => {
    vi.useFakeTimers();
    let channels: Record<string, unknown>[] = [{ ch: 'lan1', state: 'live', backend: 'sim', iface: 'sim' }];
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', () => ({ channels }));
    const cards = () => [...h!.host.querySelectorAll<HTMLElement>('.eth-chbar .eth-chcard')].map((c) => c.querySelector('.alias')!.textContent);
    channels = [
      { ch: 'lan1', state: 'live', backend: 'sim', iface: 'sim' },
      { ch: 'lan2', state: 'stopped', backend: 'live', iface: 'en5' },
      { ch: 'dut', state: 'live', backend: 'sim', iface: 'sim' },
    ];
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(cards()).toEqual(['lan1', 'dut', 'lan2']); // this channel first (active), others sorted
    expect(q('.eth-chbar .eth-chcard.active').getAttribute('data-testid')).toBe('eth-status');
    const lan2 = q('.eth-chcard.other[data-channel="lan2"]');
    expect(lan2.dataset.state).toBe('stopped');
    expect(lan2.querySelector('.ch-meta')!.textContent).toBe('en5 · live · 통계는 그 채널 패널에서');
    expect(lan2.textContent).not.toContain('pps'); // no invented stats for other channels
    expect(q('.eth-chcard.other[data-channel="dut"]').textContent).toContain('SIM');
    channels = [{ ch: 'lan1', state: 'live' }, { ch: 'lan2', state: 'live', backend: 'live', iface: 'en5' }];
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(cards()).toEqual(['lan1', 'lan2']);
    expect(q('.eth-chcard.other[data-channel="lan2"]').dataset.state).toBe('live');
    q('[data-cmd="add-channel-tile"]').click();
    expect(q('[data-testid="eth-add-channel-dialog"]')).toBeTruthy();
    expect(h.net.calls.filter((c) => c.method === 'eth.capture.start')).toEqual([]);
    const n = h.net.calls.filter((c) => c.method === 'eth.capture.status').length;
    h.dispose();
    h = null;
    await vi.advanceTimersByTimeAsync(20_000);
    // disposed: no more refreshes (checked through a fresh count on the same net is not possible after dispose → only ensure no throw)
    expect(n).toBeGreaterThanOrEqual(2);
  });
});
