// @vitest-environment jsdom
import { Terminal } from '@xterm/xterm';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { RowStore } from '../store/rowStore';
import { writeEv } from '../store/testing/fakeNet';
import { createRowTable, mergeGaps } from './rowTable';
import { createRowView } from './rowView';
import type { GapMark } from './storeMirror';
import { INPUT_COALESCE_MS, MAX_PENDING_WRITE_BYTES, TerminalView, type Timers } from './terminalView';

// ---------------- RowTable ----------------

describe('RowTable', () => {
  const setup = () => {
    let t = 0;
    const store = new RowStore({ now: () => t });
    const view = createRowView(store, { domain: 'uds' });
    const host = document.createElement('div');
    document.body.append(host);
    const table = createRowTable(host, {
      columns: [{ key: 'svc', header: '서비스', width: '8em', cell: (r) => String(r.payload.service ?? '') }],
      onNeedRender: () => {},
      now: () => t,
    });
    const draw = (gaps: GapMark[] = []) => {
      table.setItems(mergeGaps(view.list(), gaps));
      table.render();
    };
    const visible = () => [...host.querySelectorAll<HTMLElement>('.vt-row')].filter((e) => !e.hidden);
    return { store, draw, visible, host, advance: (ms: number) => (t += ms) };
  };
  afterEach(() => document.body.replaceChildren());

  it('updates a row in place: one DOM row per request_id, data-state follows the stage', () => {
    const { store, draw, visible } = setup();
    store.apply(writeEv('r1', 'requested', { seq: 1, payload: { service: 'ReadDID' } }));
    draw();
    const el0 = visible()[0]!;
    expect(el0.dataset.requestId).toBe('r1');
    expect(el0.dataset.state).toBe('requested');
    expect(el0.dataset.origin).toBe('agent');
    store.apply(writeEv('r1', 'sent', { seq: 2, payload: { service: 'ReadDID' } }));
    store.apply(writeEv('r1', 'confirmed', { seq: 3, payload: { service: 'ReadDID' } }));
    draw();
    expect(visible()).toHaveLength(1);
    expect(visible()[0]!.dataset.state).toBe('confirmed');
    expect(visible()[0]!.textContent).toContain('●확정');
  });

  it('keeps row positions fixed when an earlier row updates', () => {
    const { store, draw, visible } = setup();
    store.apply(writeEv('a', 'sent', { seq: 1, monoMs: 1 }));
    store.apply(writeEv('b', 'sent', { seq: 2, monoMs: 2 }));
    draw();
    const order = () => visible().map((e) => e.dataset.requestId);
    expect(order()).toEqual(['a', 'b']);
    store.apply(writeEv('a', 'confirmed', { seq: 3, monoMs: 3 }));
    draw();
    expect(order()).toEqual(['a', 'b']);
  });

  it('shows in-flight elapsed time, risk badge and origin letters', () => {
    const { store, draw, visible, advance } = setup();
    store.apply({ ...writeEv('r', 'sent', { seq: 1 }), risk: 'R2' });
    advance(3_100);
    draw();
    const txt = visible()[0]!.textContent ?? '';
    expect(txt).toContain('◑보냄 3.1s');
    expect(txt).toContain('R2');
    expect(txt).toContain('AGT');
  });

  it('places display.gap lines by seq with data-gap', () => {
    const { store, draw, visible } = setup();
    store.apply(writeEv('a', 'confirmed', { seq: 10, monoMs: 1 }));
    store.apply(writeEv('b', 'confirmed', { seq: 50, monoMs: 2 }));
    draw([{ kind: 'display.gap', firstSeq: 20n, lastSeq: 40n, count: 21, cause: 'slow_client' }]);
    const rows = visible();
    expect(rows.map((e) => e.dataset.requestId ?? e.dataset.gap)).toEqual(['a', 'display.gap', 'b']);
    expect(rows[1]!.textContent).toContain('21건 화면 생략');
  });
});

// ---------------- TerminalView ----------------

// jsdom lacks matchMedia (xterm's DPR watcher needs it when opened)
if (!window.matchMedia) {
  window.matchMedia = ((q: string) => ({ matches: false, media: q, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false })) as unknown as typeof window.matchMedia;
}

class ManualTimers implements Timers {
  q: { at: number; cb: () => void; id: number }[] = [];
  t = 0;
  private n = 0;
  setTimeout(cb: () => void, ms: number) {
    const id = ++this.n;
    this.q.push({ at: this.t + ms, cb, id });
    return id;
  }
  clearTimeout(h: unknown) {
    this.q = this.q.filter((x) => x.id !== h);
  }
  advance(ms: number) {
    this.t += ms;
    const due = this.q.filter((x) => x.at <= this.t);
    this.q = this.q.filter((x) => x.at > this.t);
    due.forEach((x) => x.cb());
  }
}

