// One-line STOP notice for panels that write (CAN · UART · UDS). STOP locks agent/script
// writes; the user's own input stays possible (master A12/A15), so the line says exactly
// that. Subscription is released with the panel context.

import type { PanelContext } from './context';

export const LATCH_LINE_TEXT = 'STOP — agent/script 쓰기 잠김(사용자 입력 가능)';

export function createLatchLine(ctx: Pick<PanelContext, 'latch'>): HTMLElement {
  const e = document.createElement('div');
  e.className = 'panel-latch';
  e.setAttribute('role', 'status');
  e.setAttribute('data-testid', 'panel-latch');
  e.textContent = LATCH_LINE_TEXT;
  const sync = () => {
    e.hidden = !ctx.latch.on;
  };
  sync();
  ctx.latch.subscribe(sync);
  return e;
}
