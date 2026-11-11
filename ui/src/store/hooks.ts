import { useSyncExternalStore } from 'react';
import type { Emitter } from './emitter';

/** Re-render when the store version changes; read store fields directly. */
export function useStoreVersion(store: Pick<Emitter, 'subscribe' | 'version'>): number {
  return useSyncExternalStore(store.subscribe, () => store.version);
}
