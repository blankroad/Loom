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

/** the canonical hex command line (the catalog search box is also an <input>) */
const pduInput = (host: HTMLElement) => host.querySelector<HTMLInputElement>('[aria-label="UDS 요청 (hex)"]')!;
/** logical-link grid: each row's title is the canonical one-line targetLine(), plus the cell text */
const linkText = (host: HTMLElement) => {
  const b = host.querySelector<HTMLElement>('[data-testid="uds-targets"]')!;
  return `${[...b.querySelectorAll<HTMLElement>('[data-target]')].map((r) => r.title).join('\n')}\n${b.textContent}`;
};
const didRow = (host: HTMLElement, key: string) => host.querySelector<HTMLElement>(`[data-testid="uds-last-values"] [data-did="${key}"]`)?.textContent ?? '';
const didCells = (host: HTMLElement, key: string) =>
  [...(host.querySelector<HTMLElement>(`[data-testid="uds-last-values"] [data-did="${key}"]`)?.children ?? [])].map((c) => c.textContent);
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
    const bar = linkText(h.host);
    expect(bar).toContain('sess=extended✓');
    expect(bar).toContain('TP ✓×4');
    expect(bar).toContain('제어: AGT claude-1');
    expect(didCells(h.host, 'bcm:F190').slice(0, 5)).toEqual(['bcm', 'F190', 'VIN', 'WVW123', '']); // target · DID · name · value · unit
  });

  it('dry-run preview is debounced (150 ms) and Enter sends uds.request', async () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', { targets: 'bcm' });
    h.net.responders.set('uds.request', (p) =>
      (p as { dry_run?: boolean }).dry_run ? { service: 'ReadDataByIdentifier', did_name: 'VIN', risk: 'R0' } : { request_id: 'r-9' },
    );
    h.batch([ev({ kind: 'uds.state', payload: { target: 'bcm' } })]);
    const input = pduInput(h.host);
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
    const input = pduInput(h.host);
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
const barText = linkText;
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
    expect(didRow(hh.host, 'ecu1:F190')).toContain('VINLOOMSIM0000000001');
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
    const input = pduInput(h.host);
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
    const bar = () => linkText(h!.host);
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
    expect(linkText(h.host)).toContain('ecu1 ● sim');
    h.batch([ev({ seq: '11', kind: 'uds.state', payload: state('ecu1', 'down') }), ev({ seq: '12', kind: 'uds.state', payload: state('ecu2', 'up') })]);
    const bar = linkText(h.host);
    expect(bar).toContain('ecu1 ○ down');
    expect(bar).not.toContain('ecu2');
    expect(h.net.calls).toEqual([]);
  });
});

// ---- benchmark fidelity (DTS.monaco Diagnostic Services) — docs/impl/main/research/fidelity/uds.md ----
import { buildPdu, detailPairs, filterCatalog, findTemplate, paramIssues, parsePdu, SERVICE_CATALOG, targetCells, emptyTarget } from './udsModel';

const sends = (hh: Harness) => hh.net.calls.filter((c) => c.method === 'uds.request' && !(c.params as { dry_run?: boolean }).dry_run);
const dryRuns = (hh: Harness) => hh.net.calls.filter((c) => c.method === 'uds.request' && (c.params as { dry_run?: boolean }).dry_run === true);
const svcBtn = (host: HTMLElement, sid: string) => host.querySelector<HTMLButtonElement>(`[data-testid="uds-catalog"] [data-sid="${sid}"]`)!;
const param = (host: HTMLElement, name: string) => host.querySelector<HTMLInputElement | HTMLSelectElement>(`[data-testid="uds-params"] [aria-label="${name}"]`)!;

