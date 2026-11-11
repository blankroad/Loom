"""Measure isolated cargo builds and process execution; preserves complete logs."""
import datetime, json, os, pathlib, platform, subprocess, time

ROOT = pathlib.Path(__file__).resolve().parents[3]
BASE = ROOT / 'spikes/explore'
env = os.environ.copy()
env['CARGO_HOME'] = str(BASE / '.cargo-home')
results = {'retrieved_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'host': platform.platform(), 'cargo_home': env['CARGO_HOME'], 'runs': []}
for project, binary in [('v2-saleae-rs', 'loom-saleae-rs-spike'), ('v2-evidence', 'loom-evidence-spike')]:
    path = BASE / project
    env['CARGO_TARGET_DIR'] = str(path / 'target')
    for name, command in [('cold-build', ['cargo', 'build', '--release']), ('warm-build', ['cargo', 'build', '--release', '--locked']), ('run', [str(path / 'target/release' / binary)])]:
        log = path / f'{name}.log'
        start = time.perf_counter()
        with log.open('w') as out:
            proc = subprocess.run(command, cwd=path, env=env, stdout=out, stderr=subprocess.STDOUT)
        row = {'project':project,'phase':name,'command':command,'elapsed_s':time.perf_counter()-start,'exit_code':proc.returncode,'log':str(log.relative_to(ROOT)),'cargo_target_dir':env['CARGO_TARGET_DIR']}
        results['runs'].append(row)
        (BASE / 'v2-facts/spike-results.json').write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)
        if proc.returncode:
            print(log.read_text()[-10000:], flush=True)
            raise SystemExit(proc.returncode)
