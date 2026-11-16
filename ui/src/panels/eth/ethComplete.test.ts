// @vitest-environment jsdom
// operator-upgrade B2 — ETH-01 (explicit complete range, honest badge, paging, export scope, follow range) and
// ETH-05 (unbound start screen) DOM behaviour. Spec tests authored with FUNCTIONS.md B2 (F2.13–F2.30).
import { afterEach, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { fixtureDetail, fixtureQueryRows, fixtureRecord } from './fixtures';

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
const badge = () => q('[data-testid="eth-mode"]');
const queries = () => h!.net.calls.filter((c) => c.method === 'eth.query').map((c) => c.params as { range?: Record<string, string>; max?: number; filter?: string });
const pktRows = (hh: Harness) => [...hh.host.querySelectorAll<HTMLElement>('.eth-list .vt-row')].filter((e) => !e.hidden);
const pause = () => q('.eth-panel').dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
const select = (sel: HTMLSelectElement, v: string) => {
  sel.value = v;
  sel.dispatchEvent(new Event('change'));
};
const type = (el: HTMLInputElement, v: string) => {
  el.value = v;
  el.dispatchEvent(new Event('input'));
};

/** eth.query result as the B1 daemon answers it */
const B1 = (o: Record<string, unknown> = {}) => ({
  ch: 'lan1',
  rows: [],
  range: { from: null, to: null },
  matched: 0,
  truncated: false,
  complete: true,
  scan_complete: true,
  partial_reasons: [],
  scope: { from: 'e1:seq:1', to: null, from_source: 'explicit', lookback_secs: null },
  scanned: { first_seq: null, last_seq: null, bytes: 0 },
  resume_cursor: null,
  prev_cursor: null,
  next_cursor: null,
  gaps: [],
  gaps_omitted: 0,
  ...o,
});
const anchorStatus = (start: string | null = 'e1:seq:1', extra: Record<string, unknown>[] = []) => () => ({
  channels: [{ ch: 'lan1', state: 'stopped', backend: 'sim', iface: 'sim', generation: 1, start_cursor: start, start_seq: start ? start.split(':').at(-1) : null }, ...extra],
});

describe('ETH-01 complete range (bound panel)', () => {
  it('starts at the daemon capture anchor (status start_cursor), page size 1,000, and shows 완전 only for a complete daemon result', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', anchorStatus());
    h.net.responders.set('eth.query', () =>
      B1({ rows: fixtureQueryRows(1, 1380), range: { from: '1380', to: '1380' }, matched: 1, scanned: { first_seq: '1380', last_seq: '2000', bytes: 100 }, next_cursor: 'seq:1381' }),
    );
    await flush(h); // status refresh on mount → anchor known (the mount refresh happens before the responder in this harness: refresh again)
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '5', generation: 1, payload: { ch: 'lan1', state: 'stopped' } }));
    await flush(h);
    pause();
    await flush(h);
    expect(queries().at(-1)).toEqual({ ch: 'lan1', range: { from: 'e1:seq:1' }, max: 1000, detail: false });
    expect(badge().textContent).toBe('완전 · 캡처 시작 이후 · 1행 · seq 1380–1380');
    expect(badge().dataset.complete).toBe('complete');
    const scope = q('[data-testid="eth-range-scope"]').textContent!;
    expect(scope).toBe('요청 범위 e1:seq:1 → 최신 · 스캔 seq 1380–2000 · 스캔 범위 전체 확인');
    expect(pktRows(h).map((r) => r.dataset.seq)).toEqual(['1380']);
    expect(q<HTMLSelectElement>('[data-testid="eth-range"]').value).toBe('capture');
  });

  it('unknown anchor → explicit recent-10-minutes fallback (never a from-less query); live head becomes `to`', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', anchorStatus(null));
    h.net.responders.set('eth.query', () => B1({ scope: { from: '-600s', to: 'seq:60', from_source: 'explicit', lookback_secs: null } }));
    h.batch([fixtureRecord('eth.pkt', 60, { proto: 'UDP', info: 'x', len: 60 })]);
    pause();
    await flush(h);
    expect(queries().at(-1)!.range).toEqual({ from: '-600s', to: 'seq:60' });
    expect(badge().textContent).toBe('완전 · 최근 10분 (캡처 시작 위치 모름) · 0행');
    for (const r of queries().map((x) => x.range)) expect(r?.from).toBeTruthy();
  });

  it('complete:true with loss / row_cap / default_lookback / unknown reasons is never shown as 완전; an older daemon is 완전 여부 미확인', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', anchorStatus());
    let res: unknown = null;
    h.net.responders.set('eth.query', () => res);
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '5', generation: 1, payload: { ch: 'lan1', state: 'stopped' } }));
    await flush(h);
    const run = async (r: unknown) => {
      res = r;
      select(q<HTMLSelectElement>('[data-testid="eth-range"]'), 'capture');
      await flush(h!);
      return badge();
    };
    let b = await run(B1({ rows: fixtureQueryRows(2, 1), range: { from: '1', to: '2' }, matched: 5000, truncated: true, partial_reasons: ['loss', 'row_cap'], gaps: [{}] }));
    expect(b.textContent).toBe('부분 · 캡처 시작 이후 · 2행 / 일치 5,000 · seq 1–2 — 손실 구간 1건 · 최대 1,000행 초과');
    expect(b.dataset.complete).toBe('partial');
    b = await run(B1({ partial_reasons: ['default_lookback'], complete: false }));
    expect(b.textContent).toContain('기본 조회(최근 10분)만');
    b = await run(B1({ partial_reasons: ['future_x'] }));
    expect(b.textContent).toContain('기타 사유 future_x');
    expect(b.dataset.complete).toBe('partial');
    b = await run({ rows: fixtureQueryRows(1, 9), range: { from: '9', to: '9' }, complete: true, truncated: false });
    expect(b.textContent).toBe('완전 여부 미확인 · 캡처 시작 이후 · 1행 · seq 9–9');
    expect(b.dataset.complete).toBe('unknown');
    expect(q('[data-testid="eth-range-scope"]').textContent).toBe('요청/스캔 범위 정보 없음 (이전 daemon)');
  });

  it('range select: recent N and typed seq ranges; invalid input toasts and sends nothing', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', () => B1());
    await flush(h);
    const sel = q<HTMLSelectElement>('[data-testid="eth-range"]');
    expect([...sel.options].map((o) => [o.value, o.textContent])).toEqual([
      ['capture', '캡처 시작 이후'],
      ['recent:600', '최근 10분'],
      ['recent:3600', '최근 1시간'],
      ['recent:21600', '최근 6시간'],
      ['seq', 'seq 범위…'],
    ]);
    const from = q<HTMLInputElement>('[data-testid="eth-range-from"]');
    const to = q<HTMLInputElement>('[data-testid="eth-range-to"]');
    expect(from.hidden && to.hidden).toBe(true);
    select(sel, 'recent:3600');
    await flush(h);
    expect(queries().at(-1)!.range).toEqual({ from: '-3600s' });
    expect(badge().textContent).toContain('최근 1시간');
    const before = queries().length;
    select(sel, 'seq'); // choosing "seq 범위…" only shows the inputs
    await flush(h);
    expect(from.hidden || to.hidden).toBe(false);
    expect(queries()).toHaveLength(before);
    type(from, 'abc');
    cmd('range-apply').click();
    await flush(h);
    expect(h.toasts.at(-1)).toBe('시작 seq를 숫자로 입력하세요');
    type(from, '10');
    type(to, '5');
    cmd('range-apply').click();
    await flush(h);
    expect(h.toasts.at(-1)).toBe('시작 seq가 끝 seq보다 큽니다');
    expect(queries()).toHaveLength(before);
    type(to, '20');
    cmd('range-apply').click();
    await flush(h);
    expect(queries().at(-1)!.range).toEqual({ from: 'seq:10', to: 'seq:20' });
    expect(badge().textContent).toContain('seq 10–20');
  });

  it('paging: older = same from + prev_cursor as to, newer walks back, resume continues from resume_cursor; buttons follow the page', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', anchorStatus());
    const pages: Record<string, unknown> = {
      '': B1({ rows: fixtureQueryRows(2, 4000), range: { from: '4000', to: '4001' }, matched: 3000, truncated: true, partial_reasons: ['row_cap'], prev_cursor: 'seq:3999' }),
      'seq:3999': B1({ rows: fixtureQueryRows(2, 3000), range: { from: '3000', to: '3001' }, matched: 2000, truncated: true, partial_reasons: ['row_cap'], prev_cursor: 'seq:2999' }),
      'seq:2999': B1({ rows: fixtureQueryRows(2, 2000), range: { from: '2000', to: '2001' }, matched: 1000, prev_cursor: null }),
    };
    h.net.responders.set('eth.query', (p) => {
      const r = (p as { range: { from: string; to?: string } }).range;
      if (r.from === 'e1:seq:5000') return B1({ rows: fixtureQueryRows(1, 5000), range: { from: '5000', to: '5000' }, matched: 1 });
      return pages[r.to ?? ''];
    });
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '5', generation: 1, payload: { ch: 'lan1', state: 'stopped' } }));
    await flush(h);
    // live: paging disabled
    expect([cmd('page-older').disabled, cmd('page-newer').disabled, cmd('page-resume').disabled]).toEqual([true, true, true]);
    expect([cmd('page-older').textContent, cmd('page-newer').textContent, cmd('page-resume').textContent]).toEqual(['◀ 이전 1,000행', '최신 쪽 ▶', '계속 스캔 ▶']);
    pause();
    await flush(h);
    expect([cmd('page-older').disabled, cmd('page-newer').disabled]).toEqual([false, true]);
    cmd('page-older').click();
    await flush(h);
    cmd('page-older').click();
    await flush(h);
    expect(cmd('page-older').disabled).toBe(true); // no prev_cursor on the oldest page
    cmd('page-newer').click();
    await flush(h);
    cmd('page-newer').click();
    await flush(h);
    expect(cmd('page-newer').disabled).toBe(true);
    expect(queries().map((x) => x.range)).toEqual([
      { from: 'e1:seq:1' },
      { from: 'e1:seq:1', to: 'seq:3999' },
      { from: 'e1:seq:1', to: 'seq:2999' },
      { from: 'e1:seq:1', to: 'seq:3999' },
      { from: 'e1:seq:1' },
    ]);
    // a scan-budget stop offers 계속 스캔 from the daemon's resume cursor
    pages[''] = B1({ rows: fixtureQueryRows(1, 4500), range: { from: '4500', to: '4500' }, matched: 1, complete: false, scan_complete: false, partial_reasons: ['scan_budget'], resume_cursor: 'e1:seq:5000' });
    select(q<HTMLSelectElement>('[data-testid="eth-range"]'), 'capture');
    await flush(h);
    expect(badge().textContent).toContain('스캔 한도(64 MiB) 도달');
    expect(cmd('page-resume').disabled).toBe(false);
    cmd('page-resume').click();
    await flush(h);
    expect(queries().at(-1)!.range).toEqual({ from: 'e1:seq:5000' });
    expect(pktRows(h).map((r) => r.dataset.seq)).toEqual(['5000']);
    // back to live clears the complete page; paging disabled again
    pause();
    h.frame();
    expect(cmd('page-older').disabled).toBe(true);
  });

  it('anchor follows generations: snapshot start_cursor is used; a new generation forgets it until the daemon reports the new one', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', () => ({ channels: [] }));
    h.net.responders.set('eth.query', () => B1());
    h.net.subs[0]!.h.onSnapshotPart?.({ sub: 's', n: 1, total: 1, items: [{ domain: 'eth', kind: 'eth.snapshot', state: { kind: 'eth.capture_state', ch: 'lan1', state: 'live', backend: 'sim', iface: 'sim', generation: 2, started_mono_ns: '5', start_cursor: 'e2:seq:7' } }], events: [] } as never);
    h.snapshotEnd('10');
    pause();
    await flush(h);
    expect(queries().at(-1)!.range).toEqual({ from: 'e2:seq:7', to: 'seq:10' }); // snapshot high-water = mirror head
    pause();
    h.frame();
    // generation 3 starts (control path); the book does not know its anchor yet
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '20', generation: 3, payload: { ch: 'lan1', state: 'starting' } }));
    await flush(h);
    pause();
    await flush(h);
    expect(queries().at(-1)!.range).toEqual({ from: '-600s', to: 'seq:10' });
    pause();
    h.frame();
    // the daemon now reports it (status refresh after the next state change)
    h.net.responders.set('eth.capture.status', anchorStatus('e2:seq:20'));
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '21', generation: 3, payload: { ch: 'lan1', state: 'live' } }));
    await flush(h);
    pause();
    await flush(h);
    expect(queries().at(-1)!.range).toEqual({ from: 'e2:seq:20', to: 'seq:10' });
  });

  it('export: range dialog (current view / capture / selected row / seq) → one eth.export with that explicit range; result states requested range and completeness', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', anchorStatus());
    h.net.responders.set('eth.query', () => B1({ rows: fixtureQueryRows(2, 1380), range: { from: '1380', to: '1381' }, matched: 2 }));
    h.net.responders.set('eth.show', (p) => fixtureDetail((p as { ref: string }).ref));
    let exp: Record<string, unknown> = {};
    h.net.responders.set('eth.export', () => exp);
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '5', generation: 1, payload: { ch: 'lan1', state: 'stopped' } }));
    await flush(h);
    pause();
    await flush(h);
    pktRows(h)[0]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    cmd('export').click();
    await flush(h);
    const dlg = q('[data-testid="eth-export-dialog"]');
    const radio = (v: string) => dlg.querySelector<HTMLInputElement>(`input[name="eth-export-scope"][value="${v}"]`)!;
    expect(['view', 'capture', 'selected', 'seq'].map((v) => [v, radio(v).disabled])).toEqual([['view', false], ['capture', false], ['selected', false], ['seq', false]]);
    expect(radio('view').checked).toBe(true);
    expect(dlg.querySelector('[data-testid="eth-export-filter"]')).toBeNull(); // no display filter applied
    expect(h.net.calls.some((c) => c.method === 'eth.export')).toBe(false); // opening the dialog exports nothing
    const preview = () => dlg.querySelector('[data-testid="eth-export-range"]')!.textContent;
    expect(preview()).toBe('범위 e1:seq:1 → 최신');
    radio('selected').click();
    expect(preview()).toBe('범위 seq:1380 → seq:1380');
    radio('seq').click();
    type(dlg.querySelector<HTMLInputElement>('[data-testid="eth-export-from"]')!, 'x');
    expect(preview()).toBe('시작 seq를 숫자로 입력하세요');
    expect(dlg.querySelector<HTMLButtonElement>('[data-testid="eth-export-run"]')!.disabled).toBe(true);
    radio('selected').click();
    exp = { path: '/tmp/x.pcapng', packets: 1, bytes: 60, truncated: false, comments_dropped: 0, gaps: [], filter: null, complete: false, scan_complete: true, partial_reasons: ['output_cap'], scope: { from: 'seq:1380', to: 'seq:1380', from_source: 'explicit', lookback_secs: null }, scanned: { first_seq: '1380', last_seq: '1380', bytes: 60 }, resume_cursor: null };
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-export-run"]')!.click();
    await flush(h);
    const calls = h.net.calls.filter((c) => c.method === 'eth.export');
    expect(calls.map((c) => c.params)).toEqual([{ ch: 'lan1', format: 'pcapng', range: { from: 'seq:1380', to: 'seq:1380' } }]);
    expect(h.host.querySelector('[data-testid="eth-export-dialog"]')).toBeNull();
    const res = q('[data-testid="eth-export-result"]').textContent!;
    expect(res).toContain('요청 범위seq:1380 → seq:1380');
    expect(res).toContain('표시 필터(없음)');
    expect(res).toContain('완전성부분 — 출력 한도 도달');
    // capture scope: explicit anchor range, never range-less
    q('[data-testid="eth-export-result"] button:last-of-type').click();
    cmd('export').click();
    await flush(h);
    q<HTMLInputElement>('[data-testid="eth-export-dialog"] input[value="capture"]').click();
    exp = { path: '/tmp/y.pcapng', packets: 2, bytes: 120, truncated: false, comments_dropped: 0, gaps: [], filter: null, complete: true, scan_complete: true, partial_reasons: [], scope: { from: 'e1:seq:1', to: null, from_source: 'explicit', lookback_secs: null }, scanned: { first_seq: '1380', last_seq: '1381', bytes: 120 }, resume_cursor: null };
    q<HTMLButtonElement>('[data-testid="eth-export-run"]').click();
    await flush(h);
    expect(h.net.calls.filter((c) => c.method === 'eth.export').at(-1)!.params).toEqual({ ch: 'lan1', format: 'pcapng', range: { from: 'e1:seq:1' } });
    expect(q('[data-testid="eth-export-result"]').textContent).toContain('완전성완전 — 요청 범위 전체 기록');
  });

  it('follow sends the explicit range on screen (complete page) and states completeness', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', anchorStatus());
    h.net.responders.set('eth.query', () => B1({ rows: [{ ...fixtureQueryRows(1, 1380)[0], proto: 'TCP', stream: 4 }], range: { from: '1380', to: '1380' }, matched: 1 }));
    h.net.responders.set('eth.show', (p) => fixtureDetail((p as { ref: string }).ref));
    h.net.responders.set('eth.follow', () => ({ segments: [{ dir: 'c2s', data: '4f4b', seq: '1380' }], bytes: 2, truncated: false, gaps: [{ kind: 'source' }], complete: false, scan_complete: true, partial_reasons: ['loss'], scope: { from: 'e1:seq:1', to: null, from_source: 'explicit', lookback_secs: null }, scanned: { first_seq: '1380', last_seq: '1380', bytes: 1 }, resume_cursor: null }));
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '5', generation: 1, payload: { ch: 'lan1', state: 'stopped' } }));
    await flush(h);
    pause();
    await flush(h);
    pktRows(h)[0]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    cmd('follow').click();
    await flush(h);
    expect(h.net.calls.find((c) => c.method === 'eth.follow')!.params).toEqual({ ch: 'lan1', proto: 'tcp', stream: 4, range: { from: 'e1:seq:1' } });
    const sum = q('[data-testid="eth-follow-dialog"] .eth-fs-sum');
    expect(sum.textContent).toBe('클라이언트 2 B · 서버 0 B — daemon eth.follow · 부분: 손실 구간 1건');
    expect(sum.getAttribute('title')).toBe('요청 범위 e1:seq:1 → 최신 · 스캔 seq 1380–1380 · 스캔 범위 전체 확인');
  });
});