describe('fidelity: service catalog → parameters → canonical PDU (never sends by itself)', () => {
  it('lists only supported standard-service templates, grouped, searchable', () => {
    const sids = SERVICE_CATALOG.map((t) => t.sid);
    expect(sids).toEqual(['10', '11', '27', '28', '85', '22', '2E', '14', '19', '31']);
    // R4 transfer/flash, immutable-deny 0x2A/0x86 and the TesterPresent keep-alive are not offered
    for (const no of ['34', '36', '37', '3D', '2A', '86', '3E']) expect(findTemplate(no)).toBeNull();
    expect(filterCatalog('0x22').map((t) => t.name)).toEqual(['ReadDataByIdentifier']);
    expect(filterCatalog('dtc').map((t) => t.sid)).toEqual(['85', '19']); // name substring only

    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(fixture('state')[0]!)]);
    const tree = h.host.querySelector('[data-testid="uds-catalog"]')!;
    expect(tree.textContent).toContain('[22] ReadDataByIdentifier');
    expect(tree.textContent).toContain('Stored Data Transmission');
    const search = h.host.querySelector<HTMLInputElement>('[aria-label="서비스 검색"]')!;
    search.value = '19';
    search.dispatchEvent(new Event('input'));
    h.frame();
    expect([...tree.querySelectorAll<HTMLElement>('[data-sid]')].map((b) => b.dataset.sid)).toEqual(['19']);
    expect(h.net.calls).toEqual([]); // browsing the catalog is read-only
  });

  it('picking a service and editing parameters only rewrites the hex line + dry-run; Enter sends those bytes', async () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.net.responders.set('uds.request', (p) => ((p as { dry_run?: boolean }).dry_run ? { service: 'ReadDataByIdentifier', risk: 'R0' } : { request_id: 'r-9' }));
    h.batch([envelope(fixture('state')[0]!)]);
    svcBtn(h.host, '22').click();
    h.frame();
    expect(pduInput(h.host).value).toBe('22 F1 90');
    expect(h.host.querySelector('[data-testid="uds-service"]')!.textContent).toBe('[22] ReadDataByIdentifier');
    expect(svcBtn(h.host, '22').getAttribute('aria-pressed')).toBe('true');
    const did = param(h.host, 'dataIdentifier') as HTMLInputElement;
    expect(did.value).toBe('F190');
    did.value = 'F18C';
    did.dispatchEvent(new Event('input'));
    expect(pduInput(h.host).value).toBe('22 F1 8C');
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS + 1);
    expect(sends(h)).toHaveLength(0); // selection + edit never send
    expect(dryRuns(h).map((c) => c.params)).toEqual([{ target: 'ecu1', bytes_hex: '22F18C', dry_run: true }]); // debounced: one preview
    h.host.querySelector<HTMLButtonElement>('.uds-ds-transmit')!.click(); // explicit Transmit
    expect(sends(h)).toEqual([{ method: 'uds.request', params: { target: 'ecu1', bytes_hex: '22F18C' } }]);
  });

  it('choice parameters and required-field hints; typing hex selects the matching template', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(fixture('state')[0]!)]);
    svcBtn(h.host, '10').click();
    const sel = param(h.host, 'diagnosticSessionType') as HTMLSelectElement;
    expect(sel.value).toBe('03');
    sel.value = '01';
    sel.dispatchEvent(new Event('change'));
    expect(pduInput(h.host).value).toBe('10 01');
    expect(h.host.querySelector('[data-param="type"]')!.textContent).toContain('defaultSession');

    svcBtn(h.host, '2E').click();
    expect(pduInput(h.host).value).toBe('2E');
    expect(h.host.querySelector('[data-testid="uds-param-issues"]')!.textContent).toContain('dataIdentifier 비어 있음');

    const input = pduInput(h.host);
    input.value = '31 01 FF01';
    input.dispatchEvent(new Event('input'));
    expect(h.host.querySelector('[data-testid="uds-service"]')!.textContent).toBe('[31] RoutineControl');
    expect((param(h.host, 'routineIdentifier') as HTMLInputElement).value).toBe('FF01');
    input.value = '3E 00';
    input.dispatchEvent(new Event('input'));
    expect(h.host.querySelector('[data-testid="uds-service"]')!.textContent).toContain('카탈로그 밖');
    expect(h.net.calls.filter((c) => !(c.params as { dry_run?: boolean }).dry_run)).toHaveLength(0);
  });

  it('write lock disables template selection and parameters; abort stays enabled', () => {
    h = mountPanel('uds.console', { targets: 'bcm' });
    h.frame();
    expect(svcBtn(h.host, '22').disabled).toBe(false);
    h.shell.tick(5_000);
    h.frame();
    expect(svcBtn(h.host, '22').disabled).toBe(true);
    expect(h.host.querySelector<HTMLButtonElement>('.uds-ds-transmit')!.disabled).toBe(true);
    expect(h.host.querySelector<HTMLButtonElement>('[data-testid="uds-current"] .danger')!.disabled).toBe(false);
  });

  it('model: build/parse/issues round-trip on standard templates', () => {
    const rc = findTemplate('31')!;
    expect(buildPdu(rc, { type: '01', rid: 'FF01', opt: '' })).toBe('31 01 FF 01');
    expect(parsePdu(rc, '3101FF01AA')).toEqual({ type: '01', rid: 'FF01', opt: 'AA' });
    expect(paramIssues(rc, { type: '01', rid: 'F' })).toEqual(['routineIdentifier hex 아님']);
    expect(paramIssues(findTemplate('19')!, { type: '02', mask: 'FF' })).toEqual([]);
  });
});

