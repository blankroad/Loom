// Browser bindings for NetCore (WebSocket, clock, timers). Used by the worker
// and by the inline fallback.

import type { CoreDeps, SocketEvents, SocketLike } from './core';

export function openBrowserSocket(url: string, ev: SocketEvents): SocketLike {
  const ws = new WebSocket(url);
  let done = false;
  ws.onopen = () => ev.onOpen();
  ws.onmessage = (m: MessageEvent) => {
    if (typeof m.data === 'string') ev.onText(m.data);
    else ev.onBinary?.();
  };
  ws.onclose = (c: CloseEvent) => {
    if (done) return;
    done = true;
    ev.onClose(`ws_close_${c.code}${c.reason ? `:${c.reason}` : ''}`);
  };
  ws.onerror = () => undefined; // a close event always follows
  return {
    send: (t) => ws.send(t),
    close: (code, reason) => ws.close(code, reason),
  };
}

export function browserDeps(): CoreDeps {
  return {
    openSocket: openBrowserSocket,
    now: () => performance.timeOrigin + performance.now(),
    probeAuth: async (wsUrl) => {
      const u = new URL(wsUrl);
      u.protocol = u.protocol === 'wss:' ? 'https:' : 'http:';
      try {
        // same-origin cookie goes along; never follows redirects; 2 s budget
        const r = await fetch(u.toString(), { method: 'GET', credentials: 'same-origin', cache: 'no-store', redirect: 'manual', signal: AbortSignal.timeout(2_000) });
        return r.status;
      } catch {
        return null;
      }
    },
    timers: {
      setTimeout: (cb, ms) => setTimeout(cb, ms),
      clearTimeout: (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
    },
  };
}
