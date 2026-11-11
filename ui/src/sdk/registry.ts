// Static panel registry (plan §4.4). All code panels are bundled with the SPA;
// there is no dynamic loading. Panels register at import time.

import type { PanelContext } from './context';
import { validateManifest, type PanelManifest } from './manifest';

export interface PanelInstance {
  mount(el: HTMLElement): void;
  resize(w: number, h: number): void;
  dispose(): void;
}

export type PanelFactory = (ctx: PanelContext) => PanelInstance;

export interface RegisteredPanel {
  manifest: PanelManifest;
  factory: PanelFactory;
}

const panels = new Map<string, RegisteredPanel>();

export function registerPanel(manifest: PanelManifest, factory: PanelFactory): void {
  validateManifest(manifest);
  if (panels.has(manifest.id)) throw new Error(`panel ${manifest.id} registered twice`);
  panels.set(manifest.id, { manifest, factory });
}

export function getPanel(id: string): RegisteredPanel | undefined {
  return panels.get(id);
}

export function listPanels(): RegisteredPanel[] {
  return [...panels.values()];
}

/** tests only */
export function _resetRegistryForTests(): void {
  panels.clear();
}
