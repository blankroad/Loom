// TestSessionStore — test execution state for shell.test (contract 07, test/plan §5,
// _xteam/ui-test.md). Pure reducer over `test.*` events (all priority/control kinds).
// The UI NEVER decides a verdict: it shows what the server recorded (proposed /
// effective / override / rejected) and the evidence check results.
//
// Fed by: NetClient.onControl (live) and the shell's `{domains:["test"]}` subscription
// (late join: snapshot backlog + reducer state). Events are de-duplicated by seq because
// a priority event may arrive on both paths (CORE Q10 open).
//
// Provisional payloads (no loom-proto DTO yet — report p1b-opus.md §4):
//   test.session_started {session_id, tc_id?, title, actor?, env?, plan_digest?, steps?[StepDef]}
//   test.session_state   {state}
//   test.session_ended   {result, reason?}
//   test.step_started    {step_id, n?, title?, expect_text?, timeout_ms?, domains?[{domain, channel?}]}
//   test.step_ended      {step_id}
//   test.evidence_checked{step_id, evidence_id?, ref, status, observed?, cause?, complete?, gap_in_window?}
//   test.verdict         {step_id, proposed?, effective?, by?, reason?, evidence_refs?[],
//                         override?:true, previous?:{effective, by}, rejected?:{code, message}}
//   test.note            {step_id?, text, kind?}
//   test.ask             {ask_id, question, options?[], timeout_ms?}
//   test.answer          {ask_id, value}
//
// C2: steps ≤ 500 (contract), evidence ≤ 64 per step, verdict history ≤ 8 per step,
// notes ring 200, open asks ≤ 16, seen-seq set ≤ 4,096 (oldest dropped).

import type { LoomEvent } from '../proto/provisional';
import { Emitter } from './emitter';

export type SessionState = 'PLANNED' | 'WAITING_APPROVAL' | 'RUNNING' | 'PAUSED' | 'STOPPED' | 'DONE' | string;
export type Verdict = 'pass' | 'fail' | 'inconclusive' | 'blocked' | 'skipped' | 'done';
export type SessionResult = 'passed' | 'failed' | 'inconclusive' | 'incomplete';
export type EvidenceStatus = 'verified' | 'mismatch' | 'not_found' | 'inconclusive' | 'too_large';

export interface EvidenceView {
  id: string;
  ref: string;
  status: EvidenceStatus | string;
  observed: string;
  cause: string | null;
  complete: boolean | null;
  gapInWindow: boolean;
}

export interface VerdictView {
  proposed: Verdict | null;
  effective: Verdict | null;
  by: string;
  reason: string | null;
  refs: string[];
  override: boolean;
  previous: { effective: Verdict | null; by: string } | null;
  rejected: { code: string; message: string } | null;
  atSeq: bigint;
}

export interface StepView {
  id: string;
  n: number;
  title: string;
  expectText: string;
  timeoutMs: number | null;
  domains: { domain: string; channel: string | null }[];
  status: 'pending' | 'running' | 'ended';
  startedWallMs: number | null;
  endedWallMs: number | null;
  startedSeq: bigint | null;
  endedSeq: bigint | null;
  evidence: EvidenceView[];
  /** latest verdict record (may be a rejected PASS attempt) */
  verdict: VerdictView | null;
  /** last ACCEPTED verdict (rejections never replace it) */
  accepted: VerdictView | null;
  history: VerdictView[];
}

export interface AskView {
  id: string;
  question: string;
  options: string[];
  timeoutMs: number | null;
  /** client clock when received (remaining time counts down locally) */
  atMs: number;
  stepId: string | null;
}

export interface NoteView {
  stepId: string | null;
  text: string;
  kind: string | null;
  origin: string;
}

const MAX_STEPS = 500;
const MAX_EVIDENCE = 64;
const MAX_HISTORY = 8;
const MAX_NOTES = 200;
const MAX_ASKS = 16;
const MAX_SEEN = 4_096;

const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {});
const s = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const n = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const VERDICTS: readonly string[] = ['pass', 'fail', 'inconclusive', 'blocked', 'skipped', 'done'];
const verdict = (v: unknown): Verdict | null => (typeof v === 'string' && VERDICTS.includes(v) ? (v as Verdict) : null);
const wallMs = (ev: LoomEvent): number => {
  const w = ev.ts?.wall_ns;
  return typeof w === 'string' && /^\d+$/.test(w) ? Number(BigInt(w) / 1_000_000n) : 0;
};

