// @vitest-environment jsdom
// operator-upgrade B4a DOM contract — ETH-02 tree → filter menu, ETH-03 follow by stream_proto, ETH-04 replay from
// the GUI (typed daemon path), REPLAY chips, and the B2 follow-up (live badge tooltip). FUNCTIONS.md F4.8–F4.16.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ev, mountPanel, type Harness } from '../../sdk/testing/harness';
import './ethPanel';
import { fixtureRecord } from './fixtures';
import { loadConfigs } from './ifaceConfigs';

let h: Harness | null = null;
afterEach(() => {
  h?.dispose();
  h = null;
  vi.useRealTimers();
  localStorage.clear();
  Reflect.deleteProperty(navigator, 'clipboard');
  document.body.replaceChildren();
});
const flush = async (hh: Harness) => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
  hh.frame();
};
const q = <T extends Element = HTMLElement>(sel: string) => h!.host.querySelector<T>(sel)!;
const cmd = (c: string) => q<HTMLButtonElement>(`[data-cmd="${c}"]`);
const pktRows = (hh: Harness) => [...hh.host.querySelectorAll<HTMLElement>('.eth-list .vt-row')].filter((e) => !e.hidden);
const writes = () => h!.net.calls.filter((c) => !['eth.capture.status', 'eth.ifaces', 'eth.filter.check', 'eth.show', 'eth.query'].includes(c.method));
const input = (el: HTMLInputElement, v: string) => {
  el.value = v;
  el.dispatchEvent(new Event('input'));
};

const TREE = [
  {
    name: 'udp',
    label: 'User Datagram Protocol, Src Port: 40000, Dst Port: 65201',
    filter: 'udp',
    off: 0,
    len: 8,
    children: [
      { name: 'udp.dstport', label: 'Destination port: 65201', filter: 'udp.dstport == 65201', off: 2, len: 2, children: [] },
      { name: 'x.opaque', label: 'Opaque', filter: null, off: 4, len: 2, children: [] },
    ],
  },
];
const show = (p: unknown) => ({ ref: (p as { ref: string }).ref, len: 8, caplen: 8, bytes: { encoding: 'hex', data: '9c40feb100080000' }, tree: TREE });
const node = (label: string) => [...h!.host.querySelectorAll<HTMLElement>('.th-node')].find((n) => n.textContent?.includes(label))!;
const menu = () => h!.host.querySelector<HTMLElement>('[data-testid="eth-fld-menu"]');
const item = (c: string) => menu()!.querySelector<HTMLButtonElement>(`[data-cmd="${c}"]`)!;
const context = (label: string) => node(label).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 30, clientY: 40 }));

