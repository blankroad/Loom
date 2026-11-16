// UART module chrome — the agreed REV6.1 web workbench (work/reports/uart-workbench/opus/index.html, app.js
// renderSide/msScopeHTML/openSessionDialog/macro editor), same structure, order and labels, wired to the real
// engine (store.ts). The work area is the real Dockview (free split/resize/tabs).
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { DockviewApi, IDockviewPanel } from 'dockview-core';
import { useStoreVersion } from '../../store/hooks';
import { useServices } from '../../shell/ShellContext';
import type { ShellServices } from '../../shell/services';
import { closeGroup, splitGroup } from '../../shell/dock/dockHost';
import { I } from './Icon';
import { openTagText, sessionDot } from '../../panels/uart/paneLayout';
import { compileRules, HIGHLIGHT_COLORS, HIGHLIGHT_MAX_RULES, type HighlightRule } from '../../panels/uart/highlight';
import { BAUDS, COLORS, EOLS, frameOf, MS_LABEL, nid, SESSION_CHAR_DELAY_MAX, SESSION_LINE_DELAY_MAX, SESSION_RECONNECT_WINDOW_S, SESSION_SCROLLBACK, stepText, uartBench, type EolKey, type Macro, type MacroStep, type Session } from './workbench';
import './bench.css';

