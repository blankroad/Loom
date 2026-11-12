import { useEffect, useState } from 'react';
import { EXPLORER_DOMAINS, viewsForChannel } from '../store/channels';
import { InterfaceConnect } from './InterfaceConnect';
import './explorer.css';
import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';

const NEW_MARK_MS = 10_000;
const GEN_MARK_MS = 10_000;

const STATE_LABEL = { open: '연결됨', closed: '닫힘', unknown: '상태 미확인' };

/** Live interfaces first. Inactive discovery/history is retained behind explicit disclosure. */
export function ChannelBar({ domains: scopedDomains }: { domains?: readonly string[] } = {}) {
  const { shell, showSlot, now } = useServices();
  useStoreVersion(shell);
  const [query, setQuery] = useState('');
  const [showInactive, setShowInactive] = useState(false);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const allChannels = [...shell.channels.values()].filter((c) => !scopedDomains || scopedDomains.includes(c.domain));
  const active = allChannels.filter((c) => c.state === 'open');
  const inactiveCount = allChannels.length - active.length;
  const chans = showInactive ? allChannels : active;
  const t = now();
  const marked = chans.some(
    (c) => (c.addedLiveMs !== null && t - c.addedLiveMs < NEW_MARK_MS) || (c.genChangedMs !== null && t - c.genChangedMs < GEN_MARK_MS),
  );
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!marked) return;
    const h = setInterval(() => setTick((x) => x + 1), 1_000);
    return () => clearInterval(h);
  }, [marked]);
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const matches = chans.filter((c) => {
    const text = `${c.domain} ${c.channel} ${c.state} ${STATE_LABEL[c.state]}`.toLocaleLowerCase();
    return terms.every((term) => text.includes(term));
  });
  const domains = [...EXPLORER_DOMAINS, ...new Set(chans.map((c) => c.domain).filter((d) => !EXPLORER_DOMAINS.some((known) => known === d)).sort())];
  return (
    <nav className="channel-bar channel-explorer" aria-label="채널">
      <header className="explorer-header">
        <div className="explorer-heading"><strong>인터페이스</strong><span aria-label="연결된 인터페이스 수">{active.length}</span></div>
        <InterfaceConnect />
        <label className="explorer-search">
          <span className="explorer-search-label">채널 검색</span>
          <input type="search" aria-label="채널 검색" placeholder="이름, 도메인, 상태 검색" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => { if (e.key === 'Escape') setQuery(''); }} />
        </label>
        <p className="explorer-subtitle">채널을 선택해 보기를 엽니다.</p>
        {inactiveCount > 0 && <button type="button" className="explorer-inactive-toggle" aria-expanded={showInactive} onClick={() => setShowInactive((value) => !value)}>{showInactive ? '연결된 인터페이스만 보기' : `비활성 인터페이스 ${inactiveCount}개 보기`}</button>}
      </header>
      <div className="explorer-groups">
        {chans.length === 0 ? <p className="explorer-empty">연결된 채널 없음<br /><span>사용할 인터페이스를 추가하세요.</span></p>
          : matches.length === 0 ? <p className="explorer-empty" role="status">검색 결과 없음<button type="button" onClick={() => setQuery('')}>검색 지우기</button></p>
          : domains.map((domain) => {
            const channels = matches.filter((c) => c.domain === domain).sort((a, b) => a.channel.localeCompare(b.channel));
            if (!channels.length) return null;
            const expanded = terms.length > 0 || !collapsed.has(domain);
            return <section className="explorer-group" key={domain} aria-label={`${domain.toUpperCase()} 채널`}>
              <button type="button" className="explorer-group-toggle" aria-expanded={expanded} onClick={() => setCollapsed((prev) => {
                const next = new Set(prev);
                if (next.has(domain)) next.delete(domain); else next.add(domain);
                return next;
              })}><span aria-hidden="true">{expanded ? '▾' : '▸'}</span> {domain.toUpperCase()} <span className="explorer-count">{channels.length}</span></button>
              {expanded && <ul className="explorer-channel-list">{channels.map((c) => {
                const views = viewsForChannel(c.domain, c.channel);
                const first = views[0];
                const isNew = c.addedLiveMs !== null && t - c.addedLiveMs < NEW_MARK_MS;
                const genNew = c.genChangedMs !== null && t - c.genChangedMs < GEN_MARK_MS;
                const cls = `chan chan-${c.state}${isNew ? ' chan-new' : ''}${genNew ? ' chan-gen' : ''}`;
                const label = <><span className="explorer-channel-name">{c.channel}</span><span className="explorer-state">{c.state === 'closed' ? '(닫힘)' : STATE_LABEL[c.state]}</span>{c.generation !== null && <span className="explorer-generation">gen{c.generation}{genNew ? ' ↻' : ''}</span>}</>;
                return <li key={c.channel} className="explorer-channel">
                  {first ? <button type="button" className={cls} data-chan={`${c.domain}/${c.channel}`} data-state={c.state} aria-label={`${domain.toUpperCase()} ${c.channel} ${first.label} 열기 · ${STATE_LABEL[c.state]}`} title={`${first.label} 열기`} onClick={() => showSlot({ panel: first.panel, props: first.props })}>{label}</button>
                    : <div className={cls} data-chan={`${c.domain}/${c.channel}`} data-state={c.state}>{label}</div>}
                  {views.length ? <div className="explorer-views" aria-label={`${c.channel} 보기`}>{views.map((v) => <button key={v.panel} type="button" data-view={v.panel} aria-label={`${domain.toUpperCase()} ${c.channel} ${v.label} 보기`} onClick={() => showSlot({ panel: v.panel, props: v.props })}>{v.label}</button>)}</div>
                    : <span className="explorer-unsupported">이 버전에 지원되는 보기 없음</span>}
                </li>;
              })}</ul>}
            </section>;
          })}
      </div>
      {shell.overflow.channels > 0 && <p className="explorer-overflow">+{shell.overflow.channels} 표시 안 됨</p>}
    </nav>
  );
}

/** 1 Hz clock + heartbeat dot: updates even with no data, so a frozen page is visible. */
export function HeartbeatClock() {
  const { shell } = useServices();
  useStoreVersion(shell);
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const h = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(h);
  }, []);
  const s = shell.status;
  const cls = s.badge === 'STALLED' ? 'hb-stalled' : s.warnDot ? 'hb-warn' : 'hb-ok';
  return (
    <span className={`hb-clock ${cls}`} data-testid="hb-clock">
      <span aria-hidden="true">●</span> {now.toLocaleTimeString('ko-KR', { hour12: false })}
    </span>
  );
}
