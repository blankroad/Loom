// Operator upgrade UDS batch 4 — pure UI contracts (Opus-authored specification).
//   U8 row route from the daemon stamp · U6 forgotten/closed targets + "재연결 필요" · U1 TesterPresent text/interval ·
//   U2 seed helper · U5 local DB pre-check. Contracts: work/reports/operator-upgrade/uds/FUNCTIONS.md (B4-*).
import { describe, expect, it } from 'vitest';
import {
  emptyTarget,
  isForgotten,
  keyTemplate,
  reduceTargetState,
  ROUTE_UNRECORDED_TITLE,
  rowRoute,
  seedFromRow,
  targetCells,
  targetLine,
  tpCellText,
  tpOkCount,
  type TargetView,
} from './udsModel';
import { checkDbText, DEFAULT_PREFS, DEFAULT_TP_INTERVAL_MS, MAX_DB_TEXT_BYTES, profileDraftForTarget, profileNeedsReconnect, tpIntervalOf } from './udsPrefs';

const tv = (over: Partial<TargetView>): TargetView => ({ ...emptyTarget('ecu1'), link: 'up', backend: 'sim', addr: 'DoIP 0x1001', ...over });
const row = (state: string, payload: Record<string, unknown>) =>
  ({ requestId: 'r', state, payload, origin: 'user', clientId: null, updates: 1, stageSeenMs: 0, firstSeenMs: 0, firstWallMs: 0, channel: null }) as unknown as Parameters<typeof seedFromRow>[0];
const hex = (data: string) => ({ encoding: 'hex', data });

describe('B4-F1 rowRoute (U8)', () => {
  it('stamped rows show backend · addr · generation', () => {
    expect(rowRoute({ generation: 2, backend: 'doip', addr: 'DoIP 0x1001' })).toEqual({
      text: 'doip DoIP 0x1001 · g2', title: '요청 시점에 daemon이 기록한 경로 (generation 2)', known: true, generation: 2,
    });
    expect(rowRoute({ generation: 0, backend: null }).text).toBe('? · g0');
  });
  it('unstamped / malformed rows stay unknown — never inferred', () => {
    for (const p of [{}, { backend: 'sim', addr: 'DoIP 0x1001' }, { generation: '2' }, { generation: -1 }, { generation: 1.5 }]) {
      expect(rowRoute(p)).toEqual({ text: '?', title: ROUTE_UNRECORDED_TITLE, known: false, generation: null });
    }
  });
});

describe('B4-F2 isForgotten', () => {
  it('only an explicit forgotten:true', () => {
    expect(isForgotten({ target: 'a', forgotten: true })).toBe(true);
    for (const p of [{ target: 'a' }, { forgotten: 'true' }, null, 3]) expect(isForgotten(p)).toBe(false);
  });
});

describe('B4-F6 reduceTargetState (new fields)', () => {
  const st = {
    target: 'ecu1', generation: 3, backend: 'sim', addr: 'DoIP 0x1001', link: { state: 'up' },
    session: { id: 3, name: 'extended', confidence: 'confirmed' }, security: { level: null, confidence: 'assumed' },
    tester_present: { active: true, interval_ms: 1500, ok_count: 2, suppress: true, owner: { owner_origin: 'agent', actor_id: 'a', client_id: 'a' } },
    p2_ms: 50, p2_star_ms: 5000, lease: null, db: { hash: '0123456789abcdef', ecu: 'bench', dids: 3 },
  };
  it('reads generation, TP job and DB', () => {
    const v = reduceTargetState(undefined, st)!;
    expect([v.generation, v.tpIntervalMs, v.tpSuppress, v.tpOwner, v.tpStopCause]).toEqual([3, 1500, true, 'agent', null]);
    expect([v.dbHash, v.dbEcu, v.dbDids]).toEqual(['0123456789abcdef', 'bench', 3]);
  });
  it('a canonical state without the optional keys means "none", not "keep previous"', () => {
    const prev = reduceTargetState(undefined, st)!;
    const off = { ...st, tester_present: null, tp_stop_cause: 'STOP' } as Record<string, unknown>;
    delete off.db;
    const v = reduceTargetState(prev, off)!;
    expect([v.tpOn, v.tpIntervalMs, v.tpSuppress, v.tpOwner, v.tpStopCause]).toEqual([false, null, null, null, 'STOP']);
    expect([v.dbHash, v.dbEcu, v.dbDids]).toEqual([null, null, null]);
    const again = { ...off } as Record<string, unknown>;
    delete again.tp_stop_cause;
    expect(reduceTargetState(v, again)!.tpStopCause).toBeNull();
  });
  it('partial legacy updates (no tester_present key) keep previous values', () => {
    const prev = reduceTargetState(undefined, st)!;
    const v = reduceTargetState(prev, { target: 'ecu1', link: { state: 'down', cause: 'x' } })!;
    expect([v.generation, v.tpIntervalMs, v.dbHash, v.link]).toEqual([3, 1500, '0123456789abcdef', 'down']);
  });
});

