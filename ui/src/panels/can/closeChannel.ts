// CAN channel close from the bound panel (CAN-6): impact preview + result text. Pure (no DOM, no RPC).
// Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B2-CLOSE.
//
// · The panel reads can.periodic.list{channel} and can.status{channel} once, in parallel, right before the
//   preview (no polling). A failed read is UNKNOWN in the preview, never "0 / 없음".
// · can.close stops every periodic job of the channel (all owners, reason channel_closed), closes its ISO-TP
//   endpoints, cancels pending bursts and detaches simulator nodes (crates/loom-can worker.rs close()).
// · Closing the panel tab only closes the view; the preview says so.
// · Job owner/origin/state strings come from the daemon → rendered with textContent only.
//
// SCAFFOLD (Opus): types/constants/signatures. Function bodies are Sonnet batch-2 work.

import type { LoomEvent } from '../../sdk';
import { fmtId } from './sendLine';

/** jobs listed one per line in the preview; the rest are counted */
export const CLOSE_JOBS_SHOWN = 8;
/** last line of every preview (view-close vs channel-close difference stays explicit) */
export const VIEW_CLOSE_NOTE = '패널 탭만 닫으려면 탭의 ×를 사용하세요 — 보기만 닫히고 채널은 열린 채로 남습니다.';

export interface JobBrief {
  job: string;
  id: number;
  ext: boolean;
  owner: string;
  origin: string;
  periodMs: number;
  state: string;
}

export interface ChannelBrief {
  isotp: number;
  bursts: number;
  nodes: number;
}

/** null field = unknown (that read failed or had an unexpected shape) */
export interface CloseImpact {
  channel: string;
  jobs: JobBrief[] | null;
  status: ChannelBrief | null;
  /** read failures, in order: list first, then status */
  errors: string[];
}

/** `can.periodic.list` result → jobs of `channel`; null when the shape is not `{jobs: [...]}` */
export function parseJobs(v: unknown, channel: string): JobBrief[] | null {
  const isCount = (x: unknown): x is number => typeof x === 'number' && Number.isInteger(x) && x >= 0;
  if (typeof v !== 'object' || v === null) return null;
  const arr = (v as { jobs?: unknown }).jobs;
  if (!Array.isArray(arr)) return null;
  const out: JobBrief[] = [];
  for (const e of arr as unknown[]) {
    if (typeof e !== 'object' || e === null) return null;
    const o = e as Record<string, unknown>;
    if (
      typeof o.job !== 'string' ||
      typeof o.channel !== 'string' ||
      !isCount(o.id) ||
      typeof o.ext !== 'boolean' ||
      typeof o.owner !== 'string' ||
      typeof o.origin !== 'string' ||
      !isCount(o.period_ms) ||
      typeof o.state !== 'string'
    ) {
      return null;
    }
    if (o.channel !== channel) continue;
    out.push({ job: o.job, id: o.id, ext: o.ext, owner: o.owner, origin: o.origin, periodMs: o.period_ms, state: o.state });
  }
  return out;
}

/** `can.status{channel}` result → that channel's counts; null when the channel entry or a count is missing */
export function parseChannelBrief(v: unknown, channel: string): ChannelBrief | null {
  const isCount = (x: unknown): x is number => typeof x === 'number' && Number.isInteger(x) && x >= 0;
  if (typeof v !== 'object' || v === null) return null;
  const arr = (v as { channels?: unknown }).channels;
  if (!Array.isArray(arr)) return null;
  const entry = (arr as unknown[]).find((e): e is Record<string, unknown> => typeof e === 'object' && e !== null && (e as Record<string, unknown>).channel === channel);
  if (!entry) return null;
  const { isotp_endpoints: isotp, pending_bursts: bursts, nodes } = entry;
  if (!isCount(isotp) || !isCount(bursts) || !isCount(nodes)) return null;
  return { isotp, bursts, nodes };
}

