// B5 spec (Opus): CAN-5 DB chip / names / decoded text. FUNCTIONS.md §B5-DB.
import { describe, expect, it } from 'vitest';
import { DB_NAMES_MAX, DB_PAGES_MAX, DbNames, dbChip, dbLoadParams, decodedLines, parseDbInfo } from './dbView';

const SHA = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90';
const info = (o: Record<string, unknown> = {}) => ({
  channels: [
    { channel: 'other', loaded: false },
    {
      channel: 'ch0', loaded: true, path: '/data/dbc/body.dbc', db_sha256: SHA, messages: 12, signals: 80, nodes: 3,
      encoding: 'utf8', encoding_guessed: false, warnings: [], warnings_total: 0, blocked: [], blocked_total: 0, ...o,
    },
  ],
});

describe('parseDbInfo', () => {
  it('loaded / not loaded / unknown', () => {
    expect(parseDbInfo(info(), 'ch0')).toEqual({
      loaded: true, path: '/data/dbc/body.dbc', sha: SHA, messages: 12, encoding: 'utf8', encodingGuessed: false, warningsTotal: 0, blockedTotal: 0,
    });
    expect(parseDbInfo({ channels: [{ channel: 'ch0', loaded: false }] }, 'ch0')).toEqual({
      loaded: false, path: null, sha: null, messages: null, encoding: null, encodingGuessed: false, warningsTotal: 0, blockedTotal: 0,
    });
    expect(parseDbInfo(info(), 'ch9')).toBeNull();
    expect(parseDbInfo({ channels: [{ channel: 'ch0' }] }, 'ch0')).toBeNull();
    expect(parseDbInfo(null, 'ch0')).toBeNull();
  });
});

describe('dbChip', () => {
  it('unknown is not "none"', () => {
    expect(dbChip(null)).toEqual({ text: 'DB: ?', title: 'DB 상태를 읽지 못함 — 눌러서 다시 읽기·불러오기', cls: 'ct-db-unknown' });
    expect(dbChip(parseDbInfo({ channels: [{ channel: 'ch0', loaded: false }] }, 'ch0'))).toEqual({
      text: 'DB: 없음', title: '이 채널에 DBC 없음 — 눌러서 데몬 컴퓨터 경로의 DBC 불러오기', cls: 'ct-db-none',
    });
  });
  it('loaded: file name + sha8; warnings/guessed encoding mark it', () => {
    const c = dbChip(parseDbInfo(info(), 'ch0'));
    expect(c.text).toBe('DB: body.dbc · a1b2c3d4');
    expect(c.cls).toBe('ct-db-ok');
    expect(c.title).toBe(`/data/dbc/body.dbc\nsha256 ${SHA} · 메시지 12개 · 인코딩 utf8`);
    const w = dbChip(parseDbInfo(info({ path: 'C:\\dbc\\x.dbc', encoding: 'windows1252', encoding_guessed: true, warnings_total: 2, blocked_total: 1 }), 'ch0'));
    expect(w.text).toBe('DB: x.dbc · a1b2c3d4');
    expect(w.cls).toBe('ct-db-warn');
    expect(w.title).toBe(`C:\\dbc\\x.dbc\nsha256 ${SHA} · 메시지 12개 · 인코딩 windows1252 (추정)\n경고 2\n디코드/인코드 막힘 1개`);
  });
});

describe('dbLoadParams', () => {
  it('absolute daemon path; encoding only when not auto', () => {
    expect(dbLoadParams(' /data/body.dbc ', 'auto', 'ch0')).toEqual({ ok: true, params: { channel: 'ch0', path: '/data/body.dbc' } });
    expect(dbLoadParams('/d/b.dbc', 'windows1252', 'ch0')).toEqual({ ok: true, params: { channel: 'ch0', path: '/d/b.dbc', encoding: 'windows1252' } });
    expect(dbLoadParams('D:/x.dbc', 'utf8', 'ch0').ok).toBe(true);
    expect(dbLoadParams('', 'auto', 'ch0')).toEqual({ ok: false, error: '데몬 컴퓨터의 DBC 절대 경로를 입력하세요' });
    expect(dbLoadParams('body.dbc', 'auto', 'ch0')).toEqual({ ok: false, error: '경로는 데몬 컴퓨터의 절대 경로여야 합니다' });
    expect(dbLoadParams(`/${'a'.repeat(4096)}`, 'auto', 'ch0')).toEqual({ ok: false, error: '경로가 너무 깁니다 (≤ 4096자)' });
  });
});

