// Boundary between the net layer (ui-sol: src/net/**) and the stores/shell
// (ui-opus: src/store/**, src/sdk/**, src/shell/**). Owner: ui-lead.
// The implementation runs WS + decode in a Web Worker; this interface is what the
// main thread sees. Panels never import this module (eslint I-UI1).

import type {
  EventsBatch,
  HelloResult,
  LoomEvent,
  SnapshotPart,
  SubscribeFilter,
  U64,
} from '../proto/provisional';

export type ConnState =
  | { status: 'connecting'; attempt: number }
  | { status: 'open'; hello: HelloResult }
  | { status: 'reconnecting'; attempt: number; retryInMs: number; reason: string }
  | { status: 'closed'; reason: string };

export interface ClockEstimate {
  /** daemon mono (ns) ≈ performance.timeOrigin+performance.now() (ms) * 1e6 + thetaNs */
  thetaNs: bigint;
  /** ± uncertainty, ms */
  errMs: number;
  clockId: string;
  synced: boolean;
}

export interface SubscribeOptions {
  snapshot: boolean;
  maxBatchMs?: number;
  encoding?: 'json' | 'binary';
  coalesceBy?: 'id';
  /**
   * Shell-owned stream (Activity, test, STOP-adjacent monitoring): never suspended or folded into
   * a union by the mux, so its handlers never see a lease resync. Not part of the subscription key.
   */
  pinned?: boolean;
  /**
   * Initial visibility lease (default true). `false` = the handle is opened hidden: it never makes the
   * mux relieve pressure (suspend/merge) before the caller could say so. Pinned handles are always
   * active. Not part of the subscription key and never sent to the daemon.
   */
  active?: boolean;
}

/** Worker-side receive/decode times of a delivery (performance.timeOrigin+now, ms). */
export interface DeliveryTiming {
  recv_ms: number;
  decode_ms: number;
}

/** Extra core.viewer.presence fields; the net remembers the latest and resends them after reconnect. */
export interface PresenceExtra {
  channel?: string | null;
  rendered_through_seq?: U64 | null;
  oldest_pending_age_ms?: number | null;
}

/** Ordered callbacks for one subscription. snapshot parts arrive before end, live strictly after the end's stream_seq. */
export interface SubscriptionHandlers {
  onSnapshotPart?(part: SnapshotPart): void;
  onSnapshotEnd?(h: U64): void;
  onBatch(batch: EventsBatch, timing?: DeliveryTiming): void;
  /** subscription was lost and must be re-snapshotted (epoch change, ring overrun, disconnect) */
  onResync?(reason: string): void;
}

export interface Subscription {
  readonly id: string;
  close(): void;
  /**
   * Visibility lease (default active). A hidden handle keeps its server subscription until an
   * ACTIVE handle needs the slot; then it is suspended (onResync("suspended_hidden")) and
   * re-placed with a fresh snapshot when it becomes active again. Optional so fakes stay valid.
   */
  setActive?(active: boolean): void;
}

export interface NetClient {
  readonly state: ConnState;
  onState(cb: (s: ConnState) => void): () => void;
  /**
   * Control-path events (see isControlEvent): delivered one by one, immediately, never batched.
   * UI-internal convention (lead, 2026-11-11): `core.heartbeat` notifications are also delivered
   * here as a synthetic event (kind 'core.heartbeat', payload = Heartbeat fields) — the shell's
   * STALLED detection depends on it. Client-made events (heartbeat, local display.gap) carry
   * producer_id 'ui-net', ts.source 'synthetic', origin 'device', payload.detail.local = true;
   * gap events name the affected stream in domain/channel.
   */
  onControl(cb: (ev: LoomEvent) => void): () => void;
  /** JSON-RPC call. Mutations are never retried automatically. */
  call<R = unknown>(method: string, params?: unknown, opts?: { signal?: AbortSignal; timeoutMs?: number }): Promise<R>;
  /** Shares one server subscription per identical (filter, options) with refcount. */
  subscribe(filter: SubscribeFilter, opts: SubscribeOptions, h: SubscriptionHandlers): Subscription;
  clock(): ClockEstimate;
  /** Report tab visibility to the daemon (core.viewer.presence). */
  setVisible(visible: boolean, extra?: PresenceExtra): void;
  /** Fallback STOP when the WS is down: POST /api/v1/stop. */
  stopViaHttp(): Promise<void>;
  close(): void;
}