describe('fidelity: logical-link grid (TesterPresent ≠ CAN-TP, honest confidence)', () => {
  it('one row per target with structured cells; TesterPresent column is the 0x3E keep-alive', () => {
    const [connected, extended] = fixture('state') as [FixtureEvent, FixtureEvent];
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(connected)]);
    const cell = (col: string) => h!.host.querySelector(`[data-target="ecu1"] [data-col="${col}"]`)!.textContent;
    expect(cell('link')).toBe('● up');
    expect(cell('backend')).toBe('sim');
    expect(cell('addr')).toBe('DoIP 0x1001');
    expect(cell('session')).toBe('default?'); // assumed, not confirmed
    expect(cell('security')).toBe('🔒?');
    expect(cell('tester-present')).toBe('off');
    h.batch([envelope(extended)]);
    expect(cell('session')).toBe('extended✓');
    expect(cell('tester-present')).toBe('on ✓×1');
    const header = [...h.host.querySelectorAll('[data-testid="uds-targets"] [role="columnheader"]')].map((c) => c.textContent);
    expect(header).toContain('TesterPresent');
    const tpHead = [...h.host.querySelectorAll<HTMLElement>('[role="columnheader"]')].find((c) => c.textContent === 'TesterPresent')!;
    expect(tpHead.title).toContain('ISO-TP/CAN-TP 전송 상태가 아님');
    const all = h.host.textContent!;
    for (const no of ['cantp', 'CAN-TP 연결', 'EthDiag', 'canoe']) expect(all).not.toContain(no);
  });

  it('unreported link is never painted as up', () => {
    expect(targetCells(emptyTarget('x'), 0, 'sim').link.text).toBe('? 미보고');
  });

  it('clicking a link row selects the target for the composer (no write)', () => {
    h = mountPanel('uds.console');
    h.batch([
      ev({ kind: 'uds.state', payload: { target: 'a', backend: 'sim', link: { state: 'up' } } }),
      ev({ kind: 'uds.state', payload: { target: 'b', backend: 'doip', addr: '0x1002', link: { state: 'up' } } }),
    ]);
    const row = h.host.querySelector<HTMLElement>('[data-testid="uds-targets"] [data-target="b"]')!;
    row.click();
    h.frame();
    expect(h.host.querySelector<HTMLSelectElement>('[aria-label="대상"]')!.value).toBe('b');
    expect(h.host.querySelector('[data-target="b"]')!.getAttribute('aria-selected')).toBe('true');
    expect(h.net.calls).toEqual([]);
  });
});