/** settled reads → impact (rejections and bad shapes become unknown + an error line) */
export function closeImpact(channel: string, list: PromiseSettledResult<unknown>, status: PromiseSettledResult<unknown>): CloseImpact {
  const errMsg = (reason: unknown): string => (reason instanceof Error ? reason.message : String(reason));
  const errors: string[] = [];
  let jobs: JobBrief[] | null;
  if (list.status === 'fulfilled') {
    jobs = parseJobs(list.value, channel);
    if (jobs === null) errors.push('주기 송신 목록 형식을 알 수 없음');
  } else {
    jobs = null;
    errors.push(`주기 송신 목록 조회 실패: ${errMsg(list.reason)}`);
  }
  let brief: ChannelBrief | null;
  if (status.status === 'fulfilled') {
    brief = parseChannelBrief(status.value, channel);
    if (brief === null) errors.push('채널 상태 형식을 알 수 없음');
  } else {
    brief = null;
    errors.push(`채널 상태 조회 실패: ${errMsg(status.reason)}`);
  }
  return { channel, jobs, status: brief, errors };
}

/** preview lines, in order (see contract table) */
export function closeImpactLines(i: CloseImpact): string[] {
  const lines: string[] = [`채널 ${i.channel}을(를) 데몬에서 닫습니다 — 이 채널을 쓰는 모든 사용자·에이전트에 적용됩니다.`];
  if (i.jobs === null) lines.push('주기 송신: 알 수 없음');
  else if (i.jobs.length === 0) lines.push('주기 송신: 없음');
  else {
    const n = i.jobs.length;
    lines.push(`주기 송신 ${n}개 정지 (모든 소유자):`);
    for (const j of i.jobs.slice(0, CLOSE_JOBS_SHOWN)) {
      lines.push(`· ${j.job} ${fmtId(j.id, j.ext)} ${j.periodMs} ms — ${j.origin} ${j.owner} (${j.state})`);
    }
    if (n > CLOSE_JOBS_SHOWN) lines.push(`· … 외 ${n - CLOSE_JOBS_SHOWN}개`);
  }
  if (i.status === null) lines.push('ISO-TP 엔드포인트·대기 버스트·시뮬레이터 노드: 알 수 없음');
  else {
    const { isotp, bursts, nodes } = i.status;
    lines.push(`ISO-TP 엔드포인트: ${isotp ? `${isotp}개 닫힘` : '없음'}`);
    lines.push(`대기 중 버스트 송신: ${bursts ? `${bursts}개 취소` : '없음'}`);
    lines.push(`시뮬레이터 노드: ${nodes ? `${nodes}개 분리` : '없음'}`);
  }
  for (const e of i.errors) lines.push(`⚠ ${e}`);
  lines.push(VIEW_CLOSE_NOTE);
  return lines;
}

/** toast after a successful can.close; `result` = WriteHandle.result (may be undefined) */
export function closeDoneText(channel: string, result: unknown, i: CloseImpact | null): string {
  let text = `CAN ${channel} 닫힘`;
  if (typeof result === 'object' && result !== null) {
    const g = (result as { generation?: unknown }).generation;
    if (typeof g === 'number' && Number.isInteger(g) && g >= 0) text += ` (generation ${g})`;
  }
  if (i && i.jobs && i.jobs.length > 0) text += ` · 주기 송신 ${i.jobs.length}개 정지`;
  return text;
}

/** `can.channel_state` control event → 'open' | 'closed'; null for other kinds/channels/states */
export function channelStateOf(ev: Pick<LoomEvent, 'kind' | 'channel' | 'payload'>, channel: string): 'open' | 'closed' | null {
  if (ev.kind !== 'can.channel_state' || ev.channel !== channel) return null;
  const p = ev.payload;
  const st = typeof p === 'object' && p !== null ? (p as { state?: unknown }).state : undefined;
  return st === 'open' || st === 'closed' ? st : null;
}
