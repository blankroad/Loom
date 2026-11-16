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

// operator-upgrade B4a (ETH-02, FUNCTIONS.md F4.3–F4.5): optional name/filter node fields and an optional
// context hook. Other consumers (no hook) keep the browser default and are unaffected.
it('context request (right click / ContextMenu / Shift+F10) selects the node first and hands it to the optional hook', () => {
  const host = document.createElement('div');
  document.body.append(host);
  const onNodeContext = vi.fn();
  const view = new TreeHex(host, { onNodeContext });
  const leafNode = { label: 'Destination port: 65201', name: 'udp.dstport', filter: 'udp.dstport == 65201', off: 2, len: 2 };
  const nodes = [{ label: 'User Datagram Protocol', name: 'udp', filter: 'udp', off: 0, len: 8, children: [leafNode] }];
  view.set(nodes, new Uint8Array(8));
  expect(view.selectedNode).toBeNull();
  const leaf = host.querySelectorAll<HTMLElement>('.th-node')[1]!;
  const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 20 });
  leaf.dispatchEvent(ev);
  expect(ev.defaultPrevented).toBe(true);
  expect(view.selectedNode).toBe(leafNode);
  expect(view.selectedLabel).toBe('Destination port: 65201');
  expect(host.querySelectorAll('.th-b.hl')).toHaveLength(2);
  expect(onNodeContext).toHaveBeenLastCalledWith(leafNode, leaf, { x: 10, y: 20 });
  // keyboard: ContextMenu key and Shift+F10 on the focused node (position from its box)
  const top = host.querySelectorAll<HTMLElement>('.th-node')[0]!;
  const k1 = new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true, cancelable: true });
  top.dispatchEvent(k1);
  expect(k1.defaultPrevented).toBe(true);
  expect(onNodeContext).toHaveBeenLastCalledWith(nodes[0], top, { x: 0, y: 0 });
  leaf.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
  expect(onNodeContext).toHaveBeenCalledTimes(3);
  expect(view.selectedNode).toBe(leafNode);
  // outside a node: nothing
  host.querySelector<HTMLElement>('.th-tree')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  expect(onNodeContext).toHaveBeenCalledTimes(3);
  view.set([], null);
  expect(view.selectedNode).toBeNull();
  view.dispose();
});

it('without the hook the default context menu is untouched and nothing is selected', () => {
  const host = document.createElement('div');
  const view = new TreeHex(host);
  view.set([{ label: 'field', name: 'x', filter: null }], null);
  const node = host.querySelector<HTMLElement>('.th-node')!;
  const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
  node.dispatchEvent(ev);
  expect(ev.defaultPrevented).toBe(false);
  expect(view.selectedNode).toBeNull();
  const k = new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true, cancelable: true });
  node.dispatchEvent(k);
  expect(k.defaultPrevented).toBe(false);
  view.dispose();
});
