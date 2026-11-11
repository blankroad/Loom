// Shell root (plan §5.1.1). Hash routes: `#/l/<preset>`, `#/p/<domain>/<panel>`,
// `#/need-auth`.

import { useEffect, useState } from 'react';
import { useStoreVersion } from '../store/hooks';
import { ActivityStrip } from './ActivityStrip';
import { ApprovalQueue } from './Approvals';
import { AriaAnnouncer, Banners, Toasts } from './Banners';
import { ChannelBar, HeartbeatClock } from './ChannelBar';
import { NEED_AUTH_TEXT, RETURN_KEY } from './bootstrap';
import { ConfirmLayer } from './ConfirmLayer';
import { FocusPrompt, TabSettingsControls } from './FocusPrompt';
import { DockLayout, LayoutBar } from './dock/DockLayout';
import { parseRoute } from './routes';
import { installPrefixKeys } from './prefixKeys';
import type { ShellServices } from './services';
import { ShellCtx, useServices } from './ShellContext';
import { StopPanel } from './StopPanel';
import { TopBar } from './TopBar';
import { WorkbenchWelcome } from './WorkbenchWelcome';
import { StepBand } from './test/TestScreen';
import './shell.css';

function rememberedRoute(): string {
  try {
    const r = sessionStorage.getItem(RETURN_KEY);
    return r && /^\/(p|l)\//.test(r) ? r : '/l/m1-default';
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

function ShellBody() {
  const s = useServices();
  const { shell, stop, uiState, net, gate, toasts } = s;
  useStoreVersion(shell);
  const route = useHashRoute();
  const welcome = shell.channels.size === 0 && route.kind === 'layout' && route.name === 'm1-default' && !s.layouts.list().includes(route.name);

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
  return (
    <div className="shell" data-testid="loom-shell" data-env={shell.env ?? 'unknown'} data-locked={shell.status.writeLocked}>
      <TopBar />
      <Banners />
      <StopPanel />
      <div className="workbench-body">
        <aside className="workbench-sidebar" aria-label="인터페이스 탐색기">
          <ChannelBar />
          <div className="sidebar-guide">
            <span className="eyebrow">탐색 안내</span>
            <p>인터페이스를 선택해 작업 패널을 여세요.</p>
            <span>패널을 열어도 장치에 연결하거나 데이터를 보내지 않습니다.</span>
          </div>
        </aside>
        <div className="workbench-main">
          <div className="workspace-heading">
            <div><span className="eyebrow">WORKSPACE</span><h1>개발 워크벤치</h1></div>
            <button type="button" className="help-button" onClick={() => uiState.setHelpOpen(true)} title="키보드 단축키 (Ctrl-] ?)">단축키 <kbd>?</kbd></button>
          </div>
          <div className="sub-bar" role="region" aria-label="워크스페이스 도구">
            <LayoutBar route={route} />
            <TabSettingsControls />
          </div>
          <div className={`workspace-surface${welcome ? ' has-welcome' : ''}`}>
            <div className="workspace-dock" inert={welcome || undefined} aria-hidden={welcome || undefined}>
              <DockLayout route={route} />
            </div>
            {welcome && <WorkbenchWelcome />}
          </div>
        </div>
      </div>
      <footer className="foot">
        <StepBand />
        <ActivityStrip />
        <div className="keys">
          <span className="shortcut-hint"><kbd>Ctrl-]</kbd> <kbd>x</kbd> STOP <span>·</span> <kbd>m</kbd> 마커 <span>·</span> <kbd>?</kbd> 도움말</span>
          <span className="footer-status"><span>LOOM / {shell.env ?? 'unknown'}</span><HeartbeatClock /></span>
        </div>
      </footer>
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
