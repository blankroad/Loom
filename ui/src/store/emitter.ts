// Minimal versioned emitter shared by all stores. React reads stores through
// useSyncExternalStore(store.subscribe, () => store.version) — see hooks.ts.

export type Unsubscribe = () => void;

export class Emitter {
  private listeners = new Set<() => void>();
  private _version = 0;

  get version(): number {
    return this._version;
  }

  subscribe = (cb: () => void): Unsubscribe => {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  };

  protected changed(): void {
    this._version++;
    for (const cb of [...this.listeners]) cb();
  }
}
