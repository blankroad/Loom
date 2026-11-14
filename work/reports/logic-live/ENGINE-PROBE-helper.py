"""Foreground-only sigrok probe; demo and synthetic stdin exclusively."""
import json, os, selectors, signal, subprocess, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLI = '/opt/homebrew/bin/sigrok-cli'

def waveform(kind):
    out = bytearray()
    def put(v, n=10): out.extend(bytes([v]) * n)
    if kind == 'uart':
        put(1, 200)
        for value in (0x55, 0xA3, 0x00, 0xFF):
            put(0)
            for b in range(8): put((value >> b) & 1)
            put(1, 30)
        put(1, 200)
    else:
        # D0 clock idle low; D1 MOSI; D2 active-low chip-select.
        put(4, 100)
        put(0, 30)
        for value in (0xA5, 0x3C):
            for b in range(7, -1, -1):
                v = ((value >> b) & 1) << 1
                put(v); put(v | 1)
        put(0, 20); put(4, 100)
    return bytes(out)

def run(name, decoder, samples=None, demo=False, numchannels=8, annotation=None, stop_style='signal'):
    decargv = [CLI, '-i', '-', '-I', f'binary:numchannels={numchannels}:samplerate=1000000',
               '-P', decoder, '--protocol-decoder-samplenum', '-l', '2']
    if annotation: decargv += ['-A', annotation]
    start = time.monotonic()
    dec = subprocess.Popen(decargv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    capargv = [CLI, '-d', 'demo', '-c', 'samplerate=1000000', '-C', 'D0,D1,D2,D3,D4,D5,D6,D7',
               '--continuous', '-O', 'binary', '-l', '2']
    cap = subprocess.Popen(capargv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE) if demo else None
    os.set_blocking(dec.stdin.fileno(), False)
    pending = bytearray(); input_done = False
    sel = selectors.DefaultSelector()
    for label, fd in [('decode', dec.stdout), ('decode_err', dec.stderr)]:
        os.set_blocking(fd.fileno(), False); sel.register(fd, selectors.EVENT_READ, label)
    if cap:
        for label, fd in [('raw', cap.stdout), ('capture_err', cap.stderr)]:
            os.set_blocking(fd.fileno(), False); sel.register(fd, selectors.EVENT_READ, label)
    first = {}; buffers = {k: bytearray() for k in ('decode','decode_err','capture_err')}
    raw_count = 0; written = 0; stop_at = None; eof_at = None; stop_sent = False
    rawfile = open(HERE / f'ENGINE-PROBE-{name}.bin', 'wb')
    next_write = start; iterations = 0; broken = False
    try:
        while time.monotonic() - start < 4:
            now = time.monotonic(); elapsed = now - start
            if samples is not None and elapsed < 1.1 and now >= next_write:
                # Ordinary pipe write, intentionally no EOF until t=1.5s.
                rawfile.write(samples)
                pending.extend(samples); iterations += 1
                next_write = now + .1
            if elapsed >= 1.5 and not stop_sent:
                stop_at = elapsed; stop_sent = True
                if cap:
                    if stop_style == 'key': cap.stdin.write(b'\n'); cap.stdin.flush()
                    else: cap.send_signal(signal.SIGINT)
                else: input_done = True
            if pending and not dec.stdin.closed:
                try:
                    n = os.write(dec.stdin.fileno(), pending[:65536])
                    del pending[:n]; written += n
                except BlockingIOError: pass
                except BrokenPipeError:
                    broken = True; pending.clear(); input_done = True
            if input_done and not pending and not dec.stdin.closed:
                dec.stdin.close(); eof_at = time.monotonic() - start
            if len(pending) > 4*1024*1024: raise RuntimeError('bounded tee queue overflow')
            for key, _ in sel.select(.01):
                try: data = os.read(key.fd, 65536)
                except BlockingIOError: continue
                label = key.data
                if not data:
                    sel.unregister(key.fileobj)
                    if label == 'raw': input_done = True
                    continue
                first.setdefault(label, time.monotonic() - start)
                if label == 'raw':
                    rawfile.write(data); raw_count += len(data)
                    pending.extend(data)
                else:
                    buffers[label].extend(data)
                    if label == 'decode' and b'\n' in buffers[label]:
                        first.setdefault('complete_annotation_line', time.monotonic() - start)
            if stop_sent and dec.poll() is not None and (not cap or cap.poll() is not None) and not sel.get_map(): break
    finally:
        rawfile.close()
        for proc in [cap, dec]:
            if proc is not None:
                if proc.poll() is None: proc.terminate()
                try: proc.wait(timeout=1)
                except subprocess.TimeoutExpired: proc.kill(); proc.wait(timeout=1)
                if proc.stdin and not proc.stdin.closed: proc.stdin.close()
        sel.close()
    text = buffers['decode'].decode(errors='replace')
    (HERE / f'ENGINE-PROBE-{name}.stdout.txt').write_text(text)
    (HERE / f'ENGINE-PROBE-{name}.stderr.txt').write_text(buffers['decode_err'].decode(errors='replace'))
    result = dict(name=name, decoder_argv=decargv, capture_argv=capargv if cap else None,
                  first_seconds=first, stop_requested_seconds=stop_at, input_eof_seconds=eof_at,
                  raw_bytes=raw_count, decoder_input_bytes=written, input_iterations=iterations,
                  annotation_lines=len(text.splitlines()), first_lines=text.splitlines()[:8],
                  decode_rc=dec.returncode, capture_rc=cap.returncode if cap else None,
                  broken_pipe=broken, elapsed_seconds=time.monotonic()-start,
                  stop_style=stop_style, all_children_reaped=all(p is None or p.returncode is not None for p in [cap, dec]),
                  capture_stderr=buffers['capture_err'].decode(errors='replace'))
    print(json.dumps(result), flush=True)
    return result

if __name__ == '__main__':
    results = [run('uart', 'uart:rx=0:baudrate=100000', waveform('uart'), annotation='uart=rx-data'),
               run('spi', 'spi:clk=0:mosi=1:cs=2:wordsize=8', waveform('spi'), annotation='spi=mosi-data'),
               run('demo-tee', 'spi:clk=0:mosi=1:wordsize=8', demo=True, annotation='spi=mosi-data', stop_style='key'),
               run('demo-signal', 'spi:clk=0:mosi=1:wordsize=8', demo=True, annotation='spi=mosi-data'),
               run('truncated', 'uart:rx=0:baudrate=100000', b'\xff'*4097, numchannels=9)]
    (HERE / 'ENGINE-PROBE-results.json').write_text(json.dumps(results, indent=2)+'\n')
