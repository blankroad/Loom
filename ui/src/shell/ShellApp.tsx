// Shell root (plan §5.1.1). Hash routes: `#/l/<preset>`, `#/p/<domain>/<panel>`,
// `#/need-auth`.

import { useEffect, useRef, useState } from 'react';
import { useStoreVersion } from '../store/hooks';
import { ActivityStrip } from './ActivityStrip';
import { ApprovalQueue } from './Approvals';
import { AriaAnnouncer, Banners, Toasts } from './Banners';
import { ChannelBar, HeartbeatClock } from './ChannelBar';
import { DEFAULT_ROUTE_KEY, NEED_AUTH_TEXT, RETURN_KEY } from './bootstrap';
import { ConfirmLayer } from './ConfirmLayer';
import { FocusPrompt, TabSettingsControls } from './FocusPrompt';
import { DockLayout, LayoutBar } from './dock/DockLayout';
import { retainsHistoryAfterClose } from './dock/activeWorkspace';
import { viewsForChannel } from '../store/channels';
import { parseRoute, routeModule } from './routes';
import { installPrefixKeys } from './prefixKeys';
import type { ShellServices } from './services';
import { ShellCtx, useServices } from './ShellContext';
import { StopPanel } from './StopPanel';
import { DIAGNOSTIC_OBSERVATION_DOMAINS, SERVICE_LABELS } from './routes';
import { TopBar } from './TopBar';
import { UartRail, UartStatus, useUartModule } from '../workbench/uart/UartWorkbench';
import { uartBench } from '../workbench/uart/workbench';
import { WorkbenchWelcome } from './WorkbenchWelcome';
import { StepBand } from './test/TestScreen';
import './shell.css';

function rememberedRoute(): string {
  try {
    const r = sessionStorage.getItem(RETURN_KEY);
    return r && /^\/(p|l|s)\//.test(r) ? r : '/l/m1-default';
  } catch {
    return '/l/m1-default';
  }
}

export function NeedAuth({ reason }: { reason: 'expired' | 'network' | 'malformed' | null }) {
  // No secrets here: the bootstrap secret only travels from a TTY `loom ui` and is
  // stripped from the address bar before this page renders (bootstrap.ts).
  const { shell } = useServices();
  useStoreVersion(shell);
  const connected = shell.conn.status === 'open';
  return (
    <main className="need-auth" data-testid="need-auth">
      <h1>Loom — 보기 자격이 없습니다</h1>
      {reason && (
        <p className="need-auth-reason" data-testid="need-auth-reason">
          {NEED_AUTH_TEXT[reason]}
        </p>
      )}
      {connected && (
        <p>
          이 브라우저는 이미 자격이 있어 연결됐습니다.{' '}
          <a href={`#${rememberedRoute()}`}>화면으로 →</a>
        </p>
      )}
      <p>이 브라우저에는 Loom 뷰어 쿠키가 없습니다.</p>
      <p>
        벤치 PC의 <strong>터미널</strong>에서 <code>loom ui</code>를 실행하면 브라우저가 자격과 함께 열립니다. 다른 PC에서
        보려면 벤치 터미널에서 <code>loom ui pair</code>로 받은 코드를 입력하세요.
      </p>
      <p className="muted">에이전트·자동화 브라우저의 자격은 데몬 정책에 따라 결정됩니다. 연결 후 상단에 표시된 자격을 확인하세요.</p>
    </main>
  );
}

function HelpOverlay() {
  const { uiState } = useServices();
  useStoreVersion(uiState);
  if (!uiState.helpOpen) return null;
  return (
    <div className="help-overlay" role="dialog" aria-label="도움말" onClick={() => uiState.setHelpOpen(false)}>
      <dl>
        <dt>Ctrl-] x</dt>
        <dd>STOP — agent·script 쓰기와 잡 잠금 (사용자 입력은 계속)</dd>
        <dt>Ctrl-] m</dt>
        <dd>마커</dd>
        <dt>Ctrl-] A</dt>
        <dd>승인 대기열</dd>
        <dt>Ctrl-] ?</dt>
        <dd>도움말</dd>
        <dt>Ctrl-] Ctrl-]</dt>
        <dd>0x1D를 포커스된 패널로</dd>
      </dl>
      <p className="muted">웹에서는 Ctrl-X를 STOP으로 쓰지 않습니다(브라우저 잘라내기).</p>
    </div>
  );
}