const fmtBytes = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} kB` : `${(n / 1048576).toFixed(2)} MB`);
const hex4 = (n: number | null | undefined) => (n == null ? '????' : n.toString(16).padStart(4, '0'));

function dockApi(s: ShellServices): DockviewApi | null {
  const d = s.dock.current as unknown as { api?: DockviewApi } | null;
  return d?.api ?? null;
}
function paneNoOfChannel(api: DockviewApi | null, channel: string): number | null {
  if (!api) return null;
  const p = api.panels.find((x) => (x.params as { panel?: string; props?: { channel?: string } }).panel === 'uart.terminal' && (x.params as { props?: { channel?: string } }).props?.channel === channel);
  return p ? api.groups.indexOf(p.group) + 1 : null;
}
const TERM = (channel: string) => ({ panel: 'uart.terminal', props: { channel } });

/** Show `channel`'s terminal and place it like REV6.1 placeSession: as a tab in the active pane, or in a new
 * split right/below of it. The device is never opened here. */
export function placeChannel(s: ShellServices, channel: string, where: 'tab' | 'right' | 'down' = 'tab'): void {
  const api = dockApi(s);
  const ref = api?.activeGroup ?? api?.groups[0];
  s.showSlot(TERM(channel));
  if (!api) return;
  const p: IDockviewPanel | undefined = api.panels.find((x) => (x.params as { panel?: string; props?: { channel?: string } }).panel === 'uart.terminal' && (x.params as { props?: { channel?: string } }).props?.channel === channel);
  if (!p) return;
  if (where === 'tab' && ref && p.group !== ref) p.api.moveTo({ group: ref, position: 'center' });
  else if (where !== 'tab' && ref) p.api.moveTo({ group: ref, position: where === 'right' ? 'right' : 'bottom' });
  p.api.setActive();
}

async function openAndPlace(s: ShellServices, sess: Session, where: 'tab' | 'right' | 'down'): Promise<void> {
  // the UART module's own dock: if the user switched tabs while the port was opening, the terminal is added to THIS
  // (now hidden, retained) dock — never into whichever workspace is in front, and no navigation
  const origin = s.dock.current;
  try {
    if (uartBench.channelState(sess.alias) !== 'open') {
      await uartBench.openSession(sess);
      // wait for the daemon's channel state (never assume the open succeeded)
      await new Promise<void>((resolve, reject) => {
        const done = () => uartBench.channelState(sess.alias) === 'open';
        if (done()) return resolve();
        const timer = setTimeout(() => { off(); reject(new Error('포트 열림 확인 없음 (8초) — Activity와 데몬 상태를 확인하세요')); }, 8_000);
        const off = s.shell.subscribe(() => { if (done()) { clearTimeout(timer); off(); resolve(); } });
      });
    }
    if (origin && s.dock.current !== origin) { origin.open(TERM(sess.alias)); return; }
    placeChannel(s, sess.alias, where);
  } catch (e) {
    s.toasts.push(`${sess.alias} 열기 실패: ${e instanceof Error ? e.message : String(e)}`);
  }
}

// ---------------------------------------------------------------- menus
interface MenuItem { label: string; run: () => void; disabled?: boolean; k?: string; header?: false }
interface MenuState { x: number; y: number; header?: string; items: (MenuItem | 'hr')[] }
function Menu({ m, close }: { m: MenuState; close: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onDown = (e: PointerEvent) => { if (!ref.current?.contains(e.target as Node)) close(); };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') close(); };
    window.addEventListener('pointerdown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    return () => { window.removeEventListener('pointerdown', onDown, true); window.removeEventListener('keydown', onKey, true); };
  }, [close]);
  const left = Math.min(m.x, window.innerWidth - 240);
  return <div ref={ref} className="menu" role="menu" style={{ left, top: Math.min(m.y, window.innerHeight - 40) }}>
    {m.header && <div className="mh">{m.header}</div>}
    {m.items.map((it, i) => it === 'hr' ? <hr key={i} /> : <button key={i} type="button" role="menuitem" data-mi={i} disabled={it.disabled} onClick={() => { close(); it.run(); }}>{it.label}{it.k && <span className="k">{it.k}</span>}</button>)}
  </div>;
}

// ---------------------------------------------------------------- dialogs
function Dialog({ children, onClose, label, wide }: { children: ReactNode; onClose: () => void; label: string; wide?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) {
      try { d.showModal(); } catch { d.setAttribute('open', ''); }
    }
  }, []);
  return <dialog ref={ref} aria-label={label} style={wide ? { width: 'min(720px, calc(100vw - 24px))' } : undefined} onCancel={(e) => { e.preventDefault(); onClose(); }}>{children}</dialog>;
}

type PortInfo = Awaited<ReturnType<typeof uartBench.listPorts>>[number];

function SessionDialog({ base, isNew, onClose }: { base: Session; isNew: boolean; onClose: () => void }) {
  const s = useServices();
  const [d, setD] = useState<Session>(base);
  const [ports, setPorts] = useState<PortInfo[] | null>(null);
  const [scanErr, setScanErr] = useState('');
  const [sel, setSel] = useState<string>(base.port || '');
  const [manual, setManual] = useState('');
  const [err, setErr] = useState('');
  const openNow = uartBench.channelState(base.alias) === 'open' && !isNew;
  const scan = async (announce: boolean) => {
    setScanErr('');
    try {
      const p = await uartBench.listPorts();
      setPorts(p);
      if (isNew && !base.port) {
        const used = new Set(uartBench.sessions.map((x) => x.port));
        const free = p.find((x) => !used.has(x.path)) ?? p[0];
        if (free) setSel(free.path);
        else setSel('__manual');
      }
      if (announce) s.toasts.push(`포트 ${p.length}개 감지됨`);
    } catch (e) {
      setPorts([]);
      setScanErr(`포트 조회 실패: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  useEffect(() => {
    const t = setTimeout(() => void scan(false), 0); // once on open (real uart.ports)
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- scan once on open
  }, []);
  const anno = (path: string) => {
    const t = uartBench.openTerminals.find((x) => x.path() === path && x.status().toLowerCase() === 'open');
    const saved = uartBench.sessions.filter((x) => x.port === path && x.id !== base.id);
    if (t) return t.channel === base.alias ? ` · 이 세션이 열고 있음` : ` · 사용 중: ${t.channel}`;
    if (saved.length) return ` · 저장됨: ${saved.map((x) => x.alias).join(', ')}`;
    return ' · 사용 가능';
  };
  const port = (sel === '__manual' ? manual : sel).trim();
  const busy = uartBench.openTerminals.find((x) => x.path() === port && x.channel !== base.alias && x.status().toLowerCase() === 'open');
  const info = ports?.find((p) => p.path === sel);
  const submit = (mode: 'save' | 'open') => {
    setErr('');
    if (!port) { setErr('포트를 고르세요 (감지된 포트가 없으면 새로고침 또는 수동 입력)'); return; }
    if (busy && mode === 'open') { setErr(`${port}는 ${busy.channel}가 사용 중입니다 — 같은 물리 포트는 한 탭만 열 수 있습니다. 위 버튼으로 그 세션으로 이동하세요.`); return; }
    const next: Session = { ...d, port, group: d.group.trim() || '기타' };
    if (!isNew && base.alias !== next.alias && uartBench.channelState(base.alias) === 'open') { setErr('열린 세션의 별칭은 바꿀 수 없습니다 — 포트를 닫은 뒤 변경하세요 (채널 이름 = 별칭)'); return; }
    const e = uartBench.saveSession(next);
    if (e) { setErr(e); return; }
    s.toasts.push(isNew ? `세션 ${next.alias} 저장됨` : `세션 ${next.alias} 설정 저장됨${openNow ? ' — 프레임 변경은 다시 연결할 때 적용' : ''}`);
    onClose();
    if (mode === 'open') void openAndPlace(s, uartBench.sessions.find((x) => x.alias === next.alias)!, 'tab');
  };
  const opt = <T extends string | number>(arr: (T | [T, string])[]) => arr.map((x) => Array.isArray(x) ? <option key={String(x[0])} value={x[0]}>{x[1]}</option> : <option key={String(x)} value={x}>{x}</option>);
  const groups = [...new Set(uartBench.sessions.map((x) => x.group))];
  // B6 operator options: values are clamped by sanitizeSession on save; here only NaN is kept out of the state
  const num = (v: string): number => (Number.isFinite(Number(v)) ? Number(v) : 0);
  const hl: HighlightRule[] = d.highlights ?? [];
  const setHl = (i: number, patch: Partial<HighlightRule>) => setD({ ...d, highlights: hl.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const hlErrors = compileRules(hl).errors;
  const applyPacingNow = async () => {
    try {
      await uartBench.applyPacing(base.alias, d.charDelayMs ?? 0, d.lineDelayMs ?? 0);
      s.toasts.push('전송 지연 적용됨');
    } catch (e) {
      s.toasts.push(`전송 지연 적용 실패: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  return <Dialog label={isNew ? '새 UART 세션' : `세션 설정 — ${base.alias}`} onClose={onClose}>
    <form method="dialog" noValidate onSubmit={(e) => { e.preventDefault(); submit(((e.nativeEvent as SubmitEvent).submitter as HTMLButtonElement | null)?.value === 'open' ? 'open' : 'save'); }}>
      <div className="dlg-h"><span className="ph-dot" style={{ ['--tag' as string]: d.color }} /><h2>{isNew ? '새 UART 세션' : `세션 설정 — ${base.alias}`}</h2>{openNow && <span className="ph-state connected">연결됨</span>}</div>
      <div className="dlg-b">
        <div className="grid2">
          <label className="fld"><span>별칭 (탭·대상 표시명, 중복 불가)</span><input name="alias" value={d.alias} required maxLength={32} className="mono" onChange={(e) => setD({ ...d, alias: e.target.value })} /></label>
          <label className="fld"><span>그룹</span><input name="group" value={d.group} list="ubGroups" onChange={(e) => setD({ ...d, group: e.target.value })} /><datalist id="ubGroups">{groups.map((g) => <option key={g} value={g} />)}</datalist></label>
          <div className="fld" style={{ gridColumn: '1/-1' }}><span>포트 — 감지된 UART 포트 (uart.ports)</span>
            <div style={{ display: 'flex', gap: 6 }}>
              <select name="portSel" className="mono" data-role="portsel" style={{ flex: 1, minWidth: 0 }} aria-label="감지된 포트" value={sel} onChange={(e) => setSel(e.target.value)}>
                {ports === null && <option value={sel}>{sel || '포트 조회 중…'}</option>}
                {ports?.map((p) => <option key={p.path} value={p.path}>{`${p.path} — ${[p.manufacturer, p.product].filter(Boolean).join(' ') || '장치 정보 없음'}${p.vid != null ? ` (${hex4(p.vid)}:${hex4(p.pid)})` : ''}${anno(p.path)}`}</option>)}
                {ports && !ports.length && <option value="" disabled>감지된 UART 포트 없음 — 장치 연결 후 새로고침</option>}
                {ports && sel && sel !== '__manual' && !ports.some((p) => p.path === sel) && <option value={sel}>{sel} — 현재 감지 안 됨 (저장된 값)</option>}
                <option value="__manual">수동 입력…</option>
              </select>
              <button type="button" className="btn" data-act="rescan" title="포트 다시 검색" onClick={() => void scan(true)}><I id="reset" />새로고침</button>
            </div>
            {sel === '__manual' && <input name="portManual" className="inp mono" data-role="portman" placeholder="수동 경로 (감지되지 않는 포트, 예: /dev/ttyS1, COM7, sim://bootshell)" value={manual} onChange={(e) => setManual(e.target.value)} aria-label="수동 포트 경로" />}
            <div className="hint" data-role="portinfo" aria-live="polite">
              {scanErr || (sel === '__manual' ? '감지 목록에 없는 경로만 직접 입력하세요. 열 때 같은 포트를 쓰는 탭이 있으면 거절됩니다.' : info ? `${[info.manufacturer, info.product].filter(Boolean).join(' ') || '장치 정보 없음'}${info.vid != null ? ` · VID:PID ${hex4(info.vid)}:${hex4(info.pid)}` : ''}${info.serial ? ` · SN ${info.serial}` : ''}` : '')}
              {busy && <div style={{ marginTop: 6, display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}><b style={{ color: 'var(--warn)' }}>사용 중: {busy.channel}</b>
                <button type="button" className="btn sm" data-act="port-focus" onClick={() => { onClose(); busy.activate(); }}>그 탭으로 이동</button>
                <button type="button" className="btn sm" data-act="port-move" onClick={() => { onClose(); placeChannel(s, busy.channel, 'tab'); }}>활성 pane으로 옮기기</button></div>}
            </div>
          </div>
        </div>
        <fieldset><legend>직렬 프레임</legend><div className="grid4">
          <label className="fld"><span>Baud</span><select name="baud" className="mono" value={d.baud} onChange={(e) => setD({ ...d, baud: Number(e.target.value) })}>{opt(BAUDS.includes(d.baud) ? BAUDS : [...BAUDS, d.baud])}</select></label>
          <label className="fld"><span>Data bits</span><select name="dataBits" className="mono" value={d.dataBits} onChange={(e) => setD({ ...d, dataBits: Number(e.target.value) as Session['dataBits'] })}>{opt([5, 6, 7, 8])}</select></label>
          <label className="fld"><span>Parity</span><select name="parity" className="mono" value={d.parity} onChange={(e) => setD({ ...d, parity: e.target.value as Session['parity'] })}>{opt<string>([['N', 'None'], ['E', 'Even'], ['O', 'Odd']])}</select></label>
          <label className="fld"><span>Stop bits</span><select name="stopBits" className="mono" value={d.stopBits} onChange={(e) => setD({ ...d, stopBits: Number(e.target.value) as 1 | 2 })}>{opt([1, 2])}</select></label>
          <label className="fld"><span>Flow control</span><select name="flow" className="mono" value={d.flow} onChange={(e) => setD({ ...d, flow: e.target.value as Session['flow'] })}>{opt<string>(['none', 'RTS/CTS', 'XON/XOFF'])}</select></label>
          <label className="fld"><span>전송 줄 끝</span><select name="eol" className="mono" value={d.eol} onChange={(e) => setD({ ...d, eol: e.target.value as EolKey })}>{opt<string>(Object.keys(EOLS))}</select></label>
          <label className="fld"><span>태그 색</span><select name="color" value={d.color} onChange={(e) => setD({ ...d, color: e.target.value })}>{opt<string>(COLORS)}</select></label>
        </div><p className="hint" style={{ margin: '8px 0 0' }}>장치 baud와 다르게 설정하면 수신이 깨진 문자로 보이고 명령이 통하지 않습니다. Mark/Space 패리티와 1.5 stop bit는 엔진이 지원하지 않습니다.</p></fieldset>
        <fieldset data-role="ops"><legend>운영 옵션</legend>
          <div className="grid4">
            <label className="fld"><span>문자 지연 ms</span><input type="number" name="charDelayMs" className="mono" min={0} max={SESSION_CHAR_DELAY_MAX} value={d.charDelayMs ?? 0} onChange={(e) => setD({ ...d, charDelayMs: num(e.target.value) })} /></label>
            <label className="fld"><span>줄 지연 ms</span><input type="number" name="lineDelayMs" className="mono" min={0} max={SESSION_LINE_DELAY_MAX} value={d.lineDelayMs ?? 0} onChange={(e) => setD({ ...d, lineDelayMs: num(e.target.value) })} /></label>
            <label className="fld"><span>열 때 네이티브 로그</span><select name="log" value={d.log ?? 'off'} onChange={(e) => setD({ ...d, log: e.target.value as NonNullable<Session['log']> })}><option value="off">끔</option><option value="log">.log</option><option value="jsonl">.jsonl</option></select></label>
            <label className="fld"><span>스크롤백 줄</span><input type="number" name="scrollback" className="mono" min={SESSION_SCROLLBACK.min} max={SESSION_SCROLLBACK.max} step={1000} value={d.scrollback ?? SESSION_SCROLLBACK.def} onChange={(e) => setD({ ...d, scrollback: num(e.target.value) })} /></label>
          </div>
          <div className="chk-row" style={{ marginTop: 6 }}>
            <label className="chk"><input type="checkbox" name="reconnect" checked={d.reconnect === true} onChange={(e) => setD({ ...d, reconnect: e.target.checked })} /> 분리되면 같은 장치를 다시 연결 (새 세대)</label>
            <label className="chk"><span>최대 s</span> <input type="number" name="reconnectWindowS" className="mono" min={SESSION_RECONNECT_WINDOW_S.min} max={SESSION_RECONNECT_WINDOW_S.max} disabled={d.reconnect !== true} value={d.reconnectWindowS ?? SESSION_RECONNECT_WINDOW_S.def} onChange={(e) => setD({ ...d, reconnectWindowS: num(e.target.value) })} /></label>
            {openNow && <button type="button" className="btn" data-act="pacing-apply" onClick={() => void applyPacingNow()}>지금 적용</button>}
          </div>
          {openNow && <p className="hint" style={{ margin: '6px 0 0' }}>재연결·로그 설정은 다음 열기부터 적용</p>}
          <div className="fld" style={{ marginTop: 8 }}><span>강조 규칙 (최대 {HIGHLIGHT_MAX_RULES}개 · 화면 장식만, 장치 바이트·저널은 그대로)</span>
            {hl.map((r, i) => <div key={i} className="chk-row" data-role="hlrow">
              <input type="text" name="hlPattern" className="inp mono" value={r.pattern} maxLength={256} aria-label={`강조 규칙 ${i + 1} 패턴`} onChange={(e) => setHl(i, { pattern: e.target.value })} />
              <label className="chk"><input type="checkbox" name="hlRegex" checked={r.regex} onChange={(e) => setHl(i, { regex: e.target.checked })} /> .*</label>
              <label className="chk"><input type="checkbox" name="hlCase" checked={r.caseSensitive} onChange={(e) => setHl(i, { caseSensitive: e.target.checked })} /> Aa</label>
              <select name="hlColor" value={r.color} aria-label={`강조 규칙 ${i + 1} 색`} onChange={(e) => setHl(i, { color: e.target.value as HighlightRule['color'] })}>{HIGHLIGHT_COLORS.map((c) => <option key={c} value={c}>{c}</option>)}</select>
              <button type="button" className="btn sm" data-act="hl-del" onClick={() => setD({ ...d, highlights: hl.filter((_, j) => j !== i) })}>삭제</button>
            </div>)}
            <div><button type="button" className="btn sm" data-act="hl-add" disabled={hl.length >= HIGHLIGHT_MAX_RULES} onClick={() => setD({ ...d, highlights: [...hl, { pattern: '', regex: false, caseSensitive: false, color: 'yellow' }] })}>규칙 추가</button></div>
            <div className="err-text" data-role="hlerr">{hlErrors.map((e) => `규칙 ${e.index + 1}: ${e.message}`).join(' · ')}</div>
          </div>
        </fieldset>
        <div className="chk-row">
          <label className="chk"><input type="checkbox" name="ts" checked={d.ts} onChange={(e) => setD({ ...d, ts: e.target.checked })} /> 타임스탬프</label>
        </div>
        <div className="err-text" data-role="err" role="alert">{err}</div>
      </div>
      <div className="dlg-f">
        {!isNew && <><button type="button" className="btn danger" data-act="dlg-del-session" onClick={() => { uartBench.deleteSession(base.id); s.toasts.push(`세션 ${base.alias} 삭제됨 — 고정 매크로는 미해결(실행 차단)로 남습니다`); onClose(); }}><I id="trash" />삭제</button>
          <button type="button" className="btn" data-act="dlg-dup-session" onClick={() => { const c = { ...base, id: nid('s-'), alias: `${base.alias}-2`.slice(0, 32), port: '' }; onClose(); setTimeout(() => window.dispatchEvent(new CustomEvent('loom:uart-session-dialog', { detail: { session: c, isNew: true } })), 0); }}><I id="copy" />복제</button></>}
        <span className="grow" />
        <button type="button" className="btn" data-close onClick={onClose}>취소</button>
        {isNew ? <><button type="submit" className="btn" value="save">저장</button><button type="submit" className="btn primary" value="open">저장 후 활성 pane에 열기</button></>
          : <button type="submit" className="btn primary" value="save">저장</button>}
      </div>
    </form>
  </Dialog>;
}

function MacroDialog({ base, onClose }: { base: Macro; onClose: () => void }) {
  const s = useServices();
  const [m, setM] = useState<Macro>(() => JSON.parse(JSON.stringify(base)) as Macro);
  const [tmode, setTmode] = useState<'panel' | 'fixed'>(base.fixed.length ? 'fixed' : 'panel');
  const [capturing, setCapturing] = useState(false);
  const [err, setErr] = useState('');
  const unresolved = m.fixed.filter((id) => !uartBench.sessions.some((x) => x.id === id));
  const setStep = (i: number, st: MacroStep) => setM({ ...m, steps: m.steps.map((x, j) => (j === i ? st : x)) });
  const save = () => {
    if (!m.name.trim()) { setErr('이름을 입력하세요'); return; }
    if (!m.steps.length) { setErr('step이 하나 이상 필요합니다'); return; }
    const fixed = tmode === 'fixed' ? m.fixed.filter((id) => uartBench.sessions.some((x) => x.id === id)) : [];
    if (tmode === 'fixed' && !fixed.length) { setErr('고정 대상을 하나 이상 고르세요'); return; }
    if (m.hotkey && uartBench.macros.some((x) => x.id !== m.id && x.hotkey === m.hotkey)) { setErr(`단축키 ${m.hotkey}는 다른 매크로가 씁니다`); return; }
    uartBench.saveMacro({ ...m, name: m.name.trim(), fixed });
    s.toasts.push(`매크로 ${m.name.trim()} 저장됨`);
    onClose();
  };
  return <Dialog label={`매크로 편집 — ${base.name || '새 매크로'}`} onClose={onClose} wide>
    <form method="dialog" noValidate onSubmit={(e) => { e.preventDefault(); save(); }}>
      <div className="dlg-h"><I id="macro" /><h2>{uartBench.macros.some((x) => x.id === base.id) ? `매크로 편집 — ${base.name}` : '새 매크로'}</h2></div>
      <div className="dlg-b">
        <div className="grid2">
          <label className="fld"><span>이름</span><input name="name" value={m.name} onChange={(e) => setM({ ...m, name: e.target.value })} required /></label>
          <div className="fld"><span>단축키 (터미널 입력 중에도 동작)</span>
            <button type="button" className={`btn hotkey-btn${capturing ? ' capturing' : ''}`} data-act="cap-hotkey" onClick={() => setCapturing(true)} onKeyDown={(e) => {
              if (!capturing) return;
              e.preventDefault();
              if (e.key === 'Escape') { setCapturing(false); return; }
              if (e.key === 'Backspace' || e.key === 'Delete') { setM({ ...m, hotkey: '' }); setCapturing(false); return; }
              if (['Shift', 'Control', 'Alt', 'Meta'].includes(e.key)) return;
              const k = `${e.ctrlKey ? 'Ctrl+' : ''}${e.altKey ? 'Alt+' : ''}${e.shiftKey ? 'Shift+' : ''}${e.key.length === 1 ? e.key.toUpperCase() : e.key}`;
              if (!/^F\d{1,2}$/.test(k) && !/^(Ctrl|Alt)\+/.test(k)) { setErr('단축키는 F1–F12 또는 Ctrl/Alt 조합만 (일반 키는 장치로 가야 합니다)'); return; }
              if (/^Alt\+(Shift\+)?(=|-|Enter|W|1|2|0|F|Arrow)/.test(k)) { setErr(`${k}는 작업대 예약 키입니다`); return; }
              setErr(''); setM({ ...m, hotkey: k }); setCapturing(false);
            }}>{capturing ? '키를 누르세요… (Esc 취소, Backspace 지우기)' : m.hotkey || '없음 — 눌러서 지정'}</button></div>
        </div>
        <label className="chk"><input type="checkbox" checked={m.pinned} onChange={(e) => setM({ ...m, pinned: e.target.checked })} /> pane 매크로 메뉴 맨 위에 표시 (★)</label>
        <fieldset><legend>실행 대상</legend>
          <div className="chk-row">
            <label className="chk"><input type="radio" name="tmode" value="panel" checked={tmode === 'panel'} onChange={() => setTmode('panel')} /> 매크로 패널의 실행 대상 (활성 / 연결된 모두 / 선택)</label>
            <label className="chk"><input type="radio" name="tmode" value="fixed" checked={tmode === 'fixed'} onChange={() => setTmode('fixed')} /> 고정 대상</label>
          </div>
          {tmode === 'fixed' && <div className="chk-row" style={{ marginTop: 6 }}>
            {uartBench.sessions.map((x) => <label key={x.id} className="chk"><input type="checkbox" name="fixed" value={x.id} checked={m.fixed.includes(x.id)} onChange={(e) => setM({ ...m, fixed: e.target.checked ? [...m.fixed, x.id] : m.fixed.filter((id) => id !== x.id) })} /> <b>{x.alias}</b> <small className="mono">{x.port}</small></label>)}
            {!uartBench.sessions.length && <span className="hint">저장된 세션이 없습니다</span>}
            {unresolved.map((id) => <span key={id} className="hint" data-role="fixed-unresolved">미해결 {id} (저장된 세션 없음) — 저장하면 제거됩니다</span>)}
          </div>}
        </fieldset>
        <fieldset><legend>변수 <span className="hint">— step에서 {'${이름}'}으로 사용, 실행 시 값이 비어 있으면 묻습니다</span></legend>
          {m.vars.map((v, i) => <div key={i} className="var-row"><input value={v.name} aria-label="변수 이름" onChange={(e) => setM({ ...m, vars: m.vars.map((x, j) => (j === i ? { ...x, name: e.target.value.replace(/\W/g, '') } : x)) })} /><input value={v.def} aria-label="기본값" placeholder="기본값" onChange={(e) => setM({ ...m, vars: m.vars.map((x, j) => (j === i ? { ...x, def: e.target.value } : x)) })} /><button type="button" className="ibtn danger" data-va="del" aria-label="변수 삭제" onClick={() => setM({ ...m, vars: m.vars.filter((_, j) => j !== i) })}><I id="trash" /></button></div>)}
          <button type="button" className="btn sm" style={{ marginTop: 6 }} data-va="add" onClick={() => setM({ ...m, vars: [...m.vars, { name: `v${m.vars.length + 1}`, def: '' }] })}><I id="plus" />변수 추가</button>
        </fieldset>
        <fieldset><legend>Steps</legend>
          {m.steps.map((st, i) => <div key={i} className="step-row">
            <span className="n">{i + 1}</span>
            <select data-f="type" value={st.type} aria-label="step 종류" onChange={(e) => setStep(i, e.target.value === 'send' ? { type: 'send', text: '', eol: 'session' } : e.target.value === 'delay' ? { type: 'delay', ms: 500 } : { type: 'wait', text: '', ms: 2000 })}><option value="send">send</option><option value="wait">wait</option><option value="delay">delay</option></select>
            {st.type === 'delay' ? <span className="hint">대기</span> : <input className="val" data-f="text" value={st.text} placeholder={st.type === 'send' ? '보낼 텍스트 (빈 칸 = Enter만)' : '기다릴 텍스트'} aria-label="값" onChange={(e) => setStep(i, { ...st, text: e.target.value })} />}
            {st.type === 'send' ? <select className="ms" data-f="eol" value={st.eol} aria-label="줄 끝" onChange={(e) => setStep(i, { ...st, eol: e.target.value as 'session' | EolKey })}><option value="session">세션 EOL</option>{Object.keys(EOLS).map((k) => <option key={k} value={k}>{k}</option>)}</select>
              : <input className="ms" data-f="ms" type="number" min={1} max={600000} value={st.ms} aria-label="ms" onChange={(e) => setStep(i, { ...st, ms: Number(e.target.value) })} />}
            <span className="sacts">
              <button type="button" className="ibtn" data-sa="up" aria-label="위로" disabled={i === 0} onClick={() => { const a = [...m.steps]; [a[i - 1], a[i]] = [a[i]!, a[i - 1]!]; setM({ ...m, steps: a }); }}>↑</button>
              <button type="button" className="ibtn" data-sa="down" aria-label="아래로" disabled={i === m.steps.length - 1} onClick={() => { const a = [...m.steps]; [a[i + 1], a[i]] = [a[i]!, a[i + 1]!]; setM({ ...m, steps: a }); }}>↓</button>
              <button type="button" className="ibtn danger" data-sa="del" aria-label="step 삭제" onClick={() => setM({ ...m, steps: m.steps.filter((_, j) => j !== i) })}><I id="trash" /></button>
            </span>
          </div>)}
          <div style={{ display: 'flex', gap: 6, marginTop: 6 }}>
            <button type="button" className="btn sm" data-add="send" onClick={() => setM({ ...m, steps: [...m.steps, { type: 'send', text: '', eol: 'session' }] })}><I id="send" />send</button>
            <button type="button" className="btn sm" data-add="wait" onClick={() => setM({ ...m, steps: [...m.steps, { type: 'wait', text: '', ms: 2000 }] })}>wait</button>
            <button type="button" className="btn sm" data-add="delay" onClick={() => setM({ ...m, steps: [...m.steps, { type: 'delay', ms: 500 }] })}>delay</button>
          </div>
          <p className="hint" style={{ margin: '8px 0 0' }}>wait는 데몬 uart.expect(from now)로 실제 수신을 기다립니다 — 직전 send보다 먼저 등록되어 빠른 응답도 놓치지 않습니다. 시간 초과 시 매크로는 실패로 멈추고 재전송하지 않습니다.</p>
        </fieldset>
        <pre className="preview">{m.steps.map((st, i) => `${i + 1}. ${stepText(st)}`).join('\n') || '(step 없음)'}</pre>
        <div className="err-text" role="alert">{err}</div>
      </div>
      <div className="dlg-f"><span className="grow" /><button type="button" className="btn" data-close onClick={onClose}>취소</button><button type="submit" className="btn primary">저장</button></div>
    </form>
  </Dialog>;
}

function RunVarsDialog({ m, from, onClose }: { m: Macro; from?: string; onClose: () => void }) {
  const [vals, setVals] = useState<Record<string, string>>(() => Object.fromEntries(m.vars.map((v) => [v.name, uartBench.varVals[m.id]?.[v.name] ?? v.def])));
  const scope = uartBench.resolveScope(m, from);
  return <Dialog label={`${m.name} 변수`} onClose={onClose}>
    <form method="dialog" noValidate onSubmit={(e) => {
      e.preventDefault();
      uartBench.varVals[m.id] = { ...uartBench.varVals[m.id], ...vals };
      uartBench.bump(true);
      onClose();
      uartBench.requestRun(m, from, vals);
    }}>
      <div className="dlg-h"><I id="play" /><h2>{m.name} — 변수 값</h2></div>
      <div className="dlg-b">
        <div className="target-card" data-role="target"><span className="dot" /><b>{scope.label}</b><small>값을 저장하면 바로 실행합니다</small></div>
        {m.vars.map((v, i) => <label key={v.name} className="fld"><span>{v.name}</span><input name={`var:${v.name}`} data-missing={(vals[v.name] ?? '') === ''} autoFocus={i === 0} value={vals[v.name] ?? ''} onChange={(e) => setVals({ ...vals, [v.name]: e.target.value })} className="mono" /></label>)}
      </div>
      <div className="dlg-f"><span className="grow" /><button type="button" className="btn" onClick={onClose}>취소</button><button type="submit" className="btn primary" data-role="go" disabled={m.vars.some((v) => (vals[v.name] ?? '') === '')}><I id="play" />저장 후 실행</button></div>
    </form>
  </Dialog>;
}

function KeysDialog({ onClose }: { onClose: () => void }) {
  const rows: [string, string][] = [
    ['Alt+1 / Alt+2 / Alt+0', '세션 / 매크로 패널 · 패널 접기'],
    ['Alt+Shift+=  /  Alt+Shift+-', '활성 pane 오른쪽 / 아래 분할'],
    ['Alt+Shift+Enter', '활성 pane 최대화 / 복원'],
    ['Alt+Shift+W', '활성 pane 닫기 (포트는 열린 채로)'],
    ...uartBench.macros.filter((m) => m.hotkey).map((m): [string, string] => [m.hotkey, `매크로 ${m.name} 실행`]),
    ['Ctrl-] x', 'STOP — agent·script 쓰기와 잡 잠금, 실행 중 매크로 중단'],
    ['Ctrl-] Ctrl-]', '0x1D를 포커스된 터미널로'],
    ['Ctrl+C (선택 있음)', '복사 — 선택이 없으면 장치로 ^C'],
    ['터미널 밖에서 Esc', '열린 메뉴·대화상자 닫기 (터미널 안의 Esc는 장치로 전송)'],
  ];
  return <Dialog label="단축키" onClose={onClose}>
    <div className="dlg-h"><I id="kbd" /><h2>단축키</h2></div>
    <div className="dlg-b"><div className="keys-tbl">{rows.flatMap(([k, v]) => [<kbd key={`k${k}`}>{k}</kbd>, <span key={`v${k}`}>{v}</span>])}</div>
      <p className="hint">작업대 예약 키와 매크로 단축키는 터미널에 포커스가 있어도 장치로 가지 않습니다.</p></div>
    <div className="dlg-f"><button type="button" className="btn primary" onClick={onClose}>닫기</button></div>
  </Dialog>;
}

function JsonDialog({ mode, onClose }: { mode: 'export' | 'import'; onClose: () => void }) {
  const s = useServices();
  const [text, setText] = useState(mode === 'export' ? uartBench.exportJson() : '');
  const [err, setErr] = useState('');
  const download = () => {
    const blob = new Blob([text], { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `loom-uart-${new Date().toISOString().slice(0, 10)}.json`;
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 5_000);
  };
  return <Dialog label={mode === 'export' ? '내보내기' : '가져오기'} onClose={onClose} wide>
    <div className="dlg-h"><I id={mode === 'export' ? 'down' : 'up'} /><h2>{mode === 'export' ? '세션·매크로 내보내기 (JSON)' : '세션·매크로 가져오기 (JSON)'}</h2></div>
    <div className="dlg-b">
      <textarea data-role="json" className="inp mono" style={{ height: 280, padding: 8 }} value={text} readOnly={mode === 'export'} onChange={(e) => setText(e.target.value)} placeholder="내보낸 JSON을 붙여 넣으세요 (REV6 프로토타입 내보내기도 가능)" />
      {mode === 'import' && <input type="file" accept="application/json,.json" aria-label="JSON 파일" onChange={(e) => { const f = e.target.files?.[0]; if (f) void f.text().then(setText); }} />}
      <div className="err-text" role="alert">{err}</div>
    </div>
    <div className="dlg-f"><span className="grow" />
      {mode === 'export' ? <><button type="button" className="btn" onClick={() => void navigator.clipboard?.writeText(text).then(() => s.toasts.push('클립보드에 복사됨'), () => s.toasts.push('클립보드 쓰기 실패'))}><I id="copy" />복사</button><button type="button" className="btn" onClick={download}><I id="down" />파일로 저장</button><button type="button" className="btn primary" onClick={onClose}>닫기</button></>
        : <><button type="button" className="btn" onClick={onClose}>취소</button><button type="button" className="btn primary" onClick={() => { try { s.toasts.push(uartBench.importJson(text)); onClose(); } catch (e) { setErr(`JSON 오류: ${e instanceof Error ? e.message : String(e)}`); } }}>가져오기</button></>}
    </div>
  </Dialog>;
}

// ---------------------------------------------------------------- side panel
function MacroScope() {
  useStoreVersion(uartBench);
  const open = uartBench.openTerminals;
  const ms = uartBench.ms;
  const label = uartBench.resolveScope({ fixed: [] }).label;
  return <div className="mscope" role="group" aria-label="매크로 실행 대상">
    <div className="mscope-row"><span className="mscope-l">실행 대상</span><span className="seg" role="radiogroup" aria-label="매크로 실행 대상">
      {(['active', 'all', 'selected'] as const).map((k) => <button key={k} type="button" role="radio" aria-checked={ms.mode === k} aria-pressed={ms.mode === k} data-act="ms-mode" data-mode={k} onClick={() => { uartBench.ms = { ...ms, mode: k }; uartBench.bump(); }}>{MS_LABEL[k]}</button>)}
    </span></div>
    <div className={`mscope-to${ms.mode !== 'active' ? ' multi' : ''}`} data-role="ms-recipients" aria-live="polite">{label}</div>
    {ms.mode === 'selected' && <div className="mscope-pick" role="group" aria-label="매크로를 보낼 세션 선택">
      {open.length ? open.map((t) => {
        const why = uartBench.bcWhy(t);
        return <label key={t.channel} className={`chk${why ? ' dis' : ''}`}><input type="checkbox" data-act="ms-sel" value={t.channel} checked={ms.sel.includes(t.channel)} disabled={!!why} onChange={(e) => { uartBench.ms = { ...ms, sel: e.target.checked ? [...ms.sel, t.channel] : ms.sel.filter((x) => x !== t.channel) }; uartBench.bump(); }} /> <b>{t.channel}</b> <small>{t.path() ?? ''}{why ? ` · ${why} — 제외` : ''}</small></label>;
      }) : <span className="hint">열린 세션 없음</span>}
      {ms.sel.filter((c) => !uartBench.terminal(c)).map((c) => <span key={c} className="hint">{c} · 열려 있지 않음 — 제외</span>)}
    </div>}
    <div className="mscope-note">▶·단축키 매크로에만 적용 — 키 입력 대상(툴바)과 별개. 실행할 때 수신자를 고정하므로 새로 연 세션은 실행 중에 끼지 않습니다. “고정” 매크로는 이 설정 대신 자기 대상으로 갑니다.</div>
  </div>;
}

function Side({ openMenu, dlg }: { openMenu: (m: MenuState) => void; dlg: (d: DlgState) => void }) {
  const s = useServices();
  useStoreVersion(uartBench);
  useStoreVersion(s.shell);
  const ui = uartBench.ui;
  const close = <button type="button" className="ibtn" data-act="collapse-side" title="패널 접기 (Alt+0)" aria-label="패널 접기" onClick={() => { uartBench.ui = { ...ui, side: null }; uartBench.bump(true); }}><I id="x" /></button>;
  const api = dockApi(s);
  const runMacro = (m: Macro, from?: string) => {
    if (uartBench.missingVars(m).length) dlg({ kind: 'run', macro: m, from });
    else uartBench.requestRun(m, from);
  };
  if (ui.side === 'macros') {
    return <>
      <div className="side-head"><h2>매크로</h2><span className="cnt">{uartBench.macros.length}</span>
        <button type="button" className="ibtn" data-act="new-macro" title="새 매크로" aria-label="새 매크로" onClick={() => dlg({ kind: 'macro', macro: newMacro() })}><I id="plus" /></button>{close}</div>
      <MacroScope />
      <div className="side-body" role="list">
        {uartBench.macros.map((m) => {
          const runs = [...uartBench.runs.values()].filter((r) => r.status === 'running' && r.macroId === m.id);
          const tgt = uartBench.resolveScope(m).label;
          const unresolved = m.fixed.some((sid) => !uartBench.sessions.some((x) => x.id === sid));
          return <div key={m.id} className={`mac${runs.length ? ' running' : ''}`} role="listitem" data-mid={m.id}>
            {runs.length ? <button type="button" className="btn-stop mac-stop" data-act="stop-macro-all" title={`이 매크로의 실행 대상 ${runs.length}곳 모두 중지 (남은 step 송신 안 함)`} aria-label={`${m.name} 전체 중지`} onClick={() => uartBench.stopMacroAll(m.id)}><I id="stop" />전체 중지</button>
              : <button type="button" className="ibtn mac-run" data-act="run-macro" title={`바로 실행 → ${tgt}`} aria-label={`${m.name} 바로 실행 (${tgt})`} onClick={() => runMacro(m)}><I id="play" /></button>}
            <span className="mac-name">{m.name}{m.hotkey && <span className="chip kbd">{m.hotkey}</span>}{m.fixed.length > 0 && <span className={`chip fixed${unresolved ? ' unresolved' : ''}`} title={`고정 대상: 패널 실행 대상(${MS_LABEL[uartBench.ms.mode]}) 대신 항상 ${m.fixed.map((sid) => uartBench.sessions.find((x) => x.id === sid)?.alias ?? `미해결 ${sid}`).join(', ')} — ⋯ 편집에서 변경`}>{unresolved ? '고정 · 미해결' : '고정'}</span>}</span>
            <button type="button" className="ibtn mac-more" data-act="mac-more" title="편집 · 복제 · 삭제" aria-label={`${m.name} 메뉴 (편집·복제·삭제)`} onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              openMenu({ x: r.left, y: r.bottom + 4, items: [
                { label: '편집…', run: () => dlg({ kind: 'macro', macro: m }) },
                { label: '복제', run: () => { uartBench.saveMacro({ ...m, id: nid('m-'), name: `${m.name} 복사본`, hotkey: '' }); } },
                'hr',
                { label: '삭제', run: () => { if (runs.length) uartBench.stopMacroAll(m.id); uartBench.deleteMacro(m.id); s.toasts.push(`매크로 ${m.name} 삭제됨`); } },
              ] });
            }}><I id="more" /></button>
            <span className="mac-sub">{m.steps.map((st) => stepText(st)).join(' → ')}</span>
            <span className="mac-sub" data-role="mac-target">대상: {tgt}{m.vars.length ? ` · 변수 ${m.vars.map((v) => v.name).join(', ')}` : ''}{m.pinned ? ' · ★' : ''}</span>
          </div>;
        })}
        {!uartBench.macros.length && <p className="side-note">매크로가 없습니다. “새 매크로”로 만드세요.</p>}
      </div>
      <div className="side-foot"><button type="button" className="btn sm" data-act="new-macro" onClick={() => dlg({ kind: 'macro', macro: newMacro() })}><I id="plus" />새 매크로</button><button type="button" className="btn sm" data-act="import" onClick={() => dlg({ kind: 'json', mode: 'import' })}><I id="up" />가져오기</button><button type="button" className="btn sm" data-act="export" onClick={() => dlg({ kind: 'json', mode: 'export' })}><I id="down" />내보내기</button></div>
      <div className="side-note">▶는 누르면 바로 실행합니다 — 대상은 위 “실행 대상”(활성 / 연결된 모두 / 선택)이며, “고정” 표시 매크로만 자기 대상으로 갑니다. 편집·복제·삭제는 ⋯. 실행 중 ■ 전체 중지는 이 매크로의 모든 대상을, pane의 중지는 그 대상만 멈춥니다. STOP은 실행 중인 매크로를 모두 멈춥니다.</div>
    </>;
  }
  const f = ui.filter.toLowerCase();
  const groups = [...new Set(uartBench.sessions.map((x) => x.group || '기타'))];
  const active = uartBench.activeTerminal();
  // live UART channels without a saved session (e.g. opened by an agent or the CLI) are listed too — real, open
  const adhoc = [...s.shell.channels.values()].filter((c) => c.domain === 'uart' && c.state === 'open' && !uartBench.sessionFor(c.channel));
  return <>
    <div className="side-head"><h2>저장된 UART 세션</h2><span className="cnt">{uartBench.sessions.length}</span>
      <button type="button" className="ibtn" data-act="new-session" title="새 세션" aria-label="새 세션" onClick={() => dlg({ kind: 'session', session: newSession(), isNew: true })}><I id="plus" /></button>{close}</div>
    <div className="side-tools"><input type="search" className="field-sm" id="sessFilter" placeholder="별칭, 포트, baud 검색" value={ui.filter} aria-label="세션 검색" onChange={(e) => { uartBench.ui = { ...ui, filter: e.target.value }; uartBench.bump(true); }} /></div>
    <div className="side-body" role="tree">
      {groups.map((g) => {
        const items = uartBench.sessions.filter((x) => (x.group || '기타') === g && (!f || `${x.alias} ${x.port} ${x.baud} ${x.group}`.toLowerCase().includes(f)));
        if (!items.length) return null;
        const open = !ui.closedGroups.includes(g);
        return <div key={g}>
          <button type="button" className="group-h" data-act="toggle-group" data-group={g} aria-expanded={open} onClick={() => { uartBench.ui = { ...ui, closedGroups: open ? [...ui.closedGroups, g] : ui.closedGroups.filter((x) => x !== g) }; uartBench.bump(true); }}><I id="chev" />{g} <span style={{ fontWeight: 400 }}>({items.length})</span></button>
          {open && items.map((x) => {
            const st = uartBench.channelState(x.alias);
            const pane = st === 'open' ? paneNoOfChannel(api, x.alias) : null;
            const tag = openTagText(st, pane);
            const isAct = active?.channel === x.alias;
            return <div key={x.id} className={`sess${isAct ? ' is-active' : ''}`} role="treeitem" tabIndex={0} data-sid={x.id} style={{ ['--tag' as string]: x.color }}
              title={`더블클릭/Enter: 활성 pane에 열기 · ${st === 'open' ? '연결됨' : st === 'unknown' ? '상태 확인 중' : '닫힘'}`}
              onDoubleClick={() => void openAndPlace(s, x, 'tab')}
              onKeyDown={(e) => { if (e.key === 'Enter' && e.target === e.currentTarget) void openAndPlace(s, x, 'tab'); }}>
              <span className="tagsw" aria-hidden="true" />
              <span className={`dot ${sessionDot(st)}`} />
              <span className="sess-name"><span className="sess-alias">{x.alias}</span>{tag && <span className="open-tag" title="연결됨">{tag}</span>}</span>
              <span className="sess-acts">
                <button type="button" className="ibtn" data-act="open-here" title="활성 pane에 탭으로 열기" aria-label={`${x.alias} 활성 pane에 열기`} onClick={() => void openAndPlace(s, x, 'tab')}><I id="play" /></button>
                <button type="button" className="ibtn" data-act="open-right" title="활성 pane 오른쪽 새 분할에 열기" aria-label={`${x.alias} 오른쪽 분할에 열기`} onClick={() => void openAndPlace(s, x, 'right')}><I id="split-r" /></button>
                <button type="button" className="ibtn" data-act="open-down" title="활성 pane 아래 새 분할에 열기" aria-label={`${x.alias} 아래 분할에 열기`} onClick={() => void openAndPlace(s, x, 'down')}><I id="split-d" /></button>
                <button type="button" className="ibtn" data-act="edit-session" title="세션 설정" aria-label={`${x.alias} 설정`} onClick={() => dlg({ kind: 'session', session: x, isNew: false })}><I id="gear" /></button>
              </span>
              <span className="sess-meta">{x.port} · {frameOf(x)} · {x.flow}</span>
            </div>;
          })}
        </div>;
      })}
      {adhoc.length > 0 && <div>
        <div className="group-h" aria-hidden="false" style={{ cursor: 'default' }}>저장 안 된 열린 채널 <span style={{ fontWeight: 400 }}>({adhoc.length})</span></div>
        {adhoc.map((c) => {
          const t = uartBench.terminal(c.channel);
          const tag = openTagText('open', paneNoOfChannel(api, c.channel));
          return <div key={c.channel} className={`sess${active?.channel === c.channel ? ' is-active' : ''}`} role="treeitem" tabIndex={0} data-channel={c.channel} onDoubleClick={() => placeChannel(s, c.channel, 'tab')} onKeyDown={(e) => { if (e.key === 'Enter' && e.target === e.currentTarget) placeChannel(s, c.channel, 'tab'); }}>
            <span className="tagsw" aria-hidden="true" />
            <span className={`dot ${sessionDot('open')}`} />
            <span className="sess-name"><span className="sess-alias">{c.channel}</span>{tag && <span className="open-tag" title="연결됨">{tag}</span>}</span>
            <span className="sess-acts">
              <button type="button" className="ibtn" title="활성 pane에 탭으로 보기" aria-label={`${c.channel} 활성 pane에 보기`} onClick={() => placeChannel(s, c.channel, 'tab')}><I id="play" /></button>
              <button type="button" className="ibtn" title="오른쪽 분할에 보기" aria-label={`${c.channel} 오른쪽 분할에 보기`} onClick={() => placeChannel(s, c.channel, 'right')}><I id="split-r" /></button>
              <button type="button" className="ibtn" title="아래 분할에 보기" aria-label={`${c.channel} 아래 분할에 보기`} onClick={() => placeChannel(s, c.channel, 'down')}><I id="split-d" /></button>
              <button type="button" className="ibtn" title="이 채널 설정을 세션으로 저장" aria-label={`${c.channel} 세션으로 저장`} onClick={() => dlg({ kind: 'session', session: { ...newSession(), alias: c.channel, port: t?.path() ?? '' }, isNew: true })}><I id="gear" /></button>
            </span>
            <span className="sess-meta">{[t?.path(), t?.frame(), '에이전트/CLI가 연 채널'].filter(Boolean).join(' · ')}</span>
          </div>;
        })}
      </div>}
      {!uartBench.sessions.length && !adhoc.length && <p className="side-note">저장된 세션이 없습니다. “새 세션”에서 감지된 포트를 골라 저장하세요. 저장만으로는 포트를 열지 않습니다.</p>}
    </div>
    <div className="side-foot"><button type="button" className="btn sm" data-act="new-session" onClick={() => dlg({ kind: 'session', session: newSession(), isNew: true })}><I id="plus" />새 세션</button></div>
    <div className="side-note">포트는 한 탭만 점유합니다. 이미 열린 세션을 다시 열면 그 탭이 활성 pane으로 옮겨집니다.</div>
  </>;
}

function newSession(): Session {
  const n = uartBench.sessions.length;
  return { id: nid('s-'), alias: `uart${n + 1}`, group: uartBench.sessions[0]?.group ?? '기타', port: '', baud: 115200, dataBits: 8, parity: 'N', stopBits: 1, flow: 'none', eol: 'CR', ts: true, color: COLORS[n % COLORS.length]! };
}
function newMacro(): Macro {
  return { id: nid('m-'), name: '', hotkey: '', pinned: false, vars: [], steps: [{ type: 'send', text: '', eol: 'session' }], fixed: [] };
}

type DlgState = { kind: 'session'; session: Session; isNew: boolean } | { kind: 'macro'; macro: Macro } | { kind: 'run'; macro: Macro; from?: string } | { kind: 'keys' } | { kind: 'json'; mode: 'export' | 'import' };

// ---------------------------------------------------------------- toolbar
function Toolbar({ dlg }: { dlg: (d: DlgState) => void }) {
  const s = useServices();
  useStoreVersion(uartBench);
  useStoreVersion(s.shell);
  useStoreVersion(s.uiState); // the module's own (retained) dock becomes s.dock.current after the route switch
  const [, setTick] = useState(0);
  const api = dockApi(s);
  useEffect(() => {
    if (!api) return;
    const offs = [api.onDidActiveGroupChange(() => setTick((x) => x + 1)), api.onDidMaximizedGroupChange(() => setTick((x) => x + 1)), api.onDidLayoutChange(() => setTick((x) => x + 1))];
    return () => { for (const o of offs) o.dispose(); };
  }, [api]);
  // REV6.1 always has an active pane: right after load Dockview may not have one yet → the first pane
  const g = api?.activeGroup ?? api?.groups[0];
  const maxed = !!g && g.api.isMaximized();
  const sideOpen = uartBench.ui.side !== null;
  const act = uartBench.activeTerminal();
  const bc = uartBench.bc;
  const paneN = g && api ? api.groups.indexOf(g) + 1 : null;
  return <>
    <button type="button" className="tbtn" data-cmd="toggle-side" id="tbSide" aria-controls="side" aria-expanded={sideOpen} aria-pressed={sideOpen} title="저장된 세션·매크로 패널 접기/펼치기 (Alt+0)" onClick={() => { uartBench.ui = { ...uartBench.ui, side: sideOpen ? null : 'sessions' }; uartBench.bump(true); }}><I id="sessions" /><span id="tbSideLabel">{sideOpen ? '세션 목록 접기' : '세션 목록 펼치기'}</span></button>
    <span className="vdiv" />
    <button type="button" className="tbtn" data-cmd="new-session" title="새 UART 세션 저장" onClick={() => dlg({ kind: 'session', session: newSession(), isNew: true })}><I id="plus" />새 세션</button>
    <span className="vdiv" />
    <button type="button" className="tbtn" data-cmd="split-right" id="tbSplitRight" title="활성 pane을 오른쪽으로 분할 (Alt+Shift+=)" disabled={!g} onClick={() => api && g && splitGroup(api, g, 'right')}><I id="split-r" />오른쪽 분할</button>
    <button type="button" className="tbtn" data-cmd="split-down" id="tbSplitDown" title="활성 pane을 아래로 분할 (Alt+Shift+-)" disabled={!g} onClick={() => api && g && splitGroup(api, g, 'below')}><I id="split-d" />아래 분할</button>
    <button type="button" className="tbtn" data-cmd="maximize" id="tbMax" title="활성 pane 최대화/복원 (Alt+Shift+Enter)" aria-pressed={maxed} disabled={!g} onClick={() => { if (!g) return; if (g.api.isMaximized()) g.api.exitMaximized(); else g.api.maximize(); setTick((x) => x + 1); }}><I id={maxed ? 'restore' : 'max'} /><span id="tbMaxLabel">{maxed ? '복원' : '최대화'}</span></button>
    <button type="button" className="tbtn" data-cmd="close-pane" id="tbClosePane" title="활성 pane 닫기 (Alt+Shift+W) — 포트는 열린 채로" disabled={!g} onClick={() => api && g && closeGroup(api, g)}><I id="pane-x" />pane 닫기</button>
    <span className="vdiv" />
    <div className="bc" role="group" aria-label="키 입력 대상 (터미널 타이핑만, 매크로는 매크로 패널)"><span className="bc-l">키 입력 대상</span>
      <span className="seg">
        <button type="button" id="bcCur" data-cmd="bc-current" aria-pressed={!bc.on} title="터미널 키 입력을 현재 세션에만 (기본) — 매크로 대상은 매크로 패널에서" onClick={() => uartBench.setBroadcast(false)}>현재 세션</button>
        <button type="button" id="bcAll" data-cmd="bc-all" aria-pressed={bc.on} title="터미널 키 입력을 지금 연결된 쓰기 가능 세션 모두에 (켤 때 수신자 고정) — 매크로에는 영향 없음" onClick={() => uartBench.setBroadcast(true)}>연결된 모두</button>
      </span>
      <span className={`bc-badge${bc.on ? ' on' : ''}`} id="bcBadge" aria-live="polite">
        {bc.on ? <><span data-role="bc-recipients">ALL → {bc.ids.join(', ')}</span>{bc.excluded.length > 0 && <span className="bc-ex" data-role="bc-excluded">제외: {bc.excluded.map((e) => `${e.alias}(${e.why})`).join(', ')}</span>}<button type="button" className="ibtn" data-cmd="bc-all" title="수신자 다시 고르기" aria-label="수신자 다시 고르기" onClick={() => uartBench.setBroadcast(true)}><I id="reset" /></button></>
          : act ? <>→ {act.channel} 만</> : null}
        {uartBench.runningCount > 0 && <button type="button" className="btn-stop" data-cmd="stop-all" title="실행 중인 모든 매크로 중지 (남은 step 송신 안 함)" onClick={() => uartBench.stopAll()}><I id="stop" />매크로 {uartBench.runningCount} 중지</button>}
      </span>
    </div>
    <span className="vdiv" />
    <button type="button" className="tbtn" data-cmd="new-macro" title="새 매크로" onClick={() => dlg({ kind: 'macro', macro: newMacro() })}><I id="macro" />새 매크로</button>
    <button type="button" className="tbtn" data-cmd="export" title="세션·매크로 JSON 내보내기" onClick={() => dlg({ kind: 'json', mode: 'export' })}><I id="down" />내보내기</button>
    <button type="button" className="tbtn" data-cmd="import" title="JSON 가져오기" onClick={() => dlg({ kind: 'json', mode: 'import' })}><I id="up" />가져오기</button>
    <span className="vdiv" />
    <button type="button" className="tbtn" data-cmd="reset-layout" title="레이아웃만 초기 상태(열린 세션을 한 pane의 탭으로)로" onClick={() => { if (api) resetLayout(s, api); }}><I id="reset" /><span>레이아웃<span className="tl-x"> 초기화</span></span></button>
    <button type="button" className="tbtn" data-cmd="keys" title="단축키 (?)" onClick={() => dlg({ kind: 'keys' })}><I id="kbd" />단축키</button>
    <span className="grow" />
    <span className="ctx" id="tbCtx">{act ? <>대상 <b>{act.channel}</b> {act.path() ?? ''}{paneN ? ` · pane ${paneN}` : ''}</> : '대상 없음'}</span>
  </>;
}

/** REV6.1 "레이아웃 초기화": layout only — every open UART terminal back into one pane as tabs; ports untouched. */
function resetLayout(s: ShellServices, api: DockviewApi): void {
  for (const g of [...api.groups]) if (g.api.isMaximized()) g.api.exitMaximized();
  const terms = api.panels.filter((p) => String((p.params as { panel?: string }).panel ?? '').startsWith('uart.'));
  const first = terms[0]?.group ?? api.groups[0];
  if (!first) return;
  for (const p of terms) if (p.group !== first) p.api.moveTo({ group: first, position: 'center' });
  for (const g of [...api.groups]) if (g !== first && g.panels.length === 0 && api.groups.length > 1) api.removeGroup(g);
  s.toasts.push('레이아웃 초기화 — 열린 세션을 한 pane의 탭으로 모았습니다 (포트는 그대로)');
}

/** REV6.1 empty-pane picker (opus app.js pickerHTML): the UART work area before any terminal is placed — the module
 * application, not a generic "no interface" card. Choosing a saved session opens THAT port explicitly (exactly like ▶
 * in the session list, through the gate); an open channel without a saved session is only shown. Navigating to the
 * UART tab opens nothing. */
export function UartEmptyPane() {
  const s = useServices();
  useStoreVersion(uartBench);
  useStoreVersion(s.shell);
  const adhoc = [...s.shell.channels.values()].filter((c) => c.domain === 'uart' && c.state === 'open' && !uartBench.sessionFor(c.channel));
  const newSess = () => window.dispatchEvent(new CustomEvent('loom:uart-session-dialog', { detail: { session: newSession(), isNew: true } }));
  const showMacros = () => { uartBench.ui = { ...uartBench.ui, side: 'macros' }; uartBench.bump(true); };
  return <div className="picker" data-testid="uart-empty-pane">
    <h3>pane 1 — 비어 있음</h3>
    <p>저장된 UART 세션을 이 pane에 엽니다. 다른 pane에서 이미 열린 포트는 이 pane으로 옮겨집니다 (포트는 한 탭만 점유).</p>
    <div className="plist">
      {uartBench.sessions.map((x) => {
        const open = uartBench.channelState(x.alias) === 'open';
        return <button key={x.id} type="button" className="pitem" data-act="pick" data-sid={x.id} style={{ ['--tag' as string]: x.color }} title={open ? '이미 열린 포트 — 이 pane에 보기' : '이 포트를 열고 이 pane에 터미널을 둡니다'} onClick={() => void openAndPlace(s, x, 'tab')}>
          <span className="dot" /><b>{x.alias}</b><small>{x.port} · {frameOf(x)}{open ? ' · 열림' : ''}</small>
        </button>;
      })}
      {adhoc.map((c) => <button key={c.channel} type="button" className="pitem" data-act="pick-open" data-channel={c.channel} title="에이전트/CLI가 연 채널 — 보기만 (포트를 다시 열지 않음)" onClick={() => placeChannel(s, c.channel, 'tab')}>
        <span className="dot" /><b>{c.channel}</b><small>저장 안 된 열린 채널 · 보기</small>
      </button>)}
    </div>
    {!uartBench.sessions.length && !adhoc.length && <p className="hint">저장된 세션이 없습니다. “새 세션”에서 감지된 포트(또는 수동 경로)를 골라 저장한 뒤 여세요. 저장만으로는 포트를 열지 않습니다.</p>}
    <div className="picker-acts">
      <button type="button" className="btn" data-act="new-session" onClick={newSess}><I id="plus" />새 세션…</button>
      <button type="button" className="btn" data-act="show-macros" onClick={showMacros}><I id="macro" />매크로</button>
    </div>
    <p className="hint">세션 목록의 ▶ · 오른쪽/아래 분할 버튼으로도 엽니다. 여러 세션을 pane에 나란히 둘 수 있습니다.</p>
  </div>;
}

// ---------------------------------------------------------------- module
export function UartRail() {
  useStoreVersion(uartBench);
  const side = uartBench.ui.side;
  const set = (k: 'sessions' | 'macros') => { uartBench.ui = { ...uartBench.ui, side: side === k ? null : k }; uartBench.bump(true); };
  return <>
    <button type="button" id="railSessions" data-side="sessions" aria-pressed={side === 'sessions'} aria-controls="side" title="저장된 세션 — 다시 누르면 접기 (Alt+1)" onClick={() => set('sessions')}><I id="sessions" /><span>세션</span></button>
    <button type="button" id="railMacros" data-side="macros" aria-pressed={side === 'macros'} aria-controls="side" title="매크로 — 다시 누르면 접기 (Alt+2)" onClick={() => set('macros')}><I id="macro" /><span>매크로</span></button>
  </>;
}

/** toolbar + side + dialogs/menus + hotkeys for the UART module (mounted by the shell frame) */
export function useUartModule(enabled: boolean): { toolbar: ReactNode; side: ReactNode; layer: ReactNode; sideOpen: boolean } {
  const s = useServices();
  useStoreVersion(uartBench);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [dialog, setDialog] = useState<DlgState | null>(null);
  // pane "+" (tab-add) and session duplicate → menus/dialogs
  useEffect(() => {
    if (!enabled) return;
    const onAdd = (e: Event) => {
      const el = e.target as HTMLElement;
      const r = el.getBoundingClientRect();
      const items: (MenuItem | 'hr')[] = uartBench.sessions.map((x) => ({ label: `${x.alias}  ${x.port} · ${frameOf(x)}${uartBench.channelState(x.alias) === 'open' ? ' · 열림(이 pane으로 옮김)' : ''}`, run: () => void openAndPlace(s, x, 'tab') }));
      items.push('hr', { label: '새 세션…', run: () => setDialog({ kind: 'session', session: newSession(), isNew: true }) });
      setMenu({ x: r.left, y: r.bottom + 2, header: '이 pane에 세션 열기', items });
    };
    const onDup = (e: Event) => {
      const d = (e as CustomEvent<{ session: Session; isNew: boolean }>).detail;
      setDialog({ kind: 'session', session: d.session, isNew: d.isNew });
    };
    const onMacroMenu = (e: Event) => {
      const d = (e as CustomEvent<{ channel: string; x: number; y: number }>).detail;
      const pinned = [...uartBench.macros].sort((a, b) => Number(b.pinned) - Number(a.pinned));
      const scopeLbl = uartBench.resolveScope({ fixed: [] }, d.channel).label;
      setMenu({ x: d.x, y: d.y, header: `실행 대상: ${scopeLbl}`, items: pinned.length ? pinned.map((m) => ({
        label: `${m.pinned ? '★ ' : ''}${m.name}${m.fixed.length ? ` (고정 → ${m.fixed.map((sid) => uartBench.sessions.find((x) => x.id === sid)?.alias ?? '미해결').join(', ')})` : ''}`,
        k: m.hotkey,
        run: () => { if (uartBench.missingVars(m).length) setDialog({ kind: 'run', macro: m, from: d.channel }); else uartBench.requestRun(m, d.channel); },
      })) : [{ label: '매크로 없음 — 새 매크로…', run: () => setDialog({ kind: 'macro', macro: newMacro() }) }] });
    };
    const onEdit = (e: Event) => {
      const ch = (e as CustomEvent<{ channel: string }>).detail.channel;
      const sess = uartBench.sessionFor(ch);
      setDialog({ kind: 'session', session: sess ?? { ...newSession(), alias: ch, port: uartBench.terminal(ch)?.path() ?? '' }, isNew: !sess });
    };
    const onReopen = (e: Event) => {
      const ch = (e as CustomEvent<{ channel: string }>).detail.channel;
      const sess = uartBench.sessionFor(ch);
      if (sess) void openAndPlace(s, sess, 'tab');
      else s.toasts.push(`${ch}: 저장된 세션이 없어 설정을 알 수 없습니다 — ⚙에서 세션으로 저장한 뒤 다시 여세요`);
    };
    window.addEventListener('loom:uart-add-tab', onAdd);
    window.addEventListener('loom:uart-session-dialog', onDup);
    window.addEventListener('loom:uart-macro-menu', onMacroMenu);
    window.addEventListener('loom:uart-edit-session', onEdit);
    window.addEventListener('loom:uart-reopen', onReopen);
    return () => {
      window.removeEventListener('loom:uart-add-tab', onAdd);
      window.removeEventListener('loom:uart-session-dialog', onDup);
      window.removeEventListener('loom:uart-macro-menu', onMacroMenu);
      window.removeEventListener('loom:uart-edit-session', onEdit);
      window.removeEventListener('loom:uart-reopen', onReopen);
    };
  }, [enabled, s]);
  // reserved workbench keys + macro hotkeys (captured before xterm, so they never reach the device)
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector('dialog[open]')) return;
      const k = `${e.ctrlKey ? 'Ctrl+' : ''}${e.altKey ? 'Alt+' : ''}${e.shiftKey ? 'Shift+' : ''}${e.key.length === 1 ? e.key.toUpperCase() : e.key}`;
      const code = e.code;
      const api = dockApi(s);
      const g = api?.activeGroup ?? api?.groups[0];
      let handled = true;
      if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && code === 'Digit1') { uartBench.ui = { ...uartBench.ui, side: uartBench.ui.side === 'sessions' ? null : 'sessions' }; uartBench.bump(true); }
      else if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && code === 'Digit2') { uartBench.ui = { ...uartBench.ui, side: uartBench.ui.side === 'macros' ? null : 'macros' }; uartBench.bump(true); }
      else if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && code === 'Digit0') { uartBench.ui = { ...uartBench.ui, side: uartBench.ui.side ? null : 'sessions' }; uartBench.bump(true); }
      else if (e.altKey && e.shiftKey && code === 'Equal' && api && g) splitGroup(api, g, 'right');
      else if (e.altKey && e.shiftKey && code === 'Minus' && api && g) splitGroup(api, g, 'below');
      else if (e.altKey && e.shiftKey && code === 'Enter' && g) { if (g.api.isMaximized()) g.api.exitMaximized(); else g.api.maximize(); }
      else if (e.altKey && e.shiftKey && code === 'KeyW' && api && g) closeGroup(api, g);
      else {
        const m = uartBench.macros.find((x) => x.hotkey && x.hotkey === k);
        if (m) { if (uartBench.missingVars(m).length) setDialog({ kind: 'run', macro: m }); else uartBench.requestRun(m); }
        else handled = false;
      }
      if (handled) { e.preventDefault(); e.stopPropagation(); }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [enabled, s]);
  const dlg = (d: DlgState) => setDialog(d);
  const close = () => setDialog(null);
  const layer = useMemo(() => createPortal(<div className="ub-layer">
    {menu && <Menu m={menu} close={() => setMenu(null)} />}
    {dialog?.kind === 'session' && <SessionDialog key={dialog.session.id} base={dialog.session} isNew={dialog.isNew} onClose={close} />}
    {dialog?.kind === 'macro' && <MacroDialog key={dialog.macro.id} base={dialog.macro} onClose={close} />}
    {dialog?.kind === 'run' && <RunVarsDialog m={dialog.macro} from={dialog.from} onClose={close} />}
    {dialog?.kind === 'keys' && <KeysDialog onClose={close} />}
    {dialog?.kind === 'json' && <JsonDialog mode={dialog.mode} onClose={close} />}
  </div>, document.body), [menu, dialog]);
  return {
    toolbar: enabled ? <Toolbar dlg={dlg} /> : null,
    side: enabled ? <Side openMenu={setMenu} dlg={dlg} /> : null,
    layer: enabled ? layer : null,
    sideOpen: uartBench.ui.side !== null,
  };
}

/** Footer status for the UART module (REV6.1 .shell-status: 활성 pane › target … · pane · 탭 · 매크로 실행 · 기록) */
export function UartStatus() {
  const s = useServices();
  useStoreVersion(uartBench);
  useStoreVersion(s.shell);
  const [, setTick] = useState(0);
  useEffect(() => { const h = setInterval(() => setTick((x) => x + 1), 1_000); return () => clearInterval(h); }, []);
  const api = dockApi(s);
  const act = uartBench.activeTerminal();
  const pane = act ? paneNoOfChannel(api, act.channel) : null;
  const tabs = api?.panels.filter((p) => String((p.params as { panel?: string }).panel ?? '').startsWith('uart.')).length ?? 0;
  const c = act?.counters();
  return <>
    <span className="st-target" id="stTarget">{act ? <>활성 pane {pane ?? '-'} › <b>{act.channel}</b> {act.path() ?? ''} {act.frame()} {act.status() === 'open' || act.status() === 'OPEN' ? '연결됨' : act.status()} RX {fmtBytes(c?.rx ?? 0)} TX {fmtBytes(c?.tx ?? 0)}</> : '활성 세션 없음'}</span>
    <span className="st-extra" id="stExtra">pane {api?.groups.length ?? 0} · 탭 {tabs} · 매크로 실행 {uartBench.runningCount}</span>
  </>;
}
