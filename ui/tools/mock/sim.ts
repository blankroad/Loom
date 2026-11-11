// Mock devices + write pipeline. Every write goes
//   requested -> accepted -> sent -> (device) -> confirmed | failed | outcome_unknown
// and ALL its events are published before the RPC reply (publish-before-reply,
// contract 01 §1.6). Origin comes from the connection (viewer = user,
// mock.agent = agent), never from params.
//
// Bounds (C2): pending approvals ≤ 32 (overflow: the write fails with
// approval_queue_full); one flood per kind at a time; UART line buffer ≤ 4 KiB.

import type { WriteState } from '../../src/proto/provisional';
import type { Emit, Hub } from './hub';

export interface Actor {
  origin: 'user' | 'agent' | 'script';
  clientId: string;
  /** viewer connection id (hello.conn_id) — the UART input owner names the viewer by it */
  connId?: string;
}

/** CLI exit code the agent would get for a lease preempted by the user (UI-T04, plan §0.1 E-UI-2) */
export const EXIT_PREEMPTED = 6;

export class MockRpcError extends Error {
  constructor(
    readonly code: number,
    message: string,
    readonly data?: Record<string, unknown>,
  ) {
    super(message);
  }
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const hex = (b: Uint8Array | number[]) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
const b64 = (s: string) => Buffer.from(s, 'latin1').toString('base64');
const MAX_APPROVALS = 32;
const FD_LENS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];

/** CAN DLC code for a byte length (FD: 12 B = 9 … 64 B = 15) */
export function dlcCode(len: number): number {
  const i = FD_LENS.indexOf(len);
  if (i < 0) throw new MockRpcError(-32602, `invalid CAN length ${len}`, { code: 'E_USAGE' });
  return i;
}

interface CanFrameSpec {
  id: number;
  ext: boolean;
  fd: boolean;
  brs: boolean;
  esi: boolean;
  data: Uint8Array;
}

/** `frame` (cansend: 123#0102, 18DAF110#…, 123##1<data> = FD+BRS) or structured fields */
export function parseCanFrame(p: { frame?: string; id?: number | string; ext?: boolean; fd?: boolean; brs?: boolean; esi?: boolean; data?: string; data_hex?: string }): CanFrameSpec {
  const usage = (m: string) => new MockRpcError(-32602, m, { code: 'E_USAGE' });
  if (p.frame !== undefined && p.id !== undefined) throw usage('give either frame or id/data, not both');
  let id: number;
  let ext: boolean;
  let fd = p.fd ?? false;
  let brs = p.brs;
  let dataHex: string;
  if (p.frame !== undefined) {
    const m = /^([0-9a-fA-F]{1,8})#(#([0-9a-fA-F]))?([0-9a-fA-F.]*)$/.exec(p.frame.trim());
    if (!m) throw usage(`bad frame ${p.frame}`);
    id = Number.parseInt(m[1]!, 16);
    ext = m[1]!.length > 3 || id > 0x7ff;
    if (m[2]) {
      fd = true;
      brs = (Number.parseInt(m[3]!, 16) & 1) === 1;
    }
    dataHex = m[4]!.replace(/\./g, '');
  } else {
    id = typeof p.id === 'string' ? Number.parseInt(p.id, p.id.startsWith('0x') ? 16 : 10) : (p.id ?? NaN);
    ext = p.ext ?? id > 0x7ff;
    dataHex = (p.data ?? p.data_hex ?? '').replace(/\s+/g, '');
  }
  if (!Number.isInteger(id) || id < 0 || id > (ext ? 0x1fffffff : 0x7ff)) throw usage('invalid can id');
  if (!/^([0-9a-fA-F]{2})*$/.test(dataHex)) throw usage('data: hex bytes');
  const data = Uint8Array.from(Buffer.from(dataHex, 'hex'));
  if (data.length > (fd ? 64 : 8)) throw usage(`data > ${fd ? 64 : 8} bytes`);
  if (!fd && brs) throw usage('brs needs fd');
  dlcCode(data.length);
  return { id, ext, fd, brs: fd ? (brs ?? true) : false, esi: fd ? (p.esi ?? false) : false, data };
}
const VIN = 'WVWZZZ1KZAW000001';