function useHashRoute() {
  const [hash, setHash] = useState(() => (typeof location !== 'undefined' ? location.hash : ''));
  useEffect(() => {
    const on = () => setHash(location.hash);
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  return parseRoute(hash);
}

const GENERIC_RAIL_ICON = 'M2 3h12v10H2zM5 6h6M5 8.5h6M5 11h3';

/** Startup only: a bootstrap/default route (not an explicit user route) with UART as the sole live service
 * lands on the agreed UART workbench. Explicit routes, saved layouts and later navigation are never overridden.
 *
 * Eligibility is captured ONCE per mount by a pure read (no storage writes during render, so StrictMode's double
 * initializer call and effect mount → unmount → remount see the same answer). The bootstrap marker is consumed
 * only at a completion boundary (routed, decided otherwise, navigated away, timed out, page hidden), idempotently. */
function useStartupModuleRoute(): void {
  const { shell } = useServices();
  const [startHash] = useState(autoDefaultStartHash);
  const pending = useRef<boolean>(startHash !== null);
  useEffect(() => {
    if (startHash === null || !pending.current) return;
    const unchanged = () => currentHash() === startHash;
    let off: (() => void) | null = null;
    let giveUp: ReturnType<typeof setTimeout> | null = null;
    // StrictMode/real unmount only detaches: the marker stays for the remount, which reuses `pending`/`startHash`
    const detach = () => {
      off?.();
      off = null;
      if (giveUp !== null) clearTimeout(giveUp);
      giveUp = null;
      window.removeEventListener('hashchange', onHash);
      window.removeEventListener('pagehide', settle);
    };
    const settle = () => {
      pending.current = false;
      detach();
      consumeDefaultRouteMarker();
    };
    // only a real navigation (a different route) cancels; the bootstrap's own hash write may dispatch late
    const onHash = () => { if (pending.current && !unchanged()) settle(); };
    const check = () => {
      if (!pending.current) return;
      if (!unchanged()) { settle(); return; }
      const open = [...shell.channels.values()].filter((c) => c.state === 'open');
      if (!open.length) return;
      settle();
      if (open.every((c) => c.domain === 'uart')) location.replace('#/s/uart');
    };
    window.addEventListener('hashchange', onHash);
    window.addEventListener('pagehide', settle);
    giveUp = setTimeout(settle, 6_000);
    off = shell.subscribe(check);
    check();
    return detach;
  }, [shell, startHash]);
}
function currentHash(): string {
  const h = typeof location === 'undefined' ? '' : location.hash;
  return h === '#' ? '' : h;
}
/** PURE: the start hash when this start is a bootstrap/default route, else null. Never mutates storage. */
function autoDefaultStartHash(): string | null {
  const h = currentHash();
  if (h === '' || h === '#/') return h;
  if (h !== '#/l/m1-default') return null;
  try {
    return sessionStorage.getItem(DEFAULT_ROUTE_KEY) === '1' ? h : null;
  } catch {
    return null;
  }
}
/** Idempotent completion cleanup of the bootstrap marker. */
function consumeDefaultRouteMarker(): void {
  try {
    sessionStorage.removeItem(DEFAULT_ROUTE_KEY);
  } catch {
    /* storage blocked: nothing to clean */
  }
}

function ShellBody() {
  const s = useServices();
  const { shell, stop, uiState, net, gate, toasts } = s;
  useStoreVersion(shell);
  useStoreVersion(uiState); // visible per-module dock changed (s.dock.current) → chrome re-reads it
  const route = useHashRoute();
  const module = routeModule(route);
  const isUart = module === 'uart';
  const ownsToolbar = module === 'eth' || module === 'uds'; // B's panels render their own 46px module toolbar
  const uart = useUartModule(isUart);
  const [genericSide, setGenericSide] = useState<Record<string, boolean>>({});
  const genericKey = module ?? 'integrated';
  const genericSideOpen = genericSide[genericKey] ?? (module === null);
  const sideOpen = isUart ? uart.sideOpen : genericSideOpen;
  useStartupModuleRoute();
  useEffect(() => uartBench.attach(s), [s]);
  // A stopped ETH capture view that is still docked (retained or explicitly reopened history) is real content:
  // closing the last live interface must not cover it with the empty welcome. Closed UART/CAN views are pruned anyway.
  const retainedHistory = [...shell.channels.values()].some((c) => c.state === 'closed' && retainsHistoryAfterClose(c.domain)
    && viewsForChannel(c.domain, c.channel).some((v) => s.dock.current?.find({ panel: v.panel, props: v.props })));
  const welcome = ![...shell.channels.values()].some((channel) => channel.state === 'open') && !retainedHistory && route.kind === 'layout' && route.name === 'm1-default' && !route.service && !s.layouts.list().includes(route.name);

  useEffect(
    () =>
      installPrefixKeys(window, {
        stop: () => void stop.requestStop(), // every right (contract v1.1)
        mark: () => {
          const label = `mark ${new Date().toLocaleTimeString('ko-KR', { hour12: false })}`;
          gate
            .run({ id: 'mark', rpc: 'core.mark', class: 'request', right: 'operate', confirm: 'none' }, { label })
            .then(() => toasts.push(`마커: ${label}`))
            .catch((e: unknown) => toasts.push(`마커 실패: ${e instanceof Error ? e.message : String(e)}`));
        },
        approvals: () => uiState.setApprovalsOpen(!uiState.approvalsOpen),
        help: () => uiState.setHelpOpen(!uiState.helpOpen),
        literal: () => {
          const target = document.activeElement ?? document.body;
          target.dispatchEvent(new CustomEvent('loom:literal', { bubbles: true, detail: { byte: 0x1d } }));
        },
      }),
    [stop, uiState, gate, toasts],
  );

  useEffect(() => {
    if (net.state.status === 'closed' && /auth|401|unauthor/i.test(net.state.reason) && route.kind !== 'need-auth') {
      // remember the deep link so a later `loom ui` bootstrap returns here
      try {
        sessionStorage.setItem(RETURN_KEY, location.hash.replace(/^#/, ''));
      } catch {
        /* storage blocked: return to the default layout */
      }
      location.hash = '#/need-auth';
    }
  }, [net, shell.version, route.kind]);

  if (route.kind === 'need-auth') return <NeedAuth reason={route.reason} />;
  const toggleGeneric = () => setGenericSide({ ...genericSide, [genericKey]: !genericSideOpen });
  const sideDomains = route.kind === 'layout' && route.service ? route.service === 'uds' ? DIAGNOSTIC_OBSERVATION_DOMAINS : [route.service] : route.kind === 'panel' ? [route.slot.panel.split('.')[0]!] : undefined;
  return (
    <div className="shell" data-testid="loom-shell" data-theme="light" data-module={module ?? 'integrated'} data-env={shell.env ?? 'unknown'} data-locked={shell.status.writeLocked}>
      <TopBar route={route} />
      <Banners />
      <StopPanel />
      <div className="bl-body">
        {/* One frame for every route (REV6.1 .mod-uart grid: rail | toolbar / side | work). The element path to the
            Dockview never changes, so switching services keeps every live panel instance. */}
        <main className={`mod-uart${isUart ? '' : ' mod-generic'}${sideOpen ? '' : ' side-collapsed'}${ownsToolbar ? ' mod-bare' : ''}`} id="modUart" data-module={module ?? 'integrated'}>
          <nav className="rail" aria-label={isUart ? 'UART 패널' : '작업 공간 패널'} hidden={ownsToolbar}>
            {isUart ? <UartRail /> : <button type="button" data-side="interfaces" aria-pressed={sideOpen} aria-controls="side" title="인터페이스 목록 — 다시 누르면 접기" onClick={toggleGeneric}><svg className="i" viewBox="0 0 16 16" aria-hidden="true"><path d={GENERIC_RAIL_ICON} /></svg><span>인터페이스</span></button>}
          </nav>
          <div className="mtbar" role="toolbar" aria-label={isUart ? 'UART 작업 공간' : '작업 공간'} hidden={ownsToolbar}>
            {isUart ? uart.toolbar : <>
              <button type="button" className="tbtn" aria-pressed={sideOpen} aria-controls="side" title="인터페이스 목록 접기/펼치기" onClick={toggleGeneric}><svg className="i" viewBox="0 0 16 16" aria-hidden="true"><path d={GENERIC_RAIL_ICON} /></svg><span>{sideOpen ? '목록 접기' : '목록 펼치기'}</span></button>
              <span className="vdiv" />
              {module === null && <LayoutBar route={route} />}
              <span className="grow" />
              <TabSettingsControls />
              <button type="button" className="tbtn" title="키보드 단축키 (Ctrl-] ?)" onClick={() => uiState.setHelpOpen(true)}><svg className="i" viewBox="0 0 16 16" aria-hidden="true"><rect x="1.5" y="4" width="13" height="8.5" rx="1.5" /><path d="M4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M5 10h6" /></svg>단축키</button>
            </>}
          </div>
          <aside className="side" id="side" aria-label={isUart ? '세션과 매크로' : '인터페이스 탐색기'} hidden={ownsToolbar}>
            {isUart ? uart.side : ownsToolbar ? null : <ChannelBar domains={sideDomains} />}
          </aside>
          <section className="work" aria-label="작업 공간">
            {route.kind === 'layout' && route.service === 'uds' && <div className="diagnostic-context" role="note">현재 연결 지원: DoIP·시뮬레이터. CAN·Ethernet 관찰 패널은 선택 ECU와의 대응 관계를 의미하지 않습니다.</div>}
            <div className={`workspace-surface${welcome ? ' has-welcome' : ''}`}>
              <div className="workspace-dock" inert={welcome || undefined} aria-hidden={welcome || undefined}>
                <DockLayout route={route} />
              </div>
              {welcome && <WorkbenchWelcome />}
            </div>
          </section>
        </main>
      </div>
      <footer className="shell-status foot" data-shell="status">
        {isUart ? <UartStatus /> : <span className="st-target">{module ? `${SERVICE_LABELS[module]} · 열린 ${[...shell.channels.values()].filter((c) => c.state === 'open' && c.domain === module).length}` : `통합 작업 공간 · 열린 인터페이스 ${[...shell.channels.values()].filter((c) => c.state === 'open').length}`}</span>}
        <span className="grow" />
        <StepBand />
        <ActivityStrip />
        <span className="keys"><span className="shortcut-hint"><kbd>Ctrl-]</kbd> <kbd>x</kbd> STOP</span>
          <span className="footer-status"><span>LOOM / {shell.env ?? 'unknown'}</span><HeartbeatClock /><button type="button" title="단축키 (Ctrl-] ?)" onClick={() => uiState.setHelpOpen(true)}>?</button></span></span>
      </footer>
      {uart.layer}
      <ApprovalQueue />
      <ConfirmLayer />
      <HelpOverlay />
      <FocusPrompt />
      <Toasts />
      <AriaAnnouncer />
    </div>
  );
}

export function ShellApp({ services }: { services: ShellServices }) {
  useEffect(() => services.bind(), [services]);
  return (
    <ShellCtx.Provider value={services}>
      <ShellBody />
    </ShellCtx.Provider>
  );
}
