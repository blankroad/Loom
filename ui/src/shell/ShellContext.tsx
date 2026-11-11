import { createContext, useContext } from 'react';
import type { ShellServices } from './services';

export const ShellCtx = createContext<ShellServices | null>(null);

export function useServices(): ShellServices {
  const s = useContext(ShellCtx);
  if (!s) throw new Error('ShellCtx missing');
  return s;
}
