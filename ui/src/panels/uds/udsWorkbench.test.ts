// @vitest-environment jsdom
// Bench Light UDS workbench boundaries: real action contracts (uds.connect / uds.request / uds.abort),
// duplicate-entry guards, whole-input validation, sequences (await final, stop on failure, no retry),
// project import validation, and honest response sequence. No simulated payloads: rows/states are the
// canonical Rust serde fixtures or minimal daemon-shaped events.
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { vi } from 'vitest';
import type { LoomEvent } from '../../proto/provisional';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './consolePanel';
import { MAX_PROJECT_BYTES, parseProjectText, projectJson } from './udsTools';
import { DEFAULT_PREFS, PREFS_KEY, sanitizePrefs } from './udsPrefs';
import { pduFields, seqEvents } from './udsModel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
  localStorage.clear();
});

const FIXTURES = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../crates/loom-uds/fixtures');
type FixtureEvent = { kind: string; request_id: string | null; payload: Record<string, unknown> };
const fixture = (name: string): FixtureEvent[] => JSON.parse(readFileSync(resolve(FIXTURES, `${name}.json`), 'utf8')) as FixtureEvent[];
const envelope = (f: FixtureEvent): LoomEvent =>
  ev({
    kind: f.kind,
    channel: String(f.payload.target),
    origin: f.kind === 'uds.state' || f.kind === 'uds.rx' ? 'device' : 'agent',
    ...(f.request_id !== null ? { request_id: f.request_id } : {}),
    ...(typeof f.payload.write_state === 'string' ? { risk: f.payload.risk as LoomEvent['risk'] } : {}),
    payload: f.payload,
  });
const up = (target = 'ecu1') => ev({ kind: 'uds.state', payload: { target, backend: 'sim', addr: 'DoIP 0x1001', link: { state: 'up' } } });
const sends = (hh: Harness) => hh.net.calls.filter((c) => c.method === 'uds.request' && !(c.params as { dry_run?: boolean }).dry_run);
const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
};
function deferred<T = unknown>() {
  let res!: (v: T) => void;
  let rej!: (e: unknown) => void;
  const p = new Promise<T>((a, b) => {
    res = a;
    rej = b;
  });
  return { p, res, rej };
}

describe('connect (uds.connect from a local preset; no disconnect RPC)', () => {
  it('SIM preset → exactly one uds.connect even on rapid double click; up → disabled "연결됨"', async () => {
    h = mountPanel('uds.console');
    h.frame();
    const d = deferred();
    h.net.responders.set('uds.connect', () => d.p);
    const btn = q<HTMLButtonElement>('[data-cmd="connect"]');
    expect(q<HTMLSelectElement>('[aria-label="대상"]').value).toBe('ecu1'); // default SIM bench preset, not connected
    expect(q('.link-st').textContent).toBe('연결 안 됨');
    expect(btn.disabled).toBe(false);
    btn.click();
    btn.click(); // before any render: re-entry guard
    h.frame();
    btn.click();
    await flush();
    const calls = h.net.calls.filter((c) => c.method === 'uds.connect');
    expect(calls).toEqual([{ method: 'uds.connect', params: { name: 'ecu1', sim: 'bench', timeout_ms: 5000 } }]);
    expect(q('.link-st').textContent).toBe('연결 중…'); // waiting on the daemon, never claimed connected
    d.res({});
    await flush();
    h.frame();
    expect(q('.link-st').textContent).toBe('연결 안 됨'); // the RPC result alone is not a link state
    h.batch([up('ecu1')]);
    expect(q('.link-st').textContent).toBe('연결됨');
    expect(btn.disabled).toBe(true);
    expect(btn.title).toContain('연결 해제 RPC가 없어');
    expect(h.host.textContent).not.toContain('연결 해제');
    expect(h.host.textContent).not.toContain('폴링 정지'); // uds.poll.stop is not a daemon RPC
    expect(sends(h)).toHaveLength(0); // connecting sends no request
  });

  it('DoIP preset without host cannot connect (whole-preset validation)', () => {
    h = mountPanel('uds.console');
    h.frame();
    const sel = q<HTMLSelectElement>('[aria-label="대상"]');
    sel.value = 'ecu';
    sel.dispatchEvent(new Event('change'));
    h.frame();
    const btn = q<HTMLButtonElement>('[data-cmd="connect"]');
    expect(btn.disabled).toBe(true);
    expect(btn.title).toContain('호스트');
    btn.click();
    expect(h.net.calls.filter((c) => c.method === 'uds.connect')).toHaveLength(0);
  });

  it('a saved DoIP preset connects with numeric addresses; write lock disables connect', async () => {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ ...DEFAULT_PREFS, presets: [{ id: 'b1', ecu: 'bcm', label: 'BCM', sim: false, host: '192.168.0.10', port: 13400, la: '0x1010', testerLa: '0x0E80', activation: 0, timeoutMs: 3000 }] }));
    h = mountPanel('uds.console');
    h.frame();
    q<HTMLButtonElement>('[data-cmd="connect"]').click();
    await flush();
    expect(h.net.calls.find((c) => c.method === 'uds.connect')!.params).toEqual({ name: 'bcm', host: '192.168.0.10', port: 13400, la: 0x1010, tester_la: 0x0e80, activation: 0, timeout_ms: 3000 });
    h.shell.tick(5_000); // STALLED → write lock
    h.frame();
    expect(q<HTMLButtonElement>('[data-cmd="connect"]').disabled).toBe(true);
  });
});