describe('B4-F3 TesterPresent text (U1)', () => {
  it('daemon job: interval shown, max(ok_count, folded rows) — never doubled', () => {
    const t = tv({ tpOn: true, tpOk: 5, tpIntervalMs: 2000, tpSuppress: true });
    expect(tpOkCount(t, 5)).toBe(5);
    expect(tpOkCount(t, 7)).toBe(7);
    expect(tpCellText(t, 5)).toBe('on 2.0s ✓×5');
    expect(targetCells(t, 5, 'sim').testerPresent).toBe('on 2.0s ✓×5');
    expect(targetLine(t, 5, 'sim')).toContain(' TP ✓×5 ');
  });
  it('legacy producer keeps the additive count and text', () => {
    const t = tv({ tpOn: true, tpOk: 3 });
    expect(tpOkCount(t, 1)).toBe(4);
    expect(tpCellText(t, 1)).toBe('on ✓×4');
  });
  it('off: stop cause shown, folded rows counted', () => {
    expect(tpCellText(tv({ tpStopCause: 'link down (peer closed)' }), 0)).toBe('off (link down (peer closed))');
    expect(tpCellText(tv({ tpStopCause: 'STOP' }), 4)).toBe('off (STOP) ×4');
    expect(tpCellText(tv({}), 4)).toBe('off ×4');
    expect(tpCellText(tv({}), 0)).toBe('off');
  });
});

describe('B4-F4/F5 seed helper (U2 minimum)', () => {
  it('positive 67 xx seed for an odd 27 xx', () => {
    expect(seedFromRow(row('confirmed', { bytes: hex('2701'), resp_bytes: hex('6701a1b2c3d4') }))).toEqual({ level: 1, seedHex: 'A1B2C3D4', zero: false });
    expect(seedFromRow(row('confirmed', { bytes: hex('2703'), resp_bytes: hex('670300000000') }))).toEqual({ level: 3, seedHex: '00000000', zero: true });
  });
  it('nothing for other rows', () => {
    for (const [st, req, rsp] of [
      ['confirmed', '2702aabbccdd', '6702'], // sendKey
      ['confirmed', '2701', '7f2735'], // NRC
      ['failed', '2701', '6701a1b2'],
      ['sent', '2701', '6701a1b2'],
      ['confirmed', '2701', '6703a1b2'], // level echo mismatch
      ['confirmed', '2701', '6701'], // no seed bytes
      ['confirmed', '22f190', '62f190'],
    ] as const) {
      expect(seedFromRow(row(st, { bytes: hex(req), resp_bytes: hex(rsp) }))).toBeNull();
    }
  });
  it('key template keeps the composer unsendable until the key is typed', () => {
    expect(keyTemplate(1)).toBe('27 02 <key>');
    expect(keyTemplate(0x41)).toBe('27 42 <key>');
  });
});

describe('B4-P1..P3 prefs helpers', () => {
  it('B4-P1 interval: preset value or the visible default', () => {
    const p = { ...DEFAULT_PREFS.presets[0]! };
    expect(tpIntervalOf(null)).toBe(DEFAULT_TP_INTERVAL_MS);
    expect(tpIntervalOf(p)).toBe(2000);
    expect(tpIntervalOf({ ...p, tpIntervalMs: 500 })).toBe(500);
    expect(tpIntervalOf({ ...p, tpIntervalMs: 499 })).toBe(2000);
    expect(tpIntervalOf({ ...p, tpIntervalMs: 30_001 })).toBe(2000);
    expect(tpIntervalOf({ ...p, tpIntervalMs: 1500.5 })).toBe(2000);
  });
  it('B4-P2 재연결 필요 only for an up link whose comparable address differs', () => {
    const doip = { ...profileDraftForTarget('ecu', { backend: 'doip', addr: 'DoIP 0x1001' }, 'p'), host: '10.0.0.2' };
    const up = (addr: string | null, backend = 'doip', link = 'up') => ({ backend, addr, link });
    expect(profileNeedsReconnect(doip, up('DoIP 0x1001'))).toBe(false);
    expect(profileNeedsReconnect({ ...doip, la: '0x1002' }, up('DoIP 0x1001'))).toBe(true);
    expect(profileNeedsReconnect({ ...doip, la: '4097' }, up('DoIP 0x1001'))).toBe(false); // 4097 = 0x1001
    expect(profileNeedsReconnect({ ...doip, la: '0x1002' }, up('DoIP 0x1001', 'doip', 'down'))).toBe(false);
    expect(profileNeedsReconnect({ ...doip, sim: true }, up('DoIP 0x1001'))).toBe(true);
    expect(profileNeedsReconnect({ ...doip, la: '0x1002' }, up('CAN 7E0/7E8'))).toBe(false); // not comparable → no claim
  });
  it('B4-P3 local DB pre-check (daemon validates fully)', () => {
    const ok = checkDbText('{"loom_diag":1,"dids":[]}');
    expect(ok.err).toBeNull();
    expect(ok.db).toEqual({ loom_diag: 1, dids: [] });
    expect(ok.bytes).toBe(25);
    expect(checkDbText('{').err).toMatch(/^JSON 구문 오류: /);
    expect(checkDbText('[1]').err).toBe('loom_diag DB 객체가 아님');
    expect(checkDbText('{"loom_diag":2}').err).toBe('loom_diag 1 형식이 아님');
    expect(checkDbText('{"loom_diag":1,"dids":{}}').err).toBe('dids: 배열이 아님');
    const big = checkDbText(`{"loom_diag":1,"ecu":"${'가'.repeat(17_000)}"}`);
    expect(big.err).toBe(`DB가 너무 큼 (${big.bytes} B, 최대 ${MAX_DB_TEXT_BYTES} B)`);
    expect(big.bytes).toBeGreaterThan(MAX_DB_TEXT_BYTES); // UTF-8 bytes, not UTF-16 length
    expect(big.db).toBeNull();
  });
});
