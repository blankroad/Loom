import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import {
  DEFAULT_BUNDLES, _spawnSpec, acquireLock, defaultBundles, desktopTargetDir, dirDigest, envWithout, exeSuffix,
  installedSidecarName, liveChildren, localStamp, parseBundles, platformOfTriple, shellSafeArgs, sidecarFileName,
  stopChildren, treeAlive,
} from '../lib.mjs';
import { tmpdir, write } from './helpers.mjs';

test('sidecar names follow the Tauri externalBin convention per target', () => {
  assert.equal(sidecarFileName('aarch64-apple-darwin'), 'loomd-aarch64-apple-darwin');
  assert.equal(sidecarFileName('x86_64-unknown-linux-gnu'), 'loomd-x86_64-unknown-linux-gnu');
  assert.equal(sidecarFileName('x86_64-pc-windows-msvc'), 'loomd-x86_64-pc-windows-msvc.exe');
  assert.equal(sidecarFileName('aarch64-pc-windows-msvc'), 'loomd-aarch64-pc-windows-msvc.exe');
  assert.equal(installedSidecarName('x86_64-pc-windows-msvc'), 'loomd.exe');
  assert.equal(installedSidecarName('aarch64-unknown-linux-gnu'), 'loomd');
  assert.equal(exeSuffix('x86_64-apple-darwin'), '');
  assert.throws(() => sidecarFileName('x86 64'));
  assert.throws(() => sidecarFileName('../evil'));
});

test('platform defaults are explicit and validated', () => {
  assert.equal(platformOfTriple('aarch64-apple-darwin'), 'darwin');
  assert.equal(platformOfTriple('x86_64-unknown-linux-gnu'), 'linux');
  assert.equal(platformOfTriple('x86_64-pc-windows-msvc'), 'win32');
  assert.throws(() => platformOfTriple('wasm32-unknown-unknown'));
  assert.deepEqual(defaultBundles('darwin'), ['app']);
  assert.deepEqual(defaultBundles('linux'), ['deb']);
  assert.deepEqual(defaultBundles('win32'), ['nsis']);
  assert.throws(() => defaultBundles('freebsd'));
  assert.deepEqual(parseBundles('linux', 'deb, deb'), ['deb']);
  assert.deepEqual(parseBundles('linux', ''), ['deb']);
  // only formats proven with the exact sidecar check are accepted
  for (const [plat, b] of [['linux', 'appimage'], ['linux', 'rpm'], ['darwin', 'dmg'], ['win32', 'msi']]) assert.throws(() => parseBundles(plat, b), /not valid/);
  assert.throws(() => parseBundles('linux', 'nsis'), /not valid on linux/);
  assert.throws(() => parseBundles('win32', 'app'), /not valid on win32/);
  assert.ok(Object.isFrozen(DEFAULT_BUNDLES.linux));
});

test('desktop target dir: LOOM_DESKTOP_TARGET_DIR > CARGO_TARGET_DIR > default, never inside desktop/', () => {
  const desktopDir = '/repo/desktop';
  const base = { platform: 'linux', tmpdir: '/var/tmp', desktopDir };
  assert.equal(desktopTargetDir({ ...base, env: {} }), '/tmp/loom-tauri-build');
  assert.equal(desktopTargetDir({ ...base, platform: 'darwin', env: {} }), '/tmp/loom-tauri-build');
  assert.equal(desktopTargetDir({ ...base, env: { CARGO_TARGET_DIR: '/ci/t' } }), '/ci/t');
  assert.equal(desktopTargetDir({ ...base, env: { CARGO_TARGET_DIR: '/ci/t', LOOM_DESKTOP_TARGET_DIR: '/x/y' } }), '/x/y');
  assert.equal(desktopTargetDir({ ...base, env: { CARGO_TARGET_DIR: '  ' } }), '/tmp/loom-tauri-build');
  assert.throws(() => desktopTargetDir({ ...base, env: { CARGO_TARGET_DIR: '/repo/desktop/target' } }), /inside/);
  assert.throws(() => desktopTargetDir({ ...base, env: { CARGO_TARGET_DIR: '/repo/desktop' } }), /inside/);
  assert.equal(desktopTargetDir({ ...base, env: { CARGO_TARGET_DIR: '/repo/desktop-target' } }), '/repo/desktop-target');
  const win = desktopTargetDir({ platform: 'win32', tmpdir: 'C:\\Users\\u\\AppData\\Local\\Temp', desktopDir: 'C:\\src\\Loom\\desktop', env: {} });
  assert.equal(win, 'C:\\Users\\u\\AppData\\Local\\Temp\\loom-tauri-build');
  assert.throws(() => desktopTargetDir({ platform: 'win32', tmpdir: 'C:\\t', desktopDir: 'C:\\src\\Loom\\desktop', env: { CARGO_TARGET_DIR: 'C:\\src\\Loom\\desktop\\target' } }), /inside/);
});

