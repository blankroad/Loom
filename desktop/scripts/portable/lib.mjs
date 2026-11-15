// Shared helpers for the portable desktop packaging entrypoint (desktop/scripts/package.mjs).
// Node builtins only; no shell for anything we pass paths to. Pure helpers are exported for tests.
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const SCRIPTS_DIR = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const DESKTOP_DIR = path.dirname(SCRIPTS_DIR);
export const REPO_DIR = path.dirname(DESKTOP_DIR);

export const SIDECAR_BASE = 'loomd';
export const STALE_EXIT = 3;

/** Tauri externalBin naming (tauri-utils 2.10 `external_binaries`): `<base>-<triple>` + `.exe` for Windows targets. */
export function exeSuffix(triple) {
  return /windows/.test(triple) ? '.exe' : '';
}
export function sidecarFileName(triple) {
  if (!triple || /\s|\//.test(triple)) throw new Error(`bad target triple: ${JSON.stringify(triple)}`);
  return `${SIDECAR_BASE}-${triple}${exeSuffix(triple)}`;
}
/** Installed name next to the app executable (tauri-build `copy_binaries` strips `-<triple>`). */
export function installedSidecarName(triple) {
  return `${SIDECAR_BASE}${exeSuffix(triple)}`;
}

/** Platform key from a triple: darwin | linux | win32. */
export function platformOfTriple(triple) {
  if (/apple-darwin$/.test(triple)) return 'darwin';
  if (/windows/.test(triple)) return 'win32';
  if (/linux/.test(triple)) return 'linux';
  throw new Error(`unsupported desktop target ${triple}`);
}

/** Explicit default Tauri bundle targets per platform (mirrors tauri.conf.json + tauri.<os>.conf.json). */
export const DEFAULT_BUNDLES = Object.freeze({
  darwin: Object.freeze(['app']),
  linux: Object.freeze(['deb']), // AppImage: experimental, failed exact-sidecar proof (linuxdeploy rewrites the ELF)
  win32: Object.freeze(['nsis']),
});
/** Supported = validated with the exact sidecar check. Nothing else is accepted. */
export const ALLOWED_BUNDLES = DEFAULT_BUNDLES;
export function defaultBundles(platform) {
  const b = DEFAULT_BUNDLES[platform];
  if (!b) throw new Fail(`no desktop bundles for platform ${platform}`);
  return [...b];
}
export function parseBundles(platform, spec) {
  if (!spec) return defaultBundles(platform);
  const list = [...new Set(String(spec).split(/[,\s]+/).filter(Boolean))];
  const bad = list.filter((b) => !ALLOWED_BUNDLES[platform]?.includes(b));
  if (bad.length || !list.length) {
    throw new Fail(`bundles ${bad.join(',') || '(none)'} not valid on ${platform} (allowed: ${ALLOWED_BUNDLES[platform]})`);
  }
  return list;
}

/**
 * Desktop-workspace cargo target dir: LOOM_DESKTOP_TARGET_DIR > CARGO_TARGET_DIR > default
 * (/tmp/loom-tauri-build on macOS/Linux — same as build-app.sh and its lock — %TEMP%\loom-tauri-build on Windows).
 * Never inside desktop/ (desktop/target is forbidden: it would ship next to sources and confuse the mac QA).
 */
export function desktopTargetDir({ env = process.env, platform = process.platform, tmpdir = os.tmpdir(), desktopDir = DESKTOP_DIR } = {}) {
  const pick = (v) => (v && String(v).trim() ? String(v) : '');
  const p = pathFor(platform);
  let dir = pick(env.LOOM_DESKTOP_TARGET_DIR) || pick(env.CARGO_TARGET_DIR);
  if (!dir) dir = platform === 'win32' ? p.join(tmpdir, 'loom-tauri-build') : '/tmp/loom-tauri-build';
  dir = p.resolve(dir);
  const rel = p.relative(p.resolve(desktopDir), dir);
  if (rel === '' || (!rel.startsWith('..') && !p.isAbsolute(rel))) {
    throw new Error(`desktop target dir ${dir} is inside ${desktopDir}; use a directory outside desktop/ (no desktop/target)`);
  }
  return dir;
}
export function pathFor(platform) {
  return platform === 'win32' ? path.win32 : path.posix;
}

export function sha256File(file) {
  const h = createHash('sha256');
  const fd = fs.openSync(file, 'r');
  try {
    const buf = Buffer.allocUnsafe(1 << 20);
    let n;
    while ((n = fs.readSync(fd, buf, 0, buf.length, null)) > 0) h.update(buf.subarray(0, n));
  } finally {
    fs.closeSync(fd);
  }
  return h.digest('hex');
}

/** Every regular file under dir as '/'-separated relative paths (symlinks are not followed). */
export function listFiles(dir) {
  const out = [];
  const walk = (abs, rel) => {
    for (const e of fs.readdirSync(abs, { withFileTypes: true })) {
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) walk(path.join(abs, e.name), r);
      else if (e.isFile()) out.push(r);
    }
  };
  walk(dir, '');
  return out;
}

