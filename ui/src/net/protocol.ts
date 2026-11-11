// Messages between the main-thread proxy (client.ts) and the net core
// (core.ts, normally inside net.worker.ts). Internal to src/net — the rest of
// the app only sees the NetClient interface (api.ts).

import type { LoomEvent, SubscribeFilter } from '../proto/provisional';
import type { ClockEstimate, ConnState, SubscribeOptions } from './api';
import type { DeliveryItem } from './budget';
import type { NetErrorShape } from './rpc';
import type { PresenceExtra } from './wire';

export type CoreIn =
  | { t: 'init'; url: string; clientId: string }
  | { t: 'call'; id: number; method: string; params?: unknown; timeoutMs?: number }
  | { t: 'cancel'; id: number }
  | { t: 'sub'; hid: number; filter: SubscribeFilter; opts: SubscribeOptions }
  | { t: 'unsub'; hid: number }
  /** main thread is ready for the next delivery (sent at most once per frame) */
  | { t: 'pull' }
  | { t: 'visible'; visible: boolean; extra?: PresenceExtra }
  | { t: 'close' };

export type CoreOut =
  | { t: 'state'; state: ConnState }
  /** control path: one event per message, sent the moment it is decoded */
  | { t: 'control'; ev: LoomEvent }
  | { t: 'result'; id: number; ok: true; value: unknown }
  | { t: 'result'; id: number; ok: false; error: NetErrorShape }
  /** data path: answer to one `pull` */
  | { t: 'deliver'; items: DeliveryItem[] }
  | { t: 'clock'; est: ClockEstimate };