interface Pending {
  resolve(ok: boolean): void;
  requestId: string;
}

// schema/v1 SafeStateItem ids are `<domain>.<name>`; `method` runs with origin = user (A15)
export const SAFE_STATES = [
  { id: 'can.silence', label: 'CAN 주기 송신 중지', domain: 'can', channel: 'ch0', method: 'can.periodic.stop', risk: 'R0' },
  { id: 'uds.default_session', label: 'UDS 기본 세션 복귀 (10 01)', domain: 'uds', channel: 'bcm', method: 'uds.request', risk: 'R1' },
] as const;

export class Sim {
  private reqSeq = 0;
  private apprSeq = 0;
  policyConfirm = false;
  armOutcomeUnknown = false;
  readonly approvals = new Map<string, Pending>();
  private uartLine = new Map<string, string>();
  private owner = new Map<string, { owner: 'none' | 'user' | 'agent'; viewer?: string; timer?: NodeJS.Timeout }>();
  private agentCursor = 0n;
  /** user keystrokes of the current (unfinished) line per channel, for the Activity summary (≤ 256 chars) */
  private userLine = new Map<string, string>();
  /** UDS lease holder per target (user after [제어 가져오기]); bounded by the targets used (≤ 32) */
  readonly udsLease = new Map<string, { origin: 'user' | 'agent'; actor: string }>();
  private floods = new Map<string, NodeJS.Timeout>();
  /** frames/chunks emitted by the current or last flood per kind (≤ 2 keys) — perf E2E accounting */
  readonly floodEmitted = new Map<string, number>();

  constructor(private readonly hub: Hub) {
    hub.state.set('uart.input_owner', { dut: { owner: 'none', dirty: false, hold: false } });
    hub.state.set('core.latch', { on: false });
  }

  private pub(e: Emit) {
    return this.hub.publish(e);
  }

  // ---- write pipeline -------------------------------------------------------

  /** shared write pipeline (UART/CAN/UDS here; eth/psu sims reuse it) */
  async pipeline(
    a: Actor,
    base: Omit<Emit, 'origin' | 'payload' | 'request_id' | 'client_id'> & { payload: Record<string, unknown> },
    act: (stage: (ws: WriteState, extra?: Record<string, unknown>) => void, requestId: string) => Promise<{ final: WriteState; extra?: Record<string, unknown> }>,
  ): Promise<{ request_id: string; write_state: WriteState } & Record<string, unknown>> {
    const request_id = `r-${++this.reqSeq}`;
    let payload = { ...base.payload };
    const stage = (write_state: WriteState, extra: Record<string, unknown> = {}) => {
      payload = { ...payload, ...extra, write_state };
      this.pub({ ...base, origin: a.origin, client_id: a.clientId, request_id, risk: base.risk ?? 'R1', payload });
    };
    stage('requested');
    // a user write is an intervention the agent must hear about (others_since_last, ⇢ 전달됨)
    if (a.origin === 'user') this.activity(a, 'write', base.domain, base.channel, `${base.kind} ${String(base.payload.summary ?? '')}`.trim());
    if (this.hub.latch && a.origin !== 'user') {
      stage('failed', { error: 'stop_latched' });
      throw new MockRpcError(-32010, 'STOP latch is on', { code: 'stop_latched', request_id });
    }
    if (this.policyConfirm && a.origin === 'agent') {
      const ok = await this.approval(a, request_id, base);
      if (!ok) {
        stage('failed', { error: 'approval_denied' });
        throw new MockRpcError(-32011, 'approval denied', { code: 'approval_denied', request_id });
      }
    }
    stage('accepted');
    stage('sent');
    const r = await act(stage, request_id);
    let final = r.final;
    if (this.armOutcomeUnknown) {
      this.armOutcomeUnknown = false;
      final = 'outcome_unknown';
    }
    // UART's success terminal is `sent` (contract 02 §2.3): no duplicate row
    if (final !== 'sent') stage(final, final === 'outcome_unknown' ? { ...r.extra, error: 'no_confirmation_before_timeout' } : r.extra);
    const others = a.origin === 'agent' ? this.advanceAgentCursor(a) : null;
    return { request_id, write_state: final, ...(r.extra ?? {}), ...(others ? { others_since_last: others } : {}) };
  }

