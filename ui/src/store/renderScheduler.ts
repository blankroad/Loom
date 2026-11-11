// RenderScheduler (plan §3.3, UD-13): one rAF loop for all panels.
//
// - Panels call invalidate(); nothing draws until the next animation frame and
//   the loop sleeps when no panel is dirty.
// - Frame data budget: 8 ms shared by all dirty panels this frame. Panels that
//   do not fit are carried to the next frame (still dirty, never dropped).
// - Normal: a dirty panel draws on every rAF (≤ its maxHz, default 60).
// - A panel whose draw exceeds the per-frame budget DEGRADE_AFTER frames in a
//   row is throttled to 20 Hz and flagged degraded (→ LIVE-DEGRADED). It is
//   restored after RECOVER_AFTER consecutive draws under half the budget.
// - Work outside the draw (ingest) can be reported with addWork(); a panel whose
//   draw + reported work exceeds LOAD_DEGRADE_MS_PER_S in a 1 s window is degraded
//   too, and recovers only when that load is back under a quarter of it.
// - Render samples (stage = 'draw') are kept in a fixed ring of SAMPLE_CAP.
//   With a RenderMetrics sink attached, 'draw' and 'next_raf' (draw end → next frame
//   start, paint proxy) also go to the per-panel histograms (plan §3.6).

export const FRAME_BUDGET_MS = 8;
export const DEGRADED_HZ = 20;
export const DEGRADE_AFTER = 3;
export const RECOVER_AFTER = 30;
export const SAMPLE_CAP = 1_024;
export const LOAD_DEGRADE_MS_PER_S = 300;
const LOAD_WINDOW_MS = 1_000;

/** where draw / next_raf durations go (RenderMetrics implements it) */
export interface StageSink {
  add(panelId: string, stage: 'draw' | 'next_raf', ms: number): void;
}

export interface RenderSample {
  panelId: string;
  stage: 'receive' | 'decode' | 'apply' | 'draw' | 'next_raf';
  atMs: number;
  durMs: number;
  /** highest stream seq this draw covered, when the panel reports it */
  seqThrough: bigint | null;
}

export interface FrameDeps {
  raf(cb: (t: number) => void): number;
  cancelRaf(id: number): void;
  now(): number;
}

export type DrawFn = (budgetMs: number) => bigint | void;

export interface PanelHandle {
  invalidate(): void;
  dispose(): void;
  readonly degraded: boolean;
  /** ms of panel work outside the draw callback (e.g. ingest) — counts toward the load rule */
  addWork(ms: number): void;
  /** this panel's degraded flag changed */
  onDegradedChange(cb: (degraded: boolean) => void): () => void;
  /**
   * Hidden (background dock tab, collapsed): never drawn — invalidations are remembered and
   * the panel draws once with the latest data when it becomes visible again.
   */
  setVisible(visible: boolean): void;
  readonly visible: boolean;
}

interface Entry {
  id: string;
  draw: DrawFn;
  maxHz: number;
  dirty: boolean;
  lastDrawMs: number;
  degraded: boolean;
  overStreak: number;
  underStreak: number;
  winStartMs: number;
  winWorkMs: number;
  lastLoadMsPerS: number;
  visible: boolean;
}

const defaultDeps = (): FrameDeps => ({
  raf: (cb) => requestAnimationFrame(cb),
  cancelRaf: (id) => cancelAnimationFrame(id),
  now: () => performance.now(),
});

export class RenderScheduler {
  private entries = new Map<string, Entry>();
  private rafId: number | null = null;
  private samples: (RenderSample | undefined)[] = new Array(SAMPLE_CAP);
  private sampleHead = 0;
  private sampleCount = 0;
  private degradedListeners = new Set<(panelId: string, degraded: boolean) => void>();
  /** draw end time of panels drawn in the previous frame (next_raf stage) */
  private drawnLastFrame = new Map<string, number>();
  sink: StageSink | null = null;

  constructor(private readonly deps: FrameDeps = defaultDeps()) {}

  register(id: string, draw: DrawFn, opts: { maxHz?: number } = {}): PanelHandle {
    if (this.entries.has(id)) throw new Error(`panel ${id} already registered with scheduler`);
    const e: Entry = {
      id,
      draw,
      maxHz: Math.min(60, Math.max(1, opts.maxHz ?? 60)),
      dirty: false,
      lastDrawMs: -Infinity,
      degraded: false,
      overStreak: 0,
      underStreak: 0,
      winStartMs: this.deps.now(),
      winWorkMs: 0,
      lastLoadMsPerS: 0,
      visible: true,
    };
    this.entries.set(id, e);
    return {
      invalidate: () => {
        e.dirty = true;
        this.ensureLoop();
      },
      dispose: () => {
        this.entries.delete(id);
        this.drawnLastFrame.delete(id);
      },
      get degraded() {
        return e.degraded;
      },
      addWork: (ms: number) => {
        if (ms > 0) e.winWorkMs += ms;
      },
      setVisible: (v: boolean) => {
        if (e.visible === v) return;
        e.visible = v;
        if (v) {
          e.dirty = true; // catch up with what arrived while hidden
          this.ensureLoop();
        }
      },
      get visible() {
        return e.visible;
      },
      onDegradedChange: (cb: (d: boolean) => void) =>
        this.onDegradedChange((pid, d) => {
          if (pid === id && this.entries.get(id) === e) cb(d);
        }),
    };
  }

