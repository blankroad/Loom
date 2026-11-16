// @vitest-environment jsdom
// Operator upgrade UDS batch 4 — DOM wiring (Opus-authored specification; Sonnet implements bodies only).
//   U6 [연결 해제] (E_BUSY → second click aborts) · [목록에서 제거] · forgotten state removes the target · 재연결 필요
//   U1 TesterPresent job toggle (interval from profile / default, persisted on success; off is reduce-direction)
//   U2 [자동 해제] second click → uds.security_unlock (never retried) · seed helper (copy / key template, never sends)
//   U5 DB editor pre-check → uds.db.set / uds.db.clear, saved DBs · U8 route column from the row stamp, export v2
// Contracts: work/reports/operator-upgrade/uds/FUNCTIONS.md (B4-D*).
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { LoomEvent } from '../../proto/provisional';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './consolePanel';
import { ROUTE_UNRECORDED_TITLE } from './udsModel';
import { PREFS_KEY } from './udsPrefs';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
  vi.restoreAllMocks();
});

const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const btn = (cmd: string) => q<HTMLButtonElement>(`[data-cmd="${cmd}"]`);
const flush = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};
const calls = (method: string) => h!.net.calls.filter((c) => c.method === method).map((c) => c.params);
const storedPrefs = () => JSON.parse(localStorage.getItem(PREFS_KEY) ?? 'null') as { presets: Record<string, unknown>[]; diagDbs?: { id: string; name: string; json: string }[] } | null;
const input = (el: HTMLInputElement | HTMLTextAreaElement, v: string) => {
  el.value = v;
  el.dispatchEvent(new Event('input'));
};

/** canonical uds.state (has the tester_present key) for the default SIM preset target ecu1 */
const state = (over: Record<string, unknown> = {}): LoomEvent =>
  ev({
    kind: 'uds.state',
    channel: 'ecu1',
    payload: {
      target: 'ecu1', generation: 1, backend: 'sim', addr: 'DoIP 0x1001', link: { state: 'up' },
      session: { id: 3, name: 'extended', confidence: 'confirmed' }, security: { level: null, confidence: 'assumed' },
      tester_present: null, p2_ms: 50, p2_star_ms: 5000, lease: null, ...over,
    },
  });
const rowEv = (rid: string, payload: Record<string, unknown>): LoomEvent =>
  ev({
    kind: 'uds.result', channel: 'ecu1', request_id: rid, origin: 'agent', risk: 'R0',
    payload: { target: 'ecu1', write_state: 'confirmed', sid: 0x22, risk: 'R0', ...payload },
  } as Partial<LoomEvent> & { kind: string });
const busy = () => {
  throw Object.assign(new Error('uds.disconnect: target has requests in progress; pass abort:true to abort them and close'), { code: 'E_BUSY' });
};