  private approval(a: Actor, requestId: string, base: { domain: string; channel?: string; kind: string; payload: Record<string, unknown> }): Promise<boolean> {
    if (this.approvals.size >= MAX_APPROVALS) return Promise.resolve(false);
    const approval_id = `ap-${++this.apprSeq}`;
    this.pub({
      domain: 'core',
      kind: 'core.approval_requested',
      origin: a.origin,
      client_id: a.clientId,
      request_id: requestId,
      risk: 'R2',
      payload: { approval_id, risk: 'R2', requester: a.clientId, domain: base.domain, channel: base.channel, display: { title: `${base.kind} 승인 필요`, fields: base.payload } },
    });
    return new Promise((resolve) => {
      const timer = setTimeout(() => this.resolveApproval(approval_id, 'expired'), 60_000);
      this.approvals.set(approval_id, {
        requestId,
        resolve: (ok) => {
          clearTimeout(timer);
          resolve(ok);
        },
      });
    });
  }

  resolveApproval(approvalId: string, decision: 'approve' | 'deny' | 'expired' | 'cancelled', by = 'mock'): boolean {
    const p = this.approvals.get(approvalId);
    if (!p) return false;
    this.approvals.delete(approvalId);
    this.pub({ domain: 'core', kind: 'core.approval_resolved', origin: 'user', client_id: by, request_id: p.requestId, payload: { approval_id: approvalId, decision } });
    p.resolve(decision === 'approve');
    return true;
  }

  /**
   * The agent's result carries what users did since its last result (`others_since_last`);
   * `core.agent_cursor` tells viewers their interventions reached the agent (⇢ 전달됨).
   * Counts user Activity entries (one per finished input line / user action), not keystrokes.
   */
  advanceAgentCursor(a: Actor): { count: number; origins: string[] } {
    const to = this.hub.seq;
    const from = this.agentCursor;
    const count = this.hub.ring.filter((s) => BigInt(s.ev.seq) > from && s.ev.origin === 'user' && s.ev.kind === 'core.activity').length;
    this.agentCursor = to;
    // schema/v1 agent_cursor_payload: others_count is a u64 decimal string
    this.pub({ domain: 'core', kind: 'core.agent_cursor', origin: 'agent', client_id: a.clientId, payload: { client_id: a.clientId, from_seq: String(from), to_seq: String(to), others_count: String(count) } });
    return { count, origins: count ? ['user'] : [] };
  }

  /**
   * core.activity. Payload = schema/v1 ActivityPayload {client_id, method, summary, range?{from,to}}
   * + CAN v1 `action` + the fields the shell adapter reads today (actor, verb, domain, channel,
   * range{seq_a,seq_b}) — superset until the adapter moves to the schema (report §4 C-6).
   */
  activity(a: Actor, verb: string, domain: string, channel: string | undefined, summary: string, o: { method?: string; range?: { seq_a: string; seq_b: string } } = {}): void {
    const range = o.range ? { from: o.range.seq_a, to: o.range.seq_b, seq_a: o.range.seq_a, seq_b: o.range.seq_b } : undefined;
    this.pub({
      domain: 'core',
      kind: 'core.activity',
      origin: a.origin,
      client_id: a.clientId,
      payload: { client_id: a.clientId, method: o.method ?? `${domain}.${verb}`, action: verb, verb, actor: a.origin, domain, ...(channel ? { channel } : {}), ...(range ? { range } : {}), summary },
    });
  }

  // ---- UART (bootshell on channel "dut") --------------------------------------

