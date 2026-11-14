import { describe, expect, it } from 'vitest';
import { activePresets, presetForChannels, type WorkspaceChannel } from '../routes';
import { activeLayout, availableSlot } from './activeWorkspace';
import { leaves, presetToLayout } from './layoutModel';

const domains = ['uart', 'can', 'eth', 'uds', 'psu'];
const channel = (domain: string, alias = `${domain}-real`): WorkspaceChannel => ({ domain, channel: alias, state: 'open' });

describe('actual OPEN workspace', () => {
  for (let mask = 1; mask < 32; mask++) {
    const subset = domains.filter((_, i) => mask & (1 << i));
    it(`resolves ${subset.join('+')} with multiple real aliases and eligible presets`, () => {
      const active = subset.flatMap((d) => [channel(d), channel(d, `${d}-second`)]);
      const known: WorkspaceChannel[] = [...active, ...domains.flatMap((d) => [
        { ...channel(d, 'closed-history'), state: 'closed' as const },
        { ...channel(d, 'unknown-history'), state: 'unknown' as const },
      ])];
      const defaults = presetForChannels('m1-default', known);
      expect(defaults.slots).toHaveLength(active.length);
      expect(defaults.slots.map((s) => s.props.channel ?? s.props.targets)).toEqual(active.map((c) => c.channel));
      expect(new Set(defaults.slots.map((s) => s.panel.split('.')[0]))).toEqual(new Set(subset));
      const presets = activePresets(known);
      expect(presets.some((p) => p.name === 'uart-only')).toBe(subset.includes('uart'));
      expect(presets.some((p) => p.name === 'can-uds')).toBe(subset.includes('can') && subset.includes('uds'));
      expect(presets.some((p) => p.name === 'eth')).toBe(subset.includes('eth'));
      for (const domain of ['can', 'uds', 'psu']) expect(presets.some((p) => p.name === `${domain}-only`)).toBe(subset.includes(domain));
      expect(presets.some((p) => p.name === 'test')).toBe(false);
      for (const p of presets) for (const s of p.slots) expect(active.some((c) => c.channel === (s.props.channel ?? s.props.targets))).toBe(true);
    });
  }
  it('direct combo routes retain only matching actual domains; test never invents PSU', () => {
    expect(presetForChannels('can-uds', [channel('can'), channel('eth')]).slots).toEqual([{ panel: 'can.trace', props: { channel: 'can-real' } }]);
    expect(presetForChannels('test', [], false, true).slots).toEqual([{ panel: 'shell.test', props: {} }]);
    expect(activePresets([], true).find((p) => p.name === 'test')?.slots).toEqual([{ panel: 'shell.test', props: {} }]);
  });
  it('sanitizes only the live projection and restores saved unavailable panes on reopen', () => {
    const source = presetToLayout(presetForChannels('m1-default', domains.map((d) => channel(d))));
    const original = JSON.stringify(source);
    const projected = activeLayout(source, [channel('uart')])!;
    expect(leaves(projected.root)).toEqual([{ panel: 'uart.terminal', props: { channel: 'uart-real' } }]);
    expect(JSON.stringify(source)).toBe(original);
    expect(activeLayout(source, [])).toBeNull();
    expect(activeLayout(source, domains.map((d) => channel(d)))).toEqual(source);
  });
  it('filters mixed UDS targets and preserves the selected surviving tab', () => {
    const source = presetToLayout({ name: 'saved', label: '', grid: 'stack', slots: [] });
    source.root = { tabs: [
      { panel: 'can.trace', props: { channel: 'gone' } },
      { panel: 'uds.console', props: { targets: 'ecu-a,ecu-b' } },
      { panel: 'uart.trace', props: { channel: 'serial' } },
    ], active: 2 };
    const live = activeLayout(source, [channel('uds', 'ecu-b'), channel('uart', 'serial')])!;
    expect(live.root).toEqual({ tabs: [
      { panel: 'uds.console', props: { targets: 'ecu-b' } },
      { panel: 'uart.trace', props: { channel: 'serial' } },
    ], active: 1 });
    expect(leaves(source.root)[1]!.props.targets).toBe('ecu-a,ecu-b');
  });
});

