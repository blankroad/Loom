// bundle / dist: Tauri build of the verified staged sidecar, per-platform bundle checks, manifest.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import {
  DESKTOP_DIR, REPO_DIR, Fail, acquireLock, desktopTargetDir, hasCommand, installedSidecarName, listFiles, log,
  mkTemp, platformOfTriple, probe, readJson, rmrf, run, runOk, sha256File, stopChildren, utcStamp, writeFileAtomic,
} from './lib.mjs';
import { checkReasons } from './stage.mjs';

const TAG = 'bundle';

/** Where Tauri 2 writes each bundle kind under <target>/<profile>/bundle (official layout). */
export const BUNDLE_GLOBS = Object.freeze({
  app: { dir: 'macos', test: (n) => n.endsWith('.app') },
  dmg: { dir: 'dmg', test: (n) => n.endsWith('.dmg') },
  deb: { dir: 'deb', test: (n) => n.endsWith('.deb') },
  rpm: { dir: 'rpm', test: (n) => n.endsWith('.rpm') },
  appimage: { dir: 'appimage', test: (n) => n.endsWith('.AppImage') },
  nsis: { dir: 'nsis', test: (n) => n.endsWith('-setup.exe') },
  msi: { dir: 'msi', test: (n) => n.endsWith('.msi') },
});

function freshness(p) {
  const st = fs.statSync(p);
  let m = st.mtimeMs;
  if (st.isDirectory()) {
    for (const f of ['Contents/Info.plist', 'Contents/MacOS']) {
      try {
        m = Math.max(m, fs.statSync(path.join(p, f)).mtimeMs);
      } catch {
        /* not an .app */
      }
    }
  }
  return m;
}

/** Exactly one artifact per requested kind, produced by this build (mtime >= sinceMs). */
export function collectArtifacts(bundleDir, kinds, sinceMs) {
  return kinds.map((kind) => {
    const g = BUNDLE_GLOBS[kind];
    if (!g) throw new Fail(`unknown bundle kind ${kind}`);
    const dir = path.join(bundleDir, g.dir);
    const all = fs.existsSync(dir) ? fs.readdirSync(dir).filter(g.test).map((n) => path.join(dir, n)) : [];
    const fresh = all.filter((p) => freshness(p) >= sinceMs);
    if (fresh.length !== 1) {
      throw new Fail(`${kind}: expected exactly 1 artifact from this build in ${dir}, found ${fresh.length} (stale/other: ${all.length - fresh.length})`);
    }
    return { kind, src: fresh[0] };
  });
}

