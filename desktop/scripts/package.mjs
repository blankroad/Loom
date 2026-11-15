#!/usr/bin/env node
// Portable Loom desktop packaging (macOS / Ubuntu / Windows). Node builtins only, no shell.
//
//   node desktop/scripts/package.mjs stage  [--profile release|debug] [--from <loomd>] [--reuse-ui] [--skip-verify]
//   node desktop/scripts/package.mjs check
//   node desktop/scripts/package.mjs verify [<loomd>]
//   node desktop/scripts/package.mjs bundle [--debug] [--bundles a,b] [--out-dir D] [--strict|--no-strict]
//   node desktop/scripts/package.mjs dist   [--debug] [--bundles a,b] [--out-dir D] [--strict|--no-strict]
//
// stage : fresh `pnpm build` of ui/ + `cargo build --locked -p loom-gateway --bin loomd` in the main
//         workspace (CARGO_TARGET_DIR removed; LOOM_STAGE_TARGET_DIR overrides) → copies it to
//         desktop/src-tauri/binaries/loomd-<host triple>[.exe] + loomd.ui-index.html + loomd.stamp.json,
//         then verify. Same files/stamp as stage-loomd.sh (mac shell and this path interoperate).
// check : exit 0 when the staged sidecar is current and verified, 3 when stale.
// verify: run <loomd> under a private temp LOOM_HOME; byte-compare every ui/dist file served at /ui/.
// bundle: requires check; `tauri build --bundles <platform defaults> --ci -- --locked` with
//         CARGO_TARGET_DIR = LOOM_DESKTOP_TARGET_DIR > CARGO_TARGET_DIR > /tmp/loom-tauri-build
//         (%TEMP%\loom-tauri-build on Windows) under <target>/.cargo-desktop.lock; checks the sidecar inside
//         each bundle; copies installables + artifacts.json to --out-dir (default desktop/dist).
//         Defaults: macOS app · Linux deb,appimage · Windows nsis. Strict (default when CI=true): a bundle
//         whose contents could not be inspected (WARN) fails.
// dist  : stage (release unless --debug) + bundle.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Fail, REPO_DIR, defaultBundles, hostTriple, liveChildren, parseBundles, platformOfTriple } from './portable/lib.mjs';
import { check, layout, stage, verify } from './portable/stage.mjs';
import { bundle } from './portable/bundle.mjs';

export function parseArgs(argv) {
  const [first, ...more] = argv;
  const helpFirst = first === '-h' || first === '--help';
  const cmd = helpFirst ? 'help' : first;
  const rest = helpFirst ? [] : more;
  const o = { cmd, positional: [] };
  const value = (i, name) => {
    const v = rest[i + 1];
    if (v === undefined || v.startsWith('--')) throw new Fail(`${name} needs a value`, 2);
    return v;
  };
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i];
    switch (a) {
      case '--profile': o.profile = value(i++, a); break;
      case '--from': o.from = value(i++, a); break;
      case '--reuse-ui': o.reuseUi = true; break;
      case '--skip-verify': o.skipVerify = true; break;
      case '--debug': o.debug = true; break;
      case '--release': o.debug = false; break;
      case '--bundles': o.bundles = value(i++, a); break;
      case '--out-dir': o.outDir = path.resolve(value(i++, a)); break;
      case '--strict': o.strict = true; break;
      case '--no-strict': o.strict = false; break;
      case '-h': case '--help': o.cmd = 'help'; break;
      default:
        if (a.startsWith('-')) throw new Fail(`unknown option ${a}`, 2);
        o.positional.push(a);
    }
  }
  return o;
}

function help() {
  const src = fs.readFileSync(fileURLToPath(import.meta.url), 'utf8');
  process.stdout.write(`${src.split('\n').slice(1).filter((l) => l.startsWith('//')).map((l) => l.replace(/^\/\/ ?/, '')).join('\n')}\n`);
}

export async function main(argv) {
  const o = parseArgs(argv);
  if (!o.cmd || o.cmd === 'help') return help(), o.cmd ? 0 : 2;
  const major = Number(process.versions.node.split('.')[0]);
  if (major < 24) throw new Fail(`Node ${process.versions.node}: desktop packaging needs Node >= 24 (ui engines)`, 2);
  const triple = hostTriple();
  const L = layout(REPO_DIR, triple);
  const platform = platformOfTriple(triple);
  switch (o.cmd) {
    case 'check':
      return check(L);
    case 'verify': {
      const bin = o.positional[0] ? path.resolve(o.positional[0]) : L.staged;
      return (await verify(L, bin)) ? 0 : 1;
    }
    case 'stage':
      await stage(L, { profile: o.profile || 'release', from: o.from, reuseUi: o.reuseUi, skipVerify: o.skipVerify });
      return 0;
    case 'bundle':
    case 'dist': {
      const bundles = o.bundles ? parseBundles(platform, o.bundles) : defaultBundles(platform);
      if (o.cmd === 'dist') await stage(L, { profile: o.debug ? 'debug' : 'release', reuseUi: o.reuseUi });
      await bundle(L, { debug: !!o.debug, bundles, outDir: o.outDir, strict: o.strict });
      return 0;
    }
    default:
      throw new Fail(`unknown command ${o.cmd} (stage|check|verify|bundle|dist)`, 2);
  }
}

const isMain = !!process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code ?? 0),
    (e) => {
      for (const c of liveChildren()) c.kill();
      process.stderr.write(`package: ${e instanceof Fail ? e.message : e.stack || e}\n`);
      process.exit(e instanceof Fail ? e.exitCode : 1);
    },
  );
}
