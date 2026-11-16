// @vitest-environment jsdom
// operator-upgrade C3 — runtime GUI finding (ROOT, SIX daemon 56402): a late-opened finished replay
// (`upqa-eth-c1mid`, daemon state replay_done, generation 1) showed its OWN status chip LIVE with
// [캡처 정지] enabled, while eth.capture.status and the other cards said REPLAY_DONE
// (integration/browser/eth-c1mid-ended-state.json). Within one generation the worker goes
// starting → live → terminal and never back; the eth.capture.status reply (no event seq) is the
// authoritative current state. FUNCTIONS.md FC3.1–FC3.3. No fake event seq is ever made up.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { supersedesCapture, TERMINAL_CAPTURE_STATES } from './ethModel';

describe('supersedesCapture (FC3.1)', () => {
  it('never moves backwards within a generation; newer generation wins; older loses', () => {
    expect([...TERMINAL_CAPTURE_STATES].sort()).toEqual(['error', 'permission_denied', 'replay_done', 'stopped']);
    // nothing known yet / payload without a state
    expect(supersedesCapture(null, null, 'live', 1)).toBe(true);
    expect(supersedesCapture('?', 1, 'live', 1)).toBe(true);
    expect(supersedesCapture('replay_done', 1, undefined, 1)).toBe(true);
    expect(supersedesCapture('replay_done', 1, null, 1)).toBe(true);
    // forward within a generation
    expect(supersedesCapture('starting', 1, 'live', 1)).toBe(true);
    expect(supersedesCapture('live', 1, 'replay_done', 1)).toBe(true);
    expect(supersedesCapture('live', 1, 'LIVE', 1)).toBe(true);
    expect(supersedesCapture('stopped', 1, 'error', 1)).toBe(true);
    // backwards within a generation (the late-open bug)
    expect(supersedesCapture('replay_done', 1, 'live', 1)).toBe(false);
    expect(supersedesCapture('replay_done', 1, 'LIVE', 1)).toBe(false);
    expect(supersedesCapture('stopped', 1, 'starting', 1)).toBe(false);
    expect(supersedesCapture('live', 1, 'starting', 1)).toBe(false);
    expect(supersedesCapture('permission_denied', 1, 'PERMISSION', 1)).toBe(true); // legacy name = same terminal
    // unknown generation on either side = same generation
    expect(supersedesCapture('replay_done', null, 'live', 1)).toBe(false);
    expect(supersedesCapture('replay_done', 1, 'live', null)).toBe(false);
    // generations
    expect(supersedesCapture('replay_done', 1, 'starting', 2)).toBe(true);
    expect(supersedesCapture('live', 2, 'replay_done', 1)).toBe(false);
  });
});

describe('eth.packets — own capture state of a late-opened finished replay (FC3.2/FC3.3)', () => {
  let h: Harness | null = null;
  afterEach(() => {
    h?.dispose();
    h = null;
    vi.useRealTimers();
  });
  const status = () => h!.host.querySelector<HTMLElement>('[data-testid="eth-status"]')!;
  const stopBtn = () => [...h!.host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('캡처 정지'))!;
  const staleSnapshot = (state: string, generation = 1) =>
    h!.net.subs[0]!.h.onSnapshotPart?.({
      sub: 's',
      n: 1,
      total: 1,
      items: [{ domain: 'eth', kind: 'eth.snapshot', state: { kind: 'eth.capture_state', ch: 'c1mid', state, backend: 'replay', iface: 'upqa-eth-mid.pcapng', generation } }],
      events: [],
    } as never);
  const capEv = (seq: number, state: string, generation = 1) =>
    ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'c1mid', seq: String(seq), generation, origin: 'device', payload: { ch: 'c1mid', state, backend: 'replay', iface: 'upqa-eth-mid.pcapng' } });
  const done = { ch: 'c1mid', state: 'replay_done', backend: 'replay', iface: 'upqa-eth-mid.pcapng', generation: 1, start_cursor: 'e1:seq:10366', start_seq: '10366' };

  it('status reply replay_done corrects a stale LIVE snapshot, and later stale inputs of that generation cannot undo it', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'c1mid' });
    h.net.responders.set('eth.capture.status', () => ({ channels: [done] }));
    staleSnapshot('live');
    h.snapshotEnd('10400');
    h.frame();
    expect(status().textContent).toContain('LIVE'); // what ROOT saw before the periodic status refresh
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(status().dataset.state).toBe('replay_done');
    expect(status().textContent).toContain('REPLAY_DONE');
    expect(status().textContent).not.toContain('LIVE');
    expect(stopBtn().disabled).toBe(true);
    // a stale running state of the same generation, even with a higher seq, cannot undo it
    h.batch([capEv(10500, 'live')]);
    h.net.emitControl(capEv(10501, 'live'));
    h.frame();
    expect(status().dataset.state).toBe('replay_done');
    expect(stopBtn().disabled).toBe(true);
    // a new generation (restart) is accepted
    h.net.emitControl(capEv(10600, 'live', 2));
    h.frame();
    expect(status().dataset.state).toBe('live');
    expect(stopBtn().disabled).toBe(false);
  });

  it('status reply first, stale snapshot afterwards: stays REPLAY_DONE', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'c1mid' });
    h.net.responders.set('eth.capture.status', () => ({ channels: [done] }));
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(status().dataset.state).toBe('replay_done');
    staleSnapshot('live');
    h.snapshotEnd('10400');
    h.frame();
    expect(status().dataset.state).toBe('replay_done');
    expect(stopBtn().disabled).toBe(true);
  });

  it('a stale status reply (no seq) never regresses a newer control state; an older generation is ignored', async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'c1mid' });
    let reply: Record<string, unknown>[] = [];
    h.net.responders.set('eth.capture.status', () => ({ channels: reply }));
    h.net.emitControl(capEv(20, 'stopped', 3));
    h.frame();
    expect(status().dataset.state).toBe('stopped');
    reply = [{ ...done, state: 'live', generation: 3 }];
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(status().dataset.state).toBe('stopped');
    reply = [{ ...done, state: 'live', generation: 2 }];
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(status().dataset.state).toBe('stopped');
    // a reply without a state changes nothing; start_cursor handling stays as before
    reply = [{ ch: 'c1mid', start_cursor: 'e1:seq:10366' }];
    await vi.advanceTimersByTimeAsync(5_000);
    h.frame();
    expect(status().dataset.state).toBe('stopped');
  });
});
