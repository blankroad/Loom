// Shell wiring for the video domain (explorer views, channel state words, history retention, preset, standalone workbench).
import { describe, expect, it } from 'vitest';
import { availableSlot, retainsHistoryAfterClose } from './dock/activeWorkspace';
import { parseRoute, presetForChannels, SERVICE_LABELS } from './routes';
import { readChannelSignal, viewsForChannel } from '../store/channels';
import { ev } from '../store/testing/fakeNet';
import { getPanel } from '../sdk/registry';
import '../panels/video/videoPanel';

const CAP = 'v0123456789ab';

describe('video shell wiring', () => {
  it('capture_state maps to open/closed; record_state never changes the channel', () => {
    const sig = (st: string) => readChannelSignal(ev({ kind: 'video.capture_state', domain: 'video', channel: CAP, payload: { capture_id: CAP, state: st } }))!.state;
    for (const s of ['opening', 'live', 'stalled', 'stopping']) expect(sig(s)).toBe('open');
    for (const s of ['closed', 'failed']) expect(sig(s)).toBe('closed');
    expect(readChannelSignal(ev({ kind: 'video.record_state', domain: 'video', channel: CAP, payload: { capture_id: CAP, state: 'failed', recording_id: 'rec-x-1' } }))).toBeNull();
    expect(viewsForChannel('video', CAP)).toEqual([{ panel: 'video.workbench', label: '영상 · 상태', props: { channel: CAP } }]);
    expect(retainsHistoryAfterClose('video')).toBe(true);
  });

  it('standalone workbench is always available; pinned one follows capture history', () => {
    expect(availableSlot({ panel: 'video.workbench', props: {} }, [])).toEqual({ panel: 'video.workbench', props: {} });
    const pinnedSlot = { panel: 'video.workbench', props: { channel: CAP } };
    expect(availableSlot(pinnedSlot, [{ domain: 'video', channel: CAP, state: 'closed' }])).toEqual(pinnedSlot);
  });

  it('preset opens only active captures; service route exists', () => {
    const p = presetForChannels('video', [{ domain: 'video', channel: 'a', state: 'open' }, { domain: 'video', channel: 'b', state: 'closed' }]);
    expect(p.slots).toEqual([{ panel: 'video.workbench', props: { channel: 'a' } }]);
    expect(SERVICE_LABELS.video).toBe('비디오 캡처');
    expect(parseRoute('#/s/video')).toMatchObject({ service: 'video' });
  });

  it('panel manifest: open/snapshot/record need operate; close/record stop are watch safety actions', () => {
    const m = getPanel('video.workbench')!.manifest;
    const right = Object.fromEntries(m.actions.map((a) => [a.id, a.right]));
    expect(right).toEqual({ open: 'operate', snapshot: 'operate', record_start: 'operate', record_stop: 'watch', close: 'watch' });
    expect(m.actions.filter((a) => a.safetyDirection === 'reduce').map((a) => a.id).sort()).toEqual(['close', 'record_stop']);
    expect(m.media).toEqual({ prefixes: ['/api/v1/video/'] });
    expect(m.topics[0]!.filter({}).kinds).toEqual(['video.capture_state', 'video.health', 'video.snapshot', 'video.record_state']);
  });
});
