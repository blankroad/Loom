// MainNetClient narrow dispatch (REVIEW-FIX F1, F4): a loss event of a merged/covering stream reaches
// only the handles whose domain/channel it concerns (contract 02: a gap names the AFFECTED stream);
// subscription-wide losses stay shared. The initial lease travels inside the `sub` message.

import { describe, expect, it } from 'vitest';
import type { EventsBatch, LoomEvent, SubscribeFilter } from '../proto/provisional';
import type { DeliveryItem } from './budget';
import { MainNetClient, type CorePort } from './client';
import type { CoreIn, CoreOut } from './protocol';
import { batch, ev } from './testing/fixtures';

function setup() {
  const posted: CoreIn[] = [];
  let feed: (m: CoreOut) => void = () => {};
  const port: CorePort = {
    post: (m) => posted.push(m),
    listen: (cb) => (feed = cb),
    terminate: () => {},
  };
  const client = new MainNetClient(port, { clientId: 'ui-t', stopUrl: 'http://x/stop', frames: { request: () => {} } });
  const got = new Map<number, string[]>();
  const open = (filter: SubscribeFilter) => {
    const id = posted.length;
    client.subscribe(filter, { snapshot: false }, {
      onBatch: (b: EventsBatch) => got.get(hid)!.push(...b.events.map((e) => `${e.kind}@${e.domain}/${e.channel ?? '*'}`), ...(b.display_gap ? ['display_gap'] : [])),
    });
    const hid = (posted[id] as Extract<CoreIn, { t: 'sub' }>).hid;
    got.set(hid, []);
    return hid;
  };
  const deliver = (items: DeliveryItem[]) => feed({ t: 'deliver', items });
  return { client, posted, got, open, deliver };
}

const gap = (seq: number, kind: string, domain: string, channel: string | null): LoomEvent => {
  const e = ev(seq, kind, { domain, payload: { count: '3', cause: 'driver_overrun' } });
  if (channel === null) delete e.channel;
  else e.channel = channel;
  return e;
};

describe('MainNetClient narrow gap scoping (F1)', () => {
  it('a merged can/[a,b] stream: source.gap on a reaches a only; domain-wide and subscription-wide gaps reach both', () => {
    const t = setup();
    const a = t.open({ domains: ['can'], channels: ['a'] });
    const b = t.open({ domains: ['can'], channels: ['b'] });
    // mux merged both into one union subscription (eid 7) and bound them narrowly
    t.deliver([
      { k: 'bind', eid: 7, hid: a, narrow: true },
      { k: 'bind', eid: 7, hid: b, narrow: true },
    ]);
    t.deliver([
      {
        k: 'batch',
        eid: 7,
        batch: batch('s1', [
          ev(1, 'can.frame', { channel: 'a' }),
          gap(2, 'source.gap', 'can', 'a'), // own (a) / foreign (b)
          ev(3, 'can.frame', { channel: 'b' }),
          gap(4, 'record.gap', 'can', null), // domain-wide: both
          gap(5, 'display.gap', 'display', null), // hub's subscription-level loss: both
          gap(6, 'display.gap', 'core', null), // hub subscription-start resync gap: both
        ]),
      },
    ]);
    expect(t.got.get(a)).toEqual(['can.frame@can/a', 'source.gap@can/a', 'record.gap@can/*', 'display.gap@display/*', 'display.gap@core/*']);
    // negative control: the old predicate (`|| kind.endsWith('.gap')`) also gave b `source.gap@can/a`
    expect(t.got.get(b)).toEqual(['can.frame@can/b', 'record.gap@can/*', 'display.gap@display/*', 'display.gap@core/*']);
  });

  it('a cross-domain union {can,psu}: a CAN overrun never reaches PSU; batch display_gap stays shared; kinds are ignored for own gaps', () => {
    const t = setup();
    const can = t.open({ domains: ['can'], channels: ['a'], kinds: ['can.frame'] });
    const psu = t.open({ domains: ['psu'], channels: ['p0'] });
    t.deliver([
      { k: 'bind', eid: 3, hid: can, narrow: true },
      { k: 'bind', eid: 3, hid: psu, narrow: true },
    ]);
    t.deliver([{ k: 'batch', eid: 3, batch: batch('s1', [gap(10, 'source.gap', 'can', 'a'), gap(11, 'source.gap', 'psu', 'p1')]) }]);
    expect(t.got.get(can)).toEqual(['source.gap@can/a']); // own gap despite kinds:[can.frame]
    expect(t.got.get(psu)).toEqual([]); // neither the CAN gap nor another PSU channel's gap; no empty batch
    // the subscription's own loss summary is shared by every member (an empty-event batch still delivered)
    t.deliver([{ k: 'batch', eid: 3, batch: { ...batch('s1', []), display_gap: { count: 4 } } as unknown as EventsBatch }]);
    expect(t.got.get(can)!.at(-1)).toBe('display_gap');
    expect(t.got.get(psu)).toEqual(['display_gap']);
  });

  it('a non-narrow (exact) handle still receives every event of its subscription unchanged', () => {
    const t = setup();
    const a = t.open({ domains: ['can'], channels: ['a'] });
    t.deliver([{ k: 'bind', eid: 1, hid: a, narrow: false }]);
    t.deliver([{ k: 'batch', eid: 1, batch: batch('s1', [gap(1, 'source.gap', 'can', 'zz')]) }]);
    expect(t.got.get(a)).toEqual(['source.gap@can/zz']);
  });
});

describe('MainNetClient initial lease (F4)', () => {
  it('a hidden open carries active:false inside the single sub message (no follow-up active message)', () => {
    const t = setup();
    const before = t.posted.length;
    t.client.subscribe({ domains: ['psu'] }, { snapshot: true, active: false }, { onBatch: () => {} });
    const msgs = t.posted.slice(before);
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toMatchObject({ t: 'sub', opts: { snapshot: true, active: false } });
    // the worker transport structured-clones the message: the flag survives (worker/inline parity)
    expect(structuredClone(msgs[0])).toEqual(msgs[0]);
  });
});
