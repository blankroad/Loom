// ActionGate (plan §3.4, §4.2): every panel/shell write goes through here.
//   1. safetyDirection 'reduce' (STOP, abort, emergency OFF): any right incl.
//      watch, no confirmation, allowed even while STALLED.
//   2. otherwise the connection's rights must include spec.right
//      (hierarchical approve ⊃ operate ⊃ watch) — else E_RIGHTS.
//   3. writes are locked while the shell is STALLED/disconnected — E_STALLED.
//   4. confirm 'enter_twice' | 'card' asks the shell's confirm UI; a "no" is
//      E_CANCELLED. This is mistake prevention only; the daemon policy decides
//      (F1: default policy is all-auto, so most actions use 'none').
// The gate never retries a mutation (D5).

import type { NetClient } from '../net/api';
import type { Right } from '../proto/provisional';
import { hasRight } from '../store/shellControl';
import type { ActionSpec } from './manifest';

export type GateErrorCode = 'E_RIGHTS' | 'E_STALLED' | 'E_CANCELLED';

export class GateError extends Error {
  constructor(
    readonly code: GateErrorCode,
    message: string,
  ) {
    super(message);
    this.name = 'GateError';
  }
}

export interface ConfirmRequest {
  mode: 'enter_twice' | 'card';
  actionId: string;
  rpc: string;
  label: string;
  params: unknown;
}

/** Provided by the shell (ConfirmLayer). Resolves true only on explicit user OK. */
export interface ConfirmUi {
  confirm(req: ConfirmRequest): Promise<boolean>;
}

export interface GateDeps {
  net: Pick<NetClient, 'call'>;
  rights: () => ReadonlySet<Right>;
  writeLocked: () => boolean;
  confirmUi: ConfirmUi;
}

export class ActionGate {
  constructor(private readonly deps: GateDeps) {}

  /** Throws GateError before any RPC when the action is not allowed. */
  check(spec: ActionSpec): void {
    if (spec.safetyDirection === 'reduce') return;
    if (!hasRight(this.deps.rights(), spec.right)) {
      throw new GateError('E_RIGHTS', `"${spec.label ?? spec.id}" needs ${spec.right} right`);
    }
    if (this.deps.writeLocked()) {
      throw new GateError('E_STALLED', 'connection stalled — writes locked (STOP still works)');
    }
  }

  async run<R = unknown>(spec: ActionSpec, params: unknown): Promise<R> {
    this.check(spec);
    if (spec.safetyDirection !== 'reduce' && spec.confirm !== 'none') {
      const ok = await this.deps.confirmUi.confirm({
        mode: spec.confirm,
        actionId: spec.id,
        rpc: spec.rpc,
        label: spec.label ?? spec.id,
        params,
      });
      if (!ok) throw new GateError('E_CANCELLED', 'cancelled by user');
      // re-check: rights or connection may have changed while the card was open
      this.check(spec);
    }
    return this.deps.net.call<R>(spec.rpc, params);
  }
}