  onDegradedChange(cb: (panelId: string, degraded: boolean) => void): () => void {
    this.degradedListeners.add(cb);
    return () => this.degradedListeners.delete(cb);
  }

  /** newest first */
  recentSamples(n = SAMPLE_CAP): RenderSample[] {
    const out: RenderSample[] = [];
    const m = Math.min(n, this.sampleCount);
    for (let i = 1; i <= m; i++) {
      const s = this.samples[(this.sampleHead - i + SAMPLE_CAP) % SAMPLE_CAP];
      if (s) out.push(s);
    }
    return out;
  }

  recordSample(s: RenderSample): void {
    this.samples[this.sampleHead] = s;
    this.sampleHead = (this.sampleHead + 1) % SAMPLE_CAP;
    if (this.sampleCount < SAMPLE_CAP) this.sampleCount++;
  }

  /** every panel draws on the next frame (e.g. "라이브로 이동" after a hidden period) */
  invalidateAll(): void {
    for (const e of this.entries.values()) e.dirty = true;
    if (this.entries.size > 0) this.ensureLoop();
  }

  dispose(): void {
    if (this.rafId !== null) this.deps.cancelRaf(this.rafId);
    this.rafId = null;
    this.entries.clear();
  }

  private ensureLoop(): void {
    if (this.rafId === null) this.rafId = this.deps.raf(() => this.frame());
  }

  /** exposed for tests; normally driven by rAF */
  frame(): void {
    this.rafId = null;
    const frameStart = this.deps.now();
    if (this.sink) for (const [id, end] of this.drawnLastFrame) this.sink.add(id, 'next_raf', Math.max(0, frameStart - end));
    this.drawnLastFrame.clear();
    let spent = 0;
    let anyDirtyLeft = false;
    for (const e of this.entries.values()) {
      if (!e.dirty || !e.visible) continue; // hidden: stays dirty, drawn when shown
      const minInterval = 1000 / (e.degraded ? Math.min(DEGRADED_HZ, e.maxHz) : e.maxHz);
      // small tolerance: rAF intervals jitter around 16.67 ms
      if (frameStart - e.lastDrawMs < minInterval - 1 || spent >= FRAME_BUDGET_MS) {
        anyDirtyLeft = true;
        continue;
      }
      const remaining = FRAME_BUDGET_MS - spent;
      e.dirty = false;
      const t0 = this.deps.now();
      const seq = e.draw(remaining);
      const t1 = this.deps.now();
      const dur = t1 - t0;
      spent += dur;
      if (this.sink) {
        this.sink.add(e.id, 'draw', dur);
        this.drawnLastFrame.set(e.id, t1);
      }
      e.lastDrawMs = frameStart;
      this.recordSample({
        panelId: e.id,
        stage: 'draw',
        atMs: t0,
        durMs: dur,
        seqThrough: typeof seq === 'bigint' ? seq : null,
      });
      this.updateDegrade(e, dur, frameStart);
      if (e.dirty) anyDirtyLeft = true; // panel re-invalidated during draw
    }
    // one more frame after a draw so next_raf can be measured (the loop then sleeps)
    if (anyDirtyLeft || this.drawnLastFrame.size > 0) this.ensureLoop();
  }

  private updateDegrade(e: Entry, durMs: number, nowMs: number): void {
    e.winWorkMs += durMs;
    const winMs = nowMs - e.winStartMs;
    if (winMs >= LOAD_WINDOW_MS) {
      e.lastLoadMsPerS = (e.winWorkMs * 1000) / winMs;
      e.winStartMs = nowMs;
      e.winWorkMs = 0;
      if (!e.degraded && e.lastLoadMsPerS > LOAD_DEGRADE_MS_PER_S) {
        this.setDegraded(e, true);
        return;
      }
    }
    if (durMs > FRAME_BUDGET_MS) {
      e.overStreak++;
      e.underStreak = 0;
      if (!e.degraded && e.overStreak >= DEGRADE_AFTER) this.setDegraded(e, true);
    } else {
      e.overStreak = 0;
      if (durMs <= FRAME_BUDGET_MS / 2) e.underStreak++;
      if (e.degraded && e.underStreak >= RECOVER_AFTER && e.lastLoadMsPerS < LOAD_DEGRADE_MS_PER_S / 4) this.setDegraded(e, false);
    }
  }

  private setDegraded(e: Entry, d: boolean): void {
    e.degraded = d;
    e.underStreak = 0;
    for (const cb of this.degradedListeners) cb(e.id, d);
  }
}
