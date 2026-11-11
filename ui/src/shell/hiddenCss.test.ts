// U-1 regression: an author rule such as `.vt-row{display:grid}` beats the browser's
// `[hidden]{display:none}`, so pooled rows marked hidden stayed on screen. jsdom always
// hides [hidden] whatever the author CSS says, so it cannot reproduce the bug — this test
// pins the global rule in the stylesheet instead; the real-browser check is the e2e
// acceptance suite (`e2e/m1/eth.spec.ts` acceptance 2: row count = badge).
import { beforeAll, describe, expect, it } from 'vitest';

// vitest blanks CSS modules (even `?raw`) — read the file itself
let shellCss = '';
beforeAll(async () => {
  const fs = (await import(/* @vite-ignore */ 'node:fs' as string)) as { readFileSync(p: URL, enc: 'utf8'): string };
  shellCss = fs.readFileSync(new URL('./shell.css', import.meta.url), 'utf8');
});

const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');

describe('[hidden] always hides', () => {
  it('the stylesheet was read', () => expect(shellCss.length).toBeGreaterThan(1_000));

  it('shell.css has a global `[hidden] { display: none !important }` rule', () => {
    expect(strip(shellCss)).toMatch(/(^|\n)\s*\[hidden\]\s*\{\s*display:\s*none\s*!important;?\s*\}/);
  });

  it('no component rule re-shows hidden elements with its own !important display', () => {
    // an `!important` display on a class would beat the global rule again
    const bad = strip(shellCss)
      .split('}')
      .filter((r) => /display:\s*(grid|flex|block|inline[\w-]*)\s*!important/.test(r));
    expect(bad).toEqual([]);
  });
});