const byteCompare = (a, b) => Buffer.compare(Buffer.from(a, 'utf8'), Buffer.from(b, 'utf8'));

/**
 * Order-stable digest of a directory, byte-compatible with stage-loomd.sh:
 *   (cd dir && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256) | shasum -a 256
 * i.e. sha256 over lines "<sha256 hex>  ./<rel>\n" sorted bytewise by "./<rel>".
 */
export function dirDigest(dir) {
  const names = listFiles(dir).map((r) => `./${r}`).sort(byteCompare);
  const h = createHash('sha256');
  for (const n of names) h.update(`${sha256File(path.join(dir, n.slice(2)))}  ${n}\n`);
  return h.digest('hex');
}

export function log(tag, msg) {
  process.stdout.write(`${tag}: ${msg}\n`);
}
export class Fail extends Error {
  constructor(msg, code = 1) {
    super(msg);
    this.exitCode = code;
  }
}

/**
 * Commands run without a shell. Windows `.cmd` shims (pnpm, npm) cannot be spawned directly, so
 * `viaShellOnWindows` routes them through cmd.exe — only allowed for fixed, metacharacter-free
 * args (paths go into `cwd`, never onto the cmd.exe command line).
 */
const SHELL_SAFE = /^[A-Za-z0-9._@=:+-]+$/;
export function shellSafeArgs(args) {
  for (const a of args) if (!SHELL_SAFE.test(a)) throw new Error(`refusing to pass ${JSON.stringify(a)} through cmd.exe`);
  return args;
}
function spawnSpec(cmd, args, { viaShellOnWindows = false, platform = process.platform } = {}) {
  if (viaShellOnWindows && platform === 'win32') {
    shellSafeArgs([cmd, ...args]);
    return { file: [cmd, ...args].join(' '), args: [], shell: true };
  }
  return { file: cmd, args, shell: false };
}
export { spawnSpec as _spawnSpec };

const children = new Set();
export function liveChildren() {
  return children;
}

/** True while any process of the child's tree we can observe is alive (POSIX: its process group). */
export function treeAlive(child) {
  if (child.loomGroup && child.pid) {
    try {
      process.kill(-child.pid, 0);
      return true;
    } catch (e) {
      return e.code === 'EPERM';
    }
  }
  return child.exitCode === null && child.signalCode === null;
}

/**
 * Forward a stop to every tracked child tree (POSIX: signal its process group; Windows: taskkill /T
 * on our own child pid only) and wait until each tree is gone. Returns the children still alive.
 */
export async function stopChildren(signal = 'SIGTERM', waitMs = 60_000, extra = []) {
  const list = [...new Set([...children, ...extra])];
  for (const c of list) {
    if (!c.pid || !treeAlive(c)) continue;
    try {
      if (process.platform === 'win32') spawnSync('taskkill', ['/T', '/F', '/PID', String(c.pid)], { stdio: 'ignore', windowsHide: true });
      else if (c.loomGroup) process.kill(-c.pid, signal);
      else c.kill(signal);
    } catch {
      /* already gone */
    }
  }
  const end = Date.now() + waitMs;
  let alive = list.filter(treeAlive);
  while (alive.length && Date.now() < end) {
    await sleep(200);
    alive = alive.filter(treeAlive);
  }
  if (alive.length && process.platform !== 'win32') {
    for (const c of alive) {
      try {
        if (c.loomGroup) process.kill(-c.pid, 'SIGKILL');
        else c.kill('SIGKILL');
      } catch {
        /* gone */
      }
    }
    await sleep(1000);
    alive = alive.filter(treeAlive);
  }
  return alive;
}

/** Run a command inheriting stdio; resolves with exit code (rejects only if it could not start). */
export function run(cmd, args, opts = {}) {
  const { cwd = REPO_DIR, env = process.env, viaShellOnWindows = false, capture = false, input, group = false, onSpawn } = opts;
  const s = spawnSpec(cmd, args, { viaShellOnWindows });
  // group: POSIX child leads its own process group so a forwarded signal reaches the whole build
  // tree (tauri → cargo → rustc) and its exit can be confirmed with kill(-pgid, 0).
  return new Promise((resolve, reject) => {
    const child = spawn(s.file, s.args, {
      cwd,
      env,
      shell: s.shell,
      stdio: [input === undefined ? 'ignore' : 'pipe', capture ? 'pipe' : 'inherit', capture ? 'pipe' : 'inherit'],
      windowsHide: true,
      detached: group && process.platform !== 'win32',
    });
    child.loomGroup = group && process.platform !== 'win32';
    children.add(child);
    onSpawn?.(child);
    let out = '';
    let err = '';
    if (capture) {
      child.stdout.on('data', (d) => (out += d));
      child.stderr.on('data', (d) => (err += d));
    }
    if (input !== undefined) child.stdin.end(input);
    child.on('error', (e) => {
      children.delete(child);
      reject(new Fail(`cannot run ${cmd}: ${e.message}`));
    });
    child.on('close', (code, signal) => {
      children.delete(child);
      resolve({ code: code ?? (signal ? 128 : 1), signal, stdout: out, stderr: err });
    });
  });
}
export async function runOk(cmd, args, opts = {}) {
  const r = await run(cmd, args, opts);
  if (r.code !== 0) throw new Fail(`${cmd} ${args.join(' ')} failed (exit ${r.code}${r.signal ? ` ${r.signal}` : ''})${r.stderr ? `\n${r.stderr.trim().slice(-2000)}` : ''}`);
  return r;
}

