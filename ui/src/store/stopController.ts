// STOP button state machine (plan §5.1.7, UI-T06).
//
//   click ─► "STOP 요청 중…" (optimistic, same frame)
//        ├─ server core.latch{on, acked} ─► "STOP — 잠김"
//        └─ no ack within 2 s ─────────────► "STOP 확인 안 됨 — 재시도"
//   WS not open → POST /api/v1/stop (NetClient.stopViaHttp); a failing RPC also
//   falls back to HTTP. A click alone never paints success.
//
// STOP is a safety-reducing action: allowed for every right (watch included),
// never confirmed, never blocked by STALLED.

import type { NetClient } from '../net/api';
import type { LatchView } from './adapter';
import { Emitter } from './emitter';
import type { ShellControlStore } from './shellControl';

export const STOP_ACK_TIMEOUT_MS = 2_000;

export type StopPhase = 'idle' | 'requesting' | 'locked' | 'unconfirmed';

/** Pure: what the STOP control shows given the server latch and the local request. */
export function stopView(latch: LatchView, local: 'idle' | 'requesting' | 'unconfirmed'): StopPhase {
  if (latch.on && latch.acked) return 'locked';
  if (local === 'unconfirmed') return 'unconfirmed';
  if (local === 'requesting' || latch.on) return 'requesting';
  return 'idle';
}

export interface Timers {
  setTimeout(cb: () => void, ms: number): unknown;
  clearTimeout(h: unknown): void;
}

const realTimers: Timers = {
  setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
  clearTimeout: (h) => globalThis.clearTimeout(h as ReturnType<typeof setTimeout>),
};

export class StopController extends Emitter {
  private local: 'idle' | 'requesting' | 'unconfirmed' = 'idle';
  private timer: unknown = null;
  lastError: string | null = null;
  /** which transport carried the last request — shown in the tooltip */
  lastVia: 'rpc' | 'http' | null = null;

  constructor(
    private readonly net: Pick<NetClient, 'state' | 'call' | 'stopViaHttp'>,
    private readonly shell: ShellControlStore,
    private readonly timers: Timers = realTimers,
  ) {
    super();
    shell.subscribe(() => {
      // server ack ends the local request
      if (this.shell.latch.on && this.shell.latch.acked && this.local !== 'idle') {
        this.clearTimer();
        this.local = 'idle';
        this.changed();
      }
    });
  }

  get phase(): StopPhase {
    return stopView(this.shell.latch, this.local);
  }

  /** Fire-and-track. Resolves when the request was sent (not when acked). */
  async requestStop(reason = 'user'): Promise<void> {
    this.lastError = null;
    this.local = 'requesting';
    this.clearTimer();
    this.timer = this.timers.setTimeout(() => {
      this.timer = null;
      if (!(this.shell.latch.on && this.shell.latch.acked)) {
        this.local = 'unconfirmed';
        this.changed();
      }
    }, STOP_ACK_TIMEOUT_MS);
    this.changed();

    try {
      if (this.net.state.status === 'open') {
        this.lastVia = 'rpc';
        try {
          await this.net.call('core.stop', { scope: 'all', reason }, { timeoutMs: STOP_ACK_TIMEOUT_MS });
          return;
        } catch {
          // fall through to HTTP — the WS may have died under us
        }
      }
      this.lastVia = 'http';
      await this.net.stopViaHttp();
    } catch (e) {
      this.lastError = e instanceof Error ? e.message : String(e);
      this.changed();
    }
  }

  /** Release needs operate (+ confirm card in M1); caller enforces rights. */
  async release(): Promise<void> {
    await this.net.call('core.stop.release', {});
  }

  dispose(): void {
    this.clearTimer();
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      this.timers.clearTimeout(this.timer);
      this.timer = null;
    }
  }
}
