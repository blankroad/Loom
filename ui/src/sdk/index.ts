// @loom/panel-sdk — the only module panels may import (eslint I-UI1).
// Panels get data only through PanelContext; they never see NetClient or stores.

export { registerPanel } from './registry';
export type { PanelInstance, PanelFactory } from './registry';
export type { PanelContext, WriteHandle } from './context';
export type { ActionSpec, ConfirmMode, Milestone, PanelManifest, Props, TopicSpec } from './manifest';
export type { GapMark, Projection, StoreMirror } from './storeMirror';
export { GateError } from './actionGate';
export type {
  LoomEvent,
  Origin,
  Right,
  Risk,
  SubscribeFilter,
  WriteState,
} from '../proto/provisional';

/** Origin badge text and CSS class (E8: AGT 청록 · USR 노랑 · DEV 회색 · SCR 보라, always with letters). */
export const ORIGIN_BADGE: Record<'agent' | 'user' | 'device' | 'script', { text: string; cls: string }> = {
  agent: { text: 'AGT', cls: 'org-agt' },
  user: { text: 'USR', cls: 'org-usr' },
  device: { text: 'DEV', cls: 'org-dev' },
  script: { text: 'SCR', cls: 'org-scr' },
};

/** D4/A14 stage chip text (plan §5.2.3). */
export const WRITE_STATE_CHIP: Record<
  'requested' | 'accepted' | 'sent' | 'confirmed' | 'effective_verified' | 'effect_unverified' | 'failed' | 'outcome_unknown',
  string
> = {
  requested: '○요청',
  accepted: '◔수락',
  sent: '◑보냄',
  confirmed: '●확정',
  effective_verified: '●●검증',
  effect_unverified: '●?미검증',
  failed: '✗실패',
  outcome_unknown: '⌛?불확정',
};

// ---- P1 common components ----
export { VirtualTable } from './virtualTable';
export type { Cell, Column, VirtualTableOptions } from './virtualTable';
export {
  createRowTable,
  fmtWall,
  gapText,
  isFinal,
  mergeGaps,
  originCell,
  riskCell,
  stageCell,
  STAGE_CHIP,
} from './rowTable';
export type { RowItem, RowTableOptions } from './rowTable';
export type { RowFilter, RowLike, RowView } from './rowView';
export {
  bytesToHex,
  decodePayloadBytes,
  INTENT_WINDOW_MS,
  INPUT_COALESCE_MS,
  LARGE_PASTE_BYTES,
  MAX_PENDING_WRITE_BYTES,
  TerminalView,
} from './terminalView';
export type { TerminalStats, TerminalViewOptions } from './terminalView';
export { TreeHex, MAX_HEX_BYTES } from './treeHex';
export type { TreeNode } from './treeHex';
export { parseEvidenceRef, formatRequestRef, formatSeqRef } from './evidenceRef';
export type { EvidenceRef } from './evidenceRef';
export { createLatchLine, LATCH_LINE_TEXT } from './latchLine';
export { CHANNEL_PROP, validateProps } from './propsSchema';
export type { PropSchema, PropsSchema, PropsCheck } from './propsSchema';
