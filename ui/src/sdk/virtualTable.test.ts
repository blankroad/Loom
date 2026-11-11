// @vitest-environment jsdom
// U-4: only the user's own input (wheel / key / pointer drag) turns a scroll into scroll-back.
import { afterEach, describe, expect, it } from 'vitest';
import { USER_SCROLL_WINDOW_MS, VirtualTable } from './virtualTable';

let host: HTMLDivElement;
afterEach(() => host?.remove());

function make() {
  host = document.createElement('div');
  document.body.append(host);
  let t = 1_000;
  let away = 0;
  const vt = new VirtualTable<number>(host, {
    columns: [{ key: 'n', header: 'n', width: '1fr', cell: (n) => String(n) }],
    keyOf: (n) => String(n),
    rowHeight: 20,
    onNeedRender: () => {},
    onScrollAway: () => away++,
    now: () => t,
  });
  vt.setItems(Array.from({ length: 500 }, (_, i) => i));
  vt.render();
  const scroll = host.querySelector<HTMLElement>('.vt-scroll')!;
  // jsdom has no layout: give the scroller a size (10 000 px content, 200 px view)
  Object.defineProperty(scroll, 'scrollHeight', { configurable: true, get: () => 10_000 });
  let clientH = 200;
  Object.defineProperty(scroll, 'clientHeight', { configurable: true, get: () => clientH });
  const scrollTo = (top: number) => {
    scroll.scrollTop = top;
    scroll.dispatchEvent(new Event('scroll'));
  };
  return {
    vt,
    scroll,
    scrollTo,
    away: () => away,
    advance: (ms: number) => (t += ms),
    setClientH: (h: number) => (clientH = h),
  };
}

describe('VirtualTable follow / scroll-back', () => {
  it('a layout change (viewport shrinks, nothing typed) does not leave the tail', () => {
    const m = make();
    m.scrollTo(9_800); // at the bottom
    expect(m.vt.following).toBe(true);
    m.setClientH(120); // STOP panel opened above: same scrollTop, smaller view
    m.scrollTo(9_700);
    expect(m.vt.following).toBe(true);
    expect(m.away()).toBe(0);
  });

  it('wheel → scroll up = scroll-back (onScrollAway once)', () => {
    const m = make();
    m.scroll.dispatchEvent(new Event('wheel'));
    m.scrollTo(4_000);
    expect(m.vt.following).toBe(false);
    expect(m.away()).toBe(1);
    m.scrollTo(9_800); // back at the bottom → following again
    expect(m.vt.following).toBe(true);
  });

  it('keyboard and scrollbar drag count; an input long ago does not', () => {
    const m = make();
    m.scroll.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageUp' }));
    m.scrollTo(5_000);
    expect(m.away()).toBe(1);
    m.scrollTo(9_800);
    m.scroll.dispatchEvent(new Event('pointerdown'));
    m.advance(USER_SCROLL_WINDOW_MS * 3); // a long drag: still the user while the pointer is held
    m.scrollTo(3_000);
    expect(m.away()).toBe(2);
    window.dispatchEvent(new Event('pointerup'));
    m.scrollTo(9_800);
    m.advance(USER_SCROLL_WINDOW_MS + 1); // input window over
    m.scrollTo(2_000);
    expect(m.vt.following).toBe(true);
    expect(m.away()).toBe(2);
  });
});
