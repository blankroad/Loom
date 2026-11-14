// PROPOSED — design sketch for ui/src/panels/dbg/dbgModel.ts. NOT compiled, NOT part of the repo.
// Uses only existing SDK surfaces: PanelContext.store/onControl/query/act/rows/media (ui/src/sdk/context.ts),
// ActionSpec fields id/rpc/class/right/confirm/safetyDirection/label (ui/src/sdk/manifest.ts).

export type U64 = bigint; // wire: decimal string; parse with BigInt(s); never Number()

export type RunState = 'unknown' | 'running' | 'halted' | 'stepping' | 'disconnected' | 'lost' | 'error';
export type Quality = 'fresh' | 'stale' | 'unknown';

export interface Value { hex: string; bits: number; signed: boolean; dec?: string; endian?: 'little' | 'big' | 'n/a'; f?: string }

export interface TargetView {
  targetId: string;               // '<probe_alias>/<core_alias>' (= panel `channel` prop)
  coreName: string | null;        // opaque CCS name from session open
  backend: 'fake' | 'ccs-remote' | null;
  bridgeHost: string | null;      // from dbg.link payload; never a placeholder
  ccsVersion: string | null;      // 'unknown' stays 'unknown'
  state: RunState;                // starts 'unknown' — no fake halted/off/fresh default
  lastKnown: RunState | null;     // shown when state is lost/unknown
  quality: Quality;
  sessionEpoch: U64 | null;
  runEpoch: U64 | null;
  stateRev: U64;                  // the reducer ignores any event whose state_rev ≤ this (out-of-order safe)
  observedWallMs: number | null;  // age = now - observedWallMs, rendered as "N s 전 관측"
  inFlight: { requestId: string; verb: string; sinceMs: number } | null;
}

export interface SampleView {
  sampleId: string;               // '<target>@<session>.<run>.<seq>' — the exact string the agent cites
  journalSeq: U64;
  runEpoch: U64;
  values: Map<string, Value | { err: string }>;
  current: boolean;               // false as soon as TargetView.runEpoch > runEpoch or sessionEpoch differs
}

// Reducer rule (sketch):
//   onControl(dbg.run_state | dbg.link | dbg.session) → if (BigInt(p.state_rev) <= view.stateRev) drop; else apply.
//   store('samples') append(dbg.sample) → latest per key; mark current = (epochs match view).
//   run_epoch change → every SampleView.current = false; the panel greys values and labels
//     "마지막 halt(epoch N) 기준 · M s 전", it never clears them (history after disconnect is kept).
//   dbg.gap → draw a break in the series; never interpolate.
//   ctx.writeLocked / ctx.stopLatched → disable run/halt/step buttons except that STOP stays (shell owned).