describe('fidelity: transaction block + DTC pane from canonical fixtures', () => {
  const ALL = ['read_ok', 'pending_then_positive', 'nrc', 'read_timeout', 'write_outcome_unknown', 'sprmib', 'blocked_2a', 'dtc_list', 'late_response'];

  it('expanded pairs follow the reference block order with daemon labels only', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(fixture('state')[0]!), ...ALL.flatMap((n) => fixture(n)).map(envelope)]);
    rowEl(h.host, 'r-3').click();
    h.frame();
    const keys = [...h.host.querySelectorAll('.vt-detail .disp-row dt')].map((d) => d.textContent);
    expect(keys.slice(0, 7)).toEqual(['테스터', '서비스', '요청', '논리 링크', '응답 상태', '응답', 'NRC']);
    const d = detailOf(h, 'r-3');
    expect(d['서비스']).toBe('ReadDataByIdentifier — ✗ 0x31 requestOutOfRange');
    expect(d['논리 링크']).toBe('ecu1 · sim DoIP 0x1001');
    expect(detailOf(h, 'r-1')['  DID F190 VIN']).toBe('LOOMSIM0000000001');
    expect(detailOf(h, 'r-2')['응답 상태']).toContain('pending(0x78) ×');
    expect(detailOf(h, 'r-5')['응답 상태']).toMatch(/^불확정/); // ⌛? stays unknown, never success
    expect(detailOf(h, 'r-5')['서비스']).toContain('⌛? 불확정');
  });

  it('detailPairs without a known target does not invent link data', () => {
    const r = { requestId: 'x', state: 'confirmed', payload: { target: 'ghost', sid: 0x22 }, origin: 'agent', clientId: null, updates: 1, stageSeenMs: 0, firstSeenMs: 0 } as unknown as Parameters<typeof detailPairs>[0];
    const m = Object.fromEntries(detailPairs(r, undefined));
    expect(m['논리 링크']).toBe('ghost · ?');
    expect(m['서비스']).toBe('SID 0x22 — ✓');
    expect(m['응답']).toBe('—');
  });

  it('DTC pane shows raw payload.dtcs only; read/clear buttons fill templates and never send', async () => {
    vi.useFakeTimers();
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.net.responders.set('uds.request', (p) => ((p as { dry_run?: boolean }).dry_run ? { service: 'ClearDiagnosticInformation', risk: 'R2' } : {}));
    h.batch([envelope(fixture('state')[0]!), ...fixture('dtc_list').map(envelope)]);
    h.frame();
    const pane = h.host.querySelector<HTMLElement>('[data-testid="uds-dtc"]')!;
    const rowsTxt = [...pane.querySelectorAll<HTMLElement>('[data-dtc]')].map((r) => [...r.children].map((c) => c.textContent));
    expect(rowsTxt).toEqual([['923400', '0x09'], ['C10000', '0x2F']]);
    expect(pane.textContent).toContain('r-8');
    expect(pane.textContent).not.toMatch(/testFailed|MIL|confirmedDTC|warningIndicator|[PBCU]\d{4}-/); // MAIN D2: no status-bit / SAE decoding (codes stay raw, e.g. C10000)

    [...pane.querySelectorAll('button')].find((b) => b.textContent!.startsWith('지우기'))!.click();
    expect(pduInput(h.host).value).toBe('14 FF FF FF');
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS + 1);
    expect(sends(h)).toHaveLength(0);
    pduInput(h.host).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(sends(h)).toHaveLength(0); // R2 dry-run: second Enter still required
    expect(h.host.querySelector('[data-testid="uds-preview"]')!.textContent).toContain('Enter 한 번 더');
  });
});

describe('uds.console — D-D1 link loss: live control state, read-only history', () => {
  it('live link down via the control path makes the console read-only; older rows/snapshots never re-enable it', () => {
    h = mountPanel('uds.console', { targets: 'ext_doip' });
    const up = ev({ kind: 'uds.state', channel: 'ext_doip', payload: { target: 'ext_doip', backend: 'DoIP', link: { state: 'up' } } });
    h.batch([up]);
    expect(pduInput(h.host).disabled).toBe(false);
    // the gateway's link-loss uds.state is a priority kind: it arrives only through onControl
    h.net.emitControl(ev({ kind: 'uds.state', channel: 'ext_doip', payload: { target: 'ext_doip', link: { state: 'down', cause: 'tcp_reset' } } }));
    h.frame();
    expect(pduInput(h.host).disabled).toBe(true);
    // an older backlog row delivered later does not bring the link back
    h.batch([up]);
    expect(pduInput(h.host).disabled).toBe(true);
    // nor an older snapshot (as of the 'up' seq)
    h.net.subs[0]!.h.onSnapshotPart?.({ sub: 's', n: 1, total: 1, events: [], items: [{ domain: 'uds', kind: 'uds.snapshot', channel: 'ext_doip', state: { kind: 'uds.state', payload: { target: 'ext_doip', link: { state: 'up' } } } }] } as never);
    h.snapshotEnd(String(BigInt(up.seq)));
    expect(pduInput(h.host).disabled).toBe(true);
    // nothing was sent while the link was down
    pduInput(h.host).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(h.net.calls.filter((c) => c.method === 'uds.request' && !(c.params as { dry_run?: boolean }).dry_run)).toHaveLength(0);
    // a newer link up re-enables input
    h.net.emitControl(ev({ kind: 'uds.state', channel: 'ext_doip', payload: { target: 'ext_doip', link: { state: 'up' } } }));
    h.frame();
    expect(pduInput(h.host).disabled).toBe(false);
  });
});
