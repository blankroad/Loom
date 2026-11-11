// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { LoomEvent } from '../../proto/provisional';
import { ev, mountPanel, writeEv, type Harness } from '../../sdk/testing/harness';
import { PREVIEW_DEBOUNCE_MS } from './consolePanel';
import { normalizeHex, previewText, resultSymbol, timingText } from './udsModel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
});

const rowsIn = (host: HTMLElement) => [...host.querySelectorAll<HTMLElement>('.vt-row[data-request-id]')].filter((e) => !e.hidden);

describe('uds.console', () => {
  it('one request_id = one row: ○→◔→◑ pending×1..5→●', () => {
    let t = 0;
    h = mountPanel('uds.console', { targets: 'bcm' }, { now: () => t });
    const req = (state: Parameters<typeof writeEv>[1], seq: number, extra: Record<string, unknown> = {}) =>
      writeEv('t-1', state, { seq, payload: { target: 'bcm', sid: '31', service: 'RoutineControl', summary: 'start FF01', ...extra } });

    t += 100;
    h.batch([req('requested', 1)]);
    expect(rowsIn(h.host).map((r) => r.dataset.state)).toEqual(['requested']);
    t += 100;
    h.batch([req('accepted', 2)]);
    for (let i = 1; i <= 5; i++) {
      t += 1_000;
      h.batch([req('sent', 2 + i, { pending_count: i, p2_star_ms: 5000 })]);
      const rows = rowsIn(h.host);
      expect(rows).toHaveLength(1);
      expect(rows[0]!.dataset.state).toBe('sent');
      expect(rows[0]!.textContent).toContain(`pending ×${i}`);
    }
    t += 100;
    h.batch([req('confirmed', 9, { rtt_ms: 5120 })]);
    const rows = rowsIn(h.host);
    expect(rows).toHaveLength(1);
    expect(rows[0]!.dataset.requestId).toBe('t-1');
    expect(rows[0]!.dataset.state).toBe('confirmed');
    expect(rows[0]!.textContent).toContain('●확정');
    expect(rows[0]!.textContent).toContain('✓');
  });

  it('folds TesterPresent into the target bar and shows the target state', () => {
    h = mountPanel('uds.console', { targets: 'bcm' });
    h.batch([
      ev({ kind: 'uds.state', payload: { target: 'bcm', backend: 'DoIP', addr: '0x1001', session: { value: 'extended', confidence: 'confirmed' }, security: { level: 'L1🔓' }, tp: { on: true, ok_count: 3 }, p2_ms: 50, p2_star_ms: 5000, lease: { owner_origin: 'agent', actor: 'claude-1' } } }),
      writeEv('tp1', 'confirmed', { seq: 900, payload: { target: 'bcm', sid: '3E', service: 'TesterPresent' } }),
      writeEv('r1', 'confirmed', { seq: 901, payload: { target: 'bcm', sid: '22', service: 'ReadDataByIdentifier', decoded: [{ did: 'F190', name: 'VIN', value: 'WVW123' }] } }),
    ]);
    expect(rowsIn(h.host).map((r) => r.dataset.requestId)).toEqual(['r1']);
    const bar = h.host.querySelector('[data-testid="uds-targets"]')!.textContent!;
    expect(bar).toContain('sess=extended✓');
    expect(bar).toContain('TP ✓×4');
    expect(bar).toContain('제어: AGT claude-1');
    expect(h.host.querySelector('[data-testid="uds-last-values"]')!.textContent).toContain('bcm:F190 VIN=WVW123');
  });

  it('dry-run preview is debounced (150 ms) and Enter sends uds.request', async () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', { targets: 'bcm' });
    h.net.responders.set('uds.request', (p) =>
      (p as { dry_run?: boolean }).dry_run ? { service: 'ReadDataByIdentifier', did_name: 'VIN', risk: 'R0' } : { request_id: 'r-9' },
    );
    h.batch([ev({ kind: 'uds.state', payload: { target: 'bcm' } })]);
    const input = h.host.querySelector('input')!;
    for (const v of ['2', '22', '22 F', '22 F190']) {
      input.value = v;
      input.dispatchEvent(new Event('input'));
      vi.advanceTimersByTime(40);
    }
    expect(h.net.calls.filter((c) => c.method === 'uds.request')).toHaveLength(0);
    vi.advanceTimersByTime(PREVIEW_DEBOUNCE_MS);
    await vi.advanceTimersByTimeAsync(10);
    const previews = h.net.calls.filter((c) => c.method === 'uds.request');
    expect(previews).toHaveLength(1);
    expect(previews[0]!.params).toEqual({ target: 'bcm', bytes_hex: '22F190', dry_run: true });
    expect(h.host.querySelector('[data-testid="uds-preview"]')!.textContent).toContain('VIN');

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.advanceTimersByTimeAsync(10);
    const sends = h.net.calls.filter((c) => c.method === 'uds.request' && !(c.params as { dry_run?: boolean }).dry_run);
    expect(sends).toEqual([{ method: 'uds.request', params: { target: 'bcm', bytes_hex: '22F190' } }]);
  });

  it('R2+ preview needs a second Enter before sending', async () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', { targets: 'bcm' });
    h.net.responders.set('uds.request', (p) => ((p as { dry_run?: boolean }).dry_run ? { service: 'WriteDataByIdentifier', risk: 'R2' } : {}));
    h.batch([ev({ kind: 'uds.state', payload: { target: 'bcm' } })]);
    const input = h.host.querySelector('input')!;
    input.value = '2E 0101 02';
    input.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS + 1);
    const enter = () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    enter();
    expect(h.net.calls.filter((c) => !(c.params as { dry_run?: boolean }).dry_run)).toHaveLength(0);
    expect(h.host.querySelector('[data-testid="uds-preview"]')!.textContent).toContain('Enter 한 번 더');
    enter();
    expect(h.net.calls.filter((c) => c.method === 'uds.request' && !(c.params as { dry_run?: boolean }).dry_run)).toHaveLength(1);
  });

  it('command line accepts hex only in M1', () => {
    expect(normalizeHex('22 f190')).toBe('22F190');
    expect(normalizeHex('read F190')).toBeNull();
  });
});

