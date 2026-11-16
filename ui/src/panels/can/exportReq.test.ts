// B4 spec (Opus): CAN-4 export request/result (UI). FUNCTIONS.md §B4-UI.
import { describe, expect, it } from 'vitest';
import { parseIdFilter } from './idFilter';
import { EXPORT_IDS_MAX, exportIdTokens, exportRequest, exportResultLines, ringSeqs, type ExportInputs } from './exportReq';

const base: ExportInputs = {
  channel: 'ch0', format: 'blf', scope: 'ring', ringFirstSeq: 10, ringLastSeq: 42, useFilter: false,
  ids: null, dir: 'all', origins: null, path: '',
};

describe('exportIdTokens', () => {
  it('singles and ranges in normalised order', () => {
    expect(exportIdTokens(parseIdFilter('7e0-7ef 300 18DAF110')!)).toEqual(['0x300', '0x7E0-0x7EF', '0x18DAF110']);
  });
});

describe('ringSeqs', () => {
  const ring = (kinds: number[], seqs: number[], oldest = 0) => ({
    oldestAbs: oldest, total: oldest + kinds.length,
    kindAt: (a: number) => kinds[a - oldest]!, seqAt: (a: number) => seqs[a - oldest]!,
  });
  it('skips gap rows and seq 0 at both ends', () => {
    expect(ringSeqs(ring([2, 0, 0, 1, 2], [0, 7, 8, 0, 0], 5), 2)).toEqual({ first: 7, last: 8 });
  });
  it('empty ring / no frame rows within the probe → null', () => {
    expect(ringSeqs(ring([], []), 2)).toEqual({ first: null, last: null });
    expect(ringSeqs(ring(Array(70).fill(2), Array(70).fill(0)), 2)).toEqual({ first: null, last: null });
  });
});

describe('exportRequest', () => {
  it('ring scope: seq bounds of the panel', () => {
    expect(exportRequest(base)).toEqual({
      ok: true,
      params: { channel: 'ch0', format: 'blf', since: 'seq:10', until: 'seq:42' },
      scopeText: '이 패널 보관 범위 seq 10–42',
    });
  });
  it('ring scope with nothing held → error, no call', () => {
    expect(exportRequest({ ...base, ringFirstSeq: null, ringLastSeq: null })).toEqual({
      ok: false, error: '이 패널에 보관된 프레임이 없습니다 — "최근 10분"을 고르세요',
    });
  });
  it('recent scope, asc, explicit absolute path (trimmed)', () => {
    expect(exportRequest({ ...base, scope: 'recent', format: 'asc', path: '  /data/cap.asc ' })).toEqual({
      ok: true,
      params: { channel: 'ch0', format: 'asc', since: '-10m', path: '/data/cap.asc' },
      scopeText: '최근 10분',
    });
    expect(exportRequest({ ...base, path: 'C:\\logs\\a.blf' }).ok).toBe(true);
  });
  it('relative / too long path → error', () => {
    expect(exportRequest({ ...base, path: 'cap.blf' })).toEqual({ ok: false, error: '경로는 데몬 컴퓨터의 절대 경로여야 합니다 (비우면 임시 폴더)' });
    expect(exportRequest({ ...base, path: `/${'a'.repeat(4096)}` })).toEqual({ ok: false, error: '경로가 너무 깁니다 (≤ 4096자)' });
  });
  it('filter applied only when asked: ids, dir, origins in fixed order', () => {
    const withF = { ...base, ids: parseIdFilter('300 7e0-7ef'), dir: 'rx' as const, origins: new Set(['device', 'agent'] as const) };
    expect(exportRequest(withF)).toMatchObject({ ok: true, params: { channel: 'ch0', format: 'blf', since: 'seq:10', until: 'seq:42' } });
    expect((exportRequest(withF) as { params: Record<string, unknown> }).params.ids).toBeUndefined();
    expect(exportRequest({ ...withF, useFilter: true })).toEqual({
      ok: true,
      params: { channel: 'ch0', format: 'blf', since: 'seq:10', until: 'seq:42', ids: ['0x300', '0x7E0-0x7EF'], dir: 'rx', origins: ['agent', 'device'] },
      scopeText: '이 패널 보관 범위 seq 10–42 · 현재 필터',
    });
  });
  it('filter limits', () => {
    const many = parseIdFilter(Array.from({ length: EXPORT_IDS_MAX + 1 }, (_, i) => (0x100 + 2 * i).toString(16)).join(','))!;
    expect(exportRequest({ ...base, useFilter: true, ids: many })).toEqual({
      ok: false, error: `id 필터 항목 ${EXPORT_IDS_MAX + 1}개 — 내보내기는 ${EXPORT_IDS_MAX}개까지 (범위로 묶으세요)`,
    });
    expect(exportRequest({ ...base, useFilter: true, origins: new Set() })).toEqual({ ok: false, error: '출처를 하나 이상 고르세요' });
  });
});

