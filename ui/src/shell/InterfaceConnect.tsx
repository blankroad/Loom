import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { createPortal } from 'react-dom';
import { unwrapDomainResult } from '../sdk/rpcEnvelope';
import { defaultPanelFor } from '../store/channels';
import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';
import './interfaceConnect.css';

type Domain = 'uart' | 'can' | 'uds' | 'eth';
export interface ConnectionDraft {
  domain: Domain;
  name: string;
  simulated: boolean;
  address: string;
  baud: string;
}

/** Match native domain schemas; selecting an interface never sends device traffic. */
export function connectionRequest(draft: ConnectionDraft): { rpc: string; params: Record<string, unknown> } {
  const name = draft.name.trim();
  if (!/^[a-z][a-z0-9_-]{0,31}$/.test(name)) throw new Error('이름은 영문 소문자로 시작하는 1–32자 영문·숫자·_·-로 입력하세요.');
  const address = draft.address.trim();
  if (!draft.simulated && !address) throw new Error('연결할 장치 주소를 입력하세요.');
  switch (draft.domain) {
    case 'uart': {
      const baud = Number(draft.baud);
      if (!Number.isSafeInteger(baud) || baud < 1 || baud > 4_000_000) throw new Error('보레이트는 1–4,000,000 사이의 정수로 입력하세요.');
      return { rpc: 'uart.open', params: { target: draft.simulated ? 'sim://bootshell' : address, alias: name, config: { baud } } };
    }
    case 'can':
      if (!draft.simulated) throw new Error('현재 CAN 연결 화면은 시뮬레이터를 지원합니다.');
      return { rpc: 'can.open', params: { channel: name, bus: name } };
    case 'uds':
      return { rpc: 'uds.connect', params: { name, ...(draft.simulated ? { sim: 'bench' } : { host: address }), timeout_ms: 5000 } };
    case 'eth':
      return { rpc: 'eth.capture.start', params: { iface: draft.simulated ? name : address, ch: name, backend: draft.simulated ? 'sim' : 'live' } };
  }
}

/** Workbench modules that open as a panel without opening a device; the source is chosen inside the panel. */
export const MODULE_LAUNCHERS = [
  { id: 'video', label: '비디오 캡처 작업대', note: '장치·카메라를 열지 않습니다. 소스는 작업대에서 명시적으로 선택합니다(열기는 operate 권한).', slot: { panel: 'video.workbench', props: {} } },
] as const;

const LABEL: Record<Domain, string> = { uart: 'UART · 시리얼', can: 'CAN · 버스', uds: 'UDS · DoIP', eth: 'Ethernet · 캡처' };
const ADDRESS: Record<Domain, string> = { uart: '/dev/ttyUSB0 또는 COM3', can: '', uds: '192.168.0.10', eth: 'en0 또는 eth0' };

