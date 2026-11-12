import { useStoreVersion } from '../store/hooks';
import { useServices } from './ShellContext';
import { SERVICE_LABELS, type Route } from './routes';

const INTERFACE_SERVICES = ['uart', 'can', 'eth', 'psu'] as const;

/** Navigation is derived from OPEN aliases only; links never operate devices. */
export function ServiceNav({ route }: { route: Route }) {
  const { shell } = useServices();
  useStoreVersion(shell);
  const counts = new Map<string, number>();
  for (const channel of shell.channels.values()) {
    if (channel.state === 'open') counts.set(channel.domain, (counts.get(channel.domain) ?? 0) + 1);
  }
  return <nav className="service-nav" aria-label="서비스">
    <a className="service-integration" href="#/l/m1-default" aria-current={route.kind === 'layout' && !route.service && route.name === 'm1-default' ? 'page' : undefined}>통합 작업 공간</a>
    {INTERFACE_SERVICES.some((domain) => counts.has(domain)) && <span className="service-nav-label">인터페이스 서비스</span>}
    {INTERFACE_SERVICES.filter((domain) => counts.has(domain)).map((domain) =>
      <a key={domain} href={`#/s/${domain}`} aria-current={(route.kind === 'layout' && route.service === domain) || (route.kind === 'panel' && route.slot.panel.startsWith(`${domain}.`)) ? 'page' : undefined}>
        <span>{SERVICE_LABELS[domain]}</span><span className="service-count">{counts.get(domain)}</span>
      </a>)}
    {counts.has('uds') && <><span className="service-nav-label">진단</span>
      <a href="#/s/uds" aria-current={(route.kind === 'layout' && route.service === 'uds') || (route.kind === 'panel' && route.slot.panel.startsWith('uds.')) ? 'page' : undefined}>
        <span>UDS 진단</span><span className="service-count">{counts.get('uds')}</span>
      </a></>}
    {route.kind === 'layout' && ((route.service === 'can' || route.service === 'eth') && counts.has('uds') || route.service === 'uds' && (counts.has('can') || counts.has('eth'))) &&
      <div className="service-relations" aria-label="관련 화면">
        <span>관련 화면</span>
        {(route.service === 'can' || route.service === 'eth') && counts.has('uds') && <a href="#/s/uds">UDS 진단 보기 →</a>}
        {route.service === 'uds' && counts.has('can') && <a href="#/s/can">CAN 관찰 보기 →</a>}
        {route.service === 'uds' && counts.has('eth') && <a href="#/s/eth">Ethernet 관찰 보기 →</a>}
      </div>}
  </nav>;
}