  private uartRx(channel: string, text: string): void {
    this.pub({ domain: 'uart', channel, kind: 'uart.data', dir: 'rx', origin: 'device', payload: { bytes: { encoding: 'base64', data: b64(text) } } });
  }

  private setOwner(channel: string, owner: 'none' | 'user' | 'agent', viewer?: string): void {
    const cur = this.owner.get(channel);
    if (cur?.timer) clearTimeout(cur.timer);
    const next: { owner: 'none' | 'user' | 'agent'; viewer?: string; timer?: NodeJS.Timeout } = { owner };
    // viewer = hello.conn_id of the typing tab (the UI compares it with its own conn_id)
    if (viewer) next.viewer = viewer;
    // user ownership lapses after 2 s of no input (mock simplification of A1 idle)
    if (owner === 'user') next.timer = setTimeout(() => this.setOwner(channel, 'none'), 2_000);
    this.owner.set(channel, next);
    if (cur?.owner === owner && cur.viewer === viewer) return;
    const payload = { owner, ...(viewer ? { viewer } : {}), dirty: owner === 'user', hold: false };
    this.hub.state.set('uart.input_owner', { ...(this.hub.state.get('uart.input_owner') ?? {}), [channel]: payload });
    this.pub({ domain: 'uart', channel, kind: 'uart.input_owner', origin: owner === 'agent' ? 'agent' : 'user', payload });
  }

  ownerOf(channel: string): 'none' | 'user' | 'agent' {
    return this.owner.get(channel)?.owner ?? 'none';
  }

  /** bytes reached the bootshell: echo + run complete lines */
  private shell(channel: string, text: string): void {
    setTimeout(() => {
      let line = this.uartLine.get(channel) ?? '';
      let out = '';
      for (const ch of text) {
        if (ch === '\r' || ch === '\n') {
          out += '\r\n' + this.command(line.trim());
          line = '';
        } else if (ch === '\x03') {
          out += '^C\r\ndut:~# ';
          line = '';
        } else {
          out += ch;
          line = (line + ch).slice(-4096);
        }
      }
      this.uartLine.set(channel, line);
      if (out) this.uartRx(channel, out);
    }, 3);
  }

  private command(cmd: string): string {
    const prompt = 'dut:~# ';
    if (!cmd) return prompt;
    if (cmd === 'uname -a') return `Linux dut 6.6.21 #1 SMP PREEMPT aarch64 GNU/Linux\r\n${prompt}`;
    if (cmd === 'help') return `commands: uname -a, dmesg, reboot, help\r\n${prompt}`;
    if (cmd.startsWith('dmesg')) return `[    0.000000] Booting Linux on physical CPU 0x0\r\n[    1.204311] mmc0: new HS200 MMC card\r\n${prompt}`;
    if (cmd === 'reboot') {
      setTimeout(() => this.bootLog('dut'), 300);
      return 'reboot: Restarting system\r\n';
    }
    return `sh: ${cmd.split(' ')[0]}: not found\r\n${prompt}`;
  }

  bootLog(channel: string): void {
    this.uartRx(channel, 'U-Boot 2024.01 (mock)\r\nStarting kernel ...\r\n[    0.000000] Linux version 6.6.21\r\n');
    setTimeout(() => this.uartRx(channel, '\r\nmock-dut login: root (auto)\r\ndut:~# '), 50);
  }

