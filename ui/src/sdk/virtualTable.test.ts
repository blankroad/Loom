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

// ROOT follow-up (logic live marker run): ArrowUp/ArrowDown selected rows that stayed off-screen — onKey only
// called select(), unlike reveal(). Contract: work/reports/operator-upgrade/logic/FUNCTIONS.md §VT-K1.
describe('VirtualTable keyboard selection stays visible', () => {
  const key = (m: ReturnType<typeof make>, k: string, times = 1) => {
    for (let i = 0; i < times; i++) m.scroll.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));
  };
  const inView = (m: ReturnType<typeof make>, idx: number) => idx * 20 >= m.scroll.scrollTop && idx * 20 + 20 <= m.scroll.scrollTop + m.scroll.clientHeight;
  const drawnRow = (m: ReturnType<typeof make>, k: string) => {
    m.vt.render();
    const r = [...m.scroll.querySelectorAll<HTMLElement>('.vt-row')].find((e) => e.dataset.key === k && !e.hidden);
    return r ?? null;
  };

  it("ROOT's case: revealed hit + 19 × ArrowDown keeps every selected row inside the view", () => {
    const m = make();
    m.vt.reveal('100'); // search jump: 100 centred, scrollTop 1 900
    expect(m.scroll.scrollTop).toBe(1_900);
    for (let n = 101; n <= 119; n++) {
      key(m, 'ArrowDown');
      expect(m.vt.selected).toBe(String(n));
      expect(inView(m, n)).toBe(true);
    }
    expect(m.scroll.scrollTop).toBe(120 * 20 - 200); // minimal scroll: the row sits on the bottom edge
    const r = drawnRow(m, '119');
    expect(r).not.toBeNull();
    expect(r!.getAttribute('aria-selected')).toBe('true');
  });

  it('ArrowUp above the view scrolls up just enough; moves inside the view do not scroll', () => {
    const m = make();
    m.vt.reveal('300'); // scrollTop 5 900 → rows 295…304 in view
    key(m, 'ArrowDown');
    expect(m.scroll.scrollTop).toBe(5_900);
    key(m, 'ArrowUp', 11); // → 290, above the view
    expect(m.vt.selected).toBe('290');
    expect(m.scroll.scrollTop).toBe(290 * 20);
    expect(drawnRow(m, '290')).not.toBeNull();
  });

  it('navigating with the keys leaves the live tail once, and appended rows no longer move the selection away', () => {
    const m = make();
    m.vt.render(); // following: pinned to the tail
    expect(m.vt.following).toBe(true);
    m.vt.select('495');
    key(m, 'ArrowUp', 10);
    expect(m.vt.selected).toBe('485');
    expect(m.vt.following).toBe(false);
    expect(m.away()).toBe(1);
    expect(inView(m, 485)).toBe(true);
    m.vt.setItems(Array.from({ length: 520 }, (_, i) => i)); // live rows keep arriving
    expect(drawnRow(m, '485')).not.toBeNull();
    key(m, 'ArrowUp');
    expect(m.away()).toBe(1); // not repeated while already off the tail
  });

  it('without layout height the selected row becomes the first drawn row', () => {
    const m = make();
    m.setClientH(0);
    m.vt.reveal('100');
    key(m, 'ArrowDown', 60);
    expect(m.vt.selected).toBe('160');
    expect(m.scroll.scrollTop).toBe(160 * 20);
    expect(drawnRow(m, '160')).not.toBeNull();
  });

  it('Escape is unchanged: clears the selection, no scroll, no follow change', () => {
    const m = make();
    m.vt.reveal('100');
    key(m, 'Escape');
    expect(m.vt.selected).toBeNull();
    expect(m.scroll.scrollTop).toBe(1_900);
    expect(m.vt.following).toBe(false);
    const t = make();
    t.vt.render();
    key(t, 'Escape');
    expect(t.vt.following).toBe(true);
    expect(t.away()).toBe(0);
  });

  it('first ArrowDown with nothing selected keeps the old target (last row); an empty table selects nothing', () => {
    const m = make();
    m.vt.render();
    key(m, 'ArrowDown');
    expect(m.vt.selected).toBe('499');
    expect(inView(m, 499)).toBe(true);
    m.vt.setItems([]);
    key(m, 'ArrowUp');
    expect(m.vt.selected).toBeNull();
  });
});
