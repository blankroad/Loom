// @vitest-environment jsdom
// Operator upgrade UDS batch 1 (Opus-authored specification; Sonnet implements bodies only).
//   U4 transaction table: header/row tracks identical, response column last, ms visible, ECU full name.
//   U3 DTC status bits: ISO 14229-1 DTCStatusMask decode + availability mask + filter; codes stay raw.
//   U7 targets without a profile: honest recovery text, profile draft prefilled from daemon state only.
// Contracts: work/reports/operator-upgrade/uds/FUNCTIONS.md (B1-*).
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import type { LoomEvent } from '../../proto/provisional';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './consolePanel';
import {
  DTC_STATUS_BITS,
  dtcAvailabilityMask,
  dtcStatusFlags,
  dtcStatusTitle,
  filterDtcs,
  hexRowWidth,
  laFromAddr,
  latestDtcs,
  linkDownMessage,
  type DtcEntry,
} from './udsModel';
import { DEFAULT_PREFS, PREFS_KEY, presetIssues, profileDraftForTarget } from './udsPrefs';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
});

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURES = resolve(HERE, '../../../../crates/loom-uds/fixtures');
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
const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const row = (requestId: string, state: string, payload: Record<string, unknown>) =>
  ({ requestId, state, payload, origin: 'agent', clientId: null, updates: 1, stageSeenMs: 0, firstSeenMs: 0, firstWallMs: 0, channel: null }) as unknown as Parameters<typeof latestDtcs>[0][number];
const entry = (dtc: string, statusByte: number | null): DtcEntry => ({ dtc, status: statusByte === null ? '—' : `0x${statusByte.toString(16).toUpperCase().padStart(2, '0')}`, statusByte, flags: [] });

// ------------------------------------------------------------------ U3 pure contracts
describe('B1-F1 dtcStatusFlags', () => {
  it('decodes all 8 bits in bit order with availability', () => {
    const f = dtcStatusFlags(0x09, 0xff);
    expect(f.map((x) => x.bit)).toEqual([0, 1, 2, 3, 4, 5, 6, 7]);
    expect(f.filter((x) => x.set).map((x) => x.abbr)).toEqual(['TF', 'CDTC']);
    expect(f.every((x) => x.supported === true)).toBe(true);
    expect(f.map((x) => x.name)).toEqual(DTC_STATUS_BITS.map((b) => b.name));
  });
  it('unknown availability → supported null (never assumed true/false)', () => {
    const f = dtcStatusFlags(0x2f, null);
    expect(f.filter((x) => x.set).map((x) => x.abbr)).toEqual(['TF', 'TFTOC', 'PDTC', 'CDTC', 'TFSLC']);
    expect(f.every((x) => x.supported === null)).toBe(true);
  });
  it('a set bit the ECU does not advertise stays set but supported=false', () => {
    const f = dtcStatusFlags(0x80, 0x09);
    expect(f[7]).toEqual({ bit: 7, abbr: 'WIR', name: 'warningIndicatorRequested', set: true, supported: false });
    expect(f[0]!.supported).toBe(true);
    expect(f[3]!.supported).toBe(true);
    expect(f[1]!.supported).toBe(false);
  });
  it('0x00 and 0xFF boundaries', () => {
    expect(dtcStatusFlags(0x00, 0x00).some((x) => x.set)).toBe(false);
    expect(dtcStatusFlags(0xff, 0x00).every((x) => x.set && x.supported === false)).toBe(true);
  });
});

describe('B1-F2 dtcAvailabilityMask', () => {
  it('3rd byte of a 59 02 response', () => {
    expect(dtcAvailabilityMask(row('a', 'confirmed', { resp_bytes: { encoding: 'hex', data: '5902ff92340009' } }))).toBe(0xff);
    expect(dtcAvailabilityMask(row('a', 'confirmed', { resp_bytes: { encoding: 'hex', data: '5902090000' } }))).toBe(0x09);
  });
  it('null when absent, too short, negative or another report type', () => {
    expect(dtcAvailabilityMask(row('a', 'confirmed', {}))).toBeNull();
    expect(dtcAvailabilityMask(row('a', 'confirmed', { resp_bytes: { encoding: 'hex', data: '5902' } }))).toBeNull();
    expect(dtcAvailabilityMask(row('a', 'failed', { resp_bytes: { encoding: 'hex', data: '7f1931' } }))).toBeNull();
    expect(dtcAvailabilityMask(row('a', 'confirmed', { resp_bytes: { encoding: 'hex', data: '59010901' } }))).toBeNull();
  });
});