describe('udsModel', () => {
  const row = (state: string, payload: Record<string, unknown>, stageSeenMs = 0) =>
    ({ requestId: 'x', state, payload, stageSeenMs, firstSeenMs: 0 }) as unknown as Parameters<typeof resultSymbol>[0];
  it('result symbols', () => {
    expect(resultSymbol(row('failed', { nrc: 0x31, nrc_name: 'requestOutOfRange' })).text).toBe('✗ 0x31 requestOutOfRange');
    expect(resultSymbol(row('outcome_unknown', {})).text).toBe('⌛? 불확정');
    expect(resultSymbol(row('failed', { timeout: true })).text).toBe('⌛ timeout');
    expect(resultSymbol(row('confirmed', { late: true })).text).toBe('late');
    expect(resultSymbol(row('confirmed', { ok_suppressed: true })).text).toBe('✓ suppressed');
    expect(resultSymbol(row('requested', { guard: 'deny' })).text).toBe('⛔ 거부');
  });
  it('timing: elapsed/limit while in flight, p2/rtt when final', () => {
    expect(timingText(row('sent', { p2_star_ms: 5000 }, 0), 3_100)).toBe('3.1s/5.0s');
    expect(timingText(row('confirmed', { p2_ms: 9, rtt_ms: 14 }), 0)).toBe('9/14ms');
  });
});

describe('udsModel — no false success', () => {
  const row = (state: string, payload: Record<string, unknown>) =>
    ({ requestId: 'x', state, payload, stageSeenMs: 0, firstSeenMs: 0 }) as unknown as Parameters<typeof resultSymbol>[0];
  it('a final row carrying a negative NRC is ✗ even if write_state says confirmed', () => {
    expect(resultSymbol(row('confirmed', { nrc: '0x11', nrc_name: 'serviceNotSupported' })).text).toBe('✗ 0x11 serviceNotSupported');
  });
  it('0x78 responsePending is not a failure', () => {
    expect(resultSymbol(row('sent', { nrc: '0x78', pending_count: 2 })).text).toBe('⏳ pending ×2');
    expect(resultSymbol(row('confirmed', { nrc: '0x78' })).text).toBe('✓');
  });
});

