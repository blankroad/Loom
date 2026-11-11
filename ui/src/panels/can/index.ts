// CAN panels (M1: can.trace). panels/index.ts (lead) adds:
//   import { registerCanPanels } from './can';
//   registerCanPanels();
import { registerCanTrace } from './canTrace';

export { createCanTrace, CAN_TRACE_MANIFEST } from './canTrace';
export type { CanTraceStats } from './canTrace';

/** idempotent */
export function registerCanPanels(): void {
  registerCanTrace();
}
