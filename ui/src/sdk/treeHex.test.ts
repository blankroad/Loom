// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { TreeHex } from './treeHex';

afterEach(() => document.body.replaceChildren());

it('synchronizes ASCII and hex selection, resets callback, and preserves accessible selection', () => {
  const host = document.createElement('div');
  document.body.append(host);
  const onSelectNode = vi.fn();
  const view = new TreeHex(host, { onSelectNode });
  const nodes = [{ label: 'packet', off: 0, len: 3, children: [
    { label: 'field A', off: 0, len: 2 }, { label: 'field B', off: 2, len: 1 },
  ] }];
  view.set(nodes, new Uint8Array([65, 66, 0]));
  host.querySelector<HTMLElement>('.th-char[data-off="1"]')!.click();
  expect(view.selectedLabel).toBe('field A');
  expect(host.querySelectorAll('.th-char.hl')).toHaveLength(2);
  expect(host.querySelectorAll('.th-b.hl')).toHaveLength(2);
  host.querySelector<HTMLElement>('.th-b[data-off="2"]')!.click();
  expect(host.querySelectorAll('[aria-selected="true"]')).toHaveLength(1);
  expect(onSelectNode).toHaveBeenLastCalledWith(nodes[0]!.children![1]);
  view.clear();
  expect(onSelectNode).toHaveBeenLastCalledWith(null);
  expect(host.querySelectorAll('.th-char')).toHaveLength(0);
  view.dispose();
});

it('supports optional collapsed layers and keyboard folding without changing the default', () => {
  const host = document.createElement('div');
  const view = new TreeHex(host, { collapseTopLayers: true });
  view.set([{ label: 'packet', children: [{ label: 'field' }] }], null);
  const item = host.querySelector<HTMLElement>('.th-node')!;
  expect(item.getAttribute('aria-expanded')).toBe('false');
  item.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
  expect(item.getAttribute('aria-expanded')).toBe('true');
  item.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
  expect(item.getAttribute('aria-expanded')).toBe('false');
  const defaultView = new TreeHex(host);
  defaultView.set([{ label: 'packet', children: [{ label: 'field' }] }], null);
  expect(defaultView.root.querySelector('.th-node')!.getAttribute('aria-expanded')).toBe('true');
  view.dispose();
  defaultView.dispose();
});
