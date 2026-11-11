import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { readChannelSignal, readSnapshotChannel, readSnapshotChannels } from './channels';
import { ev } from './testing/fakeNet';

const udsStates = JSON.parse(readFileSync(new URL('../../../crates/loom-uds/fixtures/state.json', import.meta.url), 'utf8')) as { kind: string; payload: Record<string, unknown> }[];

describe('native interface state payloads', () => {
  it('reads real UDS object links in live events and native/wrapped reducer snapshots', () => {
    for (const fixture of udsStates) {
      expect(readChannelSignal(ev(fixture))).toMatchObject({ domain: 'uds', channel: 'ecu1', state: 'open', generation: 1 });
      expect(readSnapshotChannel({ domain: 'uds', kind: 'uds.snapshot', state: { kind: fixture.kind, payload: fixture.payload } })).toEqual({ domain: 'uds', channel: 'ecu1', state: 'open' });
      expect(readSnapshotChannel(fixture)).toEqual({ domain: 'uds', channel: 'ecu1', state: 'open' });
      expect(readSnapshotChannel({ domain: 'uds', state: fixture.payload })).toEqual({ domain: 'uds', channel: 'ecu1', state: 'open' });
    }
    for (const [state, expected] of [['down', 'closed'], ['connecting', 'unknown']] as const) {
      const payload = { target: 'ecu2', link: { state } };
      expect(readChannelSignal(ev({ kind: 'uds.state', payload }))?.state).toBe(expected);
      expect(readSnapshotChannel({ kind: 'uds.state', payload })?.state).toBe(expected);
    }
  });

  it('reads canonical ETH capture channels and live/replay completion states', () => {
    // crates/loom-eth/src/model.rs CaptureStatePayload and gateway/evidence.rs snapshot.
    for (const [state, expected] of [['live', 'open'], ['LIVE', 'open'], ['stopped', 'closed'], ['replay_done', 'closed'], ['starting', 'unknown'], ['error', 'unknown'], ['permission_denied', 'unknown']] as const) {
      const payload = { ch: 'capture-2', state, backend: 'sim', iface: 'lo' };
      expect(readChannelSignal(ev({ kind: 'eth.capture_state', payload }))).toMatchObject({ domain: 'eth', channel: 'capture-2', state: expected });
      expect(readSnapshotChannel({ kind: 'eth.capture_state', ...payload })).toEqual({ domain: 'eth', channel: 'capture-2', state: expected });
      expect(readSnapshotChannel({ domain: 'eth', state: payload })?.state).toBe(expected);
    }
  });

  it('does not confuse PSU output off with connection closed or invent missing status', () => {
    const payload = { ch: 'bench', set_v: 13.5, set_i: 1, out: false, prot: 'none', mode: 'CV' };
    expect(readChannelSignal(ev({ kind: 'psu.state', payload }))).toMatchObject({ channel: 'bench', state: 'unknown' });
    expect(readSnapshotChannel({ kind: 'psu.state', ch: 'bench', state: payload })).toEqual({ domain: 'psu', channel: 'bench', state: 'unknown' });
    expect(readChannelSignal(ev({ kind: 'psu.state', payload: { ...payload, connected: true } }))?.state).toBe('open');
    expect(readChannelSignal(ev({ kind: 'psu.state', payload: { ...payload, connected: false } }))?.state).toBe('closed');
    expect(readSnapshotChannel({ domain: 'eth', channel: 'lan', state: {} })?.state).toBe('unknown');
    expect(readChannelSignal(ev({ kind: 'psu.write_state', payload: { ...payload, write_state: 'sent' } }))).toBeNull();
  });
});

// Exact envelopes from hub/mod.rs + uart/mirror.rs + can/gateway.rs FixedReducer.
describe('gateway SnapshotItem envelopes', () => {
  it('reads UART nested state and CAN fixed state/payload without inferring connection from frames', () => {
    expect(readSnapshotChannels({ domain: 'uart', channel: 'dut', kind: 'uart.snapshot', state: {
      channel: 'dut', generation: 3, state: { state: 'open' },
    } })).toEqual([{ domain: 'uart', channel: 'dut', state: 'open', generation: 3 }]);
    for (const [state, expected] of [['open', 'open'], ['closed', 'closed'], ['error', 'unknown']] as const) {
      expect(readSnapshotChannels({ domain: 'can', channel: 'ch0', kind: 'can.snapshot', state: {
        kind: 'can.fixed', channel: 'ch0', state: { payload: { state }, generation: 2, seq: '12' },
        ids: [], ids_replaced: 0, channels_replaced: 0,
      } })).toEqual([{ domain: 'can', channel: 'ch0', state: expected, generation: 2 }]);
    }
    expect(readSnapshotChannel({ domain: 'can', channel: 'ch0', kind: 'can.snapshot', state: {
      kind: 'can.fixed', channel: 'ch0', state: null, ids: [], ids_replaced: 0, channels_replaced: 0,
    } })?.state).toBe('unknown');
  });

  it('expands core.channels string/object states and rejects reserved or invalid identities', () => {
    expect(readSnapshotChannels({ domain: 'core', kind: 'core.channels', state: { channels: [
      { domain: 'uart', channel: 'aux', state: 'open', generation: 1, seq: '10', kind: 'uart.state' },
      { domain: 'psu', channel: 'bench', state: { out: false }, generation: 1, seq: '11', kind: 'psu.state' },
      { domain: 'uds', channel: 'ecu', state: { target: 'ecu', link: { state: 'down' } }, generation: 2, seq: '12', kind: 'uds.state' },
      { domain: 'core', channel: 'fake', state: 'open' },
      { domain: 'uart', channel: '../bad', state: 'open' },
    ] } })).toEqual([
      { domain: 'uart', channel: 'aux', state: 'open', generation: 1 },
      { domain: 'psu', channel: 'bench', state: 'unknown', generation: 1 },
      { domain: 'uds', channel: 'ecu', state: 'closed', generation: 2 },
    ]);
    expect(readSnapshotChannels({ domain: 'core', kind: 'core.channels', state: { channels: null } })).toEqual([]);
  });

  it('reads native ETH and PSU SnapshotItems without treating output-off as disconnected', () => {
    expect(readSnapshotChannel({ domain: 'eth', kind: 'eth.snapshot', state: {
      kind: 'eth.capture_state', ch: 'cap', state: 'live', backend: 'sim', iface: 'lo', generation: 1,
    } })).toEqual({ domain: 'eth', channel: 'cap', state: 'open' });
    expect(readSnapshotChannel({ domain: 'psu', kind: 'psu.snapshot', state: {
      kind: 'psu.state', ch: 'bench', state: { out: false, mode: 'CV' }, meas_60s: { samples: 0, last: null },
    } })).toEqual({ domain: 'psu', channel: 'bench', state: 'unknown' });
  });
});