const hex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
const writeAsync = (t: Terminal, s: string) => new Promise<void>((r) => t.write(s, r));

describe('TerminalView', () => {
  const make = (opts: { readonly?: boolean } = {}) => {
    const timers = new ManualTimers();
    const sent: string[] = [];
    const v = new TerminalView({
      onInput: (b) => sent.push(hex(b)),
      readonly: opts.readonly ?? false,
      renderer: 'dom',
      now: () => timers.t,
      timers,
    });
    return { v, timers, sent };
  };

  const QUERIES = [
    '\x1b[c', // DA1
    '\x1b[>c', // DA2
    '\x1b[6n', // DSR / CPR
    '\x1bP$qm\x1b\\', // DECRQSS
    '\x1b]11;?\x07', // OSC 11 colour query
  ];

  it('control: an unhooked xterm DOES answer these queries on its own (fixture is meaningful)', async () => {
    const raw = new Terminal({ allowProposedApi: true });
    const out: string[] = [];
    raw.onData((d) => out.push(d));
    for (const q of QUERIES.slice(0, 4)) await writeAsync(raw, q);
    expect(out.length).toBeGreaterThanOrEqual(4);
    raw.dispose();
  });

  it('parser hooks swallow DA/DSR/DECRQSS/OSC 11 queries: zero bytes sent, counted', async () => {
    const { v, timers, sent } = make();
    for (const q of QUERIES) await writeAsync(v.term, q);
    timers.advance(50);
    expect(sent).toEqual([]);
    expect(v.stats.blockedQueries).toBeGreaterThanOrEqual(QUERIES.length);
    v.dispose();
  });

  it('intent gate: onData without a recent user event is dropped and counted', () => {
    const { v, timers, sent } = make();
    v.term.input('ls\r'); // as if the emulator produced it
    timers.advance(10);
    expect(sent).toEqual([]);
    expect(v.stats.blockedAutoInput).toBe(1);
    v.noteUserIntent();
    v.term.input('ls\r');
    timers.advance(INPUT_COALESCE_MS);
    expect(sent).toEqual(['6c730d']);
    v.dispose();
  });

  it('coalesces keys for 5 ms but sends 0x03 immediately (flushing what was queued)', () => {
    const { v, sent } = make();
    v.noteUserIntent();
    v.term.input('a');
    expect(sent).toEqual([]);
    v.term.input('\x03');
    expect(sent).toEqual(['6103']); // no timer advance needed
    v.dispose();
  });

  it('readonly (watch): neither keys nor literals are sent', () => {
    const { v, timers, sent } = make({ readonly: true });
    v.noteUserIntent();
    v.term.input('x');
    v.sendBytes(new Uint8Array([0x1d]));
    timers.advance(50);
    expect(sent).toEqual([]);
    expect(v.stats.readonlyDrops).toBeGreaterThan(0);
    v.dispose();
  });

  it('loom:literal event from the shell sends the byte immediately', () => {
    const { v, sent } = make();
    const host = document.createElement('div');
    document.body.append(host);
    v.open(host);
    host.dispatchEvent(new CustomEvent('loom:literal', { detail: { byte: 0x1d } }));
    expect(sent).toEqual(['1d']);
    v.dispose();
  });

  it('drops screen output beyond the 2 MiB pending cap, then writes a resync line', async () => {
    const { v } = make();
    const chunk = new Uint8Array(256 * 1024).fill(0x41);
    const n = Math.ceil(MAX_PENDING_WRITE_BYTES / chunk.length) + 3;
    for (let i = 0; i < n; i++) v.write(chunk); // xterm parses async → backlog builds
    expect(v.stats.droppedBytes).toBeGreaterThan(0);
    expect(v.stats.pendingWriteBytes).toBeLessThanOrEqual(MAX_PENDING_WRITE_BYTES);
    await vi.waitFor(() => expect(v.stats.pendingWriteBytes).toBe(0), { timeout: 5_000 });
    v.dispose();
  });

  it('gap/separator marks never fail (decoration or fallback line)', () => {
    const { v } = make();
    v.markGap('⋯ 화면 생략 3건');
    v.markSeparator('── 백로그 / 라이브 ──');
    expect(v.lineMarks).toBe(2);
    v.dispose();
  });
});
