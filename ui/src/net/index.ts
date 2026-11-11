// Net layer entry: createNetClient(url) -> NetClient (api.ts).
// WS + decode run in a Web Worker; without Worker support (tests, SSR-ish
// environments) the same core runs inline on the calling thread.

import type { NetClient } from './api';
import { browserDeps } from './browser';
import { MainNetClient, type CorePort, type FrameScheduler } from './client';
import { NetCore, type CoreDeps } from './core';
import type { CoreIn, CoreOut } from './protocol';

export type { NetClient } from './api';
export { NetError, type NetErrorCode, type NetErrorShape } from './rpc';
export type { DeliveryTiming } from './client';
export type { PresenceExtra } from './wire';

declare global {
  interface Window {
    /** dev builds only: lets Playwright/devtools inspect the live connection */
    __loomNet?: NetClient;
  }
}

export interface CreateNetOptions {
  /** run the core on this thread instead of a Worker */
  inline?: boolean;
  /** inline mode only: replace browser bindings (tests) */
  deps?: CoreDeps;
  frames?: FrameScheduler;
  fetch?: typeof fetch;
}

/** ws URL for `url` (absolute ws(s)/http(s), or a path such as '/rpc' resolved against the page). */
export function resolveWsUrl(url: string): string {
  const base = typeof location !== 'undefined' ? location.href : undefined;
  const u = new URL(url, base);
  if (u.protocol === 'http:') u.protocol = 'ws:';
  else if (u.protocol === 'https:') u.protocol = 'wss:';
  return u.toString();
}

/**
 * DEV builds only (E2E / local testing; never in a production bundle): the page query may
 *   ?loom_daemon=ws://localhost:<port>/rpc   connect to another local daemon (e.g. a second
 *                                            `loomd --fixture-replay` started with other flags)
 *   ?loom_rights=watch|operate|approve       forwarded as `?rights=` (loom-mock only; loomd ignores it)
 * Only loopback ws:// targets are accepted. Returns `wsUrl` unchanged otherwise.
 */
export function devWsUrl(wsUrl: string, search: string): string {
  const q = new URLSearchParams(search);
  let out = wsUrl;
  const d = q.get('loom_daemon');
  if (d) {
    try {
      const u = new URL(d);
      if (u.protocol === 'ws:' && (u.hostname === 'localhost' || u.hostname === '127.0.0.1')) out = u.toString();
    } catch {
      // malformed: keep the default
    }
  }
  const r = q.get('loom_rights');
  if (r === 'watch' || r === 'operate' || r === 'approve') {
    const u = new URL(out);
    u.searchParams.set('rights', r);
    out = u.toString();
  }
  return out;
}

export function stopUrlFor(wsUrl: string): string {
  const u = new URL(wsUrl);
  u.protocol = u.protocol === 'wss:' ? 'https:' : 'http:';
  u.pathname = '/api/v1/stop';
  u.search = '';
  u.hash = '';
  return u.toString();
}

function newClientId(): string {
  const b = new Uint8Array(6);
  crypto.getRandomValues(b);
  return `ui-${Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('')}`;
}

const defaultFrames: FrameScheduler = {
  request(cb) {
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(() => cb());
    else setTimeout(cb, 16);
  },
};

export function inlinePort(url: string, clientId: string, deps: CoreDeps): CorePort {
  let listener: ((m: CoreOut) => void) | null = null;
  let dead = false;
  // async both ways (like postMessage) so no handler runs inside the caller's stack
  const core = new NetCore(url, clientId, deps, (m) => queueMicrotask(() => !dead && listener?.(m)));
  return {
    post: (m: CoreIn) => queueMicrotask(() => !dead && core.handle(m)),
    listen: (cb) => {
      listener = cb;
      core.start();
    },
    terminate: () => {
      dead = true;
    },
  };
}

function workerPort(url: string, clientId: string): CorePort {
  const w = new Worker(new URL('./net.worker.ts', import.meta.url), { type: 'module', name: 'loom-net' });
  w.postMessage({ t: 'init', url, clientId } satisfies CoreIn);
  return {
    post: (m) => w.postMessage(m),
    listen: (cb, onError) => {
      w.onmessage = (e: MessageEvent<CoreOut>) => cb(e.data);
      w.onerror = (e) => onError(`net_worker_error:${e.message}`);
    },
    terminate: () => w.terminate(),
  };
}

export function createNetClient(url = '/rpc', opts: CreateNetOptions = {}): NetClient {
  let wsUrl = resolveWsUrl(url);
  if (import.meta.env.DEV && typeof location !== 'undefined') wsUrl = devWsUrl(wsUrl, location.search);
  const clientId = newClientId();
  const inline = opts.inline || typeof Worker === 'undefined';
  const port = inline ? inlinePort(wsUrl, clientId, opts.deps ?? browserDeps()) : workerPort(wsUrl, clientId);
  const client = new MainNetClient(port, {
    clientId,
    stopUrl: stopUrlFor(wsUrl),
    frames: opts.frames ?? defaultFrames,
    ...(opts.fetch ? { fetch: opts.fetch } : {}),
  });
  if (import.meta.env.DEV && typeof window !== 'undefined') window.__loomNet = client;
  return client;
}
