// stage / check / verify: the portable equivalent of stage-loomd.sh + verify-loomd.sh.
// Same files and stamp schema, so the mac shell scripts and this path accept each other's staging.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {
  DESKTOP_DIR, REPO_DIR, STALE_EXIT, Fail, dirDigest, listFiles, envWithout, isExecutableFile, liveChildren, localStamp, log,
  mkTemp, probe, readJson, replaceFile, rmrf, run, runOk, sha256File, sidecarFileName, sleep, utcStamp, writeFileAtomic,
  desktopTargetDir,
} from './lib.mjs';

const TAG = 'stage';

export function layout(repo = REPO_DIR, triple) {
  const binaries = path.join(repo, 'desktop', 'src-tauri', 'binaries');
  const ui = path.join(repo, 'ui');
  return {
    repo,
    triple,
    ui,
    dist: path.join(ui, 'dist'),
    distIndex: path.join(ui, 'dist', 'index.html'),
    binaries,
    staged: path.join(binaries, sidecarFileName(triple)),
    stamp: path.join(binaries, 'loomd.stamp.json'),
    uiIndex: path.join(binaries, 'loomd.ui-index.html'),
  };
}

/** Files whose change requires a UI rebuild (same list as stage-loomd.sh `ui_inputs`). */
export function uiInputs(ui) {
  const roots = ['src', 'public', 'index.html', 'package.json', 'pnpm-lock.yaml', 'vite.config.ts', 'tsconfig.json', 'tsconfig.app.json', 'tsconfig.node.json'];
  const out = [];
  const walk = (p) => {
    let st;
    try {
      st = fs.statSync(p);
    } catch {
      return;
    }
    if (st.isDirectory()) for (const e of fs.readdirSync(p)) walk(path.join(p, e));
    else if (st.isFile()) out.push({ p, mtimeMs: st.mtimeMs });
  };
  for (const r of roots) walk(path.join(ui, r));
  return out;
}
export function uiStale(L) {
  let idx;
  try {
    idx = fs.statSync(L.distIndex).mtimeMs;
  } catch {
    return true;
  }
  return uiInputs(L.ui).some((f) => f.mtimeMs > idx);
}

/** Reasons the staged sidecar is not current; [] = current. Stricter than the shell only in `target`. */
export function checkReasons(L) {
  const reasons = [];
  if (uiStale(L)) reasons.push('ui/dist older than ui sources');
  const exe = isExecutableFile(L.staged);
  if (!exe) reasons.push(`missing ${L.staged}`);
  const s = readJson(L.stamp);
  if (exe && s) {
    if (sha256File(L.staged) !== s.loomd_sha256) reasons.push('staged binary != stamp');
    if (!fs.existsSync(L.distIndex) || dirDigest(L.dist) !== s.ui_dist_digest) reasons.push('ui/dist changed since stage');
    if (s.embedded_ui_verified !== true) reasons.push('embedded UI not verified (run verify)');
    if (s.target !== L.triple) reasons.push(`stamp target ${s.target} != host ${L.triple}`);
    let same = false;
    try {
      same = fs.readFileSync(L.uiIndex).equals(fs.readFileSync(L.distIndex));
    } catch {
      /* missing → not same */
    }
    if (!same) reasons.push('app UI identity (loomd.ui-index.html) != ui/dist/index.html');
  } else {
    reasons.push('missing stamp');
  }
  return reasons;
}

export function check(L) {
  const reasons = checkReasons(L);
  if (!reasons.length) {
    log(TAG, `current (${L.staged})`);
    return 0;
  }
  for (const r of reasons) log(TAG, `stale: ${r}`);
  return STALE_EXIT;
}

/**
 * Main-workspace cargo target for the loomd build. Never the desktop cache, never inherited from
 * CARGO_TARGET_DIR (that names the desktop target in CI). LOOM_STAGE_TARGET_DIR overrides it for
 * containers that must not reuse a host-built <repo>/target.
 */
export function stageCargoEnv(env = process.env) {
  const e = envWithout(['CARGO_TARGET_DIR'], env);
  const t = env.LOOM_STAGE_TARGET_DIR && String(env.LOOM_STAGE_TARGET_DIR).trim();
  if (t) {
    const abs = path.resolve(t);
    if (abs === desktopTargetDir({ env })) throw new Fail('LOOM_STAGE_TARGET_DIR must differ from the desktop target dir');
    e.CARGO_TARGET_DIR = abs;
  }
  return e;
}