export class TestSessionStore extends Emitter {
  sessionId: string | null = null;
  title = '';
  tcId: string | null = null;
  actor: string | null = null;
  env: string | null = null;
  planDigest: string | null = null;
  state: SessionState | null = null;
  result: SessionResult | string | null = null;
  endReason: string | null = null;
  steps: StepView[] = [];
  currentStepId: string | null = null;
  asks = new Map<string, AskView>();
  notes: NoteView[] = [];
  /** user-origin events tagged with this session (operator actions) */
  userInterventions = 0;
  /** at least one test.evidence_checked arrived (else the verifier is not running yet — P2) */
  verifierSeen = false;
  private byId = new Map<string, StepView>();
  private seen = new Set<string>();
  private now: () => number;

  constructor(now: () => number = () => Date.now()) {
    super();
    this.now = now;
  }

  get active(): boolean {
    return this.sessionId !== null && this.state !== 'DONE' && this.state !== 'STOPPED' && this.result === null;
  }
  get currentStep(): StepView | null {
    return this.currentStepId ? (this.byId.get(this.currentStepId) ?? null) : null;
  }
  step(id: string): StepView | undefined {
    return this.byId.get(id);
  }

  applyMany(evs: readonly LoomEvent[]): void {
    let dirty = false;
    for (const e of evs) if (this.applyQuiet(e)) dirty = true;
    if (dirty) this.changed();
  }
  apply(ev: LoomEvent): void {
    if (this.applyQuiet(ev)) this.changed();
  }

  /** late-join reducer state from a snapshot part (`state.test` or `items[{domain:'test', state}]`) */
  applySnapshotState(state: unknown): void {
    const st = obj(state);
    const t = obj(st.test ?? (Array.isArray(st.items) ? obj(st.items.find((i) => obj(i).domain === 'test')).state : undefined));
    if (!s(t.session_id)) return;
    this.sessionId = s(t.session_id);
    this.title = s(t.title) ?? this.title;
    this.state = s(t.state) ?? this.state;
    this.currentStepId = s(t.current_step) ?? this.currentStepId;
    if (Array.isArray(t.steps)) for (const d of t.steps) this.defineStep(obj(d));
    this.changed();
  }

  /** a user-origin event carrying this session's tag (shell feeds data events here) */
  noteUserEvent(ev: LoomEvent): void {
    if (ev.origin === 'user' && ev.test_session && ev.test_session === this.sessionId && !this.markSeen(ev)) {
      this.userInterventions++;
      this.changed();
    }
  }

  private markSeen(ev: LoomEvent): boolean {
    const key = `${ev.epoch}:${ev.seq}`;
    if (this.seen.has(key)) return true;
    this.seen.add(key);
    if (this.seen.size > MAX_SEEN) {
      const first = this.seen.values().next().value;
      if (first !== undefined) this.seen.delete(first);
    }
    return false;
  }

  private defineStep(d: Record<string, unknown>): StepView | null {
    const id = s(d.step_id) ?? s(d.id);
    if (!id) return null;
    let st = this.byId.get(id);
    if (!st) {
      if (this.steps.length >= MAX_STEPS) return null;
      st = {
        id,
        n: n(d.n) ?? this.steps.length + 1,
        title: s(d.title) ?? id,
        expectText: s(d.expect_text) ?? '',
        timeoutMs: n(d.timeout_ms),
        domains: [],
        status: 'pending',
        startedWallMs: null,
        endedWallMs: null,
        startedSeq: null,
        endedSeq: null,
        evidence: [],
        verdict: null,
        accepted: null,
        history: [],
      };
      this.byId.set(id, st);
      this.steps.push(st);
      this.steps.sort((a, b) => a.n - b.n);
    } else {
      st.title = s(d.title) ?? st.title;
      st.expectText = s(d.expect_text) ?? st.expectText;
      st.timeoutMs = n(d.timeout_ms) ?? st.timeoutMs;
    }
    if (Array.isArray(d.domains)) {
      st.domains = d.domains.map(obj).flatMap((x) => (s(x.domain) ? [{ domain: s(x.domain) as string, channel: s(x.channel) }] : []));
    }
    if (d.status === 'running' || d.status === 'ended') st.status = d.status;
    return st;
  }