describe('uds.console — dry-run rows hidden', () => {
  it('rows marked dry_run never appear in the table', () => {
    h = mountPanel('uds.console', { targets: 'bcm' });
    h.batch([
      writeEv('d1', 'effect_unverified', { seq: 1, payload: { target: 'bcm', sid: 0x2e, dry_run: true } }),
      writeEv('r1', 'confirmed', { seq: 2, payload: { target: 'bcm', sid: 0x22 } }),
    ]);
    expect(rowsIn(h.host).map((r) => r.dataset.requestId)).toEqual(['r1']);
  });
});

describe('U-3: STALLED', () => {
  it('UDS command line is disabled while the shell write lock is on; abort stays enabled', () => {
    h = mountPanel('uds.console', { targets: 'bcm' });
    const input = h.host.querySelector<HTMLInputElement>('[aria-label="UDS 요청 (hex)"]')!;
    expect(input.disabled).toBe(false);
    h.shell.tick(5_000);
    h.frame();
    expect(input.disabled).toBe(true);
  });
});

// ---- canonical backend payloads: the Rust serde fixtures themselves (crates/loom-uds/fixtures,
// generated from crates/loom-uds/src/payload.rs by tests/fixtures.rs) — not a UI-side mock shape.
const FIXTURES = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../crates/loom-uds/fixtures');
type FixtureEvent = { kind: string; request_id: string | null; payload: Record<string, unknown> };
const fixture = <T = FixtureEvent[]>(name: string): T =>
  JSON.parse(readFileSync(resolve(FIXTURES, `${name}.json`), 'utf8')) as T;
/** wrap a fixture entry in the envelope the gateway adds (crates/loom-uds/src/events.rs) */
const envelope = (f: FixtureEvent): LoomEvent =>
  ev({
    kind: f.kind,
    channel: String(f.payload.target),
    origin: f.kind === 'uds.state' || f.kind === 'uds.rx' ? 'device' : 'agent',
    ...(f.request_id !== null ? { request_id: f.request_id } : {}),
    ...(typeof f.payload.write_state === 'string' ? { risk: f.payload.risk as LoomEvent['risk'] } : {}),
    payload: f.payload,
  });
const barText = (host: HTMLElement) => host.querySelector('[data-testid="uds-targets"]')!.textContent!;
const rowEl = (host: HTMLElement, id: string) => rowsIn(host).find((r) => r.dataset.requestId === id)!;
const detailOf = (hh: Harness, id: string): Record<string, string> => {
  rowEl(hh.host, id).click();
  hh.frame();
  const out: Record<string, string> = {};
  for (const d of hh.host.querySelectorAll('.vt-detail .disp-row')) out[d.querySelector('dt')!.textContent!] = d.querySelector('dd')!.textContent!;
  rowEl(hh.host, id).click(); // deselect
  hh.frame();
  return out;
};

describe('uds.console — canonical uds.state (fixtures/state.json)', () => {
  const [connected, extended] = fixture('state') as [FixtureEvent, FixtureEvent];

  it('shows link/session/security/TP/lease from the real payload, honest about assumed state', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(connected)]);
    let bar = barText(h.host);
    expect(bar).toContain('ecu1 ● sim DoIP 0x1001 sess=default? sec=🔒? TP off P2 50/5000');
    expect(bar).toContain('제어: 비어 있음');

    h.batch([envelope(extended)]);
    bar = barText(h.host);
    expect(bar).toContain('sess=extended✓ sec=🔒✓ TP ✓×1 P2 50/5000');
    expect(bar).toContain('제어: USR viewer:alice');
    expect(bar).not.toContain('sess=?');
  });

  it('lease held by this connection (lease.client_id = conn_id) is shown as 나', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' }, { hello: { conn_id: 'conn-2' } });
    h.batch([envelope(extended)]);
    expect(barText(h.host)).toContain('제어: 나');
  });

  it('link down + unlocked numeric security level from the canonical shape', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    const p = extended.payload;
    h.batch([
      envelope({
        ...extended,
        payload: { ...p, link: { state: 'down', cause: 'tcp reset' }, security: { level: 1, confidence: 'confirmed' }, tester_present: null, lease: null },
      }),
    ]);
    const bar = barText(h.host);
    expect(bar).toContain('ecu1 ○ down(tcp reset) sim');
    expect(bar).toContain('sec=L1🔓✓ TP off');
    expect(bar).toContain('제어: 비어 있음');
  });

  it('a target seen only in rows (no uds.state yet) is not painted as linked/locked', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(fixture('read_ok')[0]!)]);
    expect(barText(h.host)).toContain('ecu1 ? ? sess=? sec=? TP off');
  });
});