  /**
   * user keystrokes (write class ③): tx row + device echo; no write_state rows per key.
   * A finished line (CR/LF) or Ctrl-C returns the input to nobody (A1: the user's turn ends
   * with the line) and leaves one `core.activity{verb:"input"}` entry for the Activity strip.
   */
  uartInput(a: Actor, p: { channel?: string; data_hex?: string; input_seq?: number }): Record<string, unknown> {
    const channel = p.channel ?? 'dut';
    const bytes = Buffer.from(p.data_hex ?? '', 'hex');
    const text = bytes.toString('latin1');
    if (a.origin === 'user') this.setOwner(channel, 'user', a.connId ?? a.clientId);
    this.pub({ domain: 'uart', channel, kind: 'uart.data', dir: 'tx', origin: a.origin, client_id: a.clientId, payload: { bytes: { encoding: 'base64', data: bytes.toString('base64') }, input_seq: p.input_seq ?? 0 } });
    this.shell(channel, text);
    if (a.origin === 'user') {
      let line = this.userLine.get(channel) ?? '';
      for (const ch of text) {
        if (ch === '\r' || ch === '\n' || ch === '\x03') {
          const shown = `${line}${ch === '\x03' ? '^C' : '⏎'}`;
          this.activity(a, 'input', 'uart', channel, `input "${shown}"`);
          line = '';
          this.setOwner(channel, 'none');
        } else line = (line + ch).slice(-256);
      }
      this.userLine.set(channel, line);
    }
    return { accepted: true, input_seq: p.input_seq ?? 0 };
  }

  uartOwner(a: Actor, verb: 'yield' | 'clear' | 'takeover', channel = 'dut'): Record<string, unknown> {
    this.setOwner(channel, verb === 'takeover' ? 'user' : 'none', verb === 'takeover' ? a.clientId : undefined);
    return { owner: this.ownerOf(channel) };
  }

  async uartSend(a: Actor, p: { channel?: string; data?: string; data_hex?: string }) {
    const channel = p.channel ?? 'dut';
    const text = p.data ?? Buffer.from(p.data_hex ?? '', 'hex').toString('latin1');
    const wire = text.endsWith('\n') || text.endsWith('\r') ? text : `${text}\r`;
    return this.pipeline(a, { domain: 'uart', channel, kind: 'uart.write', dir: 'tx', payload: { bytes: { encoding: 'hex', data: hex(Buffer.from(wire, 'latin1')) }, summary: JSON.stringify(text) } }, async (stage, request_id) => {
      // the user holds the input: an agent send waits (deferred), ≤ 10 s
      if (a.origin === 'agent' && this.ownerOf(channel) === 'user') {
        stage('accepted', { deferred: true });
        // uart-ui: the chip shows "⏳ 에이전트 대기 "<text>" n/10s" from uart.agent_activity
        this.pub({ domain: 'uart', channel, kind: 'uart.agent_activity', origin: a.origin, client_id: a.clientId, payload: { kind: 'deferred_send', text: `${text}⏎`, elapsed_ms: 0, timeout_ms: 10_000 } });
        try {
          for (let waited = 0; this.ownerOf(channel) === 'user'; waited += 50) {
            if (waited >= 10_000) return { final: 'failed', extra: { error: 'input_owner_timeout' } };
            await sleep(50);
          }
        } finally {
          this.pub({ domain: 'uart', channel, kind: 'uart.agent_activity', origin: a.origin, client_id: a.clientId, payload: { kind: 'idle' } });
        }
      }
      // tx echo carries the write's request_id (same rule as CAN's bus echo, can-ui answer §1)
      this.pub({ domain: 'uart', channel, kind: 'uart.data', dir: 'tx', origin: a.origin, client_id: a.clientId, request_id, payload: { bytes: { encoding: 'base64', data: b64(wire) } } });
      this.shell(channel, wire);
      await sleep(5); // let the echo/response go out before the final stage
      return { final: 'sent' as WriteState }; // UART success terminal = sent (contract 02 §2.3)
    });
  }

  // ---- CAN (sim BCM answers 0x300 with 0x301) ---------------------------------

  /**
   * can.frame in the CAN v1 schema (docs/impl/requests/can-ui.md §2):
   * {id:number, ext, fd, brs, esi, rtr, dlc:<DLC code 0..15>, len:<bytes>, bytes{hex}} (+ frame_index)
   */
  canFrame(origin: Emit['origin'], channel: string, id: number, data: Uint8Array, dir: 'rx' | 'tx', extra: Partial<Emit> = {}, flags: { fd?: boolean; brs?: boolean; esi?: boolean; ext?: boolean; frameIndex?: number } = {}): void {
    const fd = flags.fd ?? data.length > 8;
    const payload: Record<string, unknown> = { id, ext: flags.ext ?? id > 0x7ff, fd, brs: fd ? (flags.brs ?? true) : false, esi: fd ? (flags.esi ?? false) : false, rtr: false, dlc: dlcCode(data.length), len: data.length, bytes: { encoding: 'hex', data: hex(data) } };
    if (flags.frameIndex !== undefined) payload.frame_index = flags.frameIndex;
    this.pub({ domain: 'can', channel, kind: 'can.frame', dir, origin, ...extra, payload });
  }