  private applyQuiet(ev: LoomEvent): boolean {
    if (!ev.kind.startsWith('test.')) return false;
    if (this.markSeen(ev)) return false;
    const p = obj(ev.payload);
    const seq = /^\d+$/.test(ev.seq) ? BigInt(ev.seq) : 0n;
    switch (ev.kind) {
      case 'test.session_started': {
        const sid = s(p.session_id) ?? ev.test_session ?? null;
        if (sid !== this.sessionId) this.reset();
        this.sessionId = sid;
        this.title = s(p.title) ?? '';
        this.tcId = s(p.tc_id);
        this.actor = s(p.actor) ?? ev.actor_id ?? ev.client_id ?? null;
        this.env = s(p.env) ?? ev.env ?? null;
        this.planDigest = s(p.plan_digest);
        this.state = s(p.state) ?? 'RUNNING';
        if (Array.isArray(p.steps)) for (const d of p.steps) this.defineStep(obj(d));
        return true;
      }
      case 'test.session_state':
        this.state = s(p.state) ?? this.state;
        return true;
      case 'test.session_ended':
        this.result = s(p.result) ?? 'incomplete';
        this.endReason = s(p.reason);
        this.state = this.state === 'STOPPED' || p.reason === 'stopped' ? 'STOPPED' : 'DONE';
        this.asks.clear();
        return true;
      case 'test.step_started': {
        const st = this.defineStep(p);
        if (!st) return false;
        st.status = 'running';
        st.startedWallMs = wallMs(ev);
        st.startedSeq = seq;
        this.currentStepId = st.id;
        return true;
      }
      case 'test.step_ended': {
        const st = this.byId.get(s(p.step_id) ?? '');
        if (!st) return false;
        st.status = 'ended';
        st.endedWallMs = wallMs(ev);
        st.endedSeq = seq;
        if (this.currentStepId === st.id) this.currentStepId = null;
        return true;
      }
      case 'test.evidence_checked': {
        const st = this.byId.get(s(p.step_id) ?? ev.test_step ?? '');
        if (!st) return false;
        this.verifierSeen = true;
        const id = s(p.evidence_id) ?? s(p.ref) ?? String(seq);
        const e: EvidenceView = {
          id,
          ref: s(p.ref) ?? '',
          status: s(p.status) ?? '?',
          observed: typeof p.observed === 'string' ? p.observed : p.observed === undefined ? '' : JSON.stringify(p.observed),
          cause: s(p.cause),
          complete: typeof p.complete === 'boolean' ? p.complete : null,
          gapInWindow: p.gap_in_window === true,
        };
        const i = st.evidence.findIndex((x) => x.id === id);
        if (i >= 0) st.evidence[i] = e;
        else if (st.evidence.length < MAX_EVIDENCE) st.evidence.push(e);
        return true;
      }
      case 'test.verdict': {
        const st = this.byId.get(s(p.step_id) ?? ev.test_step ?? '');
        if (!st) return false;
        const rej = obj(p.rejected);
        const prev = obj(p.previous);
        const v: VerdictView = {
          proposed: verdict(p.proposed),
          effective: verdict(p.effective),
          by: s(p.by) ?? ev.actor_id ?? ev.origin,
          reason: s(p.reason),
          refs: Array.isArray(p.evidence_refs) ? p.evidence_refs.filter((x): x is string => typeof x === 'string').slice(0, 64) : [],
          override: p.override === true,
          previous: p.previous !== undefined ? { effective: verdict(prev.effective), by: s(prev.by) ?? '?' } : null,
          rejected: s(rej.code) ? { code: s(rej.code) as string, message: s(rej.message) ?? '' } : null,
          atSeq: seq,
        };
        st.verdict = v;
        if (!v.rejected) st.accepted = v;
        st.history.push(v);
        if (st.history.length > MAX_HISTORY) st.history.shift();
        return true;
      }
      case 'test.note':
        this.notes.push({ stepId: s(p.step_id), text: s(p.text) ?? '', kind: s(p.kind), origin: ev.origin });
        if (this.notes.length > MAX_NOTES) this.notes.shift();
        if (ev.origin === 'user') this.userInterventions++;
        return true;
      case 'test.ask': {
        const id = s(p.ask_id);
        if (!id) return false;
        if (this.asks.size >= MAX_ASKS && !this.asks.has(id)) return false;
        this.asks.set(id, {
          id,
          question: s(p.question) ?? '',
          options: Array.isArray(p.options) ? p.options.filter((x): x is string => typeof x === 'string') : [],
          timeoutMs: n(p.timeout_ms),
          atMs: this.now(),
          stepId: s(p.step_id) ?? ev.test_step ?? null,
        });
        return true;
      }
      case 'test.answer':
        return this.asks.delete(s(p.ask_id) ?? '');
      default:
        return false;
    }
  }

  private reset(): void {
    this.steps = [];
    this.byId.clear();
    this.currentStepId = null;
    this.asks.clear();
    this.notes = [];
    this.result = null;
    this.endReason = null;
    this.userInterventions = 0;
    this.verifierSeen = false;
  }
}

export const VERDICT_ICON: Record<Verdict, string> = {
  pass: '✓',
  fail: '✗',
  inconclusive: '?',
  blocked: '⊘',
  skipped: '⏭',
  done: '●',
};

export const EVIDENCE_ICON: Record<EvidenceStatus, string> = {
  verified: '✓ 검증',
  mismatch: '✗ 불일치',
  not_found: '∅ 없음',
  inconclusive: '? 판정 불가',
  too_large: '⇲ 범위 큼',
};