describe('ETH-02 tree → display filter', () => {
  const setup = async () => {
    vi.useFakeTimers();
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.show', show);
    h.net.responders.set('eth.filter.check', () => ({ ok: true }));
    h.net.responders.set('eth.query', () => ({ rows: [], partial_reasons: [], complete: true }));
    h.batch([fixtureRecord('eth.pkt', 70, { proto: 'UDP', info: 'x', len: 50 })]);
    pktRows(h)[0]!.querySelector<HTMLElement>('.vt-td')!.click();
    await vi.advanceTimersByTimeAsync(5);
    h.frame();
  };

  it('right click on a field opens the field menu with daemon-made expressions; status bar names the field', async () => {
    await setup();
    expect(cmd('fld-menu').disabled).toBe(true); // no field selected yet
    context('Destination port');
    h!.frame();
    const m = menu()!;
    expect(m.getAttribute('role')).toBe('menu');
    expect(m.querySelector('.eth-fld-head')!.textContent).toBe('udp.dstport — udp.dstport == 65201');
    expect([...m.querySelectorAll('button')].map((b) => [b.dataset.cmd, b.textContent, b.disabled, b.getAttribute('role')])).toEqual([
      ['fld-apply', '필터로 적용', false, 'menuitem'],
      ['fld-prepare', '필터로 준비', false, 'menuitem'],
      ['fld-and', '…그리고(&&) 준비', false, 'menuitem'],
      ['fld-or', '…또는(||) 준비', false, 'menuitem'],
      ['fld-copy-name', '필드명 복사', false, 'menuitem'],
      ['fld-copy-filter', '필터 식 복사', false, 'menuitem'],
    ]);
    expect(document.activeElement).toBe(item('fld-apply'));
    expect([m.style.left, m.style.top]).toEqual(['30px', '40px']);
    expect(q('[data-testid="eth-sb-field"]').textContent).toContain('Destination port: 65201 (udp.dstport)');
    expect(cmd('fld-menu').disabled).toBe(false);
    // keyboard: ArrowDown moves, Escape closes
    m.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    expect(document.activeElement).toBe(item('fld-prepare'));
    m.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(menu()).toBeNull();
  });

  it('prepare and/or edits the field text only; apply validates on the server then pushes the filter down', async () => {
    await setup();
    const f = q<HTMLInputElement>('[data-testid="eth-filter-input"]');
    input(f, 'arp');
    context('Destination port');
    item('fld-and').click();
    expect(menu()).toBeNull();
    expect(f.value).toBe('(arp) && udp.dstport == 65201');
    context('Destination port');
    item('fld-or').click();
    expect(f.value).toBe('((arp) && udp.dstport == 65201) || udp.dstport == 65201');
    context('Destination port');
    item('fld-prepare').click();
    expect(f.value).toBe('udp.dstport == 65201');
    await vi.advanceTimersByTimeAsync(200);
    expect(h!.net.subs).toHaveLength(1); // nothing applied by preparing
    context('User Datagram Protocol');
    item('fld-apply').click();
    await vi.advanceTimersByTimeAsync(200);
    h!.frame();
    expect(h!.net.calls.some((c) => c.method === 'eth.filter.check' && (c.params as { expr: string }).expr === 'udp')).toBe(true);
    expect(h!.net.subs.at(-1)!.filter.ext).toEqual({ eth_filter: 'udp' });
    expect(h!.net.calls.filter((c) => c.method === 'eth.query').at(-1)!.params).toMatchObject({ filter: 'udp' });
  });

  it('a node without a daemon filter only offers its name; copy uses the clipboard; outside click closes; one menu at a time', async () => {
    await setup();
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    context('Opaque');
    expect(menu()!.querySelector('.eth-fld-head')!.textContent).toBe('x.opaque');
    for (const c of ['fld-apply', 'fld-prepare', 'fld-and', 'fld-or', 'fld-copy-filter']) {
      expect(item(c).disabled, c).toBe(true);
      expect(item(c).title, c).toBe('daemon이 이 항목의 필터 식을 주지 않았습니다');
    }
    expect(document.activeElement).toBe(item('fld-copy-name'));
    item('fld-copy-name').click();
    await vi.advanceTimersByTimeAsync(1);
    expect(writeText).toHaveBeenCalledWith('x.opaque');
    expect(h!.toasts.at(-1)).toBe('복사: x.opaque');
    context('Destination port');
    context('Destination port');
    expect(h!.host.querySelectorAll('[data-testid="eth-fld-menu"]')).toHaveLength(1);
    item('fld-copy-filter').click();
    await vi.advanceTimersByTimeAsync(1);
    expect(writeText).toHaveBeenLastCalledWith('udp.dstport == 65201');
    context('Destination port');
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    expect(menu()).toBeNull();
    // the detail-bar button opens the same menu for the selected field (keyboard/mouse discoverable);
    // its enabled state is drawn on the next frame (onSelectNode → scheduler.invalidate)
    h!.frame();
    cmd('fld-menu').click();
    expect(menu()!.querySelector('.eth-fld-head')!.textContent).toBe('udp.dstport — udp.dstport == 65201');
    // selecting another packet closes it
    h!.batch([fixtureRecord('eth.pkt', 71, { proto: 'UDP', info: 'y', len: 50 })]);
    pktRows(h!)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    expect(menu()).toBeNull();
  });
});