test('cmd.exe routing only for metacharacter-free args; no shell elsewhere', () => {
  assert.deepEqual(_spawnSpec('pnpm', ['build'], { viaShellOnWindows: true, platform: 'win32' }), { file: 'pnpm build', args: [], shell: true });
  assert.deepEqual(_spawnSpec('pnpm', ['build'], { viaShellOnWindows: true, platform: 'linux' }), { file: 'pnpm', args: ['build'], shell: false });
  assert.deepEqual(_spawnSpec('cargo', ['build', 'a b'], { platform: 'win32' }), { file: 'cargo', args: ['build', 'a b'], shell: false });
  for (const bad of ['a b', 'x&y', 'x|y', '"q"', 'C:\\p', '%PATH%', '^', 'a>b']) assert.throws(() => shellSafeArgs([bad]), /refusing/);
  assert.doesNotThrow(() => shellSafeArgs(['install', '--frozen-lockfile', 'build']));
});

test('envWithout removes keys (case-insensitively on Windows)', () => {
  assert.deepEqual(envWithout(['CARGO_TARGET_DIR'], { CARGO_TARGET_DIR: 'x', A: '1' }, 'linux'), { A: '1' });
  assert.deepEqual(envWithout(['CARGO_TARGET_DIR'], { cargo_target_dir: 'x', A: '1' }, 'linux'), { cargo_target_dir: 'x', A: '1' });
  assert.deepEqual(envWithout(['CARGO_TARGET_DIR'], { Cargo_Target_Dir: 'x', A: '1' }, 'win32'), { A: '1' });
});

test('dirDigest is byte-compatible with the stage-loomd.sh shasum pipeline', (t) => {
  const d = tmpdir(t);
  // ordering traps: '-' (0x2d) < '/' (0x2f) < '0' < 'C' < '_' < 'a' (no case-only pairs: case-insensitive FS)
  for (const f of ['a-b', 'a/b', 'a0', 'Cap', '_x', 'assets/index-B.js', 'assets/index-a.css', 'index.html', 'ü.txt']) write(path.join(d, f), `content of ${f}\n`);
  const got = dirDigest(d);
  assert.match(got, /^[0-9a-f]{64}$/);
  const shasum = spawnSync('shasum', ['--version'], { stdio: 'ignore' });
  if (process.platform === 'win32' || shasum.error) {
    t.diagnostic('shasum not available: shell cross-check skipped');
    return;
  }
  const sh = spawnSync('bash', ['-c', 'cd "$1" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -c1-64', '_', d], { encoding: 'utf8' });
  assert.equal(sh.status, 0, sh.stderr);
  assert.equal(got, sh.stdout.trim());
  write(path.join(d, 'a0'), 'changed\n');
  assert.notEqual(dirDigest(d), got);
});

test('mkdir lock: exclusive, reports owner on timeout, releasable once', async (t) => {
  const lock = path.join(tmpdir(t), 'target', '.cargo-desktop.lock');
  const release = await acquireLock(lock, { waitMs: 0, owner: 'test-a' });
  assert.match(fs.readFileSync(path.join(lock, 'owner'), 'utf8'), /^test-a /);
  await assert.rejects(acquireLock(lock, { waitMs: 300, pollMs: 50 }), /held by test-a/);
  release();
  release();
  assert.equal(fs.existsSync(lock), false);
  const again = await acquireLock(lock, { waitMs: 0 });
  again();
});

test('localStamp matches stat -f %Sm -t %Y-%m-%dT%H:%M:%S shape', () => {
  assert.equal(localStamp(new Date(2026, 0, 2, 3, 4, 5)), '2026-01-02T03:04:05');
});

test('stopChildren forwards to the whole POSIX process group and confirms it is gone', { skip: process.platform === 'win32' && 'POSIX process groups' }, async () => {
  // parent node process spawns a long-lived grandchild (like tauri → cargo) in the same group
  const code = "const {spawn}=require('node:child_process');spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});setInterval(()=>{},1000)";
  const child = spawn(process.execPath, ['-e', code], { detached: true, stdio: 'ignore' });
  child.loomGroup = true;
  liveChildren().add(child);
  await new Promise((r) => setTimeout(r, 500));
  assert.equal(treeAlive(child), true);
  const alive = await stopChildren('SIGTERM', 5000);
  liveChildren().delete(child);
  assert.deepEqual(alive, []);
  assert.equal(treeAlive(child), false, 'grandchild must be gone too');
});