describe('closed channels with retained history', () => {
  const eth = { panel: 'eth.packets', props: { channel: 'cap' } };
  it('keeps an ETH capture view after a normal stop but prunes unknown/missing ETH and closed live-only domains', () => {
    expect(availableSlot(eth, [{ domain: 'eth', channel: 'cap', state: 'closed' }])).toEqual(eth);
    expect(availableSlot(eth, [{ domain: 'eth', channel: 'cap', state: 'unknown' }])).toBeNull();
    expect(availableSlot(eth, [])).toBeNull();
    expect(availableSlot({ panel: 'can.trace', props: { channel: 'b' } }, [{ domain: 'can', channel: 'b', state: 'closed' }])).toBeNull();
  });
  it('keeps closed/disconnected UART terminal+trace views (history), prunes unknown/missing UART', () => {
    const term = { panel: 'uart.terminal', props: { channel: 'p' } };
    const trace = { panel: 'uart.trace', props: { channel: 'p' } };
    const closed: WorkspaceChannel[] = [{ domain: 'uart', channel: 'p', state: 'closed' }];
    expect(availableSlot(term, closed)).toEqual(term);
    expect(availableSlot(trace, closed)).toEqual(trace);
    expect(availableSlot(term, [{ domain: 'uart', channel: 'p', state: 'unknown' }])).toBeNull();
    expect(availableSlot(term, [])).toBeNull();
    expect(availableSlot(term, [{ domain: 'uart', channel: 'other', state: 'closed' }])).toBeNull();
    // after STOP + external disconnect the open tabs survive the live projection (explicit reopen also works)
    const layout = presetToLayout({ name: 'x', label: 'x', grid: 'stack', slots: [term, trace] });
    expect(leaves(activeLayout(layout, closed)!.root)).toEqual([term, trace]);
  });
  it('keeps a UDS console for a target after link loss (history) and prunes unknown/missing targets', () => {
    const cons = { panel: 'uds.console', props: { targets: 'ext_doip' } };
    expect(availableSlot(cons, [{ domain: 'uds', channel: 'ext_doip', state: 'closed' }])).toEqual(cons);
    expect(availableSlot(cons, [{ domain: 'uds', channel: 'ext_doip', state: 'unknown' }])).toBeNull();
    expect(availableSlot(cons, [])).toBeNull();
    expect(availableSlot({ panel: 'uds.console', props: { targets: 'ext_doip,gone' } }, [{ domain: 'uds', channel: 'ext_doip', state: 'closed' }]))
      .toEqual({ panel: 'uds.console', props: { targets: 'ext_doip' } });
    const known: WorkspaceChannel[] = [{ domain: 'uds', channel: 'ext_doip', state: 'closed' }];
    expect(presetForChannels('uds-only', known).slots).toEqual([]);
    expect(activePresets(known).some((p) => p.name === 'uds-only')).toBe(false);
  });
  it('keeps a disconnected PSU gauge (last confirmed output must stay visible) without auto-opening it', () => {
    const g = { panel: 'psu.gauge', props: { channel: 'ext-bench' } };
    const known: WorkspaceChannel[] = [{ domain: 'psu', channel: 'ext-bench', state: 'closed' }];
    expect(availableSlot(g, known)).toEqual(g);
    expect(availableSlot(g, [])).toBeNull();
    expect(presetForChannels('psu-only', known).slots).toEqual([]);
    expect(activePresets(known).some((p) => p.name === 'psu-only')).toBe(false);
  });
  it('never auto-opens a closed UART through presets or the service list', () => {
    const known: WorkspaceChannel[] = [{ domain: 'uart', channel: 'p', state: 'closed' }];
    expect(presetForChannels('uart-only', known).slots).toEqual([]);
    expect(presetForChannels('m1-default', known).slots).toEqual([]);
    expect(activePresets(known).some((p) => p.name === 'uart-only')).toBe(false);
  });
  it('never auto-opens a stopped capture through presets', () => {
    const known: WorkspaceChannel[] = [{ domain: 'eth', channel: 'cap', state: 'closed' }];
    expect(presetForChannels('m1-default', known).slots).toEqual([]);
    expect(activePresets(known).some((p) => p.name === 'eth')).toBe(false);
  });
});
