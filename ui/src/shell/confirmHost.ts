// Shell-side confirm UI used by ActionGate ('enter_twice' | 'card') and by the
// STOP release card. One card at a time. C2: at most MAX_PENDING queued; beyond
// that new requests resolve false immediately (the action is cancelled, never
// silently performed).

import type { ConfirmRequest, ConfirmUi } from '../sdk/actionGate';
import { Emitter } from '../store/emitter';

const MAX_PENDING = 8;

interface Pending {
  req: ConfirmRequest;
  resolve: (ok: boolean) => void;
}

export class ConfirmHost extends Emitter implements ConfirmUi {
  private queue: Pending[] = [];

  get current(): ConfirmRequest | null {
    return this.queue[0]?.req ?? null;
  }

  confirm(req: ConfirmRequest): Promise<boolean> {
    if (this.queue.length >= MAX_PENDING) return Promise.resolve(false);
    return new Promise((resolve) => {
      this.queue.push({ req, resolve });
      this.changed();
    });
  }

  answer(ok: boolean): void {
    const p = this.queue.shift();
    if (!p) return;
    p.resolve(ok);
    this.changed();
  }
}