  /**
   * `can.send` (CAN v1 §3): `{channel, frame:"300#01"}` (cansend syntax) or `{id, ext, fd, brs, esi, data}`;
   * `count` (≤ 100) frames `gap_ms` apart; `dry_run` → `{risk, reason, bytes, policy}` with no side effect.
   * Write rows `can.tx` (request_id + write_state); each bus echo `can.frame` dir tx carries the same
   * top-level request_id. Legacy `data_hex` / string id accepted (provisional `can.tx.send` alias).
   */
  async canSend(a: Actor, p: { channel?: string; frame?: string; id?: number | string; ext?: boolean; fd?: boolean; brs?: boolean; esi?: boolean; data?: string; data_hex?: string; count?: number; gap_ms?: number; dry_run?: boolean }) {
    const channel = p.channel ?? 'ch0';
    const f = parseCanFrame(p);
    const count = Math.max(1, Math.min(100, Math.trunc(p.count ?? 1)));
    const gapMs = Math.max(0, Math.min(10_000, Math.trunc(p.gap_ms ?? 0)));
    const frameText = `${f.id.toString(16).toUpperCase().padStart(f.ext ? 8 : 3, '0')}#${f.fd ? '#' : ''}${hex(f.data).toUpperCase()}`;
    if (p.dry_run) return { risk: 'R1', reason: 'sim channel', bytes: hex(f.data), policy: 'auto', frame: frameText };
    return this.pipeline(a, { domain: 'can', channel, kind: 'can.tx', dir: 'tx', payload: { frame: frameText, id: f.id, ext: f.ext, fd: f.fd, brs: f.brs, esi: f.esi, rtr: false, dlc: dlcCode(f.data.length), len: f.data.length, bytes: { encoding: 'hex', data: hex(f.data) }, count, summary: frameText } }, async (_stage, request_id) => {
      for (let i = 0; i < count; i++) {
        if (i > 0 && gapMs) await sleep(gapMs);
        else await sleep(1);
        // bus echo carries the request_id: the Trace merges it into the TX row (CAN v1 §1)
        this.canFrame(a.origin, channel, f.id, f.data, 'tx', { client_id: a.clientId, request_id }, { ...f, ...(count > 1 ? { frameIndex: i } : {}) });
        if (f.id === 0x300) setTimeout(() => this.canFrame('device', channel, 0x301, Uint8Array.from([f.data[0] ?? 0, 0xa5]), 'rx'), 2);
      }
      return { final: 'confirmed' as WriteState, extra: { risk: 'R1', reason: 'sim channel' } };
    });
  }

  /**
   * Read-only trace query (CAN v1 §4, simplified): the newest `max` (≤ 1,000) frames of the channel in
   * the hub ring. An AGENT query leaves `core.activity{action:"read", domain:"can", channel, range}` —
   * the Trace's "에이전트가 읽은 범위" bar; viewer queries leave nothing.
   */
  canTraceQuery(a: Actor, p: { channel?: string; max?: number }): Record<string, unknown> {
    const channel = p.channel ?? 'ch0';
    const max = Math.min(1_000, Math.max(1, Number(p.max ?? 100)));
    const rows = this.hub.ring.filter((s) => s.ev.domain === 'can' && s.ev.channel === channel && s.ev.kind === 'can.frame').slice(-max).map((s) => s.ev);
    const first = rows[0];
    const last = rows[rows.length - 1];
    if (first && last && a.origin === 'agent') {
      this.activity(a, 'read', 'can', channel, `can trace ${channel} (${rows.length} rows)`, { method: 'can.trace.query', range: { seq_a: first.seq, seq_b: last.seq } });
    }
    const range = first && last ? { seq_a: first.seq, seq_b: last.seq } : undefined;
    // D5 envelope shape (data.rows + meta) plus the earlier flat fields
    return { rows, frames: rows, meta: { truncated: false, ...(range ? { range } : {}) }, ...(range ?? {}) };
  }