describe('composer: DID / Raw whole-input validation and duplicate guard', () => {
  it('DID mode: Enter reads 22+DID; malformed DIDs send nothing', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    const did = q<HTMLInputElement>('[aria-label="DID"]');
    const enter = () => did.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    for (const bad of ['', '22 F1 90', 'F1', 'F1901', 'ZZZZ', '0x1']) {
      did.value = bad;
      did.dispatchEvent(new Event('input'));
      enter();
      h.frame();
      expect(q('.req-msg').className).toContain('err');
    }
    did.value = '22 F1 90';
    did.dispatchEvent(new Event('input'));
    h.frame();
    expect(q('.req-msg').textContent).toContain('SID 22는 자동');
    expect(sends(h)).toHaveLength(0);
    did.value = '0xf1_8c';
    did.dispatchEvent(new Event('input'));
    enter();
    await flush();
    expect(sends(h).map((c) => c.params)).toEqual([{ target: 'ecu1', bytes_hex: '22F18C' }]);
  });

  it('Raw mode: plain Enter never sends; odd/non-hex/comma inputs are rejected whole', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    q<HTMLButtonElement>('.mode-seg [data-v="raw"]').click();
    const raw = q<HTMLTextAreaElement>('[aria-label="UDS 요청 (hex)"]');
    const ctrl = () => raw.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }));
    for (const bad of ['22 F1 9', '22,F1,90', '0x2 F1', 'read F190', '']) {
      raw.value = bad;
      raw.dispatchEvent(new Event('input'));
      ctrl();
    }
    raw.value = '22 F1 90';
    raw.dispatchEvent(new Event('input'));
    raw.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(sends(h)).toHaveLength(0);
    raw.value = '0x2E 0xF1 0xA0\n02';
    raw.dispatchEvent(new Event('input'));
    h.frame();
    expect(q('.raw-prev').textContent).toContain('2E F1 A0 02');
    ctrl();
    await flush();
    expect(sends(h).map((c) => (c.params as { bytes_hex: string }).bytes_hex)).toEqual(['2EF1A002']);
  });

  it('button + Enter repeats before the RPC settles send once; the next send is allowed after it settles (no retry)', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    const d = deferred();
    h.net.responders.set('uds.request', (p) => ((p as { dry_run?: boolean }).dry_run ? { risk: 'R0' } : d.p));
    const did = q<HTMLInputElement>('[aria-label="DID"]');
    const btn = q<HTMLButtonElement>('.uds-ds-transmit');
    btn.click();
    did.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    btn.click();
    await flush();
    h.frame();
    expect(sends(h)).toHaveLength(1);
    expect(btn.disabled).toBe(true);
    d.rej(new Error('NRC 0x31 requestOutOfRange'));
    await flush();
    h.frame();
    expect(sends(h)).toHaveLength(1); // failure is shown, not retried
    expect(btn.disabled).toBe(false);
    h.net.responders.set('uds.request', () => ({ request_id: 'r-2' }));
    btn.click();
    await flush();
    expect(sends(h)).toHaveLength(2);
  });

  it('abort stays available and calls uds.abort for the selected target', async () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    h.shell.tick(5_000);
    h.frame();
    const ab = q<HTMLButtonElement>('[data-testid="uds-current"] .danger');
    expect(ab.disabled).toBe(false);
    ab.click();
    await flush();
    expect(h.net.calls.filter((c) => c.method === 'uds.abort').map((c) => c.params)).toEqual([{ target: 'ecu1' }]);
  });

  it('DID list click fills the editor only', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    q<HTMLElement>('.didlist li[data-did="F18C"]').click();
    h.frame();
    expect(q<HTMLInputElement>('[aria-label="DID"]').value).toBe('F18C');
    expect(q('.did-in').getAttribute('aria-invalid')).toBe('false');
    expect(sends(h)).toHaveLength(0);
  });
});

