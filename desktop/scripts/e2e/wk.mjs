#!/usr/bin/env node
// Native WKWebView driver for the Loom desktop app (test builds with cargo feature `e2e` only).
// Talks W3C WebDriver to the in-app server of tauri-plugin-wdio-webdriver (embedded provider of
// @wdio/tauri-service) on 127.0.0.1:<port>. Zero npm dependencies (Node >= 18 fetch).
//
//   node wk.mjs --wd <port> --out <dir> <command> [args]
//     ready                     wait for the Loom shell in the app window (after connect)
//     rx <marker>               UART trace for `dut` shows <marker> and live `tick` lines (advancing)
//     type <text> [--tab]       focus the `dut` xterm, type <text> (+Tab) + Enter as WebDriver keys
//     shows <text>              trace for `dut` shows <text> (CLI → UI shared stream)
//     launch-failed             launch page shows the Failed view + Retry (error path)
//     retry                     click Retry on the launch page
//     shot <name>               WKWebView snapshot → <out>/<name>.png
//
// Credential hygiene: never requests the page URL, title, source or cookies; JS returns only
// booleans/counters/visible panel text. location.hash is checked for `#boot=` as a boolean.
// Input is WebDriver "element send keys" → the plugin synthesizes DOM key/input events inside the
// real WKWebView (honest label: WK DOM input, not OS-level keystrokes).
import { mkdirSync, writeFileSync, appendFileSync } from 'node:fs';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const opt = (k, d) => { const i = argv.indexOf(k); if (i < 0) return d; const v = argv[i + 1]; argv.splice(i, 2); return v; };
const flag = (k) => { const i = argv.indexOf(k); if (i < 0) return false; argv.splice(i, 1); return true; };
const wd = Number(opt('--wd', process.env.LOOM_DESKTOP_QA_WEBDRIVER_PORT ?? '0'));
const out = opt('--out', '.');
const tab = flag('--tab');
const [cmd, arg] = argv;
if (!wd || !cmd) { console.error('usage: wk.mjs --wd <port> --out <dir> <command> [arg]'); process.exit(2); }
mkdirSync(out, { recursive: true });
const base = `http://127.0.0.1:${wd}`;
const log = (m) => { const l = `[wk ${cmd}] ${m}`; console.log(l); appendFileSync(join(out, 'wk.log'), l + '\n'); };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function call(method, path, body, timeoutMs = 15000) {
  const r = await fetch(base + path, {
    method, headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(timeoutMs),
  });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(`${method} ${path.replace(/\/session\/[^/]+/, '/session/:id')} → ${r.status} ${JSON.stringify(j.value ?? j).slice(0, 200)}`);
  return j.value;
}
async function until(what, fn, secs) {
  const end = Date.now() + secs * 1000; let last;
  while (Date.now() < end) { try { const v = await fn(); if (v) return v; } catch (e) { last = e; } await sleep(250); }
  throw new Error(`timeout after ${secs}s: ${what}${last ? ` (${last.message})` : ''}`);
}
async function session() {
  await until('WebDriver /status', () => call('GET', '/status', undefined, 3000).then((v) => v?.ready !== false), 30);
  const v = await call('POST', '/session', { capabilities: { alwaysMatch: {} } }, 30000);
  return v.sessionId;
}
const S = await session();
const js = (script, args = []) => call('POST', `/session/${S}/execute/sync`, { script, args });
const find = (css) => call('POST', `/session/${S}/element`, { using: 'css selector', value: css }).then((v) => Object.values(v)[0]);
async function shot(name) {
  const b64 = await call('GET', `/session/${S}/screenshot`, undefined, 30000);
  writeFileSync(join(out, `${name}.png`), Buffer.from(b64, 'base64'));
  log(`screenshot ${name}.png (${Math.round(b64.length * 0.75 / 1024)} KiB, WKWebView snapshot)`);
}
const where = () => js(`return { daemon: location.protocol === 'http:' && location.hostname === 'localhost' && location.pathname.startsWith('/ui'),
  app: location.protocol === 'tauri:', bootInHash: location.hash.startsWith('#boot='), route: location.hash.startsWith('#/') ? location.hash.slice(0, 64) : '' }`);
const goto = async (route) => { await js('location.hash = arguments[0]; return true', [route]); await sleep(300); };
const traceText = () => js(`const t = document.querySelector('.uart-trace'); return t ? t.innerText : ''`);