  startFlood(kind: 'can' | 'uart', rate: number, seconds: number): void {
    this.stopFlood(kind);
    this.floodEmitted.set(kind, 0);
    const tickMs = 10;
    const end = Date.now() + Math.min(60, Math.max(0.1, seconds)) * 1000;
    let n = 0;
    const t = setInterval(() => {
      if (Date.now() >= end) return this.stopFlood(kind);
      if (kind === 'can') {
        const per = Math.max(1, Math.round((Math.min(10_000, rate) * tickMs) / 1000));
        for (let i = 0; i < per; i++, n++) {
          this.canFrame('device', 'ch0', 0x400 + (n % 64), Uint8Array.from([n & 0xff, (n >> 8) & 0xff, 0, 0, 0, 0, 0, 0]), 'rx');
        }
        this.floodEmitted.set(kind, n);
      } else {
        const bytes = Math.max(1, Math.round((rate * tickMs) / 1000));
        const line = `[${(n++ * 0.01).toFixed(2)}] flood ${'x'.repeat(40)}\r\n`;
        this.uartRx('dut', line.repeat(Math.max(1, Math.round(bytes / line.length))));
      }
    }, tickMs);
    this.floods.set(kind, t);
  }

  floodRunning(): string[] {
    return [...this.floods.keys()];
  }

  stopFlood(kind: string): void {
    const t = this.floods.get(kind);
    if (t) clearInterval(t);
    this.floods.delete(kind);
  }

  stopAll(): void {
    for (const k of [...this.floods.keys()]) this.stopFlood(k);
  }

  // ---- UDS (read-did F190 answers after 0x78 x3) -------------------------------

  /** risk of a UDS request (mock rule: reads and TesterPresent are R0, everything else R2) */
  static udsRisk(sid: number): 'R0' | 'R2' {
    return sid === 0x22 || sid === 0x3e ? 'R0' : 'R2';
  }

