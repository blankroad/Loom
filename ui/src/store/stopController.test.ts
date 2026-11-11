import { describe, expect, it } from 'vitest';
import { ShellControlStore } from './shellControl';
import { STOP_ACK_TIMEOUT_MS, StopController, stopView, type Timers } from './stopController';
import { ev, FakeNet } from './testing/fakeNet';

class ManualTimers implements Timers {
  private q: { at: number; cb: () => void; id: number }[] = [];
  private t = 0;
  private n = 0;
  setTimeout(cb: () => void, ms: number) {
    const id = ++this.n;
    this.q.push({ at: this.t + ms, cb, id });
    return id;
  }
  clearTimeout(h: unknown) {
    this.q = this.q.filter((x) => x.id !== h);
  }
  advance(ms: number) {
    this.t += ms;
    const due = this.q.filter((x) => x.at <= this.t);
    this.q = this.q.filter((x) => x.at > this.t);
    due.forEach((x) => x.cb());
  }
}

const latchEv = (on: boolean, acked: boolean) =>
  ev({ kind: 'core.latch', payload: { on, missing: acked ? [] : ['can-worker'] } });

describe('stopView', () => {
  const off = { on: false, by: null, origin: null, acked: false, missing: [], persisted: false, jobsStopped: [], approvalsCancelled: 0, uncancellable: [], outputsHeld: [] };
  it('never shows locked without a server ack', () => {
    expect(stopView(off, 'requesting')).toBe('requesting');
    expect(stopView({ ...off, on: true }, 'idle')).toBe('requesting');
    expect(stopView({ ...off, on: true, acked: true }, 'idle')).toBe('locked');
    expect(stopView(off, 'unconfirmed')).toBe('unconfirmed');
    expect(stopView(off, 'idle')).toBe('idle');
  });
});

describe('StopController', () => {
  const setup = () => {
    const net = new FakeNet();
    const shell = new ShellControlStore();
    const timers = new ManualTimers();
    const stop = new StopController(net, shell, timers);
    net.onControl((e) => shell.applyControl(e, 0));
    return { net, shell, timers, stop };
  };

  it('requesting → locked on core.latch{on, acked}', async () => {
    const { net, stop } = setup();
    net.open();
    const p = stop.requestStop();
    expect(stop.phase).toBe('requesting'); // optimistic, same tick
    await p;
    expect(net.calls[0]?.method).toBe('core.stop');
    net.emitControl(latchEv(true, true));
    expect(stop.phase).toBe('locked');
  });

  it('→ unconfirmed when no ack within 2 s; retry goes back to requesting', async () => {
    const { net, stop, timers } = setup();
    net.open();
    await stop.requestStop();
    timers.advance(STOP_ACK_TIMEOUT_MS - 1);
    expect(stop.phase).toBe('requesting');
    timers.advance(1);
    expect(stop.phase).toBe('unconfirmed');
    await stop.requestStop();
    expect(stop.phase).toBe('requesting');
  });

  it('latched but not acked stays "requesting" — no success without ack', async () => {
    const { net, stop } = setup();
    net.open();
    await stop.requestStop();
    net.emitControl(latchEv(true, false));
    expect(stop.phase).toBe('requesting');
  });

  it('uses HTTP when the WS is not open', async () => {
    const { net, stop } = setup();
    net.setState({ status: 'reconnecting', attempt: 1, retryInMs: 500, reason: 'closed' });
    await stop.requestStop();
    expect(net.httpStops).toBe(1);
    expect(net.calls).toHaveLength(0);
    expect(stop.lastVia).toBe('http');
  });

  it('falls back to HTTP when the RPC throws', async () => {
    const { net, stop } = setup();
    net.open();
    net.responders.set('core.stop', () => {
      throw new Error('socket closed');
    });
    await stop.requestStop();
    expect(net.httpStops).toBe(1);
  });
});
