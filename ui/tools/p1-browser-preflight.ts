// Harness/protocol preflight only. This never counts as browser rendering/input acceptance.
import WebSocket from 'ws';
import { RealTc1 } from '../e2e/real-tc1-server';
const fixture = new RealTc1();
let ws: WebSocket | null = null;
let id = 0;
async function rpc(method: string, params: object): Promise<Record<string, unknown>> {
  const socket = ws!;
  const rid = ++id;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { socket.off('message', read); reject(new Error(`${method} timeout`)); }, 5000);
    const read = (raw: WebSocket.RawData) => {
      const message = JSON.parse(String(raw)) as { id?: number; error?: unknown; result?: Record<string, unknown> };
      if (message.id !== rid) return;
      clearTimeout(timer); socket.off('message', read);
      if (message.error) reject(new Error(`${method} rejected`));
      else resolve(message.result ?? {});
    };
    socket.on('message', read);
    socket.send(JSON.stringify({ jsonrpc: '2.0', id: rid, method, params }));
  });
}
try {
  await fixture.start();
  const url = new URL(fixture.loginURL());
  if (!(await fetch(`${url.origin}/ui/`)).ok) throw new Error('fresh UI not served');
  const code = new URLSearchParams(url.hash.slice(1)).get('boot');
  const response = await fetch(`${url.origin}/api/v1/bootstrap`, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code }),
  });
  const cookie = response.headers.get('set-cookie')?.split(';')[0];
  if (response.status !== 204 || !cookie) throw new Error('private bootstrap exchange failed');
  ws = new WebSocket(`${url.origin.replace('http:', 'ws:')}/rpc`, { headers: { Cookie: cookie, Origin: url.origin } });
  await new Promise<void>((resolve, reject) => { ws!.once('open', resolve); ws!.once('error', () => reject(new Error('viewer connection failed'))); });
  const hello = await rpc('core.hello', { proto: 'loom/1.0', client_id: 'tc1-protocol-preflight', want_role: 'viewer', client: { kind: 'test', version: '1', tty: false } });
  if (hello.role !== 'viewer' || hello.rights !== 'operate') throw new Error('viewer rights mismatch');
  await rpc('core.subscribe', { filter: {}, snapshot: false, max_batch_ms: 10 });
  await fixture.begin();
  const result = await fixture.result();
  if (result.result !== 'passed') throw new Error('actual powered TC-1 did not pass');
  console.log(JSON.stringify({ actual_tc1: 'passed', fresh_ui_http: true, viewer_cookie_and_subscription: true,
    browser_rendering: false, browser_input: false, note: 'Protocol/fixture validation only; actual browser gate remains mandatory.' }));
} finally { ws?.close(); await fixture.close(); }
