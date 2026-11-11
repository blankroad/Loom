/// <reference types="node" />
/** Node-only reproducible memo. No browser / rendering / end-to-end claims. */
import process from 'node:process';
import { cpus, platform, arch, totalmem } from 'node:os';
import { performance } from 'node:perf_hooks';
import { decode } from './columns';
import { canPayload, encode, event } from './columns.test-support';
const frames = Array.from({ length: 50 }, (_, batch) => encode({
  name: 'perf-200', delta: true, rows: Array.from({ length: 200 }, (_, row) => {
    const seq = String(batch * 200 + row + 1);
    return event(seq, canPayload(64), { channel: 'ch0', client_id: 'agent:1', dir: 'rx', quality: [] });
  }),
}));
let sink = 0;
function run(mode: 'decode' | 'decode-and-materialize') {
  const start = performance.now();
  for (const frame of frames) {
    const result = decode(frame); if (!result.ok) throw new Error(JSON.stringify(result.error));
    if (mode === 'decode-and-materialize') {
      const rows = result.value.materializeAll(); if (!rows.ok) throw new Error(JSON.stringify(rows.error));
      sink += rows.value.length + rows.value[199]!.seq.length;
    } else {
      // Keep the view observable, without creating any LoomEvent row objects.
      const last = result.value.get('can_id', 199); if (!last.ok) throw new Error(last.error.message);
      sink += result.value.header.count + Number(last.value);
    }
  }
  return performance.now() - start;
}
const modes = ['decode', 'decode-and-materialize'] as const;
for (let i = 0; i < 5; i++) for (const mode of modes) run(mode);
const samples = { decode: [] as number[], 'decode-and-materialize': [] as number[] };
for (let i = 0; i < 25; i++) for (const mode of i % 2 ? [...modes].reverse() : modes) samples[mode].push(run(mode));
const result = Object.fromEntries(modes.map(mode => {
  const sorted = [...samples[mode]].sort((a, b) => a - b);
  return [mode, { medianMs: sorted[12], p95Ms: sorted[23], minMs: sorted[0], maxMs: sorted[24], samplesMs: samples[mode] }];
}));
console.log(JSON.stringify({
  note: 'Node only; 10,000 full D3 CAN FD 64-byte rows in 50 independently validated frames. Not browser or render latency.',
  measuredAt: new Date().toISOString(), node: process.version, platform: platform(), arch: arch(), cpu: cpus()[0]?.model, cpuCount: cpus().length, totalMemory: totalmem(),
  warmups: 5, repetitions: 25, rows: 10000, frames: frames.length, frameBytes: frames[0]!.byteLength,
  totalWireBytes: frames.reduce((sum, frame) => sum + frame.byteLength, 0),
  method: 'Encoding excluded. Both paths include all validation and JSON sidecar parsing. Materialization adds D3 objects, decimal u64 strings, byte text and detached metadata copies. View path still allocates bounded validation/cache objects. Paired alternating order; GC enabled and uncontrolled; no timing pass threshold.',
  results: result, sink,
}, null, 2));