describe('B1-F3 filterDtcs', () => {
  const list = [entry('923400', 0x09), entry('C10000', 0x2f), entry('A00000', 0x10), entry('B00000', null)];
  it('all keeps every entry in response order (a new array)', () => {
    const out = filterDtcs(list, 'all');
    expect(out.map((e) => e.dtc)).toEqual(['923400', 'C10000', 'A00000', 'B00000']);
    expect(out).not.toBe(list);
  });
  it('confirmed / testFailed / pending select bit 3 / 0 / 2; unknown status never matches a bit filter', () => {
    expect(filterDtcs(list, 'confirmed').map((e) => e.dtc)).toEqual(['923400', 'C10000']);
    expect(filterDtcs(list, 'testFailed').map((e) => e.dtc)).toEqual(['923400', 'C10000']);
    expect(filterDtcs(list, 'pending').map((e) => e.dtc)).toEqual(['C10000']);
  });
});

describe('B1-F4 dtcStatusTitle', () => {
  it('exact multi-line breakdown incl. unsupported bits', () => {
    expect(dtcStatusTitle(0x09, 0xf9)).toBe(
      [
        'status 0x09 = 0b00001001',
        'bit0 TF testFailed: 1',
        'bit1 TFTOC testFailedThisOperationCycle: 0 (ECU 미지원)',
        'bit2 PDTC pendingDTC: 0 (ECU 미지원)',
        'bit3 CDTC confirmedDTC: 1',
        'bit4 TNCSLC testNotCompletedSinceLastClear: 0',
        'bit5 TFSLC testFailedSinceLastClear: 0',
        'bit6 TNCTOC testNotCompletedThisOperationCycle: 0',
        'bit7 WIR warningIndicatorRequested: 0',
        'availability mask 0xF9',
      ].join('\n'),
    );
  });
  it('unknown mask and missing status', () => {
    expect(dtcStatusTitle(0x01, null).split('\n').at(-1)).toBe('availability mask 미상');
    expect(dtcStatusTitle(0x01, null)).not.toContain('미지원');
    expect(dtcStatusTitle(null, 0xff)).toBe('status 미보고');
  });
});

describe('B1-F5 latestDtcs (extended view)', () => {
  it('fills statusByte/flags/availabilityMask from the canonical fixture; legacy text unchanged', () => {
    const rows = fixture('dtc_list')
      .filter((f) => f.request_id)
      .map((f) => row(f.request_id!, String(f.payload.write_state), f.payload));
    const v = latestDtcs(rows, 'ecu1')!;
    expect(v.availabilityMask).toBe(0xff);
    expect(v.entries).toEqual([
      { dtc: '923400', status: '0x09', statusByte: 9, flags: ['TF', 'CDTC'] },
      { dtc: 'C10000', status: '0x2F', statusByte: 47, flags: ['TF', 'TFTOC', 'PDTC', 'CDTC', 'TFSLC'] },
    ]);
  });
  it('legacy hex-string status and garbage status', () => {
    const v = latestDtcs([row('x', 'confirmed', { target: 't', dtcs: [{ dtc: 'AA0000', status: '0x08' }, { dtc: 'BB0000', status: 'zz' }, { dtc: 'CC0000' }] })], 't')!;
    expect(v.entries).toEqual([
      { dtc: 'AA0000', status: '0x08', statusByte: 8, flags: ['CDTC'] },
      { dtc: 'BB0000', status: '0xZZ', statusByte: null, flags: [] },
      { dtc: 'CC0000', status: '—', statusByte: null, flags: [] },
    ]);
    expect(v.availabilityMask).toBeNull();
  });
});

