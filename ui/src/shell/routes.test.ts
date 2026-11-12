import { expect, it } from 'vitest';
import { servicePresetForChannels, type WorkspaceChannel } from './routes';

it('opens CAN service in Receive/Transmit while UDS companion keeps chronological Trace', () => {
  const channels: WorkspaceChannel[] = [
    { domain: 'can', channel: 'pt', state: 'open' },
    { domain: 'can', channel: 'closed', state: 'closed' },
    { domain: 'uds', channel: 'gw', state: 'open' },
    { domain: 'uart', channel: 'mcu', state: 'open' },
  ];
  const standalone = servicePresetForChannels('can', channels);
  expect(standalone.slots).toEqual([{ panel: 'can.trace', props: { channel: 'pt', view: 'rxtx' } }]);
  const companion = servicePresetForChannels('uds', channels).slots.find((s) => s.panel === 'can.trace');
  expect(companion?.props).toEqual({ channel: 'pt' });
});
