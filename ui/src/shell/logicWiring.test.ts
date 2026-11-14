// Shell wiring for the logic domain (explorer views, channel state words, history retention, preset).
import { describe, expect, it } from 'vitest';
import { availableSlot, retainsHistoryAfterClose } from './dock/activeWorkspace';
import { presetForChannels } from './routes';
import { readChannelSignal, viewsForChannel } from '../store/channels';
import { ev } from '../store/testing/fakeNet';

const CAP = 'lc-1';

describe('logic shell wiring', () => {
  it('explorer: running capture is open, stopped/failed closed (history kept), views offered', () => {
    const sig = (st: string) => readChannelSignal(ev({ kind: 'logic.capture_state', domain: 'logic', channel: CAP, payload: { capture_id: CAP, state: st } }))!.state;
    expect(sig('starting')).toBe('open');
    expect(sig('running')).toBe('open');
    expect(sig('stopped')).toBe('closed');
    expect(sig('failed')).toBe('closed');
    expect(viewsForChannel('logic', CAP)).toEqual([{ panel: 'logic.analyzer', label: '파형 · 디코드', props: { channel: CAP } }]);
    expect(retainsHistoryAfterClose('logic')).toBe(true);
    const slot = { panel: 'logic.analyzer', props: { channel: CAP } };
    expect(availableSlot(slot, [{ domain: 'logic', channel: CAP, state: 'closed' }])).toEqual(slot);
    expect(availableSlot({ panel: 'logic.analyzer', props: {} }, [])).toEqual({ panel: 'logic.analyzer', props: {} });
  });

  it('logic preset only opens active captures', () => {
    const p = presetForChannels('logic', [{ domain: 'logic', channel: 'a', state: 'open' }, { domain: 'logic', channel: 'b', state: 'closed' }]);
    expect(p.slots).toEqual([{ panel: 'logic.analyzer', props: { channel: 'a' } }]);
  });
});