// ------------------------------------------------------------------ U6
describe('B4-D1/D2/D7/D8 U6 disconnect · forget', () => {
  it('[연결 해제] sends uds.disconnect; E_BUSY arms one second click that aborts', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state()]);
    const d = btn('disconnect');
    expect(d.hidden).toBe(false);
    expect(d.disabled).toBe(false);
    expect(d.textContent).toContain('연결 해제');
    h.net.responders.set('uds.disconnect', busy);
    d.click();
    await flush();
    h.frame();
    expect(calls('uds.disconnect')).toEqual([{ target: 'ecu1' }]);
    expect(h.toasts.at(-1)).toContain('다시 누르면');
    expect(d.classList.contains('armed')).toBe(true);
    expect(d.textContent).toContain('중단 후 해제');
    h.net.responders.set('uds.disconnect', () => ({ target: 'ecu1', aborted: { cancelled_waiting: 1, in_flight: false } }));
    d.click();
    await flush();
    h.frame();
    expect(calls('uds.disconnect')).toEqual([{ target: 'ecu1' }, { target: 'ecu1', abort: true }]);
    expect(d.classList.contains('armed')).toBe(false);
  });
  it('a closed link: honest state, no disconnect, forget offered, [연결] usable again (replace)', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state({ link: { state: 'closed', cause: 'disconnected by user' } })]);
    expect(q('.link-st').textContent).toBe('해제됨 (disconnected by user)');
    expect(q('.link-st').classList.contains('closed')).toBe(true);
    expect(btn('disconnect').hidden).toBe(true);
    expect(btn('forget').hidden).toBe(false);
    expect(btn('connect').disabled).toBe(false);
    btn('forget').click();
    await flush();
    expect(calls('uds.forget')).toEqual([{ target: 'ecu1' }]);
  });
  it('a forgotten uds.state removes the target from the link grid', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state()]);
    expect(h.host.querySelectorAll('.uds-ds-link-row[data-target="ecu1"]')).toHaveLength(1);
    h.batch([state({ link: { state: 'closed', cause: 'forgotten' }, forgotten: true })]);
    expect(h.host.querySelectorAll('.uds-ds-link-row[data-target="ecu1"]')).toHaveLength(0);
    expect(btn('forget').hidden).toBe(true);
    h.batch([state({ generation: 2 })]); // connected again later: back
    expect(h.host.querySelectorAll('.uds-ds-link-row[data-target="ecu1"]')).toHaveLength(1);
  });
  it('재연결 필요 only when the profile cannot describe the live link', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state()]);
    expect(q('[data-testid="uds-reconnect-needed"]').hidden).toBe(true);
    h.batch([state({ backend: 'doip' })]); // SIM profile, real DoIP link
    expect(q('[data-testid="uds-reconnect-needed"]').hidden).toBe(false);
    expect(btn('connect').title).not.toContain('RPC가 없어');
  });
});

// ------------------------------------------------------------------ U1
describe('B4-D3/D7 U1 TesterPresent job', () => {
  it('start uses the shown interval, persists it to the profile; off is one click', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state()]);
    const tp = btn('tp');
    const iv = q<HTMLInputElement>('.tp-iv');
    expect(tp.textContent).toBe('TP 시작');
    expect(iv.value).toBe('2000');
    expect(q('.tp-info').textContent).toContain('기본 주기 2000 ms');
    input(iv, '1500');
    tp.click();
    await flush();
    expect(calls('uds.tester_present')).toEqual([{ target: 'ecu1', on: true, interval_ms: 1500 }]);
    expect(storedPrefs()!.presets.find((p) => p.ecu === 'ecu1')!.tpIntervalMs).toBe(1500);
    h.batch([state({ tester_present: { active: true, interval_ms: 1500, ok_count: 2, suppress: true, owner: { owner_origin: 'user', actor_id: 'c-me', client_id: 'c-me' } } })]);
    expect(tp.textContent).toBe('TP 중지');
    expect(iv.disabled).toBe(true);
    expect(q('.tp-info').textContent).toContain('on 1.5s ✓×2');
    tp.click();
    await flush();
    expect(calls('uds.tester_present').at(-1)).toEqual({ target: 'ecu1', on: false });
  });
  it('an out-of-range interval never sends; a stopped job shows its cause', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state({ tp_stop_cause: 'link down (peer closed)' })]);
    input(q<HTMLInputElement>('.tp-iv'), '100');
    btn('tp').click();
    await flush();
    expect(calls('uds.tester_present')).toHaveLength(0);
    expect(h.toasts.at(-1)).toContain('500–30000');
    expect(q('.tp-info').textContent).toContain('off (link down (peer closed))');
  });
  it('cannot start on a link that is not up', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state({ link: { state: 'down', cause: 'x' } })]);
    expect(btn('tp').disabled).toBe(true);
  });
});

