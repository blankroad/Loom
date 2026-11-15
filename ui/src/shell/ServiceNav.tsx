import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';
import { routeModule, SERVICE_LABELS, SERVICE_TAB_ORDER, type Route, type ServiceDomain } from './routes';

/** Seven PERMANENT module tabs (user rule, superseding the earlier active-services-only bar): UART, CAN, Ethernet,
 * UDS, PSU, logic, video are always reachable, whatever is connected — each is a full module application with its own
 * Connect/configuration. Counts are truthful: the number of OPEN channels of that service (UDS: real UDS
 * connections only; CAN/Ethernet observations are not counted as diagnostics). A tab is navigation only: links never
 * open, close or send to a device. Integrated (통합) stays the user's own observation workspace of actual channels. */
const ORDER = SERVICE_TAB_ORDER;
const ICON: Record<ServiceDomain | 'integrated', string> = {
  integrated: 'M2.5 2.5h4.5v4.5H2.5zM9 2.5h4.5v4.5H9zM2.5 9h4.5v4.5H2.5zM9 9h4.5v4.5H9z',
  uart: 'M2 3h12v10H2zM4.5 6.5l2 1.5-2 1.5M8 10h3.5',
  eth: 'M3 2.5h10v7H3zM5.5 9.5v2M10.5 9.5v2M3.5 13.5h9',
  uds: 'M8 2.5v3M8 10.5v3M2.5 8h3M10.5 8h3M5 5l6 6M11 5l-6 6',
  can: 'M2 6h12M2 10h12M5 6v4M11 6v4',
  psu: 'M9 1.5 4 9h4l-1 5.5L12 7H8z',
  logic: 'M1.5 11h3V5h3v6h3V5h3.5',
  video: 'M2 4h8.5v8H2zM10.5 7l3.5-2v6l-3.5-2',
};
/** full label + a short form the header uses on narrow windows (both readable; the full name stays in title/aria) */
const LABEL: Record<ServiceDomain, { full: string; short: string }> = {
  uart: { full: 'UART', short: 'UART' }, can: { full: 'CAN', short: 'CAN' }, eth: { full: 'Ethernet', short: 'ETH' },
  uds: { full: 'UDS 진단', short: 'UDS' }, psu: { full: 'PSU', short: 'PSU' }, logic: { full: SERVICE_LABELS.logic, short: '로직' }, video: { full: SERVICE_LABELS.video, short: '비디오' },
};

function Icon({ d }: { d: string }) {
  return <svg className="i" viewBox="0 0 16 16" aria-hidden="true"><path d={d} /></svg>;
}

export function ServiceNav({ route }: { route: Route }) {
  const { shell } = useServices();
  useStoreVersion(shell);
  const counts = new Map<string, number>();
  for (const channel of shell.channels.values()) {
    if (channel.state === 'open') counts.set(channel.domain, (counts.get(channel.domain) ?? 0) + 1);
  }
  const current = routeModule(route);
  return <nav className="svcnav service-nav" aria-label="서비스">
    <a className="service-integration" href="#/l/m1-default" title="통합 작업 공간 — 연결된 인터페이스에서 고른 관찰 보기" aria-current={current === null ? 'page' : undefined}><Icon d={ICON.integrated} /><span>통합</span></a>
    {ORDER.map((domain) => {
      const n = counts.get(domain) ?? 0;
      const { full, short } = LABEL[domain];
      return <a key={domain} className={short !== full ? 'has-short' : undefined} href={`#/s/${domain}`} data-service={domain} data-count={n} title={`${full} 작업대 · 열린 인터페이스 ${n}개`} aria-label={`${full}${n ? `, 열린 ${n}개` : ''}`} aria-current={current === domain ? 'page' : undefined}>
        <Icon d={ICON[domain]} /><span className="svc-label">{full}</span>{short !== full && <span className="svc-short" aria-hidden="true">{short}</span>}{n > 0 && <span className="service-count" aria-hidden="true">{n}</span>}
      </a>;
    })}
  </nav>;
}
