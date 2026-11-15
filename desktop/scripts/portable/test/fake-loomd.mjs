// Test double for `loomd run --port 0` (verify tests): argv = <dist dir> <mode>.
// Modes: ok | tamper (one file differs) | nocsp | otherpid (daemon.json names another pid) | exit | silent.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const [dist, mode] = process.argv.slice(2);
const home = process.env.LOOM_HOME;
if (mode === 'exit') {
  console.error('fake loomd: bind failed');
  process.exit(3);
}
if (mode === 'silent') setInterval(() => {}, 1000);
else {
  const server = http.createServer((req, res) => {
    const u = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    if (!u.startsWith('/ui/')) return res.writeHead(404).end();
    const rel = u.slice(4) || 'index.html';
    const f = path.join(dist, rel);
    if (!f.startsWith(dist) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) return res.writeHead(404).end();
    let body = fs.readFileSync(f);
    if (mode === 'tamper' && rel.endsWith('.css')) body = Buffer.concat([body, Buffer.from('/*x*/')]);
    const h = mode === 'nocsp' ? {} : { 'content-security-policy': "default-src 'self'; script-src 'self'" };
    res.writeHead(200, h).end(body);
  });
  server.listen(0, '127.0.0.1', () => {
    const run = path.join(home, 'run');
    fs.mkdirSync(run, { recursive: true });
    const pid = mode === 'otherpid' ? process.pid + 100000 : process.pid;
    fs.writeFileSync(path.join(run, 'daemon.json'), JSON.stringify({ port: server.address().port, pid, token: 'secret', version: 'fake' }));
  });
}