describe('DbNames', () => {
  const page = (msgs: { name: string; id_raw: number; ext: boolean }[], next: number | null, sha = SHA) => ({
    channel: 'ch0', db_sha256: sha, messages: msgs.map((m, i) => ({ index: i, id: '0x', size: 8, ...m })), total_messages: 99, next_cursor: next, truncated: next !== null,
  });
  it('pages until done; std/ext ids are distinct keys', () => {
    const n = new DbNames();
    n.reset(SHA);
    expect(n.addPage(page([{ name: 'EngineData', id_raw: 0x100, ext: false }], 1))).toBe(1);
    expect(n.complete).toBe(false);
    expect(n.addPage(page([{ name: 'Diag', id_raw: 0x100, ext: true }], null))).toBeNull();
    expect(n.complete).toBe(true);
    expect(n.cut).toBe(false);
    expect(n.pages).toBe(2);
    expect(n.nameOf(0x100, false)).toBe('EngineData');
    expect(n.nameOf(0x100, true)).toBe('Diag');
    expect(n.nameOf(0x101, false)).toBeNull();
  });
  it('a page of another DB is ignored; reset forgets', () => {
    const n = new DbNames();
    n.reset(SHA);
    const v = n.version;
    expect(n.addPage(page([{ name: 'X', id_raw: 1, ext: false }], 5, 'ffff'))).toBeNull();
    expect(n.nameOf(1, false)).toBeNull();
    expect(n.version).toBe(v);
    n.addPage(page([{ name: 'X', id_raw: 1, ext: false }], null));
    n.reset(null);
    expect(n.nameOf(1, false)).toBeNull();
    expect(n.sha).toBeNull();
    expect(n.complete).toBe(false);
  });
  it(`bounds: ≤ ${DB_PAGES_MAX} pages, ≤ ${DB_NAMES_MAX} names → cut, never "complete"`, () => {
    const n = new DbNames();
    n.reset(SHA);
    let next: number | null = 0;
    for (let i = 0; i < DB_PAGES_MAX && next !== null; i++) next = n.addPage(page([{ name: `M${i}`, id_raw: i, ext: false }], i + 1));
    expect(next).toBeNull();
    expect(n.pages).toBe(DB_PAGES_MAX);
    expect(n.cut).toBe(true);
    expect(n.complete).toBe(false);
    const m = new DbNames();
    m.reset(SHA);
    const big = Array.from({ length: DB_NAMES_MAX + 5 }, (_, i) => ({ name: `N${i}`, id_raw: i, ext: false }));
    expect(m.addPage(page(big, 77))).toBeNull();
    expect(m.cut).toBe(true);
    expect(m.nameOf(DB_NAMES_MAX - 1, false)).toBe(`N${DB_NAMES_MAX - 1}`);
    expect(m.nameOf(DB_NAMES_MAX, false)).toBeNull();
  });
});

describe('decodedLines', () => {
  it('signals with unit/label/raw; mismatches; short frame', () => {
    expect(
      decodedLines({
        message: 'EngineData',
        signals: [
          { name: 'Rpm', value: 1500.5, unit: 'rpm', raw: 6002 },
          { name: 'Gear', value: 3, unit: '', raw: 3, label: 'Third' },
          { name: 'Temp', unavailable: 'short_frame', needed: 8, actual: 4 },
          'junk',
        ],
        length_mismatch: { expected: 8, actual: 4 },
        fd_mismatch: true,
      }),
    ).toEqual([
      'EngineData · 길이 불일치 (DBC 8 B, 프레임 4 B) · FD 불일치',
      '  Rpm = 1500.5 rpm · raw 6002',
      '  Gear = 3 (Third) · raw 3',
      '  Temp: 값 없음 (짧은 프레임 — 필요 8 B, 실제 4 B)',
    ]);
  });
  it('not in the DB / errors / unknown shape', () => {
    expect(decodedLines(undefined)).toEqual(['DB에 없는 id — 디코드할 메시지 없음']);
    expect(decodedLines(null)).toEqual(['DB에 없는 id — 디코드할 메시지 없음']);
    expect(decodedLines({ message: 'Broken', error: 'overlapping signals' })).toEqual(['디코드 실패 (Broken): overlapping signals']);
    expect(decodedLines({ message: null, error: 'several DBC messages share this id' })).toEqual(['디코드 실패: several DBC messages share this id']);
    expect(decodedLines({ message: 'X', signals: [{ name: 'S', unavailable: 'mux' }] })).toEqual(['X', '  S: 값 없음 (mux)']);
    expect(decodedLines(42)).toEqual(['디코드 결과 형식을 알 수 없음']);
    expect(decodedLines({ foo: 1 })).toEqual(['디코드 결과 형식을 알 수 없음']);
  });
});