describe('exportResultLines', () => {
  const ok = {
    channel: 'ch0', format: 'blf', location: 'daemon', path: '/tmp/a.blf', sidecar: '/tmp/a.blf.loom.jsonl', frames: 3,
    gaps: 0, gaps_dropped: 0, truncated: false, truncated_reason: null, next_cursor: null, complete: true,
    range: { from: '1', to: '3' }, clock_id: 'c', wall_anchor_known: true,
  };
  it('complete file on the daemon machine', () => {
    expect(exportResultLines(ok)).toEqual({
      lines: [
        '데몬 컴퓨터에 저장됨 (브라우저 다운로드 아님): /tmp/a.blf',
        '사이드카: /tmp/a.blf.loom.jsonl',
        '3프레임 · BLF · seq 1–3 · 갭 0',
      ],
      path: '/tmp/a.blf',
      warn: false,
    });
  });
  it('cut + gaps + unknown wall', () => {
    const r = exportResultLines({
      ...ok, format: 'asc', frames: 2, gaps: 1, gaps_dropped: 2, truncated: true, truncated_reason: 'clock_change',
      next_cursor: 'e1:seq:3', complete: false, range: { from: '1', to: '2' }, wall_anchor_known: false,
    });
    expect(r.lines).toEqual([
      '데몬 컴퓨터에 저장됨 (브라우저 다운로드 아님): /tmp/a.blf',
      '사이드카: /tmp/a.blf.loom.jsonl',
      '2프레임 · ASC · seq 1–2 · 갭 1 (+2 미기록) · 벽시계 없음 (상대 시각)',
      '잘림 (데몬 시계 바뀜) — 이어서: since e1:seq:3',
      '⚠ 범위 안에 저널 손실 있음 (파일·사이드카에 갭 기록)',
    ]);
    expect(r.warn).toBe(true);
    expect(exportResultLines({ ...ok, truncated: true, truncated_reason: 'max_frames', next_cursor: 'e1:seq:9', complete: false }).lines[3]).toBe(
      '잘림 (파일당 1,000,000프레임 상한) — 이어서: since e1:seq:9',
    );
    expect(exportResultLines({ ...ok, truncated: true, truncated_reason: 'scan_budget', next_cursor: 'e1:seq:9', complete: false }).lines[3]).toBe(
      '잘림 (스캔 예산) — 이어서: since e1:seq:9',
    );
  });
  it('incomplete scan without gaps or cut', () => {
    const r = exportResultLines({ ...ok, complete: false });
    expect(r.lines[r.lines.length - 1]).toBe('⚠ 저널 스캔이 범위 전체를 덮지 못함');
    expect(r.warn).toBe(true);
  });
  it('no file; unknown shape', () => {
    expect(exportResultLines({ ...ok, path: null, sidecar: null, frames: 0, range: null })).toEqual({
      lines: ['선택 범위에 프레임이 없어 파일을 만들지 않았습니다'], path: null, warn: false,
    });
    expect(exportResultLines(undefined)).toEqual({ lines: ['결과 형식을 알 수 없음'], path: null, warn: true });
    expect(exportResultLines({ path: 3 })).toEqual({ lines: ['결과 형식을 알 수 없음'], path: null, warn: true });
  });
});
