// Net Worker: owns the WebSocket and decoding (plan §3.3). The main thread
// talks to it through CoreIn/CoreOut (protocol.ts) via MainNetClient.

import { browserDeps } from './browser';
import { NetCore } from './core';
import type { CoreIn, CoreOut } from './protocol';

const scope = globalThis as unknown as DedicatedWorkerGlobalScope;
let core: NetCore | null = null;

scope.onmessage = (e: MessageEvent<CoreIn>) => {
  const m = e.data;
  if (m.t === 'init') {
    if (core) return;
    core = new NetCore(m.url, m.clientId, browserDeps(), (out: CoreOut) => scope.postMessage(out));
    core.start();
    return;
  }
  core?.handle(m);
};
