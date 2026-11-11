// Mock test session (TC-1 cold-boot smoke) for shell.test — p1b-opus.md §5 items 1–3.
// test.* events are priority kinds: the hub sends them on the control path AND to `{domains:["test"]}`
// subscriptions (late join = snapshot backlog). Shapes follow src/store/testSession.ts head comment.
// RPC echoes: test.pause/resume → test.session_state · test.skip → skipped verdict · test.retry →
// step_started again · test.answer → test.answer · test.verdict{override} (viewer approve only;
// agent → E_DENIED) · test.end{abort} → session_ended{incomplete, aborted} · test.plan.show.
// The agent side (mock.agent) can begin a step and propose a verdict; a PASS citing evidence
// from before the step began is rejected (E_OUT_OF_WINDOW) and recorded as such.
// Bounds (C2): one session at a time, ≤ 16 steps, verdict history kept only as events.

import type { Hub } from './hub';
import { MockRpcError, type Actor } from './sim';

export const STEPS = [
  { step_id: 'S1', n: 1, title: '전원 사이클', expect_text: 'PSU OFF → ON (자극 전용)', timeout_ms: 5_000, domains: [{ domain: 'psu', channel: 'bench' }] },
  { step_id: 'S2', n: 2, title: 'UART "login:" 30 s', expect_text: 'uart.match "login:" ≤ 30 s', timeout_ms: 30_000, domains: [{ domain: 'uart', channel: 'dut' }] },
  { step_id: 'S3', n: 3, title: 'CAN PowerMode == 2 (5 s)', expect_text: 'can.signal VehState.PowerMode == 2', timeout_ms: 5_000, domains: [{ domain: 'can', channel: 'ch0' }] },
  { step_id: 'S4', n: 4, title: 'UDS DTC mask 0x09 → 0건', expect_text: 'uds.dtc_absent mask=0x09', timeout_ms: 5_000, domains: [{ domain: 'uds', channel: 'bcm' }] },
  { step_id: 'S5', n: 5, title: '(수동) 계기판 부팅 로고', expect_text: '사람 판정', timeout_ms: 120_000, domains: [] },
];

const AGENT_ID = 'claude-1';

export class TestRun {
  session: string | null = null;
  private stepBegin = new Map<string, bigint>();
  /** last accepted effective verdict per step (for override `previous`) */
  private effective = new Map<string, { effective: string; by: string }>();
  private n = 0;

  constructor(private readonly hub: Hub) {}

  private ev(kind: string, payload: Record<string, unknown>, o: { origin?: Actor['origin']; actor?: string; step?: string } = {}) {
    return this.hub.publish({
      domain: 'test',
      kind,
      origin: o.origin ?? 'agent',
      actor_id: o.actor ?? AGENT_ID,
      client_id: o.actor ?? AGENT_ID,
      ...(this.session ? { test_session: this.session } : {}),
      ...(o.step ? { test_step: o.step } : {}),
      payload,
    });
  }

  private need(session?: unknown): string {
    if (!this.session) throw new MockRpcError(-32005, 'no test session', { code: 'not_found' });
    if (session !== undefined && session !== this.session) throw new MockRpcError(-32005, `unknown session ${String(session)}`, { code: 'not_found' });
    return this.session;
  }

  beginStep(stepId: string): void {
    const ev = this.ev('test.step_started', { step_id: stepId }, { step: stepId });
    this.stepBegin.set(stepId, BigInt(ev.seq));
  }

  private verdict(stepId: string, v: string, refs: string[], by = AGENT_ID): void {
    this.ev('test.verdict', { step_id: stepId, proposed: v, effective: v, by, evidence_refs: refs }, { step: stepId });
    this.effective.set(stepId, { effective: v, by });
  }

  /**
   * Run TC-1 up to the manual step S5 (question open). Includes one PASS rejected for a
   * fabricated seq, one user intervention note, a mismatch → fail. Events are spaced by `gapMs`.
   */
  async runTc1(gapMs = 10): Promise<{ session: string; last_seq: string }> {
    this.session = `ts-${String(++this.n).padStart(4, '0')}`;
    this.stepBegin.clear();
    this.effective.clear();
    const pause = () => new Promise<void>((r) => setTimeout(r, gapMs));
    this.ev('test.session_started', { session_id: this.session, tc_id: 'TC-1', title: '콜드 부팅 스모크', actor: AGENT_ID, env: 'sim', plan_digest: 'sha256:41ab', steps: STEPS });
    this.ev('test.session_state', { state: 'RUNNING' });
    for (const s of ['S1', 'S2', 'S3', 'S4']) {
      await pause();
      this.beginStep(s);
      const begin = this.stepBegin.get(s)!;
      if (s === 'S1') this.verdict(s, 'done', []);
      if (s === 'S2') {
        this.ev('test.evidence_checked', { step_id: s, evidence_id: 'e1', ref: `uart:dut@e1:seq:${begin + 1n}`, status: 'verified', observed: 'buildroot login:', complete: true }, { step: s });
        this.verdict(s, 'pass', [`uart:dut@e1:seq:${begin + 1n}`]);
      }
      if (s === 'S3') {
        await this.agentVerdict({ step: s, verdict: 'pass', evidence_refs: [`can:ch0@e1:seq:${begin - 5n}`] }).catch(() => undefined);
        this.ev('test.note', { step_id: s, kind: 'operator_action', text: 'user tx 0x123 (판정 제외)' }, { origin: 'user', actor: 'viewer:tab-1', step: s });
        this.ev('test.evidence_checked', { step_id: s, evidence_id: 'e2', ref: `can:ch0@e1:seq:${begin + 2n}`, status: 'verified', observed: 'PowerMode=2', complete: true }, { step: s });
        this.verdict(s, 'pass', [`can:ch0@e1:seq:${begin + 2n}`]);
      }
      if (s === 'S4') {
        this.ev('test.evidence_checked', { step_id: s, evidence_id: 'e3', ref: `uds:bcm@e1:seq:${begin + 1n}`, status: 'mismatch', observed: 'dtc_count=1', complete: true, gap_in_window: false }, { step: s });
        this.verdict(s, 'fail', [`uds:bcm@e1:seq:${begin + 1n}`]);
      }
      this.ev('test.step_ended', { step_id: s }, { step: s });
    }
    await pause();
    this.beginStep('S5');
    this.ev('test.ask', { ask_id: 'q1', question: '계기판에 부팅 로고가 보입니까?', options: ['예', '아니요'], timeout_ms: 120_000, step_id: 'S5' }, { step: 'S5' });
    return { session: this.session, last_seq: String(this.hub.seq) };
  }