  /**
   * uds.request. Mock-only params: `mock_pending` (number of 0x78 responsePending before the
   * answer to 22 F190; default 3, ≤ 20) and `mock_pending_ms` (gap between them, default 50).
   * - dry_run: preview only — NO write rows, no audit (uds-ui U3)
   * - a negative response ends the row in `failed` (UDS plan §6.1), never `confirmed`
   * - after a user [제어 가져오기] the agent's R1+ requests fail with exit 6 (lease preempted)
   */
  async udsRequest(a: Actor, p: { target?: string; hex?: string; dry_run?: boolean; mock_pending?: number; mock_pending_ms?: number }) {
    const target = p.target ?? 'bcm';
    const req = Buffer.from((p.hex ?? '').replace(/\s+/g, ''), 'hex');
    if (!req.length) throw new MockRpcError(-32602, 'empty request', { code: 'invalid_params' });
    const sid = req[0]!;
    const service = ({ 0x10: 'DiagnosticSessionControl', 0x22: 'ReadDataByIdentifier', 0x3e: 'TesterPresent', 0x2e: 'WriteDataByIdentifier' } as Record<number, string>)[sid] ?? `SID 0x${sid.toString(16)}`;
    const summary = sid === 0x22 && req.length >= 3 ? `read-did ${hex(req.subarray(1, 3)).toUpperCase()}` : service;
    const risk = Sim.udsRisk(sid);
    if (p.dry_run) {
      const did = sid === 0x22 && req.length >= 3 ? hex(req.subarray(1, 3)).toUpperCase() : null;
      return { dry_run: true, bytes: hex(req), service, did_name: did === 'F190' ? 'VIN' : did, risk, reason: risk === 'R0' ? 'read-only' : 'changes ECU state', needs: {}, current: null, diff: null };
    }
    const lease = this.udsLease.get(target);
    if (a.origin === 'agent' && lease?.origin === 'user' && risk !== 'R0') {
      const request_id = `r-${++this.reqSeq}`;
      const base = { domain: 'uds', channel: target, kind: 'uds.request', dir: 'tx' as const, origin: a.origin, client_id: a.clientId, request_id, risk, payload: { target, sid, service, summary, req_hex: hex(req), pending_count: 0 } };
      this.pub({ ...base, payload: { ...base.payload, write_state: 'requested' } });
      this.pub({ ...base, payload: { ...base.payload, write_state: 'failed', error: 'lease_preempted' } });
      const others = this.advanceAgentCursor(a);
      throw new MockRpcError(-32012, `UDS ${target}: control lease held by the user`, { code: 'lease_preempted', exit_code: EXIT_PREEMPTED, request_id, others_since_last: others });
    }
    const pending = Math.max(0, Math.min(20, Math.trunc(p.mock_pending ?? 3)));
    const pendingMs = Math.max(5, Math.min(1_000, Math.trunc(p.mock_pending_ms ?? 50)));
    const started = Date.now();
    return this.pipeline(a, { domain: 'uds', channel: target, kind: 'uds.request', dir: 'tx', risk, payload: { target, sid, service, summary, req_hex: hex(req), pending_count: 0 } }, async (stage) => {
      let resp: number[];
      if (sid === 0x22 && req[1] === 0xf1 && req[2] === 0x90) {
        for (let k = 1; k <= pending; k++) {
          await sleep(pendingMs);
          stage('sent', { pending_count: k, nrc: '0x78', nrc_name: 'requestCorrectlyReceivedResponsePending' });
        }
        await sleep(Math.max(20, pendingMs)); // the last pending stage stays on screen as long as the others
        resp = [0x62, 0xf1, 0x90, ...Buffer.from(VIN, 'latin1')];
      } else if (sid === 0x10) {
        resp = [0x50, req[1] ?? 1, 0x00, 0x32, 0x01, 0xf4];
      } else if (sid === 0x3e) {
        resp = [0x7e, 0x00];
      } else {
        await sleep(5);
        // negative response = failed (UDS plan §6.1): never a "confirmed" row with an NRC
        return { final: 'failed' as WriteState, extra: { resp_hex: hex([0x7f, sid, 0x11]), nrc: '0x11', nrc_name: 'serviceNotSupported', rtt_ms: Date.now() - started } };
      }
      await sleep(5);
      return { final: 'confirmed' as WriteState, extra: { nrc: undefined, nrc_name: undefined, resp_hex: hex(resp), rtt_ms: Date.now() - started, ...(sid === 0x22 ? { value: VIN, decoded: [{ did: 'F190', name: 'VIN', value: VIN }] } : {}) } };
    });
  }

  /** [제어 가져오기]: the lease moves to the caller; uds.state carries it to every viewer */
  udsTakeover(a: Actor, target: string): Record<string, unknown> {
    if (this.udsLease.size >= 32 && !this.udsLease.has(target)) throw new MockRpcError(-32602, 'too many targets', { code: 'invalid_params' });
    const holder = { origin: a.origin === 'user' ? ('user' as const) : ('agent' as const), actor: a.connId ?? a.clientId };
    this.udsLease.set(target, holder);
    this.pub({
      domain: 'uds',
      channel: target,
      kind: 'uds.state',
      origin: a.origin,
      client_id: a.clientId,
      payload: { target, link: 'up', backend: 'sim', session: { value: 'default', confidence: 'assumed' }, security: { level: 0 }, lease: { owner_origin: holder.origin, actor: holder.actor } },
    });
    this.activity(a, 'lease_takeover', 'uds', target, `UDS ${target} lease -> ${a.origin}`);
    return { owner: holder.origin };
  }
}
