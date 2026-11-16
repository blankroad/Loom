// @vitest-environment jsdom
// operator-upgrade B4a — pure contract: filter combination (ETH-02), stream index space (ETH-03), replay configs (ETH-04).
// Spec tests authored with FUNCTIONS.md F4.1, F4.2, F4.6, F4.7.
import { afterEach, describe, expect, it } from 'vitest';
import { combineFilter, readPkt } from './ethModel';
import { captureParams, IFACE_CFG_KEY, loadConfigs, MAX_REPLAY_PATH, replayLabel, sanitizeConfigs, type IfaceConfig } from './ifaceConfigs';

afterEach(() => localStorage.clear());

describe('combineFilter (F4.2)', () => {
  it('prepares and/or around the current field text', () => {
    expect(combineFilter('', 'udp', '&&')).toBe('udp');
    expect(combineFilter('   ', 'udp', '||')).toBe('udp');
    expect(combineFilter('arp', 'udp.dstport == 65201', '&&')).toBe('(arp) && udp.dstport == 65201');
    expect(combineFilter(' a || b ', 'c', '||')).toBe('(a || b) || c');
  });
});

describe('readPkt streamProto (F4.1)', () => {
  const row = (o: Record<string, unknown>) => readPkt({ seq: '7', ...o }, 'lan1')!;
  it('index space only with a stream number', () => {
    expect(row({ stream: 3, stream_proto: 'udp' }).streamProto).toBe('udp');
    expect(row({ stream: '3', stream_proto: 'tcp' }).streamProto).toBe('tcp');
    expect(row({ stream: 3 }).streamProto).toBeNull();
    expect(row({ stream: 3, stream_proto: 'sctp' }).streamProto).toBeNull();
    expect(row({ stream_proto: 'udp' }).streamProto).toBeNull();
  });
});

const REPLAY: IfaceConfig = { alias: 'rp', iface: 'x.pcapng', backend: 'replay', bpf: '', snaplen: 262144, promisc: false, bufferMib: 64, replay: { path: '/d/x.pcapng', speed: 'max', repeat: false } };

describe('replay interface configs (F4.6, F4.7)', () => {
  it('keeps valid replay configs; drops broken ones; strips stray replay options from live/sim', () => {
    const live = { alias: 'l', iface: 'en0', backend: 'live', bpf: 'port 1', snaplen: 1, promisc: false, bufferMib: 1, replay: REPLAY.replay };
    const out = sanitizeConfigs([
      REPLAY,
      { ...REPLAY, alias: 'r2', speed: undefined, replay: { path: '/d/y.pcap', speed: 2, repeat: true } },
      { ...REPLAY, alias: 'noopts', replay: undefined },
      { ...REPLAY, alias: 'empty', replay: { path: '  ', speed: 'max', repeat: false } },
      { ...REPLAY, alias: 'long', replay: { path: '/' + 'a'.repeat(MAX_REPLAY_PATH), speed: 'max', repeat: false } },
      { ...REPLAY, alias: 'speed3', replay: { path: '/d/x', speed: 3, repeat: false } },
      { ...REPLAY, alias: 'rep', replay: { path: '/d/x', speed: 'max', repeat: 'yes' } },
      { ...REPLAY, alias: 'bpf', bpf: 'port 1' },
      live,
    ]);
    expect(out.map((c) => c.alias)).toEqual(['rp', 'r2', 'bpf', 'l']);
    expect(out[1]!.replay).toEqual({ path: '/d/y.pcap', speed: 2, repeat: true });
    expect(out[2]!.bpf).toBe(''); // BPF is live-only (replay like sim)
    expect('replay' in out[3]!).toBe(false);
    expect(out[3]!.bpf).toBe('port 1');
    localStorage.setItem(IFACE_CFG_KEY, JSON.stringify([REPLAY]));
    expect(loadConfigs()).toEqual([REPLAY]);
  });
  it('capture params of a replay config carry the replay options and no BPF', () => {
    expect(captureParams({ ...REPLAY, bpf: 'tcp' })).toEqual({ iface: 'x.pcapng', ch: 'rp', backend: 'replay', snaplen: 262144, promisc: false, buffer_mib: 64, replay: { path: '/d/x.pcapng', speed: 'max', repeat: false } });
    expect(captureParams({ ...REPLAY, replay: { path: '/d/x', speed: 10, repeat: true } }).replay).toEqual({ path: '/d/x', speed: 10, repeat: true });
    // live/sim unchanged
    expect(captureParams({ alias: 'a', iface: 'en0', backend: 'live', bpf: ' port 13400 ', snaplen: 9, promisc: true, bufferMib: 2 })).toEqual({ iface: 'en0', ch: 'a', backend: 'live', bpf: 'port 13400', snaplen: 9, promisc: true, buffer_mib: 2 });
  });
  it('replayLabel = base name, bounded, never empty', () => {
    expect(replayLabel('/data/logs/run 1.pcapng')).toBe('run 1.pcapng');
    expect(replayLabel('C:\\caps\\a.pcap')).toBe('a.pcap');
    expect(replayLabel('  /a/b.pcap  ')).toBe('b.pcap');
    expect(replayLabel('')).toBe('replay');
    expect(replayLabel('/dir/')).toBe('replay');
    expect(replayLabel('/' + 'x'.repeat(200))).toBe('x'.repeat(128));
  });
});
