// P0 smoke: page loads, the shell mounts, the net worker reaches the mock
// daemon (core.hello done -> state 'open'). The lead extends this once the
// shell and panels land (E-UI-1/2/3 via DOM data-request-id / data-origin).
// `window.__loomNet` exists only in dev builds (src/net/index.ts). e2e/ is typechecked
// with node libs (no DOM), hence the globalThis casts inside page callbacks.

import { expect, test } from '@playwright/test';

interface NetProbe {
  state: { status: string; hello?: { env?: string; rights?: string } };
}

test('page loads and connects to the mock daemon', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto('./');
  await expect(page.getByTestId('loom-shell')).toBeVisible();
  await page.waitForFunction(() => (globalThis as unknown as { __loomNet?: NetProbe }).__loomNet?.state.status === 'open', null, { timeout: 15_000 });
  const hello = await page.evaluate(() => (globalThis as unknown as { __loomNet: NetProbe }).__loomNet.state.hello);
  expect(hello?.env).toBe('sim');
  expect(errors).toEqual([]);
});

test('net layer reconnects after the daemon drops the socket', async ({ page }) => {
  await page.goto('./');
  await page.waitForFunction(() => (globalThis as unknown as { __loomNet?: NetProbe }).__loomNet?.state.status === 'open', null, { timeout: 15_000 });
  // drop every connection from the daemon side, via a throwaway WS from the page
  await page.evaluate(async () => {
    const ws = new WebSocket(`${(globalThis as unknown as { location: { origin: string } }).location.origin.replace(/^http/, 'ws')}/rpc`);
    await new Promise((r) => (ws.onopen = r));
    const call = (id: number, method: string, params: unknown) =>
      new Promise((r) => {
        ws.addEventListener('message', (m) => (JSON.parse(String(m.data)) as { id?: number }).id === id && r(null));
        ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
      });
    await call(1, 'core.hello', { proto: 'loom/1.0', client_id: 'e2e-ctl', want_role: 'viewer', client: { kind: 'e2e', version: '0', tty: false } });
    await call(2, 'mock.inject', { kind: 'disconnect' });
    ws.close();
  });
  await page.waitForFunction(() => (globalThis as unknown as { __loomNet?: NetProbe }).__loomNet?.state.status === 'reconnecting', null, { timeout: 5_000 });
  await page.waitForFunction(() => (globalThis as unknown as { __loomNet?: NetProbe }).__loomNet?.state.status === 'open', null, { timeout: 10_000 });
});
