// U7 batch 1 — contract tests for paneLayout.ts (FUNCTIONS.md §B1.2). Opus-owned specification; Sonnet must not edit.
import { describe, expect, it } from 'vitest';
import {
  connClass,
  HEADER_COMPACT_MIN_PX,
  HEADER_FULL_MIN_PX,
  headerMode,
  identityPlan,
  identityTitle,
  openTagText,
  sessionDot,
  TOOL_ORDER,
  toolPlacement,
  type HeaderMode,
} from './paneLayout';

describe('connClass (moved from terminalPanel, same mapping)', () => {
  it.each([
    ['open', 'connected'],
    ['OPEN', 'connected'],
    ['disconnected', 'lost'],
    ['link lost', 'lost'],
    ['reconnecting', 'waiting'],
    ['wait_port', 'waiting'],
    ['?', 'unknown'],
    ['closed', 'disconnected'],
  ])('%s → %s', (s, c) => expect(connClass(s)).toBe(c));
});

describe('headerMode', () => {
  it('uses the header width thresholds', () => {
    expect(headerMode(1600)).toBe('full');
    expect(headerMode(HEADER_FULL_MIN_PX)).toBe('full');
    expect(headerMode(HEADER_FULL_MIN_PX - 1)).toBe('compact');
    expect(headerMode(HEADER_COMPACT_MIN_PX)).toBe('compact');
    expect(headerMode(HEADER_COMPACT_MIN_PX - 1)).toBe('narrow');
    expect(headerMode(200)).toBe('narrow');
  });
  it('an unknown width (hidden tab, not laid out) keeps the current mode', () => {
    for (const w of [0, -3, Number.NaN, Number.POSITIVE_INFINITY]) expect(headerMode(w)).toBeNull();
  });
});

describe('toolPlacement', () => {
  it('full keeps every tool in the header', () => {
    expect(toolPlacement('full')).toEqual({ header: [...TOOL_ORDER], overflow: [] });
  });
  it('compact and narrow move secondary tools into the ⋯ row', () => {
    expect(toolPlacement('compact')).toEqual({
      header: ['view', 'pause', 'rec', 'search', 'send', 'plug', 'more'],
      overflow: ['clear', 'ts', 'eol', 'macro', 'theme', 'gear'],
    });
    expect(toolPlacement('narrow')).toEqual({
      header: ['rec', 'search', 'plug', 'more'],
      overflow: ['view', 'pause', 'clear', 'ts', 'eol', 'macro', 'send', 'theme', 'gear'],
    });
  });
  it.each<HeaderMode>(['full', 'compact', 'narrow'])('%s partitions TOOL_ORDER and never hides REC, port or ⋯', (m) => {
    const p = toolPlacement(m);
    expect([...p.header, ...p.overflow].sort()).toEqual([...TOOL_ORDER].sort());
    expect(p.header.filter((t) => p.overflow.includes(t))).toEqual([]);
    const order = (xs: string[]) => xs.map((x) => TOOL_ORDER.indexOf(x as never));
    expect(order(p.header)).toEqual([...order(p.header)].sort((a, b) => a - b));
    expect(order(p.overflow)).toEqual([...order(p.overflow)].sort((a, b) => a - b));
    for (const t of ['rec', 'plug', 'more'] as const) expect(p.header).toContain(t);
  });
});

describe('identity', () => {
  it('drops path and counters before baud; baud is never dropped', () => {
    expect(identityPlan('full')).toEqual({ path: true, frame: 'full', counts: true });
    expect(identityPlan('compact')).toEqual({ path: false, frame: 'baud', counts: false });
    expect(identityPlan('narrow')).toEqual({ path: false, frame: 'baud', counts: false });
  });
  it('the target tooltip carries every field, naming what is unknown', () => {
    expect(identityTitle({ channel: 'dut', stateLabel: '연결됨', path: '/dev/ttyUSB0', frame: '115200 8N1 · none · CR', rx: '1.0 kB', tx: '12 B' }))
      .toBe('dut · 연결됨 · /dev/ttyUSB0 · 115200 8N1 · none · CR · RX 1.0 kB · TX 12 B');
    expect(identityTitle({ channel: 'aux', stateLabel: '닫힘', path: null, frame: '', rx: '0 B', tx: '0 B' }))
      .toBe('aux · 닫힘 · 경로 없음 · 프레임 미확인 · RX 0 B · TX 0 B');
  });
  it('session list: open chip and state dot', () => {
    expect(openTagText('open', 2)).toBe('pane 2');
    expect(openTagText('open', null)).toBe('열림');
    for (const s of ['closed', 'unknown', 'none'] as const) expect(openTagText(s, 1)).toBeNull();
    expect(sessionDot('open')).toBe('on');
    expect(sessionDot('closed')).toBe('off');
    expect(sessionDot('none')).toBe('off');
    expect(sessionDot('unknown')).toBe('unknown');
  });
});
