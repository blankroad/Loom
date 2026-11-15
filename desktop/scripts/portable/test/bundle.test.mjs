import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { BUNDLE_GLOBS, collectArtifacts, isStrict, tauriCli } from '../bundle.mjs';
import { setMtime, tmpdir, write } from './helpers.mjs';

test('collectArtifacts picks exactly the artifact this build produced', (t) => {
  const b = tmpdir(t);
  const since = Date.now() - 1000;
  const old = Date.now() - 3_600_000;
  setMtime(write(path.join(b, 'deb', 'Loom_0.0.9_amd64.deb'), 'old'), old);
  write(path.join(b, 'deb', 'Loom_0.1.0_amd64.deb'), 'new');
  write(path.join(b, 'appimage', 'Loom_0.1.0_amd64.AppImage'), 'new');
  write(path.join(b, 'appimage', 'build_appimage.sh'), 'tool'); // not an artifact
  write(path.join(b, 'nsis', 'Loom_0.1.0_x64-setup.exe'), 'new');
  write(path.join(b, 'macos', 'Loom.app', 'Contents', 'Info.plist'), '<plist/>');
  const got = collectArtifacts(b, ['deb', 'appimage', 'nsis', 'app'], since);
  assert.deepEqual(got.map((a) => [a.kind, path.basename(a.src)]), [
    ['deb', 'Loom_0.1.0_amd64.deb'], ['appimage', 'Loom_0.1.0_amd64.AppImage'], ['nsis', 'Loom_0.1.0_x64-setup.exe'], ['app', 'Loom.app'],
  ]);
  write(path.join(b, 'deb', 'Loom_0.1.1_amd64.deb'), 'second fresh');
  assert.throws(() => collectArtifacts(b, ['deb'], since), /exactly 1 .* found 2/);
  assert.throws(() => collectArtifacts(b, ['msi'], since), /found 0/);
  assert.throws(() => collectArtifacts(b, ['zip'], since), /unknown bundle kind/);
  // a stale-only directory fails instead of shipping an old installer
  const c = tmpdir(t);
  setMtime(write(path.join(c, 'nsis', 'Loom_0.1.0_x64-setup.exe'), 'old'), old);
  assert.throws(() => collectArtifacts(c, ['nsis'], since), /found 0 \(stale\/other: 1\)/);
});

test('bundle globs cover every default bundle kind', () => {
  for (const k of ['app', 'deb', 'appimage', 'nsis']) assert.ok(BUNDLE_GLOBS[k], k);
  assert.ok(BUNDLE_GLOBS.nsis.test('Loom_0.1.0_x64-setup.exe'));
  assert.ok(!BUNDLE_GLOBS.nsis.test('loom-desktop.exe'));
});

test('strict (WARN = not verified = failure) defaults on in CI', () => {
  assert.equal(isStrict({}, { CI: 'true' }), true);
  assert.equal(isStrict({}, { CI: '1' }), true);
  assert.equal(isStrict({}, {}), false);
  assert.equal(isStrict({ strict: false }, { CI: 'true' }), false);
  assert.equal(isStrict({ strict: true }, {}), true);
});

test('tauri CLI must be the pinned devDependency, with an actionable message', (t) => {
  const d = tmpdir(t);
  write(path.join(d, 'package.json'), JSON.stringify({ devDependencies: { '@tauri-apps/cli': '2.12.4' } }));
  assert.throws(() => tauriCli(d), /pinned @tauri-apps\/cli 2\.12\.4 not installed .*npm ci --prefix desktop/);
  write(path.join(d, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'), "console.log('tauri-cli 2.0.0')");
  assert.throws(() => tauriCli(d), /have 'tauri-cli 2\.0\.0'/);
  fs.writeFileSync(path.join(d, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'), "console.log('tauri-cli 2.12.4')");
  assert.equal(tauriCli(d).version, 'tauri-cli 2.12.4');
});

test('manifest paths: repo-relative inside the repo, absolute outside', async () => {
  const { manifestPath_ } = await import('../bundle.mjs');
  const repo = path.resolve('/r/Loom');
  assert.equal(manifestPath_(path.join(repo, 'desktop', 'dist', 'Loom_0.1.0_amd64.deb'), repo), 'desktop/dist/Loom_0.1.0_amd64.deb');
  const out = path.resolve('/elsewhere/out/x.AppImage');
  assert.equal(manifestPath_(out, repo), out.split(path.sep).join('/'));
});
