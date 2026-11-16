// @vitest-environment jsdom
// operator-upgrade C1 (ETH-RT-03): Loom's own eth.send copy carries `link {domain: 'eth', request_id}`
// (worker), never `tx_request_id`. The panel must project it as the packet's tx request id
// (data-request-id + "NIC 관찰 ✓" on the TX row) without turning it into a UDS link chip.
import { afterEach, describe, expect, it } from 'vitest';
import { mountPanel, writeEv, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { readEthLinkRequestId, readPkt } from './ethModel';
import { fixtureRecord } from './fixtures';

const ethLink = (rid: unknown) => ({ domain: 'eth', request_id: rid, role: 'tx', confidence: 'exact' });

describe('readEthLinkRequestId', () => {
  it('accepts only domain eth with a non-empty string request id', () => {
    expect(readEthLinkRequestId(ethLink('w32'))).toBe('w32');
    expect(readEthLinkRequestId({ domain: 'uds', request_id: 'u1' })).toBeNull();
    expect(readEthLinkRequestId({ request_id: 'w1' })).toBeNull();
    expect(readEthLinkRequestId(ethLink(''))).toBeNull();
    expect(readEthLinkRequestId(ethLink(7))).toBeNull();
    expect(readEthLinkRequestId(null)).toBeNull();
    expect(readEthLinkRequestId('w1')).toBeNull();
  });

  it('readPkt: eth link → txRequestId only; uds link unchanged; legacy tx_request_id wins', () => {
    const base = { seq: '5', proto: 'UDP', len: 66, info: 'x' };
    const e = readPkt({ ...base, link: ethLink('w32') }, 'live')!;
    expect(e.txRequestId).toBe('w32');
    expect(e.link).toBeNull();
    const u = readPkt({ ...base, link: { domain: 'uds', request_id: 'u1', role: 'req', confidence: 'exact' } }, 'live')!;
    expect(u.txRequestId).toBeNull();
    expect(u.link?.requestId).toBe('u1');
    const legacy = readPkt({ ...base, tx_request_id: 'old', link: ethLink('w32') }, 'live')!;
    expect(legacy.txRequestId).toBe('old');
  });
});

describe('eth.packets — own eth.send copy', () => {
  let h: Harness | null = null;
  afterEach(() => {
    h?.dispose();
    h = null;
  });

  it('live pkt with link{domain: eth} gets data-request-id and marks the TX row "NIC 관찰 ✓"', () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.batch([writeEv('w32', 'sent', { seq: 10, kind: 'eth.tx', payload: { kind: 'raw', summary: 'UDP 40000 → 65201' } })]);
    const txRow = () => h!.host.querySelector<HTMLElement>('.eth-tx-host .vt-row[data-request-id="w32"]')!;
    expect(txRow().textContent).toContain('NIC 관찰 —');
    h.batch([
      fixtureRecord(
        'eth.pkt',
        12,
        { proto: 'UDP', len: 66, info: '40000 → 65201 Len=24', attributed_origin: 'agent', link: ethLink('w32') },
        { dir: 'tx' },
      ),
    ]);
    h.frame();
    const pkt = h.host.querySelector<HTMLElement>('.eth-list .vt-row[data-request-id="w32"]');
    expect(pkt).not.toBeNull();
    expect(pkt!.textContent).not.toContain('↔UDS');
    expect(txRow().textContent).toContain('NIC 관찰 ✓');
  });
});
