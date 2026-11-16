// U5b (operator-upgrade/uart B6) — per-session highlight rules: text/regex → one of six palette colours, drawn as xterm
// decorations only (device bytes and the journal are never changed). Contract: FUNCTIONS.md §B6.1 (Opus owns
// types/constants/signatures; bodies are Sonnet B6). Pure: no DOM, no xterm.

export const HIGHLIGHT_MAX_RULES = 16;
export const HIGHLIGHT_MAX_PATTERN = 256;
/** live decorations per terminal at most (oldest disposed first) */
export const HIGHLIGHT_MAX_DECORATIONS = 2_000;
/** matches per logical line at most */
export const HIGHLIGHT_MAX_PER_LINE = 64;
export const HIGHLIGHT_COLORS = ['red', 'orange', 'yellow', 'green', 'blue', 'purple'] as const;
export type HighlightColor = (typeof HIGHLIGHT_COLORS)[number];
/** CSS colour of each palette entry (background of the decoration; text stays the terminal's) */
export const HIGHLIGHT_CSS: Record<HighlightColor, string> = {
  red: 'rgba(207, 34, 46, 0.45)',
  orange: 'rgba(188, 76, 0, 0.45)',
  yellow: 'rgba(191, 135, 0, 0.45)',
  green: 'rgba(26, 127, 55, 0.45)',
  blue: 'rgba(9, 105, 218, 0.45)',
  purple: 'rgba(130, 80, 223, 0.45)',
};

export interface HighlightRule {
  pattern: string;
  regex: boolean;
  caseSensitive: boolean;
  color: HighlightColor;
}
export interface CompiledRule {
  re: RegExp;
  color: HighlightColor;
}
export interface HighlightSpan {
  start: number;
  end: number;
  color: HighlightColor;
}

/** Rules → global RegExps (literal patterns escaped; `i` unless caseSensitive). Empty patterns skipped; invalid regex
 * reported per index, never thrown; at most HIGHLIGHT_MAX_RULES compiled. §B6.1 */
export function compileRules(rules: readonly HighlightRule[]): { compiled: CompiledRule[]; errors: { index: number; message: string }[] } {
  const compiled: CompiledRule[] = [];
  const errors: { index: number; message: string }[] = [];
  for (let index = 0; index < rules.length; index++) {
    if (compiled.length >= HIGHLIGHT_MAX_RULES) break;
    const rule = rules[index]!;
    if (!rule.pattern) continue;
    const source = rule.regex ? rule.pattern : rule.pattern.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    try {
      compiled.push({ re: new RegExp(source, `g${rule.caseSensitive ? '' : 'i'}`), color: rule.color });
    } catch (e) {
      errors.push({ index, message: e instanceof Error ? e.message : String(e) });
    }
  }
  return { compiled, errors };
}

/** Non-overlapping spans in `text`, left to right; at one position the earliest rule wins; zero-length matches skipped;
 * at most HIGHLIGHT_MAX_PER_LINE. §B6.1 */
export function matchLine(text: string, compiled: readonly CompiledRule[]): HighlightSpan[] {
  const spans: HighlightSpan[] = [];
  let pos = 0;
  while (pos <= text.length && spans.length < HIGHLIGHT_MAX_PER_LINE) {
    let best: { start: number; end: number; color: HighlightColor } | null = null;
    for (const rule of compiled) {
      rule.re.lastIndex = pos;
      let m = rule.re.exec(text);
      // zero-length matches are never spans: look further for a real one
      while (m !== null && m[0].length === 0) {
        rule.re.lastIndex = m.index + 1;
        m = rule.re.exec(text);
      }
      if (m !== null && (best === null || m.index < best.start)) best = { start: m.index, end: m.index + m[0].length, color: rule.color };
    }
    if (best === null) break;
    spans.push(best);
    pos = best.end;
  }
  for (const rule of compiled) rule.re.lastIndex = 0;
  return spans;
}

/** Persisted value → valid rules (drops malformed entries, clamps pattern length, ≤ HIGHLIGHT_MAX_RULES). §B6.1 */
export function sanitizeRules(v: unknown): HighlightRule[] {
  if (!Array.isArray(v)) return [];
  const out: HighlightRule[] = [];
  for (const x of v) {
    if (out.length >= HIGHLIGHT_MAX_RULES) break;
    if (typeof x !== 'object' || x === null) continue;
    const r = x as Record<string, unknown>;
    const color = r.color;
    if (typeof r.pattern !== 'string' || typeof color !== 'string' || !(HIGHLIGHT_COLORS as readonly string[]).includes(color)) continue;
    out.push({
      pattern: r.pattern.slice(0, HIGHLIGHT_MAX_PATTERN),
      regex: r.regex === true,
      caseSensitive: r.caseSensitive === true,
      color: color as HighlightColor,
    });
  }
  return out;
}
