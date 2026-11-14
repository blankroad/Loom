// ctx.media: declared, same-origin, read-only media GET (S16). Generic — no panel id is special.
import { describe, expect, it, vi } from 'vitest';
import { RenderScheduler } from '../store/renderScheduler';
import { RowStore } from '../store/rowStore';
import { ShellControlStore } from '../store/shellControl';
import { FakeNet, TEST_HELLO } from '../store/testing/fakeNet';
import { ActionGate } from './actionGate';
import { checkMediaPath, createPanelContext, MediaPathError, SimpleCursorBus, UndeclaredError } from './context';
import { validateManifest, type PanelManifest } from './manifest';

const base = (extra: Partial<PanelManifest> = {}): PanelManifest => ({
  id: 'demo.viewer',
  domain: 'demo',
  title: 'demo',
  version: 1,
  milestone: 'M1',
  topics: [],
  queries: [],
  actions: [],
  memory: { maxBytes: 1 << 20 },
  render: { maxHz: 30, budgetMs: 4 },
  ...extra,
});

function make(m: PanelManifest) {
  const net = new FakeNet();
  net.open();
  const shell = new ShellControlStore();
  shell.setConn({ status: 'open', hello: TEST_HELLO }, 0);
  const calls = vi.spyOn(net, 'call');
  const mediaFetch = vi.fn<(u: string, i: RequestInit) => Promise<Response>>(async () => new Response('x', { status: 200 }));
  const h = createPanelContext(m, {}, {
    net,
    gate: new ActionGate({ net, rights: () => shell.rights, writeLocked: () => false, confirmUi: { confirm: async () => true } }),
    scheduler: new RenderScheduler({ raf: () => 1, cancelRaf: () => {}, now: () => 0 }),
    rows: new RowStore(),
    shell,
    cursor: new SimpleCursorBus(),
    ui: { reveal: () => {}, openPanel: () => {}, toast: () => {} },
    mediaFetch,
    origin: 'http://localhost:7777',
  });
  return { ...h, mediaFetch, calls };
}

describe('checkMediaPath', () => {
  const P = ['/api/v1/demo/'];
  it('accepts canonical declared paths with plain name=value queries', () => {
    expect(checkMediaPath(P, '/api/v1/demo/abc/frame')).toBeNull();
    expect(checkMediaPath(P, '/api/v1/demo/abc/frame?id=42')).toBeNull();
  });
  it('refuses traversal, encoding, foreign origins, undeclared prefixes and credential-like queries', () => {
    for (const bad of [
      '/api/v1/demo/../stop',
      '/api/v1/demo/./x',
      '/api/v1/demo//x',
      '/api/v1/demo/%2e%2e/x',
      '/api/v1/demo/x#frag',
      '/api/v1/demo/x\\y',
      '//evil.example/api/v1/demo/x',
      'http://evil.example/api/v1/demo/x',
      '/api/v1/demo/',
      '/api/v1/other/x',
      '/api/v1/stop',
      '/api/v1/demo/x?token=abc',
      '/api/v1/demo/x?access_key=1',
      '/api/v1/demo/x?id=1&session=2',
      '/api/v1/demo/x?id',
      '/api/v1/demo/x?id=a b',
      '/api/v1/demo/x?a=1&b=2&c=3&d=4&e=5',
      `/api/v1/demo/${'x'.repeat(600)}`,
    ]) {
      expect(checkMediaPath(P, bad), bad).not.toBeNull();
    }
  });
});

describe('manifest media declaration', () => {
  it('is optional and validated', () => {
    expect(() => validateManifest(base())).not.toThrow();
    expect(() => validateManifest(base({ media: { prefixes: ['/api/v1/demo/'] } }))).not.toThrow();
    for (const prefixes of [[], ['/api/v1/demo'], ['/rpc/'], ['/api/v1/../x/'], ['http://x/api/v1/demo/']]) {
      expect(() => validateManifest(base({ media: { prefixes } })), JSON.stringify(prefixes)).toThrow(/media/);
    }
  });
});

describe('ctx.media', () => {
  it('undeclared panels cannot make media requests', () => {
    const { ctx, mediaFetch } = make(base());
    expect(() => ctx.media('/api/v1/demo/x')).toThrow(UndeclaredError);
    expect(mediaFetch).not.toHaveBeenCalled();
  });

  it('GET same-origin, no-store, redirect error, credentials same-origin, signal passed; never an RPC', async () => {
    const { ctx, mediaFetch, calls } = make(base({ media: { prefixes: ['/api/v1/demo/'] } }));
    const ac = new AbortController();
    const r = await ctx.media('/api/v1/demo/c1/frame?id=3', ac.signal);
    expect(r.status).toBe(200);
    expect(mediaFetch).toHaveBeenCalledOnce();
    const [url, init] = mediaFetch.mock.calls[0]!;
    expect(url).toBe('http://localhost:7777/api/v1/demo/c1/frame?id=3');
    expect(init).toMatchObject({ method: 'GET', credentials: 'same-origin', cache: 'no-store', redirect: 'error' });
    expect(init.signal).toBe(ac.signal);
    expect(calls).not.toHaveBeenCalled();
  });

  it('refused paths throw before any request', () => {
    const { ctx, mediaFetch } = make(base({ media: { prefixes: ['/api/v1/demo/'] } }));
    for (const bad of ['/api/v1/demo/../stop', '/api/v1/demo/x?token=1', '/api/v1/other/x']) {
      expect(() => ctx.media(bad), bad).toThrow(MediaPathError);
    }
    expect(mediaFetch).not.toHaveBeenCalled();
  });
});
