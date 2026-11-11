// Synthetic eth fixtures for development and tests (tools/mock has no eth yet).
// Real bytes for the DoIP diagnostic request so tree {off,len} ranges are exact:
//   Ethernet II 0..13 · IPv4 14..33 · TCP 34..53 · DoIP hdr 54..61 · SA 62..63 · TA 64..65 · UDS 66..68

import type { LoomEvent, Origin, TreeNode } from '../../sdk';

let seqBase = 0;
function rec(kind: string, seq: number, payload: Record<string, unknown>, extra: Partial<LoomEvent> = {}): LoomEvent {
  seqBase = Math.max(seqBase, seq);
  const mono = BigInt(seq) * 1_000_000n + 12_000_000_000n;
  return {
    schema_version: 1,
    session_id: 's-eth',
    domain: 'eth',
    channel: 'lan1',
    kind,
    epoch: '1',
    seq: String(seq),
    producer_id: 'eth-worker',
    producer_epoch: '1',
    source_seq: String(seq),
    ts: { mono_ns: String(mono), wall_ns: String(1_762_770_000_000_000_000n + mono), clock_id: 'host', source: 'host_read_complete' },
    origin: 'device',
    env: 'sim',
    payload,
    ...extra,
  };
}

export const DIAG_REQ_HEX =
  '001b21aabbcc' + '001b21112233' + '0800' + // Ethernet II
  '45000037' + '00004000' + '40060000' + 'c0a80002' + 'c0a8000a' + // IPv4
  'c3c03458' + '00000091' + '00000001' + '5018ffff' + '00000000' + // TCP 50112 → 13400
  '02fd8001' + '00000007' + // DoIP v2 diag msg, len 7
  '0e801001' + // SA 0x0E80 → TA 0x1001
  '22f190'; // UDS ReadDataByIdentifier F190

export const DIAG_REQ_TREE: TreeNode[] = [
  {
    label: 'Ethernet II, 00:1b:21:11:22:33 → 00:1b:21:aa:bb:cc',
    off: 0,
    len: 14,
    children: [
      { label: 'Destination: 00:1b:21:aa:bb:cc', off: 0, len: 6 },
      { label: 'Source: 00:1b:21:11:22:33', off: 6, len: 6 },
      { label: 'Type: IPv4 (0x0800)', off: 12, len: 2 },
    ],
  },
  {
    label: 'IPv4, 192.168.0.2 → 192.168.0.10',
    off: 14,
    len: 20,
    children: [
      { label: 'Protocol: TCP (6)', off: 23, len: 1 },
      { label: 'Source: 192.168.0.2', off: 26, len: 4 },
      { label: 'Destination: 192.168.0.10', off: 30, len: 4 },
    ],
  },
  {
    label: 'TCP, 50112 → 13400, stream 3, len 15',
    off: 34,
    len: 20,
    children: [
      { label: 'Source port: 50112', off: 34, len: 2 },
      { label: 'Destination port: 13400', off: 36, len: 2 },
      { label: 'Flags: PSH, ACK', off: 47, len: 1 },
    ],
  },
  {
    label: 'DoIP v2 Diagnostic message (0x8001) 0x0E80 → 0x1001',
    off: 54,
    len: 15,
    children: [
      { label: 'Version: 2', off: 54, len: 1 },
      { label: 'Inverse version: 0xfd', off: 55, len: 1 },
      { label: 'Payload type: Diagnostic message (0x8001)', off: 56, len: 2 },
      { label: 'Length: 7', off: 58, len: 4 },
      { label: 'Source address: 0x0E80', off: 62, len: 2 },
      { label: 'Target address: 0x1001', off: 64, len: 2 },
      {
        label: 'UDS 0x22 ReadDataByIdentifier',
        off: 66,
        len: 3,
        children: [
          { label: 'SID: 0x22 ReadDataByIdentifier', off: 66, len: 1 },
          { label: 'DID: 0xF190 VIN', off: 67, len: 2 },
        ],
      },
    ],
  },
];

const L = (o: Record<string, unknown>) => o;