  /** agent `loom test verdict`: a PASS citing seq before the step began is rejected (recorded) */
  async agentVerdict(p: { step?: string; verdict?: string; evidence_refs?: string[]; override?: boolean }): Promise<Record<string, unknown>> {
    this.need();
    if (p.override) throw new MockRpcError(-32003, 'verdict override is a viewer (approve) action', { code: 'E_DENIED' });
    const step = String(p.step ?? '');
    const begin = this.stepBegin.get(step);
    if (begin === undefined) throw new MockRpcError(-32602, `step ${step} not started`, { code: 'invalid_params' });
    const refs = (p.evidence_refs ?? []).slice(0, 16);
    const bad = refs.find((r) => {
      const m = /@(?:e\d+:)?seq:(\d+)/.exec(r);
      return m ? BigInt(m[1]!) < begin : false;
    });
    if (p.verdict === 'pass' && bad) {
      const seq = /seq:(\d+)/.exec(bad)![1]!;
      const rejected = { code: 'E_OUT_OF_WINDOW', message: `seq:${seq} is before step ${step} begin` };
      this.ev('test.verdict', { step_id: step, proposed: 'pass', by: AGENT_ID, evidence_refs: refs, rejected }, { step });
      throw new MockRpcError(-32020, rejected.message, { code: 'E_EVIDENCE_OUT_OF_WINDOW', detail: rejected });
    }
    this.verdict(step, String(p.verdict ?? 'pass'), refs);
    return { step, effective: p.verdict };
  }

  agentStep(p: { step?: string; begin?: boolean; end?: boolean }): Record<string, unknown> {
    this.need();
    const step = String(p.step ?? '');
    if (!STEPS.some((s) => s.step_id === step)) throw new MockRpcError(-32602, `unknown step ${step}`, { code: 'invalid_params' });
    if (p.end) this.ev('test.step_ended', { step_id: step }, { step });
    else this.beginStep(step);
    return { step, seq: String(this.hub.seq) };
  }

  /** viewer RPCs (rights are checked by the server: operate, override = approve) */
  viewer(a: Actor, method: string, p: Record<string, unknown>): Record<string, unknown> {
    const who = { origin: a.origin, actor: a.clientId };
    switch (method) {
      case 'test.plan.show':
        this.need(p.session);
        return { plan_hash: 'sha256:41ab', steps: STEPS.map((s) => ({ source_text: s.title, interpreted: s.expect_text, predicate_text: s.expect_text, risk: s.step_id === 'S1' ? 'R3' : 'R0' })) };
      case 'test.pause':
      case 'test.resume':
        this.need(p.session);
        this.ev('test.session_state', { state: method === 'test.pause' ? 'PAUSED' : 'RUNNING' }, who);
        return { ok: true };
      case 'test.skip': {
        this.need(p.session);
        const step = String(p.step ?? '');
        this.ev('test.verdict', { step_id: step, proposed: 'skipped', effective: 'skipped', by: a.clientId, reason: String(p.reason ?? '') }, { ...who, step });
        return { ok: true };
      }
      case 'test.retry': {
        this.need(p.session);
        this.beginStep(String(p.step ?? ''));
        return { ok: true };
      }
      case 'test.answer':
        this.need(p.session);
        this.ev('test.answer', { ask_id: String(p.ask_id ?? ''), value: String(p.value ?? '') }, who);
        return { ok: true };
      case 'test.verdict': {
        this.need(p.session);
        if (p.override !== true) throw new MockRpcError(-32602, 'viewers can only override verdicts', { code: 'invalid_params' });
        const step = String(p.step ?? '');
        const reason = String(p.reason ?? '').trim();
        if (!reason) throw new MockRpcError(-32602, 'override needs a reason', { code: 'invalid_params' });
        const prev = this.effective.get(step) ?? null;
        const v = String(p.verdict ?? 'inconclusive');
        this.ev('test.verdict', { step_id: step, proposed: prev?.effective ?? v, effective: v, by: a.clientId, override: true, reason, ...(prev ? { previous: prev } : {}) }, { ...who, step });
        this.effective.set(step, { effective: v, by: a.clientId });
        return { ok: true };
      }
      case 'test.end':
        this.need(p.session);
        this.ev('test.session_ended', { result: 'incomplete', reason: p.abort === true ? 'aborted' : 'ended' }, who);
        this.session = null;
        return { ok: true };
      default:
        throw new MockRpcError(-32601, `method not found: ${method}`);
    }
  }
}