// ------------------------------------------------------------------ U4 / U7 pure contracts
describe('B1-F6 hexRowWidth', () => {
  it('8 bytes below 520 px, 16 otherwise; unmeasured (0/NaN) keeps 16', () => {
    expect(hexRowWidth(0)).toBe(16);
    expect(hexRowWidth(Number.NaN)).toBe(16);
    expect(hexRowWidth(-5)).toBe(16);
    expect(hexRowWidth(1)).toBe(8);
    expect(hexRowWidth(519)).toBe(8);
    expect(hexRowWidth(520)).toBe(16);
  });
});

describe('B1-F7 laFromAddr', () => {
  it('trailing 0x LA of a daemon addr, normalised to 0xHHHH', () => {
    expect(laFromAddr('DoIP 0x1001')).toBe('0x1001');
    expect(laFromAddr('DoIP 0xe80')).toBe('0x0E80');
    expect(laFromAddr('0x0001')).toBe('0x0001');
  });
  it('no guess for other shapes', () => {
    expect(laFromAddr(null)).toBeNull();
    expect(laFromAddr('')).toBeNull();
    expect(laFromAddr('CAN 7E0/7E8')).toBeNull();
    expect(laFromAddr('DoIP 0x12345')).toBeNull();
    expect(laFromAddr('DoIP 0x1001 extra')).toBeNull();
  });
});

describe('B1-F8 linkDownMessage', () => {
  it('with a profile keeps the existing text; without one points at profile creation (no dead [연결] hint)', () => {
    expect(linkDownMessage('down', true)).toBe('링크 down — 읽기 전용 (기록은 유지). [연결]로 다시 연결');
    expect(linkDownMessage('down', false)).toBe('링크 down — 읽기 전용 (기록은 유지). 이 대상의 프로필 없음 — [이 대상으로 프로필 만들기] 후 [연결]');
  });
});

describe('B1-F9 profileDraftForTarget', () => {
  it('prefills only what the daemon reported; host empty; never another profile', () => {
    const d = profileDraftForTarget('qa-native-ecu', { backend: 'doip', addr: 'DoIP 0x1001' }, 'p-x');
    expect(d).toEqual({ id: 'p-x', ecu: 'qa-native-ecu', label: 'qa-native-ecu (daemon 보고에서)', sim: false, host: '', port: 13400, la: '0x1001', testerLa: '0x0E80', activation: 0, timeoutMs: 5000 });
    expect(presetIssues(d)).toContain('DoIP 호스트(IP) 필요'); // cannot connect until the user fills the host
  });
  it('sim backend → sim profile; unknown addr → empty LA (flagged, not guessed)', () => {
    expect(profileDraftForTarget('ecu9', { backend: 'sim', addr: 'DoIP 0x1001' }, 'p').sim).toBe(true);
    const d = profileDraftForTarget('ecu9', { backend: 'doip', addr: null }, 'p');
    expect(d.la).toBe('');
    expect(presetIssues(d)).toContain('ECU 논리 주소 0x0000–0xFFFF');
    expect(profileDraftForTarget('ecu9', undefined, 'p').la).toBe('');
  });
});

// ------------------------------------------------------------------ panel behaviour
const downState = (target: string, addr = 'DoIP 0x1001') =>
  ev({ kind: 'uds.state', channel: target, payload: { target, generation: 2, backend: 'doip', addr, link: { state: 'down', cause: 'peer closed' } } });