/** Synchronous probe (version strings etc.); never throws, returns '' on failure. */
export function probe(cmd, args, opts = {}) {
  const s = spawnSpec(cmd, args, opts);
  const r = spawnSync(s.file, s.args, { encoding: 'utf8', shell: s.shell, windowsHide: true, cwd: opts.cwd, env: opts.env, timeout: opts.timeout ?? 60_000 });
  return r.status === 0 ? String(r.stdout).trim() : '';
}
export function hasCommand(cmd, args = ['--version']) {
  const r = spawnSync(cmd, args, { stdio: 'ignore', windowsHide: true, timeout: 20_000 });
  return !r.error;
}

export function hostTriple() {
  let t = probe('rustc', ['--print', 'host-tuple']);
  if (!t) t = (probe('rustc', ['-Vv']).match(/^host:\s*(\S+)/m) || [])[1] || '';
  if (!t) throw new Fail('rustc not found (need rustc >= 1.84 for --print host-tuple)');
  return t;
}

/** Env copy without the given keys (case-insensitive on Windows). */
export function envWithout(keys, env = process.env, platform = process.platform) {
  const out = { ...env };
  for (const k of Object.keys(out)) {
    if (keys.some((x) => (platform === 'win32' ? x.toLowerCase() === k.toLowerCase() : x === k))) delete out[k];
  }
  return out;
}

/** Atomic-ish replace: copy to a sibling temp file then rename over the destination. */
export function replaceFile(src, dest, mode) {
  const tmp = `${dest}.tmp.${process.pid}`;
  fs.copyFileSync(src, tmp);
  if (mode !== undefined) fs.chmodSync(tmp, mode);
  fs.renameSync(tmp, dest);
}
export function writeFileAtomic(dest, data) {
  const tmp = `${dest}.tmp.${process.pid}`;
  fs.writeFileSync(tmp, data);
  fs.renameSync(tmp, dest);
}
export function rmrf(p) {
  fs.rmSync(p, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
export function mkTemp(prefix) {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  try {
    fs.chmodSync(d, 0o700);
  } catch {
    /* Windows: ACLs, not modes */
  }
  return d;
}
export function isExecutableFile(p, platform = process.platform) {
  try {
    const st = fs.statSync(p);
    if (!st.isFile()) return false;
    if (platform === 'win32') return true;
    fs.accessSync(p, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}
export function readJson(p) {
  try {
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch {
    return null;
  }
}
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Local time as YYYY-MM-DDTHH:MM:SS (same as `stat -f %Sm -t %Y-%m-%dT%H:%M:%S`). */
export function localStamp(d) {
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}
export function utcStamp(d = new Date()) {
  return d.toISOString().replace(/\.\d{3}Z$/, '+00:00');
}

/**
 * mkdir lock shared with build-app.sh (<target>/.cargo-desktop.lock, owner file inside).
 * Returns a release function. Waits up to waitMs; the lock is never broken automatically.
 */
export async function acquireLock(lockDir, { waitMs = 600_000, pollMs = 1000, owner = `package.mjs pid ${process.pid}` } = {}) {
  fs.mkdirSync(path.dirname(lockDir), { recursive: true });
  const deadline = Date.now() + waitMs;
  let announced = false;
  for (;;) {
    try {
      fs.mkdirSync(lockDir);
      break;
    } catch (e) {
      if (e.code !== 'EEXIST') throw e;
      const who = (() => {
        try {
          return fs.readFileSync(path.join(lockDir, 'owner'), 'utf8').trim();
        } catch {
          return 'unknown';
        }
      })();
      if (Date.now() >= deadline) throw new Fail(`lock ${lockDir} held by ${who} for longer than ${Math.round(waitMs / 1000)}s`);
      if (!announced) log('package', `waiting for lock ${lockDir} (held by ${who})`);
      announced = true;
      await sleep(pollMs);
    }
  }
  fs.writeFileSync(path.join(lockDir, 'owner'), `${owner} ${utcStamp()}\n`);
  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    rmrf(lockDir);
  };
  return release;
}
