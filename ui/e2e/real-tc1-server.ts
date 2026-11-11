// Bridge to the actual powered-DUT Rust integration test. No mock events or copied verdicts.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, readFile, writeFile, unlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

const delay = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
export class RealTc1 {
  private child: ChildProcess | null = null;
  private dir = '';
  private startError: Error | null = null;
  private login = '';
  private completed = false;
  async start() {
    const binary = process.env.LOOM_TC1_TEST_BIN;
    if (!binary) throw new Error('LOOM_TC1_TEST_BIN must select the default-feature tc1_live_smoke executable');
    this.dir = await mkdtemp(path.join(tmpdir(), 'loom-tc1-browser-'));
    this.child = spawn(binary, ['--exact', 'tc1_live_powered_dut_judged_from_journal', '--nocapture'], {
      env: { ...process.env, LOOM_TC1_BROWSER_DIR: this.dir, LOOM_TC1_UI_DIR: path.resolve('dist') },
      stdio: 'ignore', // The private bootstrap URL must never enter a test log/trace.
    });
    this.child.on('error', (e) => { this.startError = e; });
    const ready = await this.read('ready.json', 30_000);
    if (typeof ready.url !== 'string' || !ready.url.startsWith('http://localhost:')) throw new Error('invalid private TC bootstrap URL');
    this.login = ready.url;
    await unlink(path.join(this.dir, 'ready.json'));
  }
  loginURL() { return this.login; }
  async begin() { await writeFile(path.join(this.dir, 'begin'), 'begin'); }
  async result() {
    const result = await this.read('result.json', 45_000);
    this.completed = true;
    return result;
  }
  private async read(name: string, timeout: number): Promise<Record<string, unknown>> {
    const end = performance.now() + timeout;
    while (performance.now() < end) {
      if (this.startError) throw this.startError;
      if (this.child?.exitCode !== null && this.child?.exitCode !== undefined) throw new Error('TC fixture exited before handshake');
      try { return JSON.parse(await readFile(path.join(this.dir, name), 'utf8')) as Record<string, unknown>; }
      catch { await delay(25); }
    }
    throw new Error(`TC fixture ${name} timeout`);
  }
  async close() {
    const child = this.child;
    if (!child) return;
    if (this.dir) await writeFile(path.join(this.dir, 'release'), 'release');
    const end = performance.now() + (this.completed ? 15_000 : 2_000);
    while (child.exitCode === null && child.signalCode === null && performance.now() < end) await delay(50);
    if (child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM');
      await new Promise<void>((resolve) => { child.once('exit', () => resolve()); setTimeout(resolve, 3000); });
    }
    if (this.dir) await unlink(path.join(this.dir, 'ready.json')).catch(() => {});
    this.login = '';
    if (child.exitCode === null && child.signalCode === null) throw new Error('own TC fixture cleanup timeout');
    if (this.completed && child.exitCode !== 0) throw new Error('actual TC fixture validation/cleanup failed');
  }
}
