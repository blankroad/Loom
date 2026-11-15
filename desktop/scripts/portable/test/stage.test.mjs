import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { dirDigest, sha256File } from '../lib.mjs';
import { checkReasons, layout, stageCargoEnv, verify } from '../stage.mjs';
import { fakeRepo, setMtime, write } from './helpers.mjs';

const TRIPLE = process.platform === 'win32' ? 'x86_64-pc-windows-msvc' : process.platform === 'darwin' ? 'aarch64-apple-darwin' : 'x86_64-unknown-linux-gnu';

function stageFake(t) {
  const L = layout(fakeRepo(t), TRIPLE);
  write(L.staged, 'fake loomd bytes');
  fs.chmodSync(L.staged, 0o755);
  fs.copyFileSync(L.distIndex, L.uiIndex);
  const stamp = { loomd_sha256: sha256File(L.staged), ui_dist_digest: dirDigest(L.dist), profile: 'release', target: TRIPLE, embedded_ui_verified: true };
  write(L.stamp, JSON.stringify(stamp, null, 2));
  return { L, stamp };
}

test('check: current staging has no reasons; each invariant is enforced', (t) => {
  const { L, stamp } = stageFake(t);
  assert.deepEqual(checkReasons(L), []);
  const mutate = (fn, re) => {
    const save = { stamp: fs.readFileSync(L.stamp), staged: fs.readFileSync(L.staged), ui: fs.readFileSync(L.uiIndex) };
    fn();
    assert.ok(checkReasons(L).some((r) => re.test(r)), `${re} in ${JSON.stringify(checkReasons(L))}`);
    fs.writeFileSync(L.stamp, save.stamp);
    fs.writeFileSync(L.staged, save.staged);
    fs.writeFileSync(L.uiIndex, save.ui);
    assert.deepEqual(checkReasons(L), []);
  };
  mutate(() => fs.appendFileSync(L.staged, 'x'), /staged binary != stamp/);
  mutate(() => fs.writeFileSync(L.stamp, JSON.stringify({ ...stamp, embedded_ui_verified: false })), /not verified/);
  mutate(() => fs.writeFileSync(L.stamp, JSON.stringify({ ...stamp, target: 'x86_64-pc-windows-gnu' })), /stamp target/);
  mutate(() => fs.writeFileSync(L.stamp, JSON.stringify({ ...stamp, ui_dist_digest: '0'.repeat(64) })), /ui\/dist changed/);
  mutate(() => fs.writeFileSync(L.uiIndex, 'other'), /UI identity/);
  mutate(() => fs.writeFileSync(L.stamp, 'not json'), /missing stamp/);
  // a UI source newer than ui/dist → stale
  const src = path.join(L.ui, 'src', 'main.ts');
  setMtime(src, Date.now() + 60_000);
  assert.ok(checkReasons(L).includes('ui/dist older than ui sources'));
  setMtime(src, Date.now() - 60_000);
  fs.rmSync(L.staged);
  assert.ok(checkReasons(L).some((r) => r.startsWith('missing ')));
});

test('stage cargo env: CARGO_TARGET_DIR removed, LOOM_STAGE_TARGET_DIR used, desktop cache rejected', () => {
  assert.equal('CARGO_TARGET_DIR' in stageCargoEnv({ CARGO_TARGET_DIR: '/ci/desktop', PATH: 'p' }), false);
  assert.equal(stageCargoEnv({ CARGO_TARGET_DIR: '/ci/desktop', LOOM_STAGE_TARGET_DIR: '/ci/root' }).CARGO_TARGET_DIR, path.resolve('/ci/root'));
  assert.throws(() => stageCargoEnv({ CARGO_TARGET_DIR: '/ci/desktop', LOOM_STAGE_TARGET_DIR: '/ci/desktop' }), /must differ/);
});

// Fake daemon: like `loomd run --port 0` — writes run/daemon.json under LOOM_HOME, serves files at /ui/.
const FAKE = fileURLToPath(new URL('./fake-loomd.mjs', import.meta.url));
const fake = (dist, mode) => [process.execPath, [FAKE, dist, mode]];

test('verify: PASS only when every file matches and CSP is present; stops its own daemon; removes temp home', async (t) => {
  const { L } = stageFake(t);
  const homes = () => fs.readdirSync(os.tmpdir()).filter((n) => n.startsWith('loom-verify-'));
  const before = new Set(homes());
  assert.equal(await verify(L, '', { spawnCmd: fake(L.dist, 'ok'), quiet: true }), true);
  assert.equal(await verify(L, '', { spawnCmd: fake(L.dist, 'tamper'), quiet: true }), false);
  assert.equal(await verify(L, '', { spawnCmd: fake(L.dist, 'nocsp'), quiet: true }), false);
  await assert.rejects(verify(L, '', { spawnCmd: fake(L.dist, 'otherpid'), quiet: true }), /discovery pid .* != spawned pid/);
  await assert.rejects(verify(L, '', { spawnCmd: fake(L.dist, 'exit'), quiet: true }), /exited early/);
  await assert.rejects(verify(L, '', { spawnCmd: fake(L.dist, 'silent'), quiet: true, startupMs: 800 }), /no run\/daemon.json/);
  const leaked = homes().filter((h) => !before.has(h));
  assert.deepEqual(leaked, [], 'temp LOOM_HOMEs removed');
});

test('verify marks the stamp only for the exact staged binary sha', async (t) => {
  const { L } = stageFake(t);
  const s = JSON.parse(fs.readFileSync(L.stamp, 'utf8'));
  fs.writeFileSync(L.stamp, JSON.stringify({ ...s, embedded_ui_verified: false }));
  // spawnCmd path has no binary sha → must not mark verified
  assert.equal(await verify(L, '', { spawnCmd: fake(L.dist, 'ok'), quiet: true }), true);
  assert.equal(JSON.parse(fs.readFileSync(L.stamp, 'utf8')).embedded_ui_verified, false);
});
