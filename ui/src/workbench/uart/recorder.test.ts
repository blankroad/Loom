// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { PENDING_LINE_MAX, Recorder } from './recorder';

const enc = new TextEncoder();
const prefs = { format: 'log' as const, rx: true, tx: true, fileTs: false };

describe('REC .log assembly (raw journal bytes in, readable lines out)', () => {
  it('keeps a Korean character split across two UART chunks intact (per-direction streaming decode)', async () => {
    const r = Recorder.buffer('mcu', prefs, () => {});
    const b = enc.encode('상태 정상\n'); // 상 = EC 83 81
    r.feed('rx', b.slice(0, 2), 1);
    r.feed('rx', b.slice(2), 2);
    await r.stop();
    expect(r.bufferText()).toContain('RX  상태 정상\n');
    expect(r.bufferText()).not.toContain('�');
  });

  it('keeps RX and TX decoders and partial lines separate when chunks interleave', async () => {
    const r = Recorder.buffer('mcu', prefs, () => {});
    const rx = enc.encode('응답 OK\n');
    const tx = enc.encode('명령\r\n');
    r.feed('rx', rx.slice(0, 1), 1);
    r.feed('tx', tx.slice(0, 4), 2);
    r.feed('rx', rx.slice(1), 3);
    r.feed('tx', tx.slice(4), 4);
    await r.stop();
    const text = r.bufferText();
    expect(text).toContain('RX  응답 OK\n');
    expect(text).toContain('TX  명령\n');
    expect(text).not.toContain('�');
  });

  it('writes an unterminated stream out in bounded, visibly marked pieces without losing bytes', async () => {
    const r = Recorder.buffer('log', prefs, () => {});
    const chunk = 'x'.repeat(4096);
    const n = Math.ceil((PENDING_LINE_MAX * 2.5) / chunk.length);
    for (let i = 0; i < n; i++) r.feed('rx', enc.encode(chunk), i);
    expect(r.state).toBe('recording');
    await r.stop('test');
    const lines = r.bufferText().split('\n').filter((l) => l.startsWith('RX  '));
    expect(lines.length).toBe(3);
    expect(lines[0]).toContain('⟨줄바꿈 없이 64 KiB — 다음 줄에 이어짐⟩');
    expect(lines.at(-1)).toContain('⟨줄바꿈 없음⟩');
    const total = lines.map((l) => (l.slice(4).match(/x+/)?.[0] ?? '').length).reduce((a, b) => a + b, 0);
    expect(total).toBe(n * chunk.length);
  });

  it('flushes a trailing partial multi-byte sequence on stop as U+FFFD instead of dropping it', async () => {
    const r = Recorder.buffer('mcu', prefs, () => {});
    r.feed('rx', enc.encode('끝').slice(0, 2), 1);
    await r.stop();
    expect(r.bufferText()).toContain('RX  � ⟨줄바꿈 없음⟩');
  });

  it('stops with an explicit error at the buffer limit and keeps what was recorded exportable', () => {
    const r = Recorder.buffer('mcu', prefs, () => {});
    const line = enc.encode(`${'y'.repeat(1023)}\n`);
    for (let i = 0; i < 6 * 1024 && r.state === 'recording'; i++) r.feed('rx', line, i);
    expect(r.state).toBe('error');
    expect(r.error).toContain('버퍼 한도');
    expect(r.bufferText().length).toBeGreaterThan(4 * 1024 * 1024);
  });

  it('.jsonl keeps the exact chunk bytes as hex', async () => {
    const r = Recorder.buffer('mcu', { ...prefs, format: 'jsonl' }, () => {});
    r.feed('rx', new Uint8Array([0x1b, 0x5b, 0xff, 0x41]), 5);
    await r.stop();
    const rec = r.bufferText().split('\n').filter(Boolean).map((l) => JSON.parse(l) as Record<string, unknown>).find((x) => x.dir === 'rx');
    expect(rec).toMatchObject({ dir: 'rx', len: 4, hex: '1b5bff41', ms: 5 });
  });
});
