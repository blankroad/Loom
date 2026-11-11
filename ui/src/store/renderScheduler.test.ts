import { describe, expect, it } from 'vitest';
import { DEGRADE_AFTER, FRAME_BUDGET_MS, RECOVER_AFTER, RenderScheduler, type FrameDeps } from './renderScheduler';

function fakeFrames() {
  let t = 0;
  let pending: (() => void) | null = null;
  const deps: FrameDeps = {
    raf: (cb) => {
      pending = () => cb(t);
      return 1;
    },
    cancelRaf: () => {
      pending = null;
    },
    now: () => t,
  };
  return {
    deps,
    advance: (ms: number) => {
      t += ms;
    },
    /** run one rAF frame 16.7 ms later; returns whether a frame was pending */
    step(): boolean {
      t += 16.7;
      const p = pending;
      pending = null;
      p?.();
      return p !== null;
    },
  };
}

describe('RenderScheduler', () => {
  it('draws invalidated panels once per frame and sleeps when idle', () => {
    const f = fakeFrames();
    const s = new RenderScheduler(f.deps);
    let draws = 0;
    const h = s.register('p', () => {
      draws++;
    });
    expect(f.step()).toBe(false); // nothing scheduled
    h.invalidate();
    h.invalidate();
    expect(f.step()).toBe(true);
    expect(draws).toBe(1);
    expect(f.step()).toBe(false);
  });

  it('degrades a panel to 20 Hz after repeated over-budget draws, then recovers', () => {
    const f = fakeFrames();
    const s = new RenderScheduler(f.deps);
    const changes: boolean[] = [];
    s.onDegradedChange((_, d) => changes.push(d));
    let cost = FRAME_BUDGET_MS + 2;
    let draws = 0;
    const h = s.register('slow', () => {
      draws++;
      f.advance(cost);
    });
    for (let i = 0; i < DEGRADE_AFTER; i++) {
      h.invalidate();
      f.step();
    }
    expect(h.degraded).toBe(true);
    expect(changes).toEqual([true]);

    // at 20 Hz, a frame 16.7 ms after the last draw is skipped (carried over)
    cost = 1;
    const before = draws;
    h.invalidate();
    f.step();
    expect(draws).toBe(before);
    // eventually it draws again, and recovers after enough cheap draws
    for (let i = 0; i < RECOVER_AFTER * 4 && h.degraded; i++) {
      h.invalidate();
      f.step();
    }
    expect(h.degraded).toBe(false);
    expect(changes).toEqual([true, false]);
  });

  it('records draw samples in a bounded ring', () => {
    const f = fakeFrames();
    const s = new RenderScheduler(f.deps);
    const h = s.register('p', () => 42n);
    h.invalidate();
    f.step();
    const [smp] = s.recentSamples(1);
    expect(smp).toMatchObject({ panelId: 'p', stage: 'draw', seqThrough: 42n });
  });
});
