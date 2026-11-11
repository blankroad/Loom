// Activity timeline (plan §5.3): agent/script/user actions incl. reads with the
// seq range they covered. C2 bound: fixed ring of `capacity` entries (default
// 5,000); when full the oldest entry is overwritten and `dropped` counts it.
//
// "⇢ 에이전트에게 전달됨": a user entry is delivered once any agent cursor
// (core.agent_cursor) has advanced to or past its seq.

import type { LoomEvent } from '../proto/provisional';
import { readActivity, readAgentCursor, type ActivityEntry } from './adapter';
import { Emitter } from './emitter';

export class ActivityStore extends Emitter {
  readonly capacity: number;
  private ring: (ActivityEntry | undefined)[];
  private head = 0; // next write index
  private count = 0;
  private _dropped = 0;
  private _agentCursor = -1n;

  constructor(capacity = 5_000) {
    super();
    if (capacity < 1) throw new Error('capacity must be ≥ 1');
    this.capacity = capacity;
    this.ring = new Array<ActivityEntry | undefined>(capacity);
  }

  get size(): number {
    return this.count;
  }
  get dropped(): number {
    return this._dropped;
  }
  /** highest seq any agent has acknowledged via others_since_last */
  get agentCursor(): bigint {
    return this._agentCursor;
  }

  push(entry: ActivityEntry): void {
    if (this.count === this.capacity) this._dropped++;
    else this.count++;
    this.ring[this.head] = entry;
    this.head = (this.head + 1) % this.capacity;
    this.changed();
  }

  applyEvent(ev: LoomEvent): void {
    if (ev.kind === 'core.activity') this.push(readActivity(ev));
    else if (ev.kind === 'core.agent_cursor') {
      const c = readAgentCursor(ev);
      if (c && c.toSeq > this._agentCursor) {
        this._agentCursor = c.toSeq;
        this.changed();
      }
    }
  }

  latest(): ActivityEntry | null {
    if (this.count === 0) return null;
    return this.ring[(this.head - 1 + this.capacity) % this.capacity] ?? null;
  }

  /** newest first, at most n entries */
  recent(n: number): ActivityEntry[] {
    const out: ActivityEntry[] = [];
    const m = Math.min(n, this.count);
    for (let i = 1; i <= m; i++) {
      const e = this.ring[(this.head - i + this.capacity) % this.capacity];
      if (e) out.push(e);
    }
    return out;
  }

  isDelivered(e: ActivityEntry): boolean {
    return e.origin === 'user' && e.seq <= this._agentCursor;
  }
}
