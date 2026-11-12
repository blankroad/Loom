// SimPsu fixtures: 60 s at 1 Hz with a SCPI no-response gap at t = 30 s.
import type { LoomEvent } from '../../sdk';

export function rec(kind: string, seq: number, tS: number, payload: Record<string, unknown>, metrics?: LoomEvent['metrics']): LoomEvent {
  const mono = BigInt(Math.round(tS * 1000)) * 1_000_000n;
  return {
    schema_version: 1,
    session_id: 's-psu',
    domain: 'psu',
    channel: 'bench',
    kind,
    epoch: '1',
    seq: String(seq),
    producer_id: 'psu',
    producer_epoch: '1',
    source_seq: String(seq),
    ts: { mono_ns: String(mono), wall_ns: String(1_762_770_000_000_000_000n + mono), clock_id: 'host', source: 'device' },
    origin: 'device',
    env: 'sim',
    payload,
    ...(metrics ? { metrics } : {}),
  };
}

export function psuFixture(seconds = 60): LoomEvent[] {
  const out: LoomEvent[] = [rec('psu.state', 1, 0, { set_v: 13.5, set_i: 2.0, out: true, prot: 'ok', mode: 'CV' })];
  for (let s = 1; s <= seconds; s++) {
    if (s === 30) {
      out.push(rec('source.gap', 1 + s, s, { count: 'unknown', cause: 'scpi_no_response' }));
      continue;
    }
    const v = 13.49 + (s % 3) * 0.005;
    const i = 0.35 + (s % 5) * 0.01;
    out.push(rec('psu.state', 1 + s, s, {}, [
      { name: 'voltage', value: v, unit: 'V' },
      { name: 'current', value: i, unit: 'A' },
      { name: 'power', value: v * i, unit: 'W' },
    ]));
  }
  return out;
}
