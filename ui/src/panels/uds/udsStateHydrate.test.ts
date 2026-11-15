// @vitest-environment jsdom
// Regression (TASK-UDS-STATE): a bound uds.console{targets} mounted AFTER its target connected.
// Real flow: DockLayout opens the bound panel before it removes the shell-seeded unbound one, so the SDK mux joins the
// still-open `{domains:['uds']}` entry and replays that entry's snapshot, taken before the connect (no uds item).
// The live `uds.state` (priority kind) went out on the control path before the bound panel existed. Shapes below are
// the native ones: snapshot item = gateway hub `SnapshotItem{domain,kind:'uds.snapshot',state}` wrapping
// StateReducer `{kind:'uds.state',payload}`; uds.status reply = loom_proto::Envelope around handlers::status
// `{targets:[StatePayload], others_since_last}`; the payload is the QA-captured qa-uds state (UDS-CONNECT.json).
import { afterEach, describe, expect, it } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './consolePanel';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  localStorage.clear();
});

/** qa/UDS-CONNECT.json data.state — real DoIP fixture 127.0.0.1:60492, ECU 0x1001, tester 0x0E80 */
const QA_STATE = {
  addr: 'DoIP 0x1001',
  backend: 'doip',
  generation: 1,
  lease: null,
  link: { cause: 'connected', state: 'up' },
  p2_ms: 50,
  p2_star_ms: 5000,
  security: { confidence: 'assumed', level: null },
  session: { confidence: 'assumed', id: 1, name: 'default' },
  target: 'qa-uds',
  tester_present: null,
};
const envelope = (data: unknown) => ({
  schema_version: 1,
  ok: true,
  cmd: 'uds.status',
  data,
  meta: { truncated: false, record_health: 'ok', env: 'sim', latch: false },
  warnings: [],
  others_since_last: { count: 0, from_cursor: 'w:0', origins: [] },
});
const statusReply = (...states: object[]) => envelope({ targets: states, others_since_last: { count: 0, from_cursor: 'w:0', origins: [] } });
const deferred = <T,>() => {
  let res!: (v: T) => void;
  let rej!: (e: unknown) => void;
  const p = new Promise<T>((a, b) => {
    res = a;
    rej = b;
  });
  return { p, res, rej };
};
const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
};
/** the replayed pre-connect snapshot of the seeded unbound panel's entry: core items only, no uds.snapshot */
const staleReplay = (hh: Harness, through: string, items: object[] = []) => {
  hh.net.subs[0]!.h.onSnapshotPart?.({
    sub: 'sub-seed',
    n: 1,
    total: 1,
    events: [],
    items: [{ domain: 'core', kind: 'core.snapshot', state: { stop: { latched: false } } }, ...items],
  } as never);
  hh.snapshotEnd(through);
};
const q = (sel: string) => h!.host.querySelector<HTMLElement>(sel)!;
const statusCalls = (hh: Harness) => hh.net.calls.filter((c) => c.method === 'uds.status');
const sends = (hh: Harness) => hh.net.calls.filter((c) => c.method === 'uds.request' && !(c.params as { dry_run?: boolean }).dry_run);