/** a short capture: routing activation, UDS request/response (linked), ARP, TCP retrans, SOME/IP-SD */
export function fixtureEvents(start = 41230): LoomEvent[] {
  const pkt = (seq: number, origin: Origin, p: Record<string, unknown>, dir: 'rx' | 'tx' = 'rx') =>
    rec('eth.pkt', seq, p, { dir, origin: 'device', ...(origin !== 'device' ? {} : {}) });
  return [
    rec('eth.capture_state', start - 1, { state: 'LIVE', iface: 'lo', bpf: 'port 13400 or arp', snaplen: 262144, ts_precision: 'us', drop_stats: 'available', sim: true, started_mono_ns: '12000000000' }),
    pkt(start, 'device', { src: '192.168.0.2', dst: '192.168.0.10', proto: 'DoIP', len: 64, info: 'Routing activation request (0x0005) SA=0x0E80 type=0x00', attributed_origin: 'agent', layers: L({ doip: { type: '0x0005' } }) }, 'tx'),
    pkt(start + 10, 'device', {
      src: '192.168.0.2', dst: '192.168.0.10', proto: 'DoIP/UDS', len: 69, info: 'Diag 0x0E80→0x1001 22 F1 90 ReadDataByIdentifier',
      attributed_origin: 'agent',
      layers: L({ doip: { type: '0x8001' }, uds: { sid: '0x22', did: 'F190' } }),
      link: { domain: 'uds', request_id: 't-000418', role: 'request', confidence: 'exact', target: 'bcm' },
    }, 'tx'),
    pkt(start + 12, 'device', {
      src: '192.168.0.10', dst: '192.168.0.2', proto: 'DoIP/UDS', len: 88, info: 'Diag 0x1001→0x0E80 62 F1 90 "WVWZZZ1KZAW000001"',
      layers: L({ doip: { type: '0x8001' }, uds: { sid: '0x62' } }),
      link: { domain: 'uds', request_id: 't-000418', role: 'response', confidence: 'probable', target: 'bcm' },
    }),
    pkt(start + 20, 'device', { src: '192.168.0.10', dst: '192.168.0.2', proto: 'DoIP/UDS', len: 72, info: 'Diag 0x1001→0x0E80 7F 2E 31 requestOutOfRange', layers: L({ uds: { sid: '0x7f', nrc: '0x31' } }) }),
    pkt(start + 30, 'device', { src: '00:1b:21:aa:bb:cc', dst: 'ff:ff:ff:ff:ff:ff', proto: 'ARP', len: 42, info: 'Who has 192.168.0.2? Tell 192.168.0.10', layers: L({ arp: { opcode: 1 } }) }),
    pkt(start + 40, 'device', { src: '192.168.0.2', dst: '192.168.0.10', proto: 'TCP', len: 66, info: '[TCP Retransmission] 50112 → 13400', layers: L({ tcp: { analysis: { retransmission: true } } }) }),
    pkt(start + 50, 'device', { src: '192.168.0.20', dst: '224.224.224.245', proto: 'SOME/IP-SD', len: 110, info: 'OfferService 0x1234 ttl 3', layers: L({ someipsd: {} }) }),
  ];
}

export function fixtureDetail(ref: string): Record<string, unknown> {
  return { ref, len: DIAG_REQ_HEX.length / 2, bytes: { encoding: 'hex', data: DIAG_REQ_HEX }, tree: DIAG_REQ_TREE };
}

/** `n` complete rows for eth.query (journal-backed) */
export function fixtureQueryRows(n: number, firstSeq = 50_000): Record<string, unknown>[] {
  return Array.from({ length: n }, (_, i) => ({
    ref: `eth:lan1@seq:${firstSeq + i}`,
    seq: String(firstSeq + i),
    ts: { mono_ns: String(13_000_000_000 + i * 1_000), wall_ns: String(1_762_770_013_000_000_000n + BigInt(i * 1_000)), source: 'host_read_complete' },
    dir: 'rx',
    origin: 'device',
    len: 64,
    proto: 'SOME/IP',
    info: `Notification 0x1234.0x8001 #${i}`,
  }));
}

export { rec as fixtureRecord };
