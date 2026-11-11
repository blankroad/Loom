// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, writeEv, type Harness } from '../../sdk/testing/harness';
import { PREVIEW_DEBOUNCE_MS } from './consolePanel';
import { normalizeHex, resultSymbol, timingText } from './udsModel';

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
    expect(previews[0]!.params).toEqual({ target: 'bcm', hex: '22F190', dry_run: true });
    expect(h.host.querySelector('[data-testid="uds-preview"]')!.textContent).toContain('VIN');

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.advanceTimersByTimeAsync(10);
    const sends = h.net.calls.filter((c) => c.method === 'uds.request' && !(c.params as { dry_run?: boolean }).dry_run);
    expect(sends).toEqual([{ method: 'uds.request', params: { target: 'bcm', hex: '22F190' } }]);
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