let ok = true;
try {
  switch (cmd) {
    case 'ready': {
      await until('Loom shell in daemon origin', async () => (await where()).daemon && await js(`return !!document.querySelector('[data-testid="loom-shell"]')`), 60);
      const w = await where();
      if (w.bootInHash) throw new Error('boot fragment still in location.hash');
      const rights = await js(`const r = document.querySelector('[data-testid="rights"]'); return r ? r.innerText : ''`);
      const vis = await js(`return { visibility: document.visibilityState, focus: document.hasFocus() }`);
      const raf = await call('POST', `/session/${S}/execute/async`, { script: `const done = arguments[arguments.length - 1]; let n = 0; const t0 = performance.now(); const f = () => { if (++n < 30) requestAnimationFrame(f); else done(Math.round(1000 * n / (performance.now() - t0))); }; requestAnimationFrame(f); setTimeout(() => done(n), 3000);`, args: [] });
      log(`PASS native WK window shows Loom shell on daemon origin (boot fragment stripped; rights="${rights.trim()}"; visibility=${vis.visibility}; focus=${vis.focus}; rAF≈${raf}/s)`);
      if (vis.visibility !== 'visible') log('WARN page hidden (occluded window) — renderer paused; results not representative');
      await shot('01-ready');
      break;
    }
    case 'rx': {
      await goto('#/p/uart/trace?channel=dut');
      await until(`trace shows ${arg}`, async () => (await traceText()).includes(arg), 20);
      const ticks = async () => Math.max(0, ...[...(await traceText()).matchAll(/tick (\d+)/g)].map((m) => Number(m[1])));
      const t1 = await ticks(); await shot('02-rx-trace-a');
      const t2 = await until('tick advances', async () => { const t = await ticks(); return t > t1 ? t : 0; }, 15);
      await shot('02-rx-trace-b');
      log(`PASS UART RX rendered live in native WK trace: '${arg}' present, tick ${t1} → ${t2}`);
      break;
    }
    case 'shows': {
      await goto('#/p/uart/trace?channel=dut');
      await until(`trace shows ${arg}`, async () => (await traceText()).includes(arg), 20);
      await shot(`03-shows-${arg}`);
      log(`PASS CLI-originated '${arg}' appears in native WK trace (shared stream)`);
      break;
    }
    case 'type': {
      await goto('#/p/uart/terminal?channel=dut');
      const el = await until('xterm textarea', () => find('.xterm-helper-textarea'), 20);
      // the plugin does not resolve W3C element references inside execute/sync args → query in-page
      const focused = await js(`const t = document.querySelector('.xterm-helper-textarea'); t.focus(); return document.activeElement === t`);
      log(`xterm textarea focused: ${focused}`);
      const text = arg + (tab ? '' : '') + ''; // Tab, Enter (W3C key codes)
      await call('POST', `/session/${S}/element/${el}/value`, { text });
      const focusKept = await js(`return !!(document.activeElement && document.activeElement.classList.contains('xterm-helper-textarea'))`);
      log(`sent '${arg}'${tab ? ' + Tab' : ''} + Enter via WebDriver keys to xterm (focus kept after keys: ${focusKept})`);
      await sleep(800); await shot(`04-typed-${arg}`);
      break;
    }
    case 'launch-failed': {
      await until('launch page Failed view', async () => (await where()).app && await js(`const f = document.getElementById('failed'); return !!f && !f.hidden`), 30);
      const msg = await js(`return document.getElementById('message').innerText`);
      if (/#boot=|Bearer|token=/i.test(msg)) throw new Error('error view leaks credential-like text');
      log(`PASS launch page shows Failed view; message: ${JSON.stringify(msg.slice(0, 200))}; Retry visible=${await js(`const b=document.getElementById('retry'); return !!b && b.offsetParent!==null`)}`);
      await shot('05-error-view');
      break;
    }
    case 'retry': {
      const b = await find('#retry');
      await call('POST', `/session/${S}/element/${b}/click`, {});
      log('clicked Retry (WebDriver element click in launch page)');
      break;
    }
    case 'open-trace-sidebar': { // open dut trace from the sidebar button (no deep link)
      await goto('#/');
      const clicked = await js(`const b = [...document.querySelectorAll('button')].find((x) => x.innerText.trim() === '트레이스'); if (!b) return false; b.click(); return true`);
      await sleep(1500);
      const t = await traceText();
      log(`sidebar 트레이스 clicked=${clicked}; trace rows text length=${t.length}; has tick=${/tick \d+/.test(t)}; empty-label=${t.includes('수신 없음')}`);
      await shot('06-trace-from-sidebar');
      break;
    }
    case 'diag': { // non-secret UI state: panels, their params, trace/terminal DOM sizes
      const d = await js(`return {
        hash: location.hash.startsWith('#boot=') ? '<boot>' : location.hash.slice(0, 80),
        panels: [...document.querySelectorAll('[data-panel], [data-panel-id], .panel-tab, [role=tab]')].slice(0, 10).map((e) => (e.getAttribute('data-panel') || e.getAttribute('data-panel-id') || '') + '|' + e.innerText.trim().slice(0, 40)),
        traceRows: document.querySelectorAll('.uart-trace [data-dir]').length,
        traceText: (document.querySelector('.uart-trace')?.innerText || '').slice(0, 120),
        xterm: !!document.querySelector('.xterm'), canvases: document.querySelectorAll('.xterm canvas').length,
        xtermRowsText: (document.querySelector('.xterm-rows')?.innerText || '').slice(0, 120),
        webgl: !!document.createElement('canvas').getContext('webgl2'),
      }`);
      log('diag ' + JSON.stringify(d));
      break;
    }
    case 'shot': await shot(arg ?? 'shot'); break;
    default: throw new Error(`unknown command ${cmd}`);
  }
} catch (e) { ok = false; log(`FAIL ${e.message}`); try { await shot(`fail-${cmd}`); } catch { /* window may be gone */ } }
finally { await call('DELETE', `/session/${S}`, undefined, 5000).catch(() => {}); }
process.exit(ok ? 0 : 1);
