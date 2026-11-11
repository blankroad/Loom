// Prefix key `Ctrl-]` (plan §5.2.1, D9/R-1). Installed on window in the CAPTURE
// phase so it works even when a terminal panel has focus.
//   Ctrl-] x  STOP          Ctrl-] m  marker
//   Ctrl-] A  approvals     Ctrl-] ?  help
//   Ctrl-] Ctrl-]  → literal 0x1D delivered to the focused panel
// The prefix disarms after PREFIX_TIMEOUT_MS or on any other key.
// Web does NOT bind Ctrl-X to STOP (browser cut, R-1).

export const PREFIX_TIMEOUT_MS = 2_000;

export interface PrefixHandlers {
  stop(): void;
  mark(): void;
  approvals(): void;
  help(): void;
  /** deliver 0x1D to the focused panel */
  literal(): void;
  /** prefix armed/disarmed (status hint) */
  armed?(on: boolean): void;
}

function isPrefix(e: KeyboardEvent): boolean {
  return e.ctrlKey && !e.altKey && !e.metaKey && (e.key === ']' || e.code === 'BracketRight');
}

const MODIFIERS = new Set(['Control', 'Shift', 'Alt', 'Meta']);

export function installPrefixKeys(
  target: Pick<Window, 'addEventListener' | 'removeEventListener'>,
  h: PrefixHandlers,
  now: () => number = () => performance.now(),
): () => void {
  let armedAt: number | null = null;

  const onKey = (ev: Event): void => {
    const e = ev as KeyboardEvent;
    if (armedAt !== null && now() - armedAt > PREFIX_TIMEOUT_MS) {
      armedAt = null;
      h.armed?.(false);
    }
    if (armedAt === null) {
      if (!isPrefix(e)) return;
      armedAt = now();
      e.preventDefault();
      e.stopPropagation();
      h.armed?.(true);
      return;
    }
    if (MODIFIERS.has(e.key)) return; // e.g. Shift before 'A' / '?'
    armedAt = null;
    h.armed?.(false);
    e.preventDefault();
    e.stopPropagation();
    if (isPrefix(e)) return h.literal();
    switch (e.key) {
      case 'x':
        return h.stop();
      case 'm':
        return h.mark();
      case 'A':
        return h.approvals();
      case '?':
        return h.help();
      default:
        return; // unknown second key: swallowed, nothing sent to the panel
    }
  };

  target.addEventListener('keydown', onKey, true);
  return () => target.removeEventListener('keydown', onKey, true);
}
