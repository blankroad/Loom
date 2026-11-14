#!/usr/bin/env python3
"""External UART device fixture: a real pty that Loom opens like any serial port.

  python3 -I uart_fixture.py --state DIR [--banner-every SECONDS]

Writes DIR/tty (slave path for `loom uart open`), DIR/pid, and appends every byte it RECEIVES from
Loom to DIR/rx.log (proof that UI keystrokes / CLI sends reached the device). Behaves like a tiny
shell: echoes input, answers each line with `ack:<line>` then a `fx> ` prompt; a periodic
`tick <n>` line (default every 2 s) shows RX streaming continuously (daemon persistence checks).
Stops on SIGTERM/SIGINT or when DIR/stop exists. Never touches any other device.
"""
import argparse, os, pty, select, signal, sys, time, tty

ap = argparse.ArgumentParser()
ap.add_argument("--state", required=True)
ap.add_argument("--banner-every", type=float, default=2.0)
a = ap.parse_args()
os.makedirs(a.state, exist_ok=True)
master, slave = pty.openpty()
tty.setraw(slave)  # raw line discipline so bytes pass unchanged
name = os.ttyname(slave)
open(os.path.join(a.state, "pid"), "w").write(str(os.getpid()))
open(os.path.join(a.state, "tty"), "w").write(name)
rx = open(os.path.join(a.state, "rx.log"), "ab", buffering=0)
running = True
def stop(*_):
    global running
    running = False
signal.signal(signal.SIGTERM, stop); signal.signal(signal.SIGINT, stop)
os.write(master, b"\r\nloom-fixture ready\r\nfx> ")
line, n, nxt = b"", 0, time.monotonic() + a.banner_every
while running and not os.path.exists(os.path.join(a.state, "stop")):
    r, _, _ = select.select([master], [], [], 0.2)
    if r:
        try:
            data = os.read(master, 4096)
        except OSError:
            data = b""
        if data:
            rx.write(data)
            for b in data:
                c = bytes([b])
                if c in (b"\r", b"\n"):
                    if line:
                        os.write(master, b"\r\nack:" + line + b"\r\nfx> ")
                    line = b""
                elif c in (b"\x7f", b"\x08"):
                    line = line[:-1]; os.write(master, b"\b \b")
                else:
                    line += c; os.write(master, c)
    if a.banner_every > 0 and time.monotonic() >= nxt:
        n += 1; nxt = time.monotonic() + a.banner_every
        os.write(master, b"\r\ntick %d\r\nfx> " % n + line)
os.close(master); os.close(slave)