describe('B1-P1 U4 transaction table', () => {
  it('response column is last; header and rows share one track list', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(fixture('state')[0]!), ...fixture('read_ok').map((f) => envelope(f))]);
    const head = q('.hist .vt-head');
    expect([...head.children].map((c) => c.textContent)).toEqual(['#', '시각', 'ECU', '경로', 'DID · SID', '요청', '상태', '78', '경과', '최종 응답']);
    const r = q('.hist .vt-row[data-request-id="r-1"]');
    expect(r.style.gridTemplateColumns).toBe(head.style.gridTemplateColumns);
    expect([...r.children].at(-1)!.classList.contains('c-rsp')).toBe(true);
    expect([...r.children].at(-1)!.textContent).toContain('62 F1 90');
  });
  it('time keeps milliseconds; ECU cell always carries the full name as title', () => {
    const name = 'qa-native-ecu-long-name';
    h = mountPanel('uds.console', { targets: name });
    const st = fixture('state')[0]!;
    const rows = fixture('read_ok').map((f) => ({ ...f, payload: { ...f.payload, target: name } }));
    h.batch([envelope({ ...st, payload: { ...st.payload, target: name } }), ...rows.map((f) => envelope(f))]); // ev() default wall_ns ≠ 0
    const r = q(`.hist .vt-row[data-request-id="r-1"]`);
    expect(r.querySelector('.c-t')!.textContent).toMatch(/^\d\d:\d\d:\d\d\.\d{3}$/);
    expect((r.querySelector('.c-t') as HTMLElement).title).toMatch(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/);
    expect((r.querySelector('.c-ecu') as HTMLElement).title).toBe(name);
  });
  it('regression guard: history rows never size to max-content (the cause of the U4 misalignment)', () => {
    const css = readFileSync(resolve(HERE, 'uds.css'), 'utf8');
    const rules = css.split('}').filter((r) => r.includes('.hist .vt-row') || r.includes('.hist .vt-head'));
    expect(rules.length).toBeGreaterThan(0);
    for (const r of rules) expect(r).not.toContain('max-content');
  });
  it('export buttons carry their own class so labels never overlap', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    const exp = [...h.host.querySelectorAll<HTMLButtonElement>('.hist .pane-h .ibtn.exp')];
    expect(exp.map((b) => b.textContent)).toEqual(['JSON', 'CSV']);
  });
  it('hex dump wraps at 8 bytes in a narrow pane and 16 when unmeasured', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(fixture('state')[0]!), ...fixture('read_ok').map((f) => envelope(f))]);
    q<HTMLElement>('.hist .vt-row[data-request-id="r-1"]').click();
    h.frame();
    expect(h.host.querySelectorAll('.hex .hx-row')).toHaveLength(2); // 20 B response, jsdom width 0 → 16/row
    const hex = q<HTMLElement>('.hex');
    Object.defineProperty(hex, 'clientWidth', { configurable: true, value: 400 });
    h.instance.resize?.(400, 300);
    h.frame();
    expect(h.host.querySelectorAll('.hex .hx-row')).toHaveLength(3); // 8/row
    expect([...h.host.querySelectorAll('.hex .hx-row')[0]!.querySelectorAll('.hx-h b')]).toHaveLength(8);
  });
});

describe('B1-P2 U3 DTC pane filter', () => {
  it('filter seg narrows rows, keeps order, shows counts; codes and raw status stay', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([envelope(fixture('state')[0]!), ...fixture('dtc_list').map((f) => envelope(f))]);
    const pane = q('[data-testid="uds-dtc"]');
    const shown = () => [...pane.querySelectorAll<HTMLElement>('[data-dtc]')].map((r) => r.dataset.dtc);
    const pick = (v: string) => {
      pane.querySelector<HTMLButtonElement>(`.dtc-filter [data-v="${v}"]`)!.click();
      h!.frame();
    };
    expect(shown()).toEqual(['923400', 'C10000']);
    expect(pane.querySelector('.dtc-count')!.textContent).toBe('표시 2/2');
    pick('pending');
    expect(shown()).toEqual(['C10000']);
    expect(pane.querySelector('.dtc-count')!.textContent).toBe('표시 1/2');
    expect(pane.querySelector('.dtc-filter [data-v="pending"]')!.getAttribute('aria-checked')).toBe('true');
    pick('all');
    expect(shown()).toEqual(['923400', 'C10000']);
    const st = pane.querySelector<HTMLElement>('[data-dtc="923400"] [data-cell="status"]')!;
    expect(st.textContent).toBe('0x09');
    expect(st.title).toBe(dtcStatusTitle(0x09, 0xff));
  });
  it('a filter with no match says so and keeps the total', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    const dl = fixture('dtc_list').map((f) =>
      f.kind === 'uds.result' ? { ...f, payload: { ...f.payload, dtcs: [{ dtc: '923400', status: 0x08 }] } } : f,
    );
    h.batch([envelope(fixture('state')[0]!), ...dl.map((f) => envelope(f))]);
    const pane = q('[data-testid="uds-dtc"]');
    pane.querySelector<HTMLButtonElement>('.dtc-filter [data-v="testFailed"]')!.click();
    h.frame();
    expect(pane.querySelectorAll('[data-dtc]')).toHaveLength(0);
    expect(pane.textContent).toContain('해당 DTC 없음 (전체 1)');
  });
});