// ------------------------------------------------------------------ U2
describe('B4-D4/D7 U2 security unlock', () => {
  it('first click only arms (R3 text); the second sends exactly one uds.security_unlock', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state()]);
    input(q<HTMLInputElement>('.sec-level'), '01');
    input(q<HTMLInputElement>('.sec-prov'), 'bench');
    const u = btn('unlock');
    u.click();
    await flush();
    h.frame();
    expect(calls('uds.security_unlock')).toHaveLength(0);
    expect(q('.sec-msg').textContent).toContain('R3');
    expect(q('.sec-msg').textContent).toContain('다시 누르면');
    expect(u.classList.contains('armed')).toBe(true);
    h.net.responders.set('uds.security_unlock', () => ({ target: 'ecu1', level: 1, unlocked: true, already_unlocked: false, seed_request_id: 'w1.seed' }));
    u.click();
    await flush();
    h.frame();
    expect(calls('uds.security_unlock')).toEqual([{ target: 'ecu1', level: 1, provider: 'bench' }]);
    expect(q('.sec-msg').textContent).toBe('레벨 01 해제됨');
    const p = storedPrefs()!.presets.find((x) => x.ecu === 'ecu1')!;
    expect([p.keyProvider, p.unlockLevel]).toEqual(['bench', 1]);
  });
  it('changing level/provider after arming re-arms; invalid inputs never send; failure is not retried', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state()]);
    const lv = q<HTMLInputElement>('.sec-level');
    const pv = q<HTMLInputElement>('.sec-prov');
    input(lv, '02');
    input(pv, 'bench');
    btn('unlock').click();
    await flush();
    expect(q('.sec-msg').textContent).toContain('홀수');
    input(lv, '01');
    btn('unlock').click(); // arm
    input(pv, 'other');
    btn('unlock').click(); // different key → arms again, no call
    await flush();
    expect(calls('uds.security_unlock')).toHaveLength(0);
    h.net.responders.set('uds.security_unlock', () => {
      throw Object.assign(new Error('uds.security_unlock: NRC 0x35 invalidKey'), { code: 'E_NRC' });
    });
    btn('unlock').click();
    await flush();
    h.frame();
    expect(calls('uds.security_unlock')).toEqual([{ target: 'ecu1', level: 1, provider: 'other' }]);
    expect(q('.sec-msg').textContent).toContain('재시도 없음');
    expect(q('.sec-msg').classList.contains('err')).toBe(true);
  });
  it('seed helper: a positive 67 01 row offers copy + key template; the template never sends', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state(), rowEv('w-7.seed', { sid: 0x27, bytes: { encoding: 'hex', data: '2701' }, resp_bytes: { encoding: 'hex', data: '6701a1b2c3d4' }, risk: 'R1' })]);
    expect(q('[data-testid="uds-fields"] [data-testid="uds-seed"]')).toBeNull();
    q<HTMLElement>('.hist .vt-row[data-request-id="w-7.seed"]').click();
    h.frame();
    const bar = q('[data-testid="uds-fields"] [data-testid="uds-seed"]');
    expect(bar.querySelector('.seed-hex')!.textContent).toBe('A1 B2 C3 D4');
    const write = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    btn('seed-copy').click();
    await flush();
    expect(write).toHaveBeenCalledWith('A1B2C3D4');
    btn('seed-template').click();
    h.frame();
    const raw = q<HTMLTextAreaElement>('[aria-label="UDS 요청 (hex)"]');
    expect(raw.value).toBe('27 02 <key>');
    expect(raw.getAttribute('aria-invalid')).toBe('true');
    await flush();
    expect(calls('uds.request').filter((p) => !(p as { dry_run?: boolean }).dry_run)).toHaveLength(0);
  });
});

