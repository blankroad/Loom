// Launch/error page. Talks to the Rust side only through the two app commands granted to this
// local page (capabilities/launch.json); the daemon-served Loom UI has no IPC access.
(function () {
  'use strict';
  const ipc = window.__TAURI_INTERNALS__;
  const $ = (id) => document.getElementById(id);
  const panels = ['connecting', 'failed', 'connected'];
  let busy = false;

  function show(name) {
    for (const p of panels) $(p).hidden = p !== name;
  }

  function render(view) {
    if (!view || typeof view.state !== 'string') return;
    if (view.state === 'connecting') {
      show('connecting');
      $('connecting-home').textContent = view.home ? 'Loom home: ' + view.home : '';
    } else if (view.state === 'failed') {
      show('failed');
      $('message').textContent = view.message || 'Unknown error';
      $('home').textContent = view.home || '—';
      $('daemon-bin').textContent = view.daemon_bin || '—';
      $('retry').disabled = false;
    } else if (view.state === 'connected') {
      show('connected');
    }
  }

  async function poll() {
    if (!ipc) {
      show('failed');
      $('message').textContent = 'This page must run inside the Loom desktop app.';
      $('retry').disabled = true;
      return;
    }
    try {
      render(await ipc.invoke('launch_state'));
    } catch (e) {
      // keep polling; the Rust side owns the truth
    }
    setTimeout(poll, 250);
  }

  $('retry').addEventListener('click', async () => {
    if (busy || !ipc) return;
    busy = true;
    $('retry').disabled = true;
    try {
      render(await ipc.invoke('retry'));
    } finally {
      busy = false;
    }
  });

  poll();
})();