describe('uds.console — bound panel mounted after connect restores native state', () => {
  it('stale replayed snapshot + uds.status → header up, DoIP route, overview, P2 (one read-only call, no connect)', async () => {
    h = mountPanel('uds.console', { targets: 'qa-uds' });
    const d = deferred<unknown>();
    h.net.responders.set('uds.status', () => d.p);
    expect(statusCalls(h)).toHaveLength(0); // waits for the native snapshot first
    staleReplay(h, '300'); // seed subscribed before the connect (uds.state seq 323)
    expect(statusCalls(h)).toEqual([{ method: 'uds.status', params: {} }]);
    expect(q('.link-st').textContent).toBe('연결 안 됨'); // never inferred from the bound target name
    d.res(statusReply(QA_STATE, { ...QA_STATE, target: 'other-ecu' }));
    await flush();
    h.frame();
    expect(q('.link-st').textContent).toBe('연결됨');
    expect(q('.route-sum').textContent).toBe('doip · DoIP 0x1001');
    expect(q('.conn-ov').textContent).toContain('qa-uds · doip');
    expect(q('.conn-ov').textContent).not.toContain('other-ecu'); // out of scope
    expect(q('.sess-chip').textContent).toContain('P2 50/5000');
    expect(q('.sess-chip').textContent).toContain('default');
    // one-shot: more frames/snapshots never poll
    h.frame();
    staleReplay(h, '400', [{ domain: 'uds', kind: 'uds.snapshot', state: { kind: 'uds.state', payload: QA_STATE } }]);
    expect(statusCalls(h)).toHaveLength(1);
    expect(h.net.calls.filter((c) => c.method === 'uds.connect')).toHaveLength(0);
    expect(sends(h)).toHaveLength(0);
  });

  it('a live link-down received while uds.status is in flight wins over the delayed reply', async () => {
    h = mountPanel('uds.console', { targets: 'qa-uds' });
    const d = deferred<unknown>();
    h.net.responders.set('uds.status', () => d.p);
    staleReplay(h, '300');
    h.net.emitControl(ev({ seq: '330', kind: 'uds.state', channel: 'qa-uds', payload: { ...QA_STATE, link: { state: 'down', cause: 'tcp_reset' } } }));
    h.frame();
    expect(q('.link-st').textContent).toBe('끊김 (tcp_reset)');
    d.res(statusReply(QA_STATE)); // computed before the drop, delivered after it
    await flush();
    h.frame();
    expect(q('.link-st').textContent).toBe('끊김 (tcp_reset)');
  });

  it('after hydration, newer live control states win and a fresh native snapshot replaces it', async () => {
    h = mountPanel('uds.console', { targets: 'qa-uds' });
    h.net.responders.set('uds.status', () => statusReply(QA_STATE));
    staleReplay(h, '300');
    await flush();
    h.frame();
    expect(q('.link-st').textContent).toBe('연결됨');
    // older backlog/control state (seq ≤ what the panel had seen when it asked) cannot override the reply
    h.net.emitControl(ev({ seq: '299', kind: 'uds.state', channel: 'qa-uds', payload: { ...QA_STATE, link: { state: 'connecting' } } }));
    h.frame();
    expect(q('.link-st').textContent).toBe('연결됨');
    // a newer live link down does
    h.net.emitControl(ev({ seq: '340', kind: 'uds.state', channel: 'qa-uds', payload: { ...QA_STATE, link: { state: 'down', cause: 'tcp_reset' } } }));
    h.frame();
    expect(q('.link-st').textContent).toBe('끊김 (tcp_reset)');
    expect(q('.conn-ov').textContent).toContain('활성 연결 없음');
    // resync → fresh server snapshot with the reconnected generation
    h.net.subs[0]!.h.onResync?.('reconnect');
    staleReplay(h, '400', [{ domain: 'uds', kind: 'uds.snapshot', state: { kind: 'uds.state', payload: { ...QA_STATE, generation: 2 } } }]);
    expect(q('.link-st').textContent).toBe('연결됨');
    expect(statusCalls(h)).toHaveLength(1);
  });

  it('a native snapshot that already carries the state is kept; the reply only confirms the same truth', async () => {
    h = mountPanel('uds.console', { targets: 'qa-uds' });
    const d = deferred<unknown>();
    h.net.responders.set('uds.status', () => d.p);
    staleReplay(h, '323', [{ domain: 'uds', kind: 'uds.snapshot', state: { kind: 'uds.state', payload: QA_STATE } }]);
    expect(q('.link-st').textContent).toBe('연결됨');
    h.net.emitControl(ev({ seq: '324', kind: 'uds.state', channel: 'qa-uds', payload: { ...QA_STATE, link: { state: 'down', cause: 'tcp_reset' } } }));
    d.res(statusReply(QA_STATE));
    await flush();
    h.frame();
    expect(q('.link-st').textContent).toBe('끊김 (tcp_reset)');
  });

  it('failed/empty uds.status leaves the link unreported (no inference from the target name or a response)', async () => {
    h = mountPanel('uds.console', { targets: 'qa-uds' });
    h.net.responders.set('uds.status', () => ({
      schema_version: 1, ok: false, cmd: 'uds.status', meta: {}, warnings: [], others_since_last: { count: 0, from_cursor: 'w:0', origins: [] },
      error: { code: 'not_found', message: 'unknown target' },
    }));
    staleReplay(h, '300');
    await flush();
    h.frame();
    expect(q('.link-st').textContent).toBe('연결 안 됨');
    // a successful R0 response row alone does not make the link "up"
    h.batch([ev({ seq: '400', kind: 'uds.result', request_id: 'uds-1', channel: 'qa-uds', payload: { target: 'qa-uds', sid: '22', write_state: 'done', response_hex: '62F190' } })]);
    expect(q('.link-st').textContent).not.toBe('연결됨');
    expect(h.toasts).toEqual([]);
  });

  it('unbound application never queries uds.status and never auto-connects', async () => {
    h = mountPanel('uds.console');
    h.net.responders.set('uds.status', () => statusReply(QA_STATE));
    staleReplay(h, '300');
    await flush();
    h.frame();
    expect(h.net.calls.filter((c) => c.method === 'uds.status' || c.method === 'uds.connect')).toHaveLength(0);
    expect(q('.link-st').textContent).toBe('연결 안 됨');
  });
});