// ------------------------------------------------------------------ U5
describe('B4-D5/D7 U5 diagnostic DB', () => {
  const DB = '{"loom_diag":1,"ecu":"bench","dids":[{"id":61856,"name":"V","length":1,"codec":{"type":"hex"}}]}';
  it('pre-check → apply → remembered → clear', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state()]);
    expect(q('.db-info').textContent).toContain('적용된 DB 없음');
    const txt = q<HTMLTextAreaElement>('.db-text');
    input(txt, '{');
    h.frame();
    expect(q('.db-msg').textContent).toMatch(/^JSON 구문 오류: /);
    expect(btn('db-set').disabled).toBe(true);
    input(txt, DB);
    h.frame();
    expect(btn('db-set').disabled).toBe(false);
    h.net.responders.set('uds.db.set', () => ({ target: 'ecu1', db: { hash: '0123456789abcdef', ecu: 'bench', dids: 1 }, warnings: ['writable flags are informational'], replaced: false }));
    btn('db-set').click();
    await flush();
    h.frame();
    expect(calls('uds.db.set')).toEqual([{ target: 'ecu1', db: JSON.parse(DB) }]);
    expect(q('.db-msg').textContent).toContain('0123456789abcdef');
    expect(q('.db-msg').textContent).toContain('writable flags are informational');
    expect(storedPrefs()!.diagDbs).toEqual([{ id: 'db-0123456789abcdef', name: 'bench', json: DB }]);
    expect([...q<HTMLSelectElement>('.db-saved').options].map((o) => o.value)).toEqual(['', 'db-0123456789abcdef']);
    h.batch([state({ db: { hash: '0123456789abcdef', ecu: 'bench', dids: 1 } })]);
    expect(q('.db-info').textContent).toContain('0123456789abcdef');
    expect(btn('db-clear').disabled).toBe(false);
    h.net.responders.set('uds.db.clear', () => ({ target: 'ecu1', cleared: true }));
    btn('db-clear').click();
    await flush();
    expect(calls('uds.db.clear')).toEqual([{ target: 'ecu1' }]);
  });
  it('a saved DB loads into the editor without sending; a closed target cannot take a DB', () => {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ diagDbs: [{ id: 'db-1', name: 'bench', json: DB }] }));
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([state({ link: { state: 'closed', cause: 'disconnected by agent' } })]);
    const sel = q<HTMLSelectElement>('.db-saved');
    sel.value = 'db-1';
    sel.dispatchEvent(new Event('change'));
    h.frame();
    expect(q<HTMLTextAreaElement>('.db-text').value).toBe(DB);
    expect(btn('db-set').disabled).toBe(true);
    expect(calls('uds.db.set')).toHaveLength(0);
  });
});

// ------------------------------------------------------------------ U8
describe('B4-D9 U8 route column and export v2', () => {
  it('the cell shows the stamp of the row, never the current target route', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([
      state(),
      rowEv('r-1', { bytes: { encoding: 'hex', data: '22f190' }, generation: 1, backend: 'sim', addr: 'DoIP 0x1001' }),
      rowEv('r-2', { bytes: { encoding: 'hex', data: '22f190' } }),
    ]);
    const cellOf = (rid: string) => q(`.hist .vt-row[data-request-id="${rid}"] .c-rt`);
    expect(cellOf('r-1').textContent).toBe('sim DoIP 0x1001 · g1');
    expect(cellOf('r-2').textContent).toBe('?');
    expect(cellOf('r-2').title).toBe(ROUTE_UNRECORDED_TITLE);
    h.batch([state({ generation: 2, backend: 'doip', addr: 'DoIP 0x2002' })]);
    expect(cellOf('r-1').textContent).toBe('sim DoIP 0x1001 · g1');
    expect(cellOf('r-2').textContent).toBe('?');
  });
  it('JSON export is version 2 with the stamped route and db_hash (null when unrecorded)', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([
      state(),
      rowEv('r-1', { bytes: { encoding: 'hex', data: '22f190' }, generation: 3, backend: 'doip', addr: 'DoIP 0x1001', db_hash: '00000000000000aa' }),
      rowEv('r-2', { bytes: { encoding: 'hex', data: '22f190' } }),
    ]);
    let blob: Blob | null = null;
    Object.defineProperty(URL, 'createObjectURL', { value: (b: Blob) => ((blob = b), 'blob:x'), configurable: true });
    Object.defineProperty(URL, 'revokeObjectURL', { value: () => {}, configurable: true });
    q<HTMLButtonElement>('.ibtn.exp[title="기록 JSON 내보내기"]').click();
    expect(blob).not.toBeNull();
    const text = await new Promise<string>((res) => {
      const fr = new FileReader();
      fr.onload = () => res(String(fr.result));
      fr.readAsText(blob!);
    });
    const doc = JSON.parse(text) as { format: string; version: number; rows: Record<string, unknown>[] };
    expect([doc.format, doc.version]).toEqual(['loom.uds.history', 2]);
    const r1 = doc.rows.find((r) => r.request_id === 'r-1')!;
    const r2 = doc.rows.find((r) => r.request_id === 'r-2')!;
    expect([r1.generation, r1.backend, r1.addr, r1.db_hash]).toEqual([3, 'doip', 'DoIP 0x1001', '00000000000000aa']);
    expect([r2.generation, r2.backend, r2.addr, r2.db_hash]).toEqual([null, null, null, null]);
    expect('route_first_seen' in r1).toBe(false);
  });
});
