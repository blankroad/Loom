import { describe, expect, it } from 'vitest';
import { NetError, RpcClient } from './rpc';
import { ManualTimers } from './testing/fixtures';

function setup(open = true, maxInFlight?: number) {
  const sent: string[] = [];
  const timers = new ManualTimers();
  const rpc = new RpcClient({
    send: (t) => {
      if (!open) return false;
      sent.push(t);
      return true;
    },
    timers,
    ...(maxInFlight ? { maxInFlight } : {}),
  });
  return { rpc, sent, timers, setOpen: (v: boolean) => (open = v) };
}

describe('RpcClient', () => {
  it('matches responses by id, in any order', async () => {
    const { rpc, sent } = setup();
    const a = rpc.call('a');
    const b = rpc.call('b', { x: 1 });
    const ids = sent.map((s) => (JSON.parse(s) as { id: number }).id);
    expect(JSON.parse(sent[1]!)).toMatchObject({ jsonrpc: '2.0', method: 'b', params: { x: 1 } });
    rpc.handleResponse(ids[1]!, 'B', null);
    rpc.handleResponse(ids[0]!, 'A', null);
    await expect(a).resolves.toBe('A');
    await expect(b).resolves.toBe('B');
    expect(rpc.inFlight).toBe(0);
  });

  it('maps JSON-RPC errors to NetError{code:"rpc"} with data.code', async () => {
    const { rpc, sent } = setup();
    const p = rpc.call('uart.input');
    rpc.handleResponse(JSON.parse(sent[0]!).id, null, { code: -32001, message: 'forbidden', data: { code: 'rights' } });
    await expect(p).rejects.toMatchObject({ code: 'rpc', rpcCode: -32001, dataCode: 'rights' });
  });

  it('rejects requests over 64 KiB locally without sending', async () => {
    const { rpc, sent } = setup();
    const p = rpc.call('uart.input', { data_hex: 'a'.repeat(70_000) });
    await expect(p).rejects.toMatchObject({ code: 'request_too_large', maybeSent: false });
    expect(sent).toHaveLength(0);
  });

  it('counts UTF-8 bytes, not UTF-16 units', async () => {
    const { rpc, sent } = setup();
    // 30k 3-byte chars = 90 KB on the wire although .length is 30k
    await expect(rpc.call('core.mark', { label: '가'.repeat(30_000) })).rejects.toMatchObject({ code: 'request_too_large' });
    expect(sent).toHaveLength(0);
  });

  it('times out, and ignores the late response', async () => {
    const { rpc, sent, timers } = setup();
    const p = rpc.call('core.time', {}, 100);
    timers.advance(100);
    await expect(p).rejects.toMatchObject({ code: 'timeout', maybeSent: true });
    expect(rpc.handleResponse(JSON.parse(sent[0]!).id, 'late', null)).toBe(false);
  });

  it('never queues or retries: not_connected when the socket is down', async () => {
    const { rpc, sent } = setup(false);
    await expect(rpc.call('core.stop_all')).rejects.toMatchObject({ code: 'not_connected', maybeSent: false });
    expect(sent).toHaveLength(0);
  });

  it('fails every in-flight call on disconnect and does not re-send them', async () => {
    const { rpc, sent, setOpen } = setup();
    const m = rpc.call('can.tx.send', { id: 0x300 });
    const q = rpc.call('core.safe_state.list');
    rpc.failAll('disconnected', 'ws closed');
    await expect(m).rejects.toBeInstanceOf(NetError);
    await expect(m).rejects.toMatchObject({ code: 'disconnected', maybeSent: true });
    await expect(q).rejects.toMatchObject({ code: 'disconnected' });
    setOpen(true);
    expect(sent).toHaveLength(2); // nothing re-sent after "reconnect"
    expect(rpc.inFlight).toBe(0);
  });

  it('bounds in-flight calls (new call rejected, old ones untouched)', async () => {
    const { rpc } = setup(true, 2);
    const a = rpc.call('a');
    rpc.call('b').catch(() => undefined);
    await expect(rpc.call('c')).rejects.toMatchObject({ code: 'too_many_in_flight' });
    expect(rpc.inFlight).toBe(2);
    rpc.failAll('closed', 'x');
    await expect(a).rejects.toMatchObject({ code: 'closed' });
  });

  it('cancel rejects with aborted and drops the later response', async () => {
    const { rpc } = setup();
    const { id, done } = rpc.start('core.events.query');
    rpc.cancel(id);
    await expect(done).rejects.toMatchObject({ code: 'aborted' });
    expect(rpc.handleResponse(id, 1, null)).toBe(false);
  });
});