describe('uds.console — canonical row events (fixtures/*.json)', () => {
  const ALL = ['read_ok', 'pending_then_positive', 'nrc', 'read_timeout', 'write_outcome_unknown', 'sprmib', 'blocked_2a', 'dtc_list', 'late_response'];

  const mountAll = () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch(ALL.flatMap((n) => fixture(n)).map(envelope));
    return h;
  };

  it('one row per request; result never paints a failure as success', () => {
    const hh = mountAll();
    // r-6 (3E 80 TesterPresent) folds into the bar; late_response (uds.rx only) creates no row
    expect(rowsIn(hh.host).map((r) => r.dataset.requestId)).toEqual(['r-1', 'r-2', 'r-3', 'r-4', 'r-5', 'r-7', 'r-8']);
    const res = (id: string) => rowEl(hh.host, id).textContent!;
    expect(res('r-1')).toContain('✓');
    expect(res('r-3')).toContain('✗ 0x31 requestOutOfRange');
    expect(res('r-3')).toContain('50/0.9ms');
    expect(res('r-4')).toContain('⌛ timeout');
    expect(res('r-5')).toContain('⌛? 불확정');
    expect(res('r-7')).toContain('⛔ 거부');
    for (const id of ['r-3', 'r-4', 'r-5', 'r-7']) expect(res(id)).not.toMatch(/✓/);
    expect(res('r-1')).toContain('ReadDataByIdentifier');
    expect(hh.host.querySelector('[data-testid="uds-last-values"]')!.textContent).toContain('ecu1:F190 VIN=LOOMSIM0000000001');
  });

  it('detail shows the real request/response bytes, hex NRC, DTCs and block reason', () => {
    const hh = mountAll();
    const nrc = detailOf(hh, 'r-3');
    expect(nrc['요청']).toBe('22 F1 A1');
    expect(nrc['응답']).toBe('7F 22 31');
    expect(nrc['NRC']).toBe('0x31 requestOutOfRange');
    expect(nrc['등급']).toBe('R0');

    const ok = detailOf(hh, 'r-2');
    expect(ok['요청']).toBe('22 F1 90');
    expect(ok['응답']).toBe('62 F1 90 4C 4F 4F 4D 53 49 4D 30 30 30 30 30 30 30 30 30 31');
    expect(ok['NRC']).toBe('—'); // the 0x78 pendings are not a final NRC

    const unknown = detailOf(hh, 'r-5');
    expect(unknown['요청']).toBe('2E F1 A0 02');
    expect(unknown['응답']).toBe('—'); // nothing received: no invented response
    expect(unknown['등급']).toBe('R2');

    expect(detailOf(hh, 'r-8')['DTC']).toBe('923400 st=0x09 · C10000 st=0x2F');
    expect(detailOf(hh, 'r-7')['차단']).toContain('immutable_deny');
  });

  it('legacy req_hex/resp_hex rows still show their bytes', () => {
    h = mountPanel('uds.console', { targets: 'bcm' });
    h.batch([writeEv('l1', 'failed', { seq: 1, payload: { target: 'bcm', sid: '22', req_hex: '22F190', resp_hex: '7F2231', nrc: '0x31', nrc_name: 'requestOutOfRange' } })]);
    const d = detailOf(h, 'l1');
    expect(d['요청']).toBe('22 F1 90');
    expect(d['응답']).toBe('7F 22 31');
    expect(d['NRC']).toBe('0x31 requestOutOfRange');
  });
});

