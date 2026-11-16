// B2 spec (Opus): CAN-6 close impact preview. FUNCTIONS.md §B2-CLOSE.
import { describe, expect, it } from 'vitest';
import { CLOSE_JOBS_SHOWN, VIEW_CLOSE_NOTE, channelStateOf, closeDoneText, closeImpact, closeImpactLines, parseChannelBrief, parseJobs, type CloseImpact } from './closeChannel';

// shapes as produced by crates/loom-can periodic.rs job_json / worker.rs status()
const job = (n: number, o: Partial<Record<string, unknown>> = {}) => ({
  job: `p${n}`, channel: 'ch0', id: 0x100 + n, ext: false, fd: false, data: '01', period_ms: 100, owner: `cli-${n}`,
  origin: 'agent', revision: 1, applied_revision: 1, count_left: null, lease_left_ms: 1000, sent: 3, missed: 0,
  lateness_p99_us: null, state: 'running', ...o,
});
const status = (o: Partial<Record<string, unknown>> = {}) => ({
  backend: 'can_sim', latch: false, nodes: [],
  channels: [
    { channel: 'other', isotp_endpoints: 9, pending_bursts: 9, nodes: 9, periodic_jobs: 9 },
    { channel: 'ch0', isotp_endpoints: 2, pending_bursts: 0, nodes: 1, periodic_jobs: 2, ...o },
  ],
});
const ok = (value: unknown): PromiseSettledResult<unknown> => ({ status: 'fulfilled', value });
const bad = (reason: unknown): PromiseSettledResult<unknown> => ({ status: 'rejected', reason });

describe('parseJobs', () => {
  it('keeps only the channel; maps fields', () => {
    const r = parseJobs({ jobs: [job(1), job(2, { channel: 'ch1' }), job(3, { ext: true, id: 0x18daf110, origin: 'user', state: 'paused' })] }, 'ch0');
    expect(r).toEqual([
      { job: 'p1', id: 0x101, ext: false, owner: 'cli-1', origin: 'agent', periodMs: 100, state: 'running' },
      { job: 'p3', id: 0x18daf110, ext: true, owner: 'cli-3', origin: 'user', periodMs: 100, state: 'paused' },
    ]);
    expect(parseJobs({ jobs: [] }, 'ch0')).toEqual([]);
  });

  it('unexpected shape → null (unknown, never an empty list)', () => {
    expect(parseJobs(null, 'ch0')).toBeNull();
    expect(parseJobs({}, 'ch0')).toBeNull();
    expect(parseJobs({ jobs: 'x' }, 'ch0')).toBeNull();
    expect(parseJobs({ jobs: [job(1, { period_ms: '100' })] }, 'ch0')).toBeNull();
    expect(parseJobs({ jobs: [job(1), 7] }, 'ch0')).toBeNull();
  });
});

describe('parseChannelBrief', () => {
  it('the named channel entry', () => {
    expect(parseChannelBrief(status(), 'ch0')).toEqual({ isotp: 2, bursts: 0, nodes: 1 });
  });
  it('missing channel / count / bad shape → null', () => {
    expect(parseChannelBrief(status(), 'ch9')).toBeNull();
    expect(parseChannelBrief(status({ nodes: undefined }), 'ch0')).toBeNull();
    expect(parseChannelBrief(status({ isotp_endpoints: -1 }), 'ch0')).toBeNull();
    expect(parseChannelBrief({ channels: 3 }, 'ch0')).toBeNull();
    expect(parseChannelBrief(undefined, 'ch0')).toBeNull();
  });
});

describe('closeImpact', () => {
  it('both reads ok', () => {
    const i = closeImpact('ch0', ok({ jobs: [job(1)] }), ok(status()));
    expect(i).toEqual({ channel: 'ch0', jobs: [expect.objectContaining({ job: 'p1' })], status: { isotp: 2, bursts: 0, nodes: 1 }, errors: [] });
  });
  it('failures and bad shapes are unknown with an error line (list first)', () => {
    const i = closeImpact('ch0', bad(new Error('E_TIMEOUT')), ok({ nope: 1 }));
    expect(i.jobs).toBeNull();
    expect(i.status).toBeNull();
    expect(i.errors).toEqual(['주기 송신 목록 조회 실패: E_TIMEOUT', '채널 상태 형식을 알 수 없음']);
    const j = closeImpact('ch0', ok(42), bad('boom'));
    expect(j.errors).toEqual(['주기 송신 목록 형식을 알 수 없음', '채널 상태 조회 실패: boom']);
  });
});