export function tauriCli(desktop = DESKTOP_DIR) {
  const pkg = readJson(path.join(desktop, 'package.json'));
  const want = pkg?.devDependencies?.['@tauri-apps/cli'];
  const js = path.join(desktop, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
  const got = fs.existsSync(js) ? probe(process.execPath, [js, '--version']) : '';
  if (got !== `tauri-cli ${want}`) {
    throw new Fail(`pinned @tauri-apps/cli ${want} not installed (have '${got || 'none'}'); run: npm ci --prefix desktop`);
  }
  return { js, version: got };
}

class Checks {
  rows = [];
  add(check, result, detail) {
    this.rows.push({ check, result, detail: String(detail) });
    log(TAG, `${check.padEnd(20)} ${result.padEnd(4)} ${detail}`);
  }
  has(r) {
    return this.rows.some((x) => x.result === r);
  }
}

function findFile(root, name) {
  return listFiles(root).filter((r) => path.basename(r) === name).map((r) => path.join(root, r));
}

function sidecarRow(C, label, found, stagedSha) {
  if (found.length !== 1) return C.add(label, 'FAIL', `expected one sidecar, found ${found.length}`);
  const sha = sha256File(found[0]);
  C.add(label, sha === stagedSha ? 'PASS' : 'FAIL', `${sha === stagedSha ? 'sha256 == staged' : `sha256 ${sha} != staged ${stagedSha}`}`);
}

/** Inspect a produced artifact: the sidecar must be inside it, next to the app binary. */
async function inspect(C, a, ctx) {
  const { stagedSha, triple } = ctx;
  const side = installedSidecarName(triple);
  if (a.kind === 'app') {
    const r = await run('bash', [path.join(ctx.desktop, 'scripts', 'verify-bundle.sh'), a.src]);
    C.add('app:verify-bundle', r.code === 0 ? 'PASS' : 'FAIL', `verify-bundle.sh (identity across ad-hoc re-sign, embedded UI) exit ${r.code}`);
    return;
  }
  const tmp = mkTemp(`loom-inspect-${a.kind}-`);
  try {
    if (a.kind === 'deb') {
      if (!hasCommand('dpkg-deb', ['--version'])) return C.add('deb:contents', 'WARN', 'dpkg-deb not available; deb contents NOT inspected');
      await runOk('dpkg-deb', ['-x', a.src, tmp]);
      const bin = path.join(tmp, 'usr', 'bin');
      const others = fs.existsSync(bin) ? fs.readdirSync(bin).filter((n) => n !== side) : [];
      sidecarRow(C, 'deb:sidecar', fs.existsSync(path.join(bin, side)) ? [path.join(bin, side)] : [], stagedSha);
      C.add('deb:app-binary', others.length ? 'PASS' : 'FAIL', `usr/bin: ${others.join(', ') || 'none'} next to ${side}`);
      const f = probe('dpkg-deb', ['-f', a.src, 'Package', 'Version', 'Architecture', 'Depends']);
      C.add('deb:control', 'INFO', f.replace(/\n/g, '; '));
    } else if (a.kind === 'appimage') {
      fs.chmodSync(a.src, 0o755);
      const r = await run(a.src, ['--appimage-extract'], { cwd: tmp, capture: true });
      const root = path.join(tmp, 'squashfs-root');
      if (r.code !== 0 || !fs.existsSync(root)) return C.add('appimage:contents', 'WARN', `--appimage-extract failed (exit ${r.code}); AppImage contents NOT inspected`);
      const p = path.join(root, 'usr', 'bin', side);
      sidecarRow(C, 'appimage:sidecar', fs.existsSync(p) ? [p] : [], stagedSha);
      const others = fs.readdirSync(path.join(root, 'usr', 'bin')).filter((n) => n !== side);
      C.add('appimage:app-binary', others.length ? 'PASS' : 'FAIL', `usr/bin: ${others.join(', ') || 'none'}`);
    } else if (a.kind === 'nsis' || a.kind === 'msi') {
      const z = ['7z', '7z.exe', 'C:\\Program Files\\7-Zip\\7z.exe'].find((c) => hasCommand(c, ['i']));
      if (!z || a.kind === 'msi') return C.add(`${a.kind}:contents`, 'WARN', `${z ? 'msi' : '7z'} inspection not available; installer contents NOT inspected`);
      const r = await run(z, ['x', '-y', `-o${tmp}`, a.src], { capture: true });
      if (r.code !== 0) return C.add('nsis:contents', 'WARN', `7z could not extract (exit ${r.code}); installer contents NOT inspected`);
      const found = findFile(tmp, side);
      sidecarRow(C, 'nsis:sidecar', found, stagedSha);
      const dir = found.length === 1 ? path.dirname(found[0]) : '';
      const others = dir ? fs.readdirSync(dir).filter((n) => n.endsWith('.exe') && n !== side && !/uninstall/i.test(n)) : [];
      C.add('nsis:app-binary', others.length ? 'PASS' : 'FAIL', `next to ${side}: ${others.join(', ') || 'none'}`);
    } else {
      C.add(`${a.kind}:contents`, 'WARN', 'no inspector for this kind');
    }
  } finally {
    rmrf(tmp);
  }
}

function copyOut(a, outDir) {
  const dest = path.join(outDir, path.basename(a.src));
  rmrf(dest);
  if (fs.statSync(a.src).isDirectory()) fs.cpSync(a.src, dest, { recursive: true, verbatimSymlinks: true, preserveTimestamps: true });
  else fs.copyFileSync(a.src, dest);
  return dest;
}
function artifactHash(p) {
  if (!fs.statSync(p).isDirectory()) return { sha256: sha256File(p), bytes: fs.statSync(p).size, sha256_kind: 'file' };
  // Directory bundle (.app): sha256 of "<sha256>  <rel>\n" lines in byte order — content identity of every file.
  const h = createHash('sha256');
  let bytes = 0;
  for (const rel of listFiles(p).sort((x, y) => Buffer.compare(Buffer.from(x), Buffer.from(y)))) {
    const f = path.join(p, rel);
    bytes += fs.statSync(f).size;
    h.update(`${sha256File(f)}  ${rel}\n`);
  }
  return { sha256: h.digest('hex'), bytes, sha256_kind: 'tree' };
}

/** Repo-relative '/'-path when inside the repo, else the absolute path ('/'-separated). */
export function manifestPath_(dest, repo = REPO_DIR) {
  const rel = path.relative(repo, dest);
  const inside = rel && !rel.startsWith('..') && !path.isAbsolute(rel);
  return (inside ? rel : dest).split(path.sep).join('/');
}

export function isStrict(opts, env = process.env) {
  if (opts.strict !== undefined) return opts.strict;
  return /^(1|true|yes)$/i.test(String(env.CI || ''));
}

export async function bundle(L, opts = {}) {
  const { debug = false, bundles, outDir = path.join(DESKTOP_DIR, 'dist') } = opts;
  const strict = isStrict(opts);
  const platform = platformOfTriple(L.triple);
  if (platform !== process.platform) throw new Fail(`host triple ${L.triple} does not match node platform ${process.platform}`);
  const reasons = checkReasons(L);
  if (reasons.length) throw new Fail(`staged loomd is not current+verified (${reasons.join('; ')}); run: node desktop/scripts/package.mjs stage`);
  const stamp = readJson(L.stamp);
  const stagedSha = stamp.loomd_sha256;
  const cli = tauriCli();
  log(TAG, `${cli.version}; bundles ${bundles.join(',')} (${debug ? 'debug' : 'release'})`);

  const targetDir = desktopTargetDir();
  const lockfile = path.join(DESKTOP_DIR, 'Cargo.lock');
  if (!fs.existsSync(lockfile)) throw new Fail('desktop/Cargo.lock missing; refusing to resolve');
  const lockDir = path.join(targetDir, '.cargo-desktop.lock');
  const release = await acquireLock(lockDir);
  // On a signal: forward it to the build tree and release the lock ONLY once that tree is confirmed
  // gone; otherwise keep the lock (another builder must not share a live cargo target dir).
  let buildChild = null; // kept after close: its process group must be empty before unlocking
  const onSig = (sig) => {
    log(TAG, `${sig}: stopping the build process tree before releasing ${lockDir}`);
    stopChildren(sig, 60_000, buildChild ? [buildChild] : []).then((alive) => {
      if (alive.length) {
        log(TAG, `lock RETAINED: build process(es) ${alive.map((c) => c.pid).join(',')} still running; remove ${lockDir} only after they exit`);
      } else release();
      process.exit(130);
    });
  };
  for (const s of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(s, onSig);
  const C = new Checks();
  let artifacts;
  try {
    const before = sha256File(lockfile);
    const started = Date.now() - 2000;
    // tauri-build copies the sidecar into <target>/<profile>/ only when its build script reruns
    // (cargo rerun-if-changed = mtime); a sidecar restored with an old mtime would leave a stale copy.
    const now = new Date();
    fs.utimesSync(L.staged, now, now);
    const args = [cli.js, 'build', '--bundles', bundles.join(','), '--ci', ...(debug ? ['--debug'] : []), '--', '--locked'];
    const benv = { ...process.env, CARGO_TARGET_DIR: targetDir };
    log(TAG, `tauri ${args.slice(1).join(' ')} (CARGO_TARGET_DIR=${targetDir})`);
    await runOk(process.execPath, args, { cwd: path.join(DESKTOP_DIR, 'src-tauri'), env: benv, group: true, onSpawn: (c) => (buildChild = c) });
    if (sha256File(lockfile) !== before) throw new Fail('desktop/Cargo.lock changed during build');
    C.add('cargo-lock', 'PASS', 'desktop/Cargo.lock unchanged (--locked)');

    const profileDir = path.join(targetDir, debug ? 'debug' : 'release');
    const copied = path.join(profileDir, installedSidecarName(L.triple));
    sidecarRow(C, 'target:sidecar', fs.existsSync(copied) ? [copied] : [], stagedSha);
    artifacts = collectArtifacts(path.join(profileDir, 'bundle'), bundles, started);
    for (const a of artifacts) {
      C.add(`${a.kind}:artifact`, 'INFO', a.src);
      await inspect(C, a, { stagedSha, triple: L.triple, platform, desktop: DESKTOP_DIR });
    }
  } finally {
    for (const s of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.off(s, onSig);
    const alive = await stopChildren('SIGTERM', 30_000, buildChild ? [buildChild] : []); // normally none
    if (alive.length) log(TAG, `lock RETAINED: ${alive.map((c) => c.pid).join(',')} still running; remove ${lockDir} after they exit`);
    else release();
  }
  if (C.has('FAIL')) throw new Fail('bundle checks FAILED (manifest not written)');

  fs.mkdirSync(outDir, { recursive: true });
  const manifestPath = path.join(outDir, 'artifacts.json');
  const prev = readJson(manifestPath);
  for (const p of prev?.artifacts || []) {
    const abs = path.resolve(p.abs_path || path.join(REPO_DIR, p.path || ''));
    if (path.dirname(abs) === path.resolve(outDir)) rmrf(abs);
  }
  rmrf(manifestPath);
  const out = [];
  for (const a of artifacts) {
    const dest = copyOut(a, outDir);
    if (a.kind === 'app' && process.platform === 'darwin') {
      const r = await run('codesign', ['--verify', '--deep', dest], { capture: true });
      C.add('app:copy-codesign', r.code === 0 ? 'PASS' : 'FAIL', r.code === 0 ? `${dest} signature valid after copy` : r.stderr.trim());
    }
    out.push({ kind: a.kind, path: manifestPath_(dest), abs_path: dest, ...artifactHash(dest), built_from: a.src });
  }
  if (C.has('FAIL')) throw new Fail('copied bundle failed verification (manifest not written)');
  const verified = !C.has('WARN');
  const manifest = {
    schema: 1,
    generated_at: utcStamp(),
    platform,
    target: L.triple,
    profile: debug ? 'debug' : 'release',
    version: readJson(path.join(DESKTOP_DIR, 'src-tauri', 'tauri.conf.json'))?.version,
    tauri_cli: cli.version,
    loomd_sha256: stagedSha,
    loomd_profile: stamp.profile,
    ui_dist_digest: stamp.ui_dist_digest,
    baseline: stamp.baseline,
    verified,
    strict,
    artifacts: out,
    checks: C.rows,
  };
  writeFileAtomic(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  for (const a of out) log(TAG, `artifact ${a.kind} ${a.abs_path} sha256 ${a.sha256}`);
  log(TAG, `manifest ${manifestPath}`);
  if (!verified) {
    const msg = 'some bundle contents were NOT inspected (WARN rows); package is not verified';
    if (strict) throw new Fail(`${msg} — strict mode (CI) fails`);
    log(TAG, `WARNING ${msg}`);
  } else log(TAG, 'PASS (all bundle checks)');
  return manifest;
}