describe('ETH-03 follow uses the stream index space', () => {
  it('stream_proto udp (DoIP over UDP) follows udp; a legacy row without stream_proto stays tcp', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.show', show);
    h.net.responders.set('eth.follow', () => ({ segments: [], partial_reasons: [], complete: true }));
    h.batch([fixtureRecord('eth.pkt', 80, { proto: 'DoIP', info: 'x', len: 60, stream: 3, stream_proto: 'udp' }), fixtureRecord('eth.pkt', 81, { proto: 'UDP', info: 'y', len: 60, stream: 4 })]);
    pktRows(h)[0]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    expect(cmd('follow').title).toBe('선택 패킷의 대화 따라가기 — UDP stream 3');
    cmd('follow').click();
    await flush(h);
    expect(h.net.calls.filter((c) => c.method === 'eth.follow').at(-1)!.params).toMatchObject({ proto: 'udp', stream: 3 });
    expect(q('[data-testid="eth-follow-dialog"]').getAttribute('aria-label')).toBe('스트림 따라가기 — UDP stream 3');
    q('[data-testid="eth-follow-dialog"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    pktRows(h)[1]!.querySelector<HTMLElement>('.vt-td')!.click();
    await flush(h);
    cmd('follow').click();
    await flush(h);
    expect(h.net.calls.filter((c) => c.method === 'eth.follow').at(-1)!.params).toMatchObject({ proto: 'tcp', stream: 4 });
  });
});

