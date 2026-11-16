// U5b batch 6 — highlight rules (FUNCTIONS.md §B6.1). Opus-owned specification; Sonnet must not edit.
import { describe, expect, it } from 'vitest';
import { compileRules, HIGHLIGHT_COLORS, HIGHLIGHT_MAX_PATTERN, HIGHLIGHT_MAX_PER_LINE, HIGHLIGHT_MAX_RULES, matchLine, sanitizeRules, type HighlightRule } from './highlight';

const rule = (pattern: string, o: Partial<HighlightRule> = {}): HighlightRule => ({ pattern, regex: false, caseSensitive: false, color: 'red', ...o });

describe('compileRules', () => {
  it('escapes literals, honours case, reports bad regex without throwing', () => {
    const { compiled, errors } = compileRules([rule('a.b'), rule('ERR', { caseSensitive: true }), rule('(oops', { regex: true }), rule(''), rule('w[0-9]+', { regex: true, color: 'blue' })]);
    expect(compiled).toHaveLength(3);
    expect(errors).toEqual([{ index: 2, message: expect.any(String) as string }]);
    expect(compiled[0]!.re.test('a.b')).toBe(true);
    compiled[0]!.re.lastIndex = 0;
    expect('axb'.match(compiled[0]!.re)).toBeNull();
    expect(compiled[1]!.re.flags).not.toContain('i');
    expect(compiled[0]!.re.flags).toContain('g');
    expect(compiled[0]!.re.flags).toContain('i');
    expect(compiled[2]!.color).toBe('blue');
  });
  it('compiles at most HIGHLIGHT_MAX_RULES rules', () => {
    const many = Array.from({ length: HIGHLIGHT_MAX_RULES + 4 }, (_, i) => rule(`r${i}`));
    expect(compileRules(many).compiled).toHaveLength(HIGHLIGHT_MAX_RULES);
  });
});

describe('matchLine', () => {
  it('returns non-overlapping spans; earliest rule wins at a position', () => {
    const { compiled } = compileRules([rule('error', { color: 'red' }), rule('err', { color: 'yellow' }), rule('ok', { color: 'green' })]);
    expect(matchLine('ERROR: not ok, err again', compiled)).toEqual([
      { start: 0, end: 5, color: 'red' },
      { start: 11, end: 13, color: 'green' },
      { start: 15, end: 18, color: 'yellow' },
    ]);
  });
  it('skips zero-length matches and caps per line', () => {
    const { compiled } = compileRules([rule('x*', { regex: true })]);
    expect(matchLine('abc', compiled)).toEqual([]);
    const { compiled: c2 } = compileRules([rule('a')]);
    expect(matchLine('a'.repeat(500), c2)).toHaveLength(HIGHLIGHT_MAX_PER_LINE);
  });
  it('is stable across calls (no lastIndex leakage)', () => {
    const { compiled } = compileRules([rule('ok')]);
    expect(matchLine('ok ok', compiled)).toHaveLength(2);
    expect(matchLine('ok ok', compiled)).toHaveLength(2);
  });
});

describe('sanitizeRules', () => {
  it('keeps valid rules, drops malformed, clamps', () => {
    const out = sanitizeRules([
      rule('fine', { color: 'purple' }),
      { pattern: 3, regex: false, caseSensitive: false, color: 'red' },
      { pattern: 'x', regex: false, caseSensitive: false, color: 'pink' },
      { pattern: 'y'.repeat(HIGHLIGHT_MAX_PATTERN + 10), regex: 'no', caseSensitive: 1, color: 'green' },
      null,
    ]);
    expect(out).toEqual([rule('fine', { color: 'purple' }), { pattern: 'y'.repeat(HIGHLIGHT_MAX_PATTERN), regex: false, caseSensitive: false, color: 'green' }]);
    expect(sanitizeRules('nope')).toEqual([]);
    expect(sanitizeRules(Array.from({ length: 30 }, (_, i) => rule(`p${i}`)))).toHaveLength(HIGHLIGHT_MAX_RULES);
    expect(HIGHLIGHT_COLORS).toHaveLength(6);
  });
});
