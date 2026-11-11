// Panel manifest (plan §4.2) and its validation. A manifest declares everything a
// panel may touch: topics, read RPCs, write actions. PanelContext throws on any
// undeclared use (invariant I-UI1 at the API layer, plan §4.3).

import type { Right, SubscribeFilter } from '../proto/provisional';
import type { PropsSchema } from './propsSchema';

export type Props = Record<string, unknown>;
export type Milestone = 'M1' | 'M1b' | 'M2' | 'M3' | 'M4';

export interface TopicSpec {
  /** panel-internal name used with ctx.store(key) */
  key: string;
  /** builds the core.subscribe filter from the panel's props */
  filter: (props: Props) => SubscribeFilter;
  encoding?: 'json' | 'binary';
  snapshot: 'required' | 'none';
  /** UART terminal uses 10 */
  maxBatchMs?: number;
  coalesceBy?: 'id';
  /** main-thread mirror size (rows). Default 2,000, max 20,000 (C2). */
  mirrorRows?: number;
}

export type ConfirmMode = 'none' | 'enter_twice' | 'card';

export interface ActionSpec {
  id: string;
  /** `<domain>.<verb>` or `<domain>.<noun>.<verb>` — server validates via Registry */
  rpc: string;
  class: 'request' | 'job' | 'stream';
  /** minimum right (hierarchical: approve ⊃ operate ⊃ watch) */
  right: Right;
  /** UI-side mistake prevention only — the daemon policy decides */
  confirm: ConfirmMode;
  /** STOP · abort · emergency OFF: any right, never confirmed, allowed while STALLED */
  safetyDirection?: 'reduce';
  /** shown on confirm cards */
  label?: string;
}

export interface PanelManifest {
  /** `<domain>.<name>` e.g. 'uart.terminal' */
  id: string;
  domain: string;
  title: string;
  version: 1;
  milestone: Milestone;
  topics: TopicSpec[];
  queries: string[];
  actions: ActionSpec[];
  /** share of the 256 MiB tab data budget (plan §6.5) */
  memory: { maxBytes: number };
  render: { maxHz: number; budgetMs: number; lagTargetMs?: number };
  shellSlots?: ('channel_bar' | 'approval_body' | 'evidence_ref')[];
  /** props JSON Schema subset — saved layouts and deep links are validated against it (propsSchema.ts) */
  propsSchema?: PropsSchema;
}

export const TAB_DATA_BUDGET_BYTES = 256 * 1024 * 1024;
export const MIRROR_ROWS_DEFAULT = 2_000;
export const MIRROR_ROWS_MAX = 20_000;

const PANEL_ID = /^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$/;
const RPC = /^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,2}$/;
const KEY = /^[A-Za-z][A-Za-z0-9_]*$/;
const RIGHTS: readonly Right[] = ['watch', 'operate', 'approve'];
const CONFIRMS: readonly ConfirmMode[] = ['none', 'enter_twice', 'card'];
const CLASSES = ['request', 'job', 'stream'] as const;

export class ManifestError extends Error {
  constructor(
    readonly panelId: string,
    readonly problems: string[],
  ) {
    super(`panel manifest ${panelId}: ${problems.join('; ')}`);
    this.name = 'ManifestError';
  }
}

export function validateManifest(m: PanelManifest): void {
  const p: string[] = [];
  if (!PANEL_ID.test(m.id)) p.push(`id must be <domain>.<name> (got "${m.id}")`);
  else if (m.id.split('.')[0] !== m.domain) p.push(`id domain must equal domain "${m.domain}"`);
  if (!m.title) p.push('title required');
  if (m.version !== 1) p.push('version must be 1');

  const keys = new Set<string>();
  for (const t of m.topics) {
    if (!KEY.test(t.key)) p.push(`topic key "${t.key}" invalid`);
    if (keys.has(t.key)) p.push(`duplicate topic key "${t.key}"`);
    keys.add(t.key);
    if (typeof t.filter !== 'function') p.push(`topic "${t.key}" filter must be a function`);
    if (t.mirrorRows !== undefined && (t.mirrorRows < 1 || t.mirrorRows > MIRROR_ROWS_MAX)) {
      p.push(`topic "${t.key}" mirrorRows must be 1..${MIRROR_ROWS_MAX}`);
    }
  }
  for (const q of m.queries) if (!RPC.test(q)) p.push(`query "${q}" is not an RPC name`);

  const ids = new Set<string>();
  for (const a of m.actions) {
    if (!KEY.test(a.id)) p.push(`action id "${a.id}" invalid`);
    if (ids.has(a.id)) p.push(`duplicate action id "${a.id}"`);
    ids.add(a.id);
    if (!RPC.test(a.rpc)) p.push(`action "${a.id}" rpc "${a.rpc}" invalid`);
    if (!RIGHTS.includes(a.right)) p.push(`action "${a.id}" right invalid`);
    if (!CONFIRMS.includes(a.confirm)) p.push(`action "${a.id}" confirm invalid`);
    if (!CLASSES.includes(a.class)) p.push(`action "${a.id}" class invalid`);
    if (a.safetyDirection === 'reduce' && a.confirm !== 'none') {
      p.push(`action "${a.id}" is safety-reducing and must not ask for confirmation`);
    }
  }
  if (!(m.memory.maxBytes > 0 && m.memory.maxBytes <= TAB_DATA_BUDGET_BYTES)) {
    p.push(`memory.maxBytes must be 1..${TAB_DATA_BUDGET_BYTES}`);
  }
  if (!(m.render.maxHz >= 1 && m.render.maxHz <= 60)) p.push('render.maxHz must be 1..60');
  if (!(m.render.budgetMs > 0 && m.render.budgetMs <= 8)) p.push('render.budgetMs must be 0..8');
  if (p.length) throw new ManifestError(m.id, p);
}