describe('ETH-04 replay a pcap/pcapng from the GUI (typed daemon path)', () => {
  it('unbound 파일 열기 → replay form → exactly one eth.capture.start with replay options; config saved as REPLAY', async () => {
    h = mountPanel('eth.packets', {});
    h.net.responders.set('eth.capture.status', () => ({ channels: [] }));
    h.net.responders.set('eth.ifaces', () => ({ ifaces: [{ name: 'en5' }, { name: 'replay' }] }));
    await flush(h);
    expect(cmd('iface-replay').textContent).toBe('파일 열기 (replay)…');
    cmd('iface-replay').click();
    await flush(h);
    const dlg = q('[data-testid="eth-replay-dialog"]');
    expect(dlg.querySelector('header')!.textContent).toBe('파일 열기 — replay (daemon 머신의 pcap/pcapng)');
    expect(writes()).toEqual([]);
    const backend = dlg.querySelector<HTMLSelectElement>('select[aria-label="백엔드"]')!;
    expect([...backend.options].map((o) => o.value)).toEqual(['live', 'sim', 'replay']);
    expect(backend.value).toBe('replay');
    const nicField = dlg.querySelector<HTMLInputElement>('input[aria-label="NIC"]')!.closest('label')!;
    const box = dlg.querySelector<HTMLElement>('[data-testid="eth-replay-fields"]')!;
    expect([nicField.hidden, box.hidden]).toEqual([true, false]);
    expect(dlg.querySelector<HTMLInputElement>('input.bpf')!.disabled).toBe(true);
    const speed = dlg.querySelector<HTMLSelectElement>('[data-testid="eth-replay-speed"]')!;
    expect([...speed.options].map((o) => [o.value, o.textContent])).toEqual([
      ['max', '최대 속도'],
      ['1', '원래 속도 ×1'],
      ['2', '×2'],
      ['10', '×10'],
    ]);
    expect(q('[data-testid="eth-capture-msg"]').textContent).toBe('REPLAY: 파일을 재생합니다 — 실제 NIC 캡처가 아니며, 경로는 daemon이 실행 중인 머신 기준입니다.');
    input(dlg.querySelector<HTMLInputElement>('input[aria-label="새 채널 이름"]')!, 'rp1');
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-add-channel-start"]')!.click();
    await flush(h);
    expect(q('[data-testid="eth-capture-msg"]').textContent).toBe('파일 경로를 입력하세요 (daemon 머신 기준)');
    expect(writes()).toEqual([]);
    input(dlg.querySelector<HTMLInputElement>('[data-testid="eth-replay-path"]')!, ' /data/logs/run1.pcapng ');
    speed.value = '2';
    speed.dispatchEvent(new Event('change', { bubbles: true }));
    dlg.querySelector<HTMLInputElement>('[data-testid="eth-replay-repeat"]')!.checked = true;
    dlg.querySelector<HTMLButtonElement>('[data-testid="eth-add-channel-start"]')!.click();
    await flush(h);
    expect(writes()).toEqual([
      { method: 'eth.capture.start', params: { iface: 'run1.pcapng', ch: 'rp1', backend: 'replay', snaplen: 262144, promisc: false, buffer_mib: 64, replay: { path: '/data/logs/run1.pcapng', speed: 2, repeat: true } } },
    ]);
    expect(loadConfigs()).toEqual([{ alias: 'rp1', iface: 'run1.pcapng', backend: 'replay', bpf: '', snaplen: 262144, promisc: false, bufferMib: 64, replay: { path: '/data/logs/run1.pcapng', speed: 2, repeat: true } }]);
    expect([...q<HTMLSelectElement>('[data-testid="eth-iface-config"]').options].map((o) => o.textContent)).toContain('rp1 · run1.pcapng (REPLAY)');
  });

  it('the same form switches back to a NIC for live/sim; editing a saved replay config prefills it', async () => {
    localStorage.setItem('loom.eth.ifaces.v1', JSON.stringify([{ alias: 'rp2', iface: 'b.pcap', backend: 'replay', bpf: '', snaplen: 65535, promisc: false, bufferMib: 64, replay: { path: '/x/b.pcap', speed: 'max', repeat: false } }]));
    h = mountPanel('eth.packets', {});
    h.net.responders.set('eth.capture.status', () => ({ channels: [] }));
    await flush(h);
    cmd('iface-edit').click();
    await flush(h);
    const dlg = q('[data-testid="eth-connect-dialog"]');
    expect(dlg.querySelector<HTMLInputElement>('[data-testid="eth-replay-path"]')!.value).toBe('/x/b.pcap');
    expect(dlg.querySelector<HTMLSelectElement>('[data-testid="eth-replay-speed"]')!.value).toBe('max');
    const backend = dlg.querySelector<HTMLSelectElement>('select[aria-label="백엔드"]')!;
    backend.value = 'live';
    backend.dispatchEvent(new Event('change'));
    expect(dlg.querySelector<HTMLElement>('[data-testid="eth-replay-fields"]')!.hidden).toBe(true);
    expect(dlg.querySelector<HTMLInputElement>('input[aria-label="NIC"]')!.closest('label')!.hidden).toBe(false);
    expect(dlg.querySelector<HTMLInputElement>('input.bpf')!.disabled).toBe(false);
  });

  it('REPLAY chips: this channel (capture_state backend replay) and other daemon channels', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.capture.status', () => ({ channels: [{ ch: 'rp9', state: 'replaying', backend: 'replay', iface: 'a.pcap' }] }));
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '3', generation: 1, payload: { ch: 'lan1', state: 'replaying', backend: 'replay', iface: 'run1.pcapng' } }));
    await flush(h);
    const status = q('[data-testid="eth-status"]');
    const chip = status.querySelector<HTMLElement>('.wb-chip.replay')!;
    expect([chip.hidden, chip.textContent, chip.title]).toEqual([false, 'REPLAY 파일', 'daemon이 파일을 재생 중 — 실제 NIC 캡처 아님']);
    expect(status.querySelector<HTMLElement>('.wb-chip.sim')!.hidden).toBe(true);
    const card = q('[data-testid="eth-other-channels"] .eth-chcard[data-channel="rp9"]');
    expect(card.querySelector('.wb-chip.replay')!.textContent).toBe('REPLAY');
    h.net.emitControl(ev({ kind: 'eth.capture_state', domain: 'eth', channel: 'lan1', seq: '4', generation: 2, payload: { ch: 'lan1', state: 'live', backend: 'sim', iface: 'sim' } }));
    h.frame();
    expect(status.querySelector<HTMLElement>('.wb-chip.replay')!.hidden).toBe(true);
  });

  it('the bound channel capture form also offers replay', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    cmd('start-all').click();
    await flush(h);
    const be = q<HTMLSelectElement>('[data-testid="eth-capture-dialog"] select[aria-label="백엔드"]');
    expect([...be.options].map((o) => o.value)).toEqual(['live', 'sim', 'replay']);
  });
});

describe('B2 follow-up', () => {
  it('back to live clears the complete-page tooltip on the mode badge', async () => {
    h = mountPanel('eth.packets', { channel: 'lan1' });
    h.net.responders.set('eth.query', () => ({ rows: [], partial_reasons: [], complete: true, scope: { from: '-600s', to: null, from_source: 'explicit', lookback_secs: null }, scanned: { first_seq: null, last_seq: null, bytes: 0 } }));
    const panel = q('.eth-panel');
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    await flush(h);
    expect(q('[data-testid="eth-mode"]').title).not.toBe('');
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    h.frame();
    expect(q('[data-testid="eth-mode"]').title).toBe('');
  });
});
