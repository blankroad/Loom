// Mock bench PSU (SimPsu) for psu.gauge — p1b-opus.md §5 item 4, _xteam/explore-ui.md §psu.gauge.
//   psu.state {set_v, set_i, out, prot, mode} + record metrics[{voltage, current, power}] at 1 Hz
//   psu.out{channel, state:'on'|'off'}  · psu.set{channel, v?, i?}  → write rows `psu.cmd`
//   (request_id + write_state) and a fresh psu.state
//   source.gap{cause:"scpi_no_response"} on demand (and every `gapEveryS` seconds when set)
// Safety (A3): `psu.out{state:'off'}` is the emergency OFF — any right (server: watch), no
// confirm, allowed during STOP for viewers. STOP latches agent writes, never the output.
// Bounds (C2): one ticker per instance; no buffers.

import type { Hub } from './hub';
import type { Actor, Sim } from './sim';

export interface PsuState {
  set_v: number;
  set_i: number;
  out: boolean;
  prot: string;
  mode: string;
}

export class PsuSim {
  readonly channel: string;
  state: PsuState = { set_v: 13.5, set_i: 2.0, out: true, prot: 'ok', mode: 'CV' };
  private tick: NodeJS.Timeout | null = null;
  private n = 0;

  constructor(
    private readonly hub: Hub,
    private readonly sim: Sim,
    channel = 'bench',
  ) {
    this.channel = channel;
  }

  private publishState(origin: Actor['origin'] | 'device' = 'device', clientId?: string): void {
    const s = this.state;
    const v = s.out ? s.set_v - 0.01 + (this.n % 3) * 0.005 : 0;
    const i = s.out ? 0.35 + (this.n % 5) * 0.01 : 0;
    this.n++;
    this.hub.publish({
      domain: 'psu',
      channel: this.channel,
      kind: 'psu.state',
      origin,
      ...(clientId ? { client_id: clientId } : {}),
      payload: { ...s },
      metrics: [
        { name: 'voltage', value: v, unit: 'V' },
        { name: 'current', value: i, unit: 'A' },
        { name: 'power', value: v * i, unit: 'W' },
      ],
    });
  }

  start(periodMs = 1_000): void {
    this.tick ??= setInterval(() => this.publishState(), periodMs);
  }

  stop(): void {
    if (this.tick) clearInterval(this.tick);
    this.tick = null;
  }

  /** SCPI no-response window: a measurement loss the plot must show as a break */
  gap(): void {
    this.hub.publish({ domain: 'psu', channel: this.channel, kind: 'source.gap', origin: 'device', payload: { count: 'unknown', cause: 'scpi_no_response' } });
  }

  /** what STOP leaves running (core.latch.outputs_held) */
  held(): { domain: string; channel: string; label: string; state: string }[] {
    return this.state.out ? [{ domain: 'psu', channel: this.channel, label: `PSU ${this.channel} 출력`, state: 'on' }] : [];
  }

  async out(a: Actor, p: { channel?: string; state?: string }): Promise<Record<string, unknown>> {
    const on = p.state === 'on';
    return this.sim.pipeline(a, { domain: 'psu', channel: this.channel, kind: 'psu.cmd', risk: on ? 'R3' : 'R1', payload: { cmd: 'out', state: on ? 'on' : 'off', summary: `OUTP ${on ? 'ON' : 'OFF'}` } }, async () => {
      this.state = { ...this.state, out: on };
      this.publishState(a.origin, a.clientId);
      return { final: 'confirmed' };
    });
  }

  async set(a: Actor, p: { channel?: string; v?: number; i?: number }): Promise<Record<string, unknown>> {
    const v = typeof p.v === 'number' && p.v >= 0 && p.v <= 60 ? p.v : this.state.set_v;
    const i = typeof p.i === 'number' && p.i >= 0 && p.i <= 20 ? p.i : this.state.set_i;
    return this.sim.pipeline(a, { domain: 'psu', channel: this.channel, kind: 'psu.cmd', risk: 'R2', payload: { cmd: 'set', v, i, summary: `VOLT ${v};CURR ${i}` } }, async () => {
      this.state = { ...this.state, set_v: v, set_i: i };
      this.publishState(a.origin, a.clientId);
      return { final: 'confirmed' };
    });
  }
}