describe('response sequence / interpretation / bytes from real rows', () => {
  it('pending_then_positive: Tx · 0x78 ×N (count only) · final 62 with daemon-decoded value; provenance reveal', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([...fixture('state').slice(0, 1), ...fixture('pending_then_positive')].map(envelope));
    const row = h.host.querySelector<HTMLElement>('.vt-row[data-request-id="r-2"]')!;
    expect(row.className).toContain('s-positive');
    row.click();
    h.frame();
    const seq = [...h.host.querySelectorAll<HTMLElement>('.seq li')].map((li) => li.textContent);
    expect(seq[0]).toContain('22 F1 90');
    expect(seq[1]).toMatch(/0x78 responsePending ×\d/);
    expect(seq[1]).toContain('개별 수신 시각은 행에 기록되지 않음');
    expect(seq[2]).toContain('62 F1 90 4C 4F');
    const fields = q('[data-testid="uds-fields"]').textContent!;
    expect(fields).toContain('RESPONSE_PDU');
    expect(fields).toContain('daemon 해석');
    expect(fields).toContain('LOOMSIM0000000001');
    expect(q('.hex .hx-cap').textContent).toContain('UDS PDU (DoIP/TCP 프레임 아님)');
    // values pane → provenance link reselects the source row
    row.click(); // deselect
    h.frame();
    q<HTMLElement>('[data-testid="uds-last-values"] [data-did="ecu1:F190"] .cv-p a').click();
    h.frame();
    expect(h.host.querySelector('.vt-row.selected')!.getAttribute('data-request-id')).toBe('r-2');
  });

  it('timeout / unknown / denied carry no invented response bytes', () => {
    const r = (state: string, payload: Record<string, unknown>) =>
      ({ requestId: 'x', state, payload: { bytes: { encoding: 'hex', data: '22F190' }, ...payload }, firstSeenMs: 0, stageSeenMs: 0, firstWallMs: 0, updates: 1 }) as unknown as Parameters<typeof seqEvents>[0];
    expect(seqEvents(r('failed', { timeout: true, p2_star_ms: 5000 }), 0).slice(1).every((e) => e.hex === null)).toBe(true);
    expect(seqEvents(r('outcome_unknown', {}), 0)[1]!.label).toContain('불확정');
    const denied = seqEvents(r('failed', { block: { class: 'immutable_deny', reason: 'x' } }), 0);
    expect(denied[0]!.note).toBe('전송 안 됨');
    expect(denied[1]!.hex).toBeNull();
    // NRC split is structural only
    expect(pduFields('7F2231', 'rx', { nrc_name: 'requestOutOfRange' }).map((f) => [f.name, f.value])).toEqual([
      ['negativeResponse', '0x7F'], ['requestServiceId', '0x22'], ['negativeResponseCode', '0x31'],
    ]);
  });
});

describe('saved commands and sequences', () => {
  const seed = () =>
    localStorage.setItem(PREFS_KEY, JSON.stringify({
      ...DEFAULT_PREFS,
      cmds: [{ id: 'c1', name: 'VIN', hex: '22F190' }, { id: 'c2', name: 'SN', hex: '22F18C' }, { id: 'c3', name: 'PN', hex: '22F187' }],
      seqs: [{ id: 's1', name: 'ident', steps: ['c1', 'c2', 'c3'] }],
    }));
  const openWorkflow = () => {
    q<HTMLButtonElement>('[data-tool="workflow"]').click();
    h!.frame();
  };

  it('runs one step at a time awaiting each final result; stops on the first failure; no retry', async () => {
    seed();
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    const first = deferred();
    h.net.responders.set('uds.request', (p) => {
      const hex = (p as { bytes_hex: string }).bytes_hex;
      if (hex === '22F190') return first.p;
      if (hex === '22F18C') throw new Error('NRC 0x31 requestOutOfRange');
      return { request_id: 'r-x' };
    });
    openWorkflow();
    q<HTMLButtonElement>('[data-seq="s1"] [data-run="s1"]').click();
    await flush();
    expect(sends(h).map((c) => (c.params as { bytes_hex: string }).bytes_hex)).toEqual(['22F190']); // waits for step 1
    first.res({ request_id: 'r-1' });
    await vi.waitFor(() => expect(q('[data-testid="uds-seq-run"]').textContent).toContain('실패로 중지'));
    expect(sends(h).map((c) => (c.params as { bytes_hex: string }).bytes_hex)).toEqual(['22F190', '22F18C']); // 22F187 never sent
    expect([...h.host.querySelectorAll<HTMLElement>('.wf-steplist li')].map((li) => li.dataset.status)).toEqual(['ok', 'fail', 'skip']);
    await flush();
    expect(sends(h)).toHaveLength(2); // no retry
  });

  it('[중지] stops after the current step; write lock blocks the next step', async () => {
    seed();
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    const first = deferred();
    h.net.responders.set('uds.request', (p) => ((p as { bytes_hex: string }).bytes_hex === '22F190' ? first.p : { request_id: 'r' }));
    openWorkflow();
    q<HTMLButtonElement>('[data-run="s1"]').click();
    await flush();
    q<HTMLButtonElement>('[data-seq="s1"] [data-run="s1"]').click(); // now [중지]
    first.res({ request_id: 'r-1' });
    await vi.waitFor(() => expect(q('[data-testid="uds-seq-run"]').textContent).toContain('중지됨'));
    expect(sends(h)).toHaveLength(1);
  });

  it('saved command [실행] sends once per settle; repeated clicks are dropped', async () => {
    seed();
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([up()]);
    const d = deferred();
    h.net.responders.set('uds.request', () => d.p);
    openWorkflow();
    const go = () => q<HTMLButtonElement>('[data-cmd="c1"] [data-run="c1"]');
    go().click();
    go().click();
    await flush();
    h.frame();
    go().click();
    expect(sends(h)).toHaveLength(1);
    d.res({ request_id: 'r-1' });
    await flush();
    h.frame();
    expect(go().disabled).toBe(false);
  });

  it('link down disables [실행]', () => {
    seed();
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([ev({ kind: 'uds.state', payload: { target: 'ecu1', link: { state: 'down', cause: 'tcp_reset' } } })]);
    openWorkflow();
    expect(q<HTMLButtonElement>('[data-run="s1"]').disabled).toBe(true);
    expect(q<HTMLButtonElement>('[data-cmd="c1"] [data-run="c1"]').disabled).toBe(true);
  });
});

