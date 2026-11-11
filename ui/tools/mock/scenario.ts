// Scenario replay: fixtures/scenarios/*.jsonl drive background device traffic
// and scripted agent actions (the mock's stand-in for Claude Code).
//
// Line format (first line may be a header {"scenario": "...", "note": "..."}):
//   {"t_ms": 0,    "emit":  {domain, channel?, kind, dir?, origin, payload}}
//   {"t_ms": 0,    "every_ms": 100, "emit": {...}}          periodic, until stop
//   {"t_ms": 1500, "agent": {"method": "uart.send", "params": {...}}}
//   {"t_ms": 0,    "every_ms": 20000, "agent": {...}}       periodic agent action
//   {"t_ms": 0,    "call": "bootLog", "args": ["dut"]}      sim helper

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Emit } from './hub';

export interface ScenarioLine {
  t_ms: number;
  every_ms?: number;
  emit?: Emit;
  agent?: { method: string; params?: Record<string, unknown> };
  call?: 'bootLog';
  args?: string[];
}

export const SCENARIO_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'fixtures', 'scenarios');

export function loadScenario(file: string): { name: string; lines: ScenarioLine[] } {
  const full = file.includes('/') ? file : path.join(SCENARIO_DIR, file.endsWith('.jsonl') ? file : `${file}.jsonl`);
  const lines: ScenarioLine[] = [];
  let name = path.basename(full, '.jsonl');
  for (const [i, raw] of fs.readFileSync(full, 'utf8').split('\n').entries()) {
    if (!raw.trim()) continue;
    const obj = JSON.parse(raw) as Record<string, unknown>;
    if (typeof obj.scenario === 'string') {
      name = obj.scenario;
      continue;
    }
    if (typeof obj.t_ms !== 'number' || !(obj.emit || obj.agent || obj.call)) throw new Error(`${full}:${i + 1}: bad scenario line`);
    lines.push(obj as unknown as ScenarioLine);
  }
  return { name, lines };
}

export function listScenarios(): string[] {
  try {
    return fs.readdirSync(SCENARIO_DIR).filter((f) => f.endsWith('.jsonl')).sort();
  } catch {
    return [];
  }
}

export interface ScenarioHooks {
  emit(e: Emit): void;
  agent(method: string, params: Record<string, unknown>): Promise<unknown>;
  call(name: 'bootLog', args: string[]): void;
}

/** Start playing; returns stop(). Timers are the only state (bounded by line count). */
export function play(lines: ScenarioLine[], hooks: ScenarioHooks, log: (m: string) => void = () => undefined): () => void {
  const timers: NodeJS.Timeout[] = [];
  const run = (l: ScenarioLine) => {
    if (l.emit) hooks.emit(structuredClone(l.emit));
    if (l.call) hooks.call(l.call, l.args ?? []);
    if (l.agent) hooks.agent(l.agent.method, structuredClone(l.agent.params ?? {})).catch((e: unknown) => log(`agent ${l.agent?.method} failed: ${String(e)}`));
  };
  for (const l of lines) {
    timers.push(
      setTimeout(() => {
        run(l);
        if (l.every_ms && l.every_ms >= 10) timers.push(setInterval(() => run(l), l.every_ms));
      }, l.t_ms),
    );
  }
  return () => timers.forEach((t) => clearTimeout(t));
}
