// Contract between the scripts, the Tauri configs, the runtime's sidecar lookup and npm scripts.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { DEFAULT_BUNDLES, DESKTOP_DIR } from '../lib.mjs';
import { parseArgs } from '../../package.mjs';

const tauriDir = path.join(DESKTOP_DIR, 'src-tauri');
const json = (p) => JSON.parse(fs.readFileSync(path.join(tauriDir, p), 'utf8'));
const merged = (plat) => {
  const base = json('tauri.conf.json');
  const extra = plat === 'darwin' ? {} : json(plat === 'linux' ? 'tauri.linux.conf.json' : 'tauri.windows.conf.json');
  return { ...base, bundle: { ...base.bundle, ...(extra.bundle || {}) } }; // JSON merge patch: arrays replace
};

test('default bundles match the per-platform Tauri configs', () => {
  for (const plat of ['darwin', 'linux', 'win32']) assert.deepEqual(merged(plat).bundle.targets, [...DEFAULT_BUNDLES[plat]], plat);
});

test('externalBin stays binaries/loomd (runtime resolves <exe dir>/loomd[.exe])', () => {
  for (const plat of ['darwin', 'linux', 'win32']) assert.deepEqual(merged(plat).bundle.externalBin, ['binaries/loomd']);
  const cfg = fs.readFileSync(path.join(tauriDir, 'src', 'config.rs'), 'utf8');
  assert.match(cfg, /"loomd"/);
  assert.match(cfg, /EXE_SUFFIX|\.exe/, 'runtime must add .exe on Windows');
});

test('every configured icon exists; Windows has a real .ico (tauri-build resource)', () => {
  for (const plat of ['darwin', 'linux', 'win32']) {
    for (const i of merged(plat).bundle.icon) assert.ok(fs.existsSync(path.join(tauriDir, i)), `${plat}: ${i}`);
  }
  const win = merged('win32').bundle.icon.find((i) => i.endsWith('.ico'));
  assert.equal(win, 'icons/icon.ico');
  const ico = fs.readFileSync(path.join(tauriDir, win));
  assert.deepEqual([...ico.subarray(0, 4)], [0, 0, 1, 0], 'ICO header');
  assert.ok(ico.readUInt16LE(4) >= 4, 'multiple sizes');
});

test('npm scripts are portable (no POSIX-only shell syntax) and route through package.mjs', () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(DESKTOP_DIR, 'package.json'), 'utf8'));
  for (const name of ['build:app', 'build:app:debug', 'dist', 'stage', 'check', 'test']) assert.ok(pkg.scripts[name], name);
  for (const [name, cmd] of Object.entries(pkg.scripts)) {
    assert.doesNotMatch(cmd, /\$\{|\$[A-Z_]|\brm\b|\bditto\b|\bcodesign\b|\bmkdir\b|\bcp\b|\b[A-Z_]+=\S/, `${name}: ${cmd}`);
  }
  for (const name of ['build:app', 'build:app:debug', 'dist', 'stage', 'check']) assert.match(pkg.scripts[name], /^node scripts\/package\.mjs /);
  assert.equal(pkg.engines.node, '>=24');
  const lock = JSON.parse(fs.readFileSync(path.join(DESKTOP_DIR, 'package-lock.json'), 'utf8'));
  assert.equal(lock.packages[''].devDependencies['@tauri-apps/cli'], pkg.devDependencies['@tauri-apps/cli']);
  for (const p of ['darwin-arm64', 'darwin-x64', 'linux-x64-gnu', 'linux-arm64-gnu', 'win32-x64-msvc']) {
    assert.ok(lock.packages[`node_modules/@tauri-apps/cli-${p}`], `lock has native cli for ${p}`);
  }
});

test('CLI argument parsing', () => {
  assert.deepEqual(parseArgs(['dist', '--debug', '--bundles', 'deb', '--no-strict']), { cmd: 'dist', positional: [], debug: true, bundles: 'deb', strict: false });
  assert.equal(parseArgs(['--help']).cmd, 'help');
  assert.equal(parseArgs(['verify', 'x']).positional[0], 'x');
  assert.throws(() => parseArgs(['stage', '--profile']), /needs a value/);
  assert.throws(() => parseArgs(['stage', '--frobnicate']), /unknown option/);
});