describe('B1-P3 U7 target without a profile', () => {
  it('link down + no profile: no dead "[연결]로 다시 연결" hint, a make-profile button instead', () => {
    h = mountPanel('uds.console', { targets: 'qa-native-ecu' });
    h.batch([downState('qa-native-ecu')]);
    const msg = q('.req-msg').textContent!;
    expect(msg).toContain(linkDownMessage('down', false));
    expect(msg).not.toContain('[연결]로 다시 연결');
    const mk = q<HTMLButtonElement>('[data-cmd="make-profile"]');
    expect(mk.hidden).toBe(false);
    expect(q<HTMLButtonElement>('[data-cmd="connect"]').disabled).toBe(true);
  });
  it('make-profile opens a NEW draft for this target, prefilled from uds.state — never presets[0]', () => {
    h = mountPanel('uds.console', { targets: 'qa-native-ecu' });
    h.batch([downState('qa-native-ecu')]);
    q<HTMLButtonElement>('[data-cmd="make-profile"]').click();
    h.frame();
    const cfg = q('.cfg-panel');
    expect(cfg.hidden).toBe(false);
    expect(cfg.querySelector<HTMLInputElement>('[data-f="ecu"] input')!.value).toBe('qa-native-ecu');
    expect(cfg.querySelector<HTMLInputElement>('[data-f="la"] input')!.value).toBe('1001');
    expect(cfg.querySelector<HTMLInputElement>('[data-f="host"] input')!.value).toBe('');
    const sel = cfg.querySelector<HTMLSelectElement>('.cfg-sel')!;
    expect(sel.selectedOptions[0]!.textContent).toBe('새 프로필 (저장 안 됨)');
    // nothing was written: the stored presets are still the defaults (no overwrite of ecu1's SIM profile)
    expect(localStorage.getItem(PREFS_KEY)).toBeNull();
  });
  it('the [연결 설정] button on a profile-less target opens the same new draft (not the first preset)', () => {
    h = mountPanel('uds.console', { targets: 'qa-native-ecu' });
    h.batch([downState('qa-native-ecu', 'DoIP 0x0E81')]);
    [...h.host.querySelectorAll<HTMLButtonElement>('.ctx-row button')].find((b) => b.textContent!.includes('연결 설정'))!.click();
    h.frame();
    const cfg = q('.cfg-panel');
    expect(cfg.querySelector<HTMLInputElement>('[data-f="ecu"] input')!.value).toBe('qa-native-ecu');
    expect(cfg.querySelector<HTMLInputElement>('[data-f="la"] input')!.value).toBe('0E81');
    expect(DEFAULT_PREFS.presets.map((p) => p.ecu)).not.toContain('qa-native-ecu');
  });
  it('a target WITH a profile keeps the existing reconnect text and no make-profile button', () => {
    h = mountPanel('uds.console', { targets: 'ecu1' });
    h.batch([downState('ecu1')]);
    expect(q('.req-msg').textContent).toContain('[연결]로 다시 연결');
    expect(q<HTMLButtonElement>('[data-cmd="make-profile"]').hidden).toBe(true);
  });
});
