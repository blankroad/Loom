import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export function tmpdir(t, prefix = 'loom-pkg-test-') {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  t.after(() => fs.rmSync(d, { recursive: true, force: true, maxRetries: 5 }));
  return d;
}
export function write(p, data) {
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, data);
  return p;
}
export function setMtime(p, ms) {
  const d = new Date(ms);
  fs.utimesSync(p, d, d);
}

/** Minimal repo layout: ui sources older than ui/dist, a dist with nested + hashed assets. */
export function fakeRepo(t) {
  const repo = tmpdir(t);
  const old = Date.now() - 60_000;
  setMtime(write(path.join(repo, 'ui', 'src', 'main.ts'), 'export {}\n'), old);
  setMtime(write(path.join(repo, 'ui', 'package.json'), '{}\n'), old);
  write(path.join(repo, 'ui', 'dist', 'assets', 'index-AbC.js'), 'console.log(1)\n');
  write(path.join(repo, 'ui', 'dist', 'assets', 'index-xyz.css'), 'body{}\n');
  write(path.join(repo, 'ui', 'dist', 'favicon.svg'), '<svg/>');
  write(path.join(repo, 'ui', 'dist', 'index.html'), '<!doctype html><title>Loom</title>\n');
  return repo;
}
