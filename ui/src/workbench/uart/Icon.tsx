import { ICONS } from './icons';

/** React form of the REV6.1 icons. */
export function I({ id, className = 'i' }: { id: string; className?: string }) {
  return <svg className={className} viewBox="0 0 16 16" aria-hidden="true" dangerouslySetInnerHTML={{ __html: ICONS[id] ?? '' }} />;
}
