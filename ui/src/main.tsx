import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { createNetClient } from './net';
import './panels';
import { routeAfterBoot, runBootstrap } from './shell/bootstrap';
import { createShellServices } from './shell/services';
import { ShellApp } from './shell/ShellApp';

async function start(root: HTMLElement): Promise<void> {
  // `#boot=<code>`: strip it from the address bar and trade it for the viewer cookie
  // BEFORE the WebSocket opens (a cookie-less WS gets no role). No-op without a fragment.
  const boot = await runBootstrap({
    location,
    history,
    fetch: (u, i) => fetch(u, i),
    session: (() => {
      try {
        return sessionStorage;
      } catch {
        return null;
      }
    })(),
  });
  routeAfterBoot(boot, (h) => {
    location.hash = h;
  });
  // one NetClient (one WS) per tab; same origin `/rpc` (vite proxies to loomd/mock in dev)
  const services = createShellServices(createNetClient('/rpc'));
  createRoot(root).render(
    <StrictMode>
      <ShellApp services={services} />
    </StrictMode>,
  );
}

const root = document.getElementById('root');
if (root) void start(root);