describe('ETH-05 unbound start screen', () => {
  it('existing channels first with a heading, guidance as whole lines (no cut-off), duplicate toolbar start/add hidden, no range row', async () => {
    h = mountPanel('eth.packets', {});
    h.net.responders.set('eth.capture.status', () => ({ channels: [{ ch: 'qa-native-eth', state: 'stopped', backend: 'sim', iface: 'sim' }] }));
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'qa-native-eth', payload: { ch: 'qa-native-eth', state: 'stopped' } }));
    await flush(h);
    expect(q('.eth-chbar').dataset.unbound).toBe('true');
    expect(q('[data-testid="eth-other-channels"] .eth-ch-head').textContent).toBe('기존 채널 이어 보기');
    const lines = [...h.host.querySelectorAll('[data-testid="eth-status"] .ch-meta .ch-line')].map((l) => l.textContent);
    expect(lines).toEqual([
      '인터페이스 구성을 고르거나 새로 만든 뒤 [연결…] — 이 탭을 여는 것만으로는 캡처·송신하지 않습니다.',
      '이미 daemon에 있는 채널은 [열기]로 기록을 이어 봅니다 (캡처를 시작하지 않음).',
    ]);
    for (const c of ['add-channel', 'start-all', 'stop-all']) expect(cmd(c).hidden, c).toBe(true);
    expect(q('[data-testid="eth-range-row"]').hidden).toBe(true);
    expect(h.host.querySelector('.eth-list')!.textContent).toContain('채널 미연결 — 기존 채널 [열기] 또는 구성 [연결…]');
  });

  it('no daemon channels: one guidance line, no heading', async () => {
    h = mountPanel('eth.packets', {});
    h.net.responders.set('eth.capture.status', () => ({ channels: [] }));
    await flush(h);
    expect([...h.host.querySelectorAll('[data-testid="eth-status"] .ch-meta .ch-line')]).toHaveLength(1);
    expect(h.host.querySelector('.eth-ch-head')).toBeNull();
  });
});