async function buildUi(L, { reuseUi }) {
  if (reuseUi && !uiStale(L)) {
    log(TAG, 'ui/dist is current and --reuse-ui given; not rebuilding');
    return;
  }
  // Paths go in cwd; the only args on a cmd.exe line (Windows .cmd shim) are fixed literals.
  if (!fs.existsSync(path.join(L.ui, 'node_modules'))) {
    log(TAG, 'pnpm install --frozen-lockfile (ui/node_modules missing)');
    await runOk('pnpm', ['install', '--frozen-lockfile'], { cwd: L.ui, viaShellOnWindows: true });
  }
  log(TAG, 'building ui/dist (pnpm build)');
  await runOk('pnpm', ['build'], { cwd: L.ui, viaShellOnWindows: true });
  if (!fs.existsSync(L.distIndex)) throw new Fail('ui/dist/index.html missing after pnpm build');
}

async function buildLoomd(L, profile, digest) {
  const env = stageCargoEnv();
  const prof = profile === 'release' ? ['--release'] : [];
  // rust-embed misses files added/removed in ui/dist (hashed asset names): force re-embedding.
  log(TAG, `cleaning loom-ui-assets (${profile}) to force re-embedding ui/dist ${digest}`);
  await runOk('cargo', ['clean', '--quiet', '-p', 'loom-ui-assets', ...prof], { cwd: L.repo, env });
  log(TAG, `cargo build --locked ${prof.join(' ')} -p loom-gateway --bin loomd`);
  await runOk('cargo', ['build', '--locked', ...prof, '-p', 'loom-gateway', '--bin', 'loomd'], { cwd: L.repo, env });
  const meta = await runOk('cargo', ['metadata', '--format-version', '1', '--no-deps', '--locked'], { cwd: L.repo, env, capture: true });
  const targetDir = JSON.parse(meta.stdout).target_directory;
  const built = path.join(targetDir, profile, `loomd${path.extname(L.staged)}`);
  if (!isExecutableFile(built)) throw new Fail(`${built} not built`);
  return built;
}

export async function stage(L, { profile = 'release', from = '', reuseUi = false, skipVerify = false } = {}) {
  if (!['release', 'debug'].includes(profile)) throw new Fail('profile must be release|debug', 2);
  let built;
  let digest;
  let label = profile;
  if (from) {
    built = path.resolve(from);
    if (!isExecutableFile(built)) throw new Fail(`--from ${from} is not an executable file`, 2);
    label = 'prebuilt';
    if (!fs.existsSync(L.distIndex)) throw new Fail('ui/dist missing; cannot record UI identity');
    if (uiStale(L)) log(TAG, 'WARNING ui sources newer than ui/dist (the prebuilt binary may embed an older UI)');
    digest = dirDigest(L.dist);
  } else {
    await buildUi(L, { reuseUi });
    digest = dirDigest(L.dist);
    built = await buildLoomd(L, profile, digest);
  }

  fs.mkdirSync(L.binaries, { recursive: true });
  replaceFile(built, L.staged, 0o755);
  // UI build identity read by src-tauri/build.rs; verify proves the daemon serves exactly these bytes.
  replaceFile(L.distIndex, L.uiIndex);
  const sha = sha256File(L.staged);
  const version = (probe(L.staged, ['--version']).split('\n')[0] || '').trim();
  const baseline = probe('git', ['-C', L.repo, 'rev-parse', '--short', 'HEAD']) || 'unknown';
  const stamp = {
    loomd_sha256: sha,
    ui_dist_digest: digest,
    profile: label,
    target: L.triple,
    loomd_version: version,
    baseline,
    source_path: built,
    source_mtime: localStamp(fs.statSync(built).mtime),
    ui_dist_vs_sources: uiStale(L) ? 'stale_vs_sources' : 'current',
    embedded_ui_verified: false,
    staged_at: utcStamp(),
    staged_by: 'desktop/scripts/package.mjs',
  };
  writeFileAtomic(L.stamp, `${JSON.stringify(stamp, null, 2)}\n`);
  log(TAG, `staged ${L.staged} (sha256 ${sha}, ui ${digest})`);
  if (!skipVerify) {
    const ok = await verify(L, L.staged);
    if (!ok) throw new Fail('staged binary FAILED embedded-UI identity check');
  }
  return stamp;
}

function exited(child) {
  return child.exitCode !== null || child.signalCode !== null;
}
async function waitExit(child, ms) {
  const end = Date.now() + ms;
  while (!exited(child) && Date.now() < end) await sleep(100);
  return exited(child);
}