export function InterfaceConnect() {
  const { shell, net, gate, showSlot, toasts } = useServices();
  useStoreVersion(shell);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<ConnectionDraft>({ domain: 'uart', name: '', simulated: true, address: '', baud: '115200' });
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [ports, setPorts] = useState<string[]>([]);
  const [portStatus, setPortStatus] = useState('');
  const [finding, setFinding] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const title = useId();
  const portList = useId();
  const allowed = shell.can('operate') && !shell.status.writeLocked;
  const launch = (slot: (typeof MODULE_LAUNCHERS)[number]['slot']) => {
    if (pending) return;
    // observation-only: shows the panel; no RPC, no device, no fake active channel
    showSlot({ panel: slot.panel, props: { ...slot.props } });
    setOpen(false);
    trigger.current?.focus();
  };

  useEffect(() => {
    if (open) dialog.current?.showModal();
    else dialog.current?.close();
  }, [open]);

  const close = () => {
    if (pending) return;
    setOpen(false);
    trigger.current?.focus();
  };
  const findPorts = async () => {
    if (finding) return;
    setFinding(true);
    setPortStatus('');
    try {
      const result = unwrapDomainResult<{ ports?: { path?: string }[]; truncated?: boolean }>('uart.ports', await net.call('uart.ports', {}));
      const paths = Array.isArray(result?.ports) ? result.ports.flatMap((port) => typeof port.path === 'string' ? [port.path] : []).slice(0, 256) : [];
      setPorts(paths);
      setPortStatus(paths.length ? `${paths.length}개 포트 발견${result.truncated ? ' · 일부 목록' : ''}. 주소 입력란에서 선택하세요.` : '발견된 포트가 없습니다. 주소를 직접 입력할 수 있습니다.');
    } catch (e) {
      setPortStatus(`포트 조회 실패: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setFinding(false);
    }
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (pending) return;
    setError('');
    try {
      const request = connectionRequest(draft);
      const name = draft.name.trim();
      if (shell.channels.get(`${draft.domain}/${name}`)?.state === 'open') throw new Error('같은 이름의 인터페이스가 이미 연결되어 있습니다. 다른 이름을 사용하세요.');
      setPending(true);
      await gate.run({ id: 'interface.connect', rpc: request.rpc, class: 'request', right: 'operate', confirm: 'none', label: `${LABEL[draft.domain]} 연결` }, request.params);
      const slot = defaultPanelFor(draft.domain, name);
      if (slot) showSlot(slot);
      toasts.push(`${name} 연결 요청을 보냈습니다. 실제 상태는 인터페이스 목록에서 확인하세요.`);
      setOpen(false);
      trigger.current?.focus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPending(false);
    }
  };

  return <>
    <button ref={trigger} type="button" className="interface-add" aria-label="인터페이스 추가" title="인터페이스 추가" onClick={() => { setError(''); setOpen(true); }}>+ 추가</button>
    {open && createPortal(
      <dialog ref={dialog} className="interface-dialog" aria-labelledby={title} onCancel={(event) => { event.preventDefault(); close(); }}>
        <form onSubmit={(event) => void submit(event)}>
          <header><div><span className="interface-eyebrow">CONNECTIONS</span><h2 id={title}>인터페이스 추가</h2></div><button type="button" className="interface-close" aria-label="연결 창 닫기" disabled={pending} onClick={close}>×</button></header>
          <p className="muted">각 연결은 독립된 이름과 작업 탭을 가집니다.</p>
          <label>유형<select value={draft.domain} disabled={pending} onChange={(event) => setDraft({ ...draft, domain: event.target.value as Domain, address: '', simulated: true })}>{Object.entries(LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
          <label>인터페이스 이름<input autoFocus required maxLength={32} pattern="[a-z][a-z0-9_-]{0,31}" placeholder={draft.domain === 'uart' ? '예: console-a, console-b' : '예: bench-1'} value={draft.name} disabled={pending} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
          <label>연결 방식<select value={draft.simulated ? 'sim' : 'live'} disabled={pending} onChange={(event) => setDraft({ ...draft, simulated: event.target.value === 'sim' })}><option value="sim">시뮬레이터</option>{draft.domain !== 'can' && <option value="live">실제 장치</option>}</select></label>
          {!draft.simulated && <label>{draft.domain === 'uds' ? 'DoIP 호스트' : draft.domain === 'eth' ? '네트워크 인터페이스' : '시리얼 포트'}<input required list={draft.domain === 'uart' ? portList : undefined} placeholder={ADDRESS[draft.domain]} value={draft.address} disabled={pending} onChange={(event) => setDraft({ ...draft, address: event.target.value })} /></label>}
          {!draft.simulated && draft.domain === 'uart' && <div className="interface-port-finder"><datalist id={portList}>{ports.map((path) => <option key={path} value={path} />)}</datalist><button type="button" disabled={finding || pending || shell.status.writeLocked} onClick={() => void findPorts()}>{finding ? '조회 중…' : '포트 찾기'}</button>{portStatus && <p className="muted" role="status">{portStatus}</p>}</div>}
          {draft.domain === 'uart' && <label>보레이트<input type="number" min={1} max={4000000} required value={draft.baud} disabled={pending} onChange={(event) => setDraft({ ...draft, baud: event.target.value })} /></label>}
          {draft.domain === 'can' && <p className="muted">CAN은 현재 시뮬레이터 연결을 제공합니다.</p>}
          {!allowed && <p role="status">연결하려면 연결된 세션의 operate 권한이 필요합니다.</p>}
          {error && <p className="interface-error" role="alert">{error}</p>}
          <section className="interface-modules" aria-label="작업대 모듈">
            <span className="interface-eyebrow">MODULES</span>
            {MODULE_LAUNCHERS.map((m) => <div key={m.id} className="interface-module"><button type="button" data-testid={`launch-${m.id}`} disabled={pending} onClick={() => launch(m.slot)}>{m.label} 열기</button><p className="muted">{m.note}</p></div>)}
          </section>
          <footer><button type="button" disabled={pending} onClick={close}>취소</button><button type="submit" className="interface-primary" disabled={pending || !allowed}>{pending ? '연결 중…' : '연결'}</button></footer>
        </form>
      </dialog>, document.body)}
  </>;
}
