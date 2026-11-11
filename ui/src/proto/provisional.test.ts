import { describe, expect, it } from 'vitest';
import { isControlEvent } from './provisional';

// Mirrors loom-proto is_priority_kind (core_events.rs).
describe('isControlEvent', () => {
  it('routes core priority kinds, gaps and test/run domains to the control path', () => {
    for (const kind of ['core.latch', 'core.activity', 'core.mark', 'display.gap', 'source.gap', 'ui.focus_request', 'test.step_started', 'run.state'])
      expect(isControlEvent({ kind, payload: {} })).toBe(true);
  });
  it('routes every write stage (not only final ones)', () => {
    expect(isControlEvent({ kind: 'uds.request', payload: { write_state: 'sent' } })).toBe(true);
    expect(isControlEvent({ kind: 'uart.write', payload: { write_state: 'requested' } })).toBe(true);
  });
  it('routes state-like domain events', () => {
    for (const kind of ['uds.state', 'can.channel_state', 'uart.error', 'eth.capture_state', 'dlt.timeout', 'uds.late', 'can.bus_error'])
      expect(isControlEvent({ kind, payload: {} })).toBe(true);
  });
  it('keeps plain data on the data path', () => {
    for (const kind of ['can.frame', 'uart.data', 'eth.pkt', 'core.heartbeat'])
      expect(isControlEvent({ kind, payload: {} })).toBe(false);
  });
});