describe('uds.console — canonical dry-run preview (fixtures/dry_run.json)', () => {
  const [read, write, blocked, parseErr] = fixture<Record<string, unknown>[]>('dry_run');

  it('previewText reads the DryRunResult honestly', () => {
    expect(previewText(read)).toEqual({ text: 'ReadDataByIdentifier VIN · R0 UDS 0x22 ReadDataByIdentifier', risk: 'R0' });
    expect(previewText(write).risk).toBe('R2');
    expect(previewText(blocked).text).toMatch(/^⛔ 거부 ReadDataByPeriodicIdentifier · R1/);
    expect(previewText(parseErr)).toEqual({ text: '구문 오류: odd number of hex digits (위치 7)', risk: null });
    expect(previewText({ data: read }).text).toContain('VIN'); // D5 envelope
  });

  it('a real R2 dry-run result arms the second Enter', async () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.net.responders.set('uds.request', (p) => ((p as { dry_run?: boolean }).dry_run ? write : { request_id: 'r-9' }));
    h.batch([envelope(fixture('state')[0]!)]);
    const input = h.host.querySelector('input')!;
    input.value = '2E F1A0 02';
    input.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS + 1);
    expect(h.host.querySelector('[data-testid="uds-preview"]')!.textContent).toContain('WriteDataByIdentifier · R2');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(h.net.calls.filter((c) => !(c.params as { dry_run?: boolean }).dry_run)).toHaveLength(0);
    expect(h.host.querySelector('[data-testid="uds-preview"]')!.textContent).toContain('Enter 한 번 더');
  });
});

describe('uds.console native late-join state', () => {
  it('hydrates an empty-backlog second viewer without configured ghost targets or writes', () => {
    h = mountPanel('uds.console');
    const payload = { target: 'ecu1', generation: 1, link: { state: 'up' }, backend: 'sim',
      session: { id: 3, name: 'extended', confidence: 'confirmed' }, security: { level: null },
      tester_present: null, p2_ms: 50, p2_star_ms: 5000, lease: null };
    const deliver = (p = payload) => {
      h!.net.subs[0]!.h.onSnapshotPart?.({ sub: h!.net.subs[0]!.id, n: 1, total: 1, items: [
        { domain: 'uds', kind: 'uds.snapshot', state: { kind: 'uds.state', payload: p } },
        { domain: 'uart', channel: 'foreign', kind: 'uart.snapshot', state: { kind: 'uds.state', payload: { ...p, target: 'foreign' } } },
      ], events: [] });
      h!.snapshotEnd('100');
    };
    deliver();
    const bar = () => h!.host.querySelector('[data-testid="uds-targets"]')!.textContent!;
    expect(bar()).toContain('ecu1 ● sim');
    expect(bar()).toContain('sess=extended✓');
    expect(bar()).not.toContain('foreign');
    expect(rowsIn(h.host)).toHaveLength(0);
    expect(h.net.calls).toEqual([]);
    h.net.subs[0]!.h.onResync?.('reconnect');
    deliver({ ...payload, generation: 2, link: { state: 'down' } });
    expect(bar()).toContain('ecu1 ○ down');
    expect(h.toasts).toEqual([]);
    const net = h.net;
    h.dispose();
    h = null;
    expect(net.subs.every((s) => s.closed)).toBe(true);
  });

  it('honors configured targets and keeps newer live state over native snapshot/backlog', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    const state = (target: string, link: string) => ({ target, link: { state: link }, backend: 'sim' });
    h.net.subs[0]!.h.onSnapshotPart?.({ sub: 's', n: 1, total: 1, items: [
      { domain: 'uds', kind: 'uds.snapshot', state: { kind: 'uds.state', payload: state('ecu1', 'up') } },
      { domain: 'uds', kind: 'uds.snapshot', state: { kind: 'uds.state', payload: state('ecu2', 'up') } },
    ], events: [ev({ seq: '9', kind: 'uds.state', payload: state('ecu1', 'down') })] });
    h.snapshotEnd('10');
    expect(h.host.querySelector('[data-testid="uds-targets"]')!.textContent).toContain('ecu1 ● sim');
    h.batch([ev({ seq: '11', kind: 'uds.state', payload: state('ecu1', 'down') }), ev({ seq: '12', kind: 'uds.state', payload: state('ecu2', 'up') })]);
    const bar = h.host.querySelector('[data-testid="uds-targets"]')!.textContent!;
    expect(bar).toContain('ecu1 ○ down');
    expect(bar).not.toContain('ecu2');
    expect(h.net.calls).toEqual([]);
  });
});