describe('closeImpactLines', () => {
  const head = '채널 ch0을(를) 데몬에서 닫습니다 — 이 채널을 쓰는 모든 사용자·에이전트에 적용됩니다.';

  it('known impact, jobs of all owners listed', () => {
    const i: CloseImpact = {
      channel: 'ch0',
      jobs: [
        { job: 'p1', id: 0x101, ext: false, owner: 'cli-1', origin: 'agent', periodMs: 100, state: 'running' },
        { job: 'p3', id: 0x18daf110, ext: true, owner: 'ui-7', origin: 'user', periodMs: 20, state: 'paused' },
      ],
      status: { isotp: 2, bursts: 0, nodes: 1 },
      errors: [],
    };
    expect(closeImpactLines(i)).toEqual([
      head,
      '주기 송신 2개 정지 (모든 소유자):',
      '· p1 0x101 100 ms — agent cli-1 (running)',
      '· p3 18DAF110x 20 ms — user ui-7 (paused)',
      'ISO-TP 엔드포인트: 2개 닫힘',
      '대기 중 버스트 송신: 없음',
      '시뮬레이터 노드: 1개 분리',
      VIEW_CLOSE_NOTE,
    ]);
  });

  it('nothing running', () => {
    expect(closeImpactLines({ channel: 'ch0', jobs: [], status: { isotp: 0, bursts: 0, nodes: 0 }, errors: [] })).toEqual([
      head, '주기 송신: 없음', 'ISO-TP 엔드포인트: 없음', '대기 중 버스트 송신: 없음', '시뮬레이터 노드: 없음', VIEW_CLOSE_NOTE,
    ]);
  });

  it('unknown is said, never shown as none; errors are listed', () => {
    expect(closeImpactLines({ channel: 'ch0', jobs: null, status: null, errors: ['주기 송신 목록 조회 실패: x', '채널 상태 조회 실패: y'] })).toEqual([
      head,
      '주기 송신: 알 수 없음',
      'ISO-TP 엔드포인트·대기 버스트·시뮬레이터 노드: 알 수 없음',
      '⚠ 주기 송신 목록 조회 실패: x',
      '⚠ 채널 상태 조회 실패: y',
      VIEW_CLOSE_NOTE,
    ]);
  });

  it(`at most ${CLOSE_JOBS_SHOWN} job lines, then a count`, () => {
    const jobs = Array.from({ length: CLOSE_JOBS_SHOWN + 3 }, (_, n) => ({ job: `p${n}`, id: n, ext: false, owner: 'o', origin: 'agent', periodMs: 10, state: 'running' }));
    const lines = closeImpactLines({ channel: 'ch0', jobs, status: { isotp: 0, bursts: 0, nodes: 0 }, errors: [] });
    expect(lines[1]).toBe(`주기 송신 ${CLOSE_JOBS_SHOWN + 3}개 정지 (모든 소유자):`);
    expect(lines.filter((l) => l.startsWith('· p')).length).toBe(CLOSE_JOBS_SHOWN);
    expect(lines).toContain('· … 외 3개');
  });
});

describe('closeDoneText', () => {
  it('generation from the result; stopped job count when known', () => {
    const i: CloseImpact = { channel: 'ch0', jobs: [{ job: 'p1', id: 1, ext: false, owner: 'o', origin: 'agent', periodMs: 10, state: 'running' }], status: null, errors: [] };
    expect(closeDoneText('ch0', { channel: 'ch0', state: 'closed', generation: 4 }, i)).toBe('CAN ch0 닫힘 (generation 4) · 주기 송신 1개 정지');
    expect(closeDoneText('ch0', undefined, null)).toBe('CAN ch0 닫힘');
    expect(closeDoneText('ch0', { generation: 'x' }, { ...i, jobs: [] })).toBe('CAN ch0 닫힘');
    expect(closeDoneText('ch0', {}, { ...i, jobs: null })).toBe('CAN ch0 닫힘');
  });
});

describe('channelStateOf', () => {
  const e = (kind: string, channel: string, state: unknown) => ({ kind, channel, payload: { state, channel } });
  it('open/closed of this channel only', () => {
    expect(channelStateOf(e('can.channel_state', 'ch0', 'closed'), 'ch0')).toBe('closed');
    expect(channelStateOf(e('can.channel_state', 'ch0', 'open'), 'ch0')).toBe('open');
    expect(channelStateOf(e('can.channel_state', 'ch1', 'closed'), 'ch0')).toBeNull();
    expect(channelStateOf(e('can.channel_state', 'ch0', 'weird'), 'ch0')).toBeNull();
    expect(channelStateOf(e('can.job_state', 'ch0', 'closed'), 'ch0')).toBeNull();
  });
});
