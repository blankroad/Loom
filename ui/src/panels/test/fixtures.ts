// TC-1 cold-boot smoke (test/plan §2) as test.* events, for the shell.test screen.
// Includes: a PASS rejected because the agent cited an evidence seq outside the step
// window (E_OUT_OF_WINDOW), one user intervention (operator note), one question (ask).

import type { LoomEvent, Origin } from '../../sdk';

export const SESSION = 'ts-0042';

let n = 0;
function rec(kind: string, seq: number, payload: Record<string, unknown>, o: { origin?: Origin; step?: string } = {}): LoomEvent {
  n = Math.max(n, seq);
  const mono = BigInt(seq) * 100_000_000n;
  return {
    schema_version: 1,
    session_id: 's-test',
    domain: 'test',
    kind,
    epoch: '1',
    seq: String(seq),
    producer_id: 'gateway',
    producer_epoch: '1',
    source_seq: String(seq),
    ts: { mono_ns: String(mono), wall_ns: String(1_762_770_000_000_000_000n + mono), clock_id: 'host', source: 'gateway' },
    origin: o.origin ?? 'agent',
    actor_id: o.origin === 'user' ? 'viewer:tab-1' : 'claude-1',
    test_session: SESSION,
    ...(o.step ? { test_step: o.step } : {}),
    payload,
  };
}

export const TC1_STEPS = [
  { step_id: 'S1', n: 1, title: '전원 사이클', expect_text: 'PSU OFF → ON (자극 전용)', timeout_ms: 5_000, domains: [{ domain: 'psu', channel: 'bench' }] },
  { step_id: 'S2', n: 2, title: 'UART "login:" 30 s', expect_text: 'uart.match "login:" ≤ 30 s', timeout_ms: 30_000, domains: [{ domain: 'uart', channel: 'dut' }] },
  { step_id: 'S3', n: 3, title: 'CAN PowerMode == 2 (5 s)', expect_text: 'can.signal VehState.PowerMode == 2', timeout_ms: 5_000, domains: [{ domain: 'can', channel: 'ch0' }] },
  { step_id: 'S4', n: 4, title: 'UDS DTC mask 0x09 → 0건', expect_text: 'uds.dtc_absent mask=0x09', timeout_ms: 5_000, domains: [{ domain: 'uds', channel: 'bcm' }] },
  { step_id: 'S5', n: 5, title: '(수동) 계기판 부팅 로고', expect_text: '사람 판정', timeout_ms: 120_000, domains: [] },
];

export function tc1Events(): LoomEvent[] {
  return [
    rec('test.session_started', 100, { session_id: SESSION, tc_id: 'TC-1', title: '콜드 부팅 스모크', actor: 'claude-1', env: 'sim', plan_digest: 'sha256:41ab', steps: TC1_STEPS }),
    rec('test.step_started', 101, { step_id: 'S1' }, { step: 'S1' }),
    rec('test.verdict', 102, { step_id: 'S1', proposed: 'done', effective: 'done', by: 'claude-1' }, { step: 'S1' }),
    rec('test.step_ended', 103, { step_id: 'S1' }, { step: 'S1' }),
    rec('test.step_started', 104, { step_id: 'S2' }, { step: 'S2' }),
    rec('test.evidence_checked', 105, { step_id: 'S2', evidence_id: 'e1', ref: 'uart:dut@e1:seq:18231', status: 'verified', observed: 'buildroot login:', complete: true }, { step: 'S2' }),
    rec('test.verdict', 106, { step_id: 'S2', proposed: 'pass', effective: 'pass', by: 'claude-1', evidence_refs: ['uart:dut@e1:seq:18231'] }, { step: 'S2' }),
    rec('test.step_ended', 107, { step_id: 'S2' }, { step: 'S2' }),
    rec('test.step_started', 108, { step_id: 'S3' }, { step: 'S3' }),
    // the agent cites a seq from BEFORE the step started → the server rejects the PASS
    rec('test.verdict', 109, { step_id: 'S3', proposed: 'pass', by: 'claude-1', evidence_refs: ['can:ch0@e1:seq:17990'], rejected: { code: 'E_OUT_OF_WINDOW', message: 'seq:17990 is before step S3 begin' } }, { step: 'S3' }),
    rec('test.note', 110, { step_id: 'S3', kind: 'operator_action', text: 'user tx 0x123 (판정 제외)' }, { origin: 'user', step: 'S3' }),
    rec('test.evidence_checked', 111, { step_id: 'S3', evidence_id: 'e2', ref: 'can:ch0@e1:seq:18301', status: 'verified', observed: 'PowerMode=2', complete: true }, { step: 'S3' }),
    rec('test.verdict', 112, { step_id: 'S3', proposed: 'pass', effective: 'pass', by: 'claude-1', evidence_refs: ['can:ch0@e1:seq:18301'] }, { step: 'S3' }),
    rec('test.step_ended', 113, { step_id: 'S3' }, { step: 'S3' }),
    rec('test.step_started', 114, { step_id: 'S4' }, { step: 'S4' }),
    rec('test.evidence_checked', 115, { step_id: 'S4', evidence_id: 'e3', ref: 'uds:bcm@e1:seq:18402', status: 'mismatch', observed: 'dtc_count=1', complete: true, gap_in_window: false }, { step: 'S4' }),
    rec('test.verdict', 116, { step_id: 'S4', proposed: 'fail', effective: 'fail', by: 'claude-1', evidence_refs: ['uds:bcm@e1:seq:18402'] }, { step: 'S4' }),
    rec('test.step_ended', 117, { step_id: 'S4' }, { step: 'S4' }),
    rec('test.step_started', 118, { step_id: 'S5' }, { step: 'S5' }),
    rec('test.ask', 119, { ask_id: 'q1', question: '계기판에 부팅 로고가 보입니까?', options: ['예', '아니요'], timeout_ms: 120_000, step_id: 'S5' }, { step: 'S5' }),
  ];
}

/** a user override event (approve rights) for S4 */
export function overrideEvent(seq = 200): LoomEvent {
  return rec('test.verdict', seq, { step_id: 'S4', proposed: 'fail', effective: 'inconclusive', by: 'viewer:tab-1', override: true, reason: 'DTC는 이전 세션 잔여', previous: { effective: 'fail', by: 'claude-1' } }, { origin: 'user', step: 'S4' });
}

export { rec as testRecord };
