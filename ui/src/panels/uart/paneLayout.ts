// U7 (operator-upgrade/uart) — narrow-pane header layout and always-readable connection identity.
// Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B1.2 (Opus owns signatures/types/constants; bodies are
// Sonnet batch 1). Pure decisions only; terminalPanel.ts / UartWorkbench.tsx apply them to the DOM.

import { isOpenStatus } from './uartState';

export type ConnClass = 'connected' | 'lost' | 'waiting' | 'unknown' | 'disconnected';
export type HeaderMode = 'full' | 'compact' | 'narrow';
/** header tool ids, in toolbar order (each maps to one existing element of terminalPanel.ts) */
export type ToolId = 'view' | 'pause' | 'clear' | 'ts' | 'rec' | 'eol' | 'macro' | 'search' | 'send' | 'theme' | 'plug' | 'gear' | 'more';
export const TOOL_ORDER: readonly ToolId[] = ['view', 'pause', 'clear', 'ts', 'rec', 'eol', 'macro', 'search', 'send', 'theme', 'plug', 'gear', 'more'];

/** `.phead` width (CSS px) at or above which every tool and identity field stays in the header */
export const HEADER_FULL_MIN_PX = 900;
/** at or above → 'compact'; below → 'narrow' */
export const HEADER_COMPACT_MIN_PX = 520;

export interface ToolPlacement {
  /** tools left in the header, TOOL_ORDER order */
  header: ToolId[];
  /** tools moved into the ⋯ row (`.uart-overflow`), TOOL_ORDER order */
  overflow: ToolId[];
}
export interface IdentityPlan {
  /** port path element visible */
  path: boolean;
  /** 'full' = baud frame · flow · eol (today's text); 'baud' = baud only; never 'none' (baud is identity) */
  frame: 'full' | 'baud';
  /** RX/TX observed counters visible */
  counts: boolean;
}
export interface IdentityInput {
  channel: string;
  stateLabel: string;
  path: string | null;
  /** today's full frame text (may be '') */
  frame: string;
  rx: string;
  tx: string;
}
export type ChannelState = 'open' | 'closed' | 'unknown' | 'none';
export type SessionDot = 'on' | 'off' | 'unknown';

/** Same mapping terminalPanel.ts uses today (moved here). See FUNCTIONS.md §B1.2 connClass. */
export function connClass(status: string): ConnClass {
  if (isOpenStatus(status)) return 'connected';
  if (/disconn|lost/i.test(status)) return 'lost';
  if (/reconn|wait/i.test(status)) return 'waiting';
  if (status === '?') return 'unknown';
  return 'disconnected';
}

/** `null` = width unknown (hidden / not laid out) → caller keeps its current mode. See §B1.2 headerMode. */
export function headerMode(widthPx: number): HeaderMode | null {
  if (!Number.isFinite(widthPx) || widthPx <= 0) return null;
  if (widthPx >= HEADER_FULL_MIN_PX) return 'full';
  if (widthPx >= HEADER_COMPACT_MIN_PX) return 'compact';
  return 'narrow';
}

/** See FUNCTIONS.md §B1.2 toolPlacement. */
export function toolPlacement(mode: HeaderMode): ToolPlacement {
  const keep: readonly ToolId[] =
    mode === 'full' ? TOOL_ORDER : mode === 'compact' ? ['view', 'pause', 'rec', 'search', 'send', 'plug', 'more'] : ['rec', 'search', 'plug', 'more'];
  return { header: TOOL_ORDER.filter((t) => keep.includes(t)), overflow: TOOL_ORDER.filter((t) => !keep.includes(t)) };
}

/** See FUNCTIONS.md §B1.2 identityPlan. */
export function identityPlan(mode: HeaderMode): IdentityPlan {
  return mode === 'full' ? { path: true, frame: 'full', counts: true } : { path: false, frame: 'baud', counts: false };
}

/** Tooltip of `.ph-target` carrying every identity field, whatever is hidden. See §B1.2 identityTitle. */
export function identityTitle(i: IdentityInput): string {
  return [i.channel, i.stateLabel, i.path ?? '경로 없음', i.frame || '프레임 미확인', `RX ${i.rx}`, `TX ${i.tx}`].join(' · ');
}

/** Session-list open chip text, or null when no chip. See §B1.2 openTagText. */
export function openTagText(state: ChannelState, paneNo: number | null): string | null {
  return state === 'open' ? (paneNo ? `pane ${paneNo}` : '열림') : null;
}

/** Session-list dot class (connection state, not tag colour). See §B1.2 sessionDot. */
export function sessionDot(state: ChannelState): SessionDot {
  return state === 'open' ? 'on' : state === 'unknown' ? 'unknown' : 'off';
}