/**
 * Prove a loomd binary serves exactly ui/dist: run it under a fresh private LOOM_HOME on port 0,
 * byte-compare every file at /ui/<path>, check the CSP header, then stop ONLY the daemon this call
 * started (its own pid only). `spawnCmd` lets tests run a fake daemon.
 */
export async function verify(L, bin, { spawnCmd, startupMs = 15_000, quiet = false } = {}) {
  const say = (m) => !quiet && log('verify', m);
  if (!isExecutableFile(bin) && !spawnCmd) throw new Fail(`not executable: ${bin}`, 2);
  if (!fs.existsSync(L.distIndex)) throw new Fail(`${L.distIndex} missing`, 2);
  const home = mkTemp('loom-verify-');
  const logFile = path.join(home, 'loomd.out');
  const out = fs.openSync(logFile, 'w');
  const [cmd, args] = spawnCmd ? spawnCmd : [bin, ['run', '--port', '0']];
  const child = spawn(cmd, args, { env: { ...process.env, LOOM_HOME: home }, stdio: ['ignore', out, out], windowsHide: true });
  liveChildren().add(child);
  let spawnErr = null;
  child.on('error', (e) => (spawnErr = e));
  let info = null;
  let stopHow = 'not-started';
  try {
    const disc = path.join(home, 'run', 'daemon.json');
    const end = Date.now() + startupMs;
    while (Date.now() < end) {
      info = readJson(disc);
      if (info?.port) break;
      if (spawnErr) throw new Fail(`cannot start ${cmd}: ${spawnErr.message}`);
      if (exited(child)) throw new Fail(`loomd exited early:\n${fs.readFileSync(logFile, 'utf8').split('\n').slice(-20).join('\n')}`);
      await sleep(100);
    }
    if (!info?.port) throw new Fail(`no run/daemon.json within ${startupMs / 1000}s`);
    if (info.pid !== child.pid) throw new Fail(`discovery pid ${info.pid} != spawned pid ${child.pid}: not proof that this binary serves /ui`);

    const files = listFiles(L.dist);
    let bad = 0;
    for (const rel of files) {
      const url = `http://localhost:${info.port}/ui/${rel.split('/').map(encodeURIComponent).join('/')}`;
      let ok = false;
      try {
        const r = await fetch(url, { signal: AbortSignal.timeout(5000), redirect: 'error' });
        ok = r.ok && Buffer.from(await r.arrayBuffer()).equals(fs.readFileSync(path.join(L.dist, rel)));
      } catch {
        ok = false;
      }
      if (!ok && ++bad <= 5) say(`MISMATCH /ui/${rel}`);
    }
    let csp = false;
    try {
      const r = await fetch(`http://localhost:${info.port}/ui/`, { signal: AbortSignal.timeout(5000), redirect: 'error' });
      csp = /^default-src 'self'/i.test(r.headers.get('content-security-policy') || '');
      await r.arrayBuffer();
    } catch {
      csp = false;
    }
    const sha = spawnCmd ? '' : sha256File(bin);
    if (sha) say(`binary ${bin} sha256=${sha}`);
    say(`/ui files compared=${files.length} mismatched=${bad} csp=${csp ? 'yes' : 'no'} port=${info.port}`);
    const pass = bad === 0 && files.length > 0 && csp;
    if (pass && sha) {
      const s = readJson(L.stamp);
      if (s && s.loomd_sha256 === sha) {
        Object.assign(s, { embedded_ui_verified: true, embedded_ui_files: files.length, embedded_ui_verified_at: utcStamp() });
        writeFileAtomic(L.stamp, `${JSON.stringify(s, null, 2)}\n`);
        say('stamp updated (embedded_ui_verified=true)');
      }
    }
    say(pass ? 'PASS' : 'FAIL (binary does not embed current ui/dist)');
    return pass;
  } finally {
    if (!exited(child) && child.pid) {
      // Only the pid we spawned (static-asset check opens no channels/workers); bounded wait.
      child.kill(process.platform === 'win32' ? undefined : 'SIGTERM');
      if (await waitExit(child, 5000)) stopHow = 'terminate-own-pid';
      else {
        child.kill('SIGKILL');
        await waitExit(child, 2000);
        stopHow = 'kill-own-pid';
      }
    } else if (child.pid) stopHow = 'already-exited';
    liveChildren().delete(child);
    fs.closeSync(out);
    say(`stopped spawned daemon (${stopHow})`);
    try {
      rmrf(home);
    } catch (e) {
      say(`WARNING could not remove ${home}: ${e.message}`);
    }
  }
}

export { DESKTOP_DIR, run };