describe('prefs / project import (malformed input)', () => {
  it('corrupt stored prefs fall back to defaults; panel mounts without sending', () => {
    localStorage.setItem(PREFS_KEY, '{not json');
    h = mountPanel('uds.console');
    h.frame();
    expect(q<HTMLSelectElement>('[aria-label="대상"]').value).toBe('ecu1');
    expect(h.net.calls).toEqual([]);
  });

  it('sanitizePrefs drops invalid presets/commands and sequences that reference missing commands', () => {
    const p = sanitizePrefs({
      presets: [
        { id: 'ok', ecu: 'bcm', label: 'B', sim: false, host: '10.0.0.1', port: 13400, la: '0x1010', testerLa: '0x0E80', activation: 0, timeoutMs: 3000 },
        { id: 'badport', ecu: 'x', label: 'X', sim: false, host: 'h', port: 70000, la: '0x1', testerLa: '0x2', activation: 0, timeoutMs: 3000 },
        { id: 'badla', ecu: 'y', label: 'Y', sim: true, host: '', port: 1, la: 'zz', testerLa: '0x2', activation: 0, timeoutMs: 3000 },
        { id: 'ok', ecu: 'dup', label: 'D', sim: true, host: '', port: 1, la: '0x1', testerLa: '0x2', activation: 0, timeoutMs: 3000 },
      ],
      cmds: [{ id: 'c1', name: 'a', hex: '22F190' }, { id: 'c2', name: 'b', hex: '22F' }, { id: 'c3', name: '', hex: '10' }],
      seqs: [{ id: 's1', name: 'ok', steps: ['c1'] }, { id: 's2', name: 'gone', steps: ['c1', 'c2'] }],
    });
    expect(p.presets.map((x) => x.id)).toEqual(['ok']);
    expect(p.cmds.map((x) => x.id)).toEqual(['c1']);
    expect(p.seqs.map((x) => x.id)).toEqual(['s1']);
  });

  it('parseProjectText: syntax, format, version, size and empty-preset errors; valid file reports drops', () => {
    expect(parseProjectText('{').error).toContain('JSON 구문 오류');
    expect(parseProjectText('{"format":"other","version":1}').error).toContain('형식이 다름');
    expect(parseProjectText('{"format":"loom.uds.project","version":2,"prefs":{}}').error).toContain('버전');
    expect(parseProjectText('{"format":"loom.uds.project","version":1,"prefs":{"presets":[]}}').error).toContain('최소 1개');
    expect(parseProjectText('{"format":"loom.uds.project","version":1,"prefs":{"presets":{}}}').error).toContain('배열이 아님');
    expect(parseProjectText(' '.repeat(MAX_PROJECT_BYTES + 1)).error).toContain('너무 큼');
    const good = JSON.parse(projectJson(DEFAULT_PREFS)) as { prefs: Record<string, unknown[]> };
    good.prefs.cmds = [{ id: 'c1', name: 'x', hex: '22F190' }, { id: 'c2', name: 'y', hex: 'XYZ' }];
    const r = parseProjectText(JSON.stringify({ ...good, format: 'loom.uds.project', version: 1 }));
    expect(r.error).toBeNull();
    expect(r.report.cmds).toEqual([1, 1]);
    expect(r.prefs!.presets).toHaveLength(2);
  });
});
