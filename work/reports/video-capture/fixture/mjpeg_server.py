#!/usr/bin/env python3
"""Independent loopback HTTP multipart-MJPEG producer for Loom video acceptance (not product code).

Frames are drawn here with Pillow (run_id + producer frame counter + timestamp + moving bar),
independently of Loom and FFmpeg. Every sent part is logged as one NDJSON line
(producer_seq, client, bytes, sha256 of the *source* JPEG, mono/utc time). Loom re-encodes to
its own normalized JPEGs, so producer_seq / sha256 here are NOT Loom frame ids or hashes; the
burn-in counter is visible in the image only.

Endpoints (127.0.0.1 only):
  GET /stream                 multipart/x-mixed-replace; boundary=loomframe
  GET /control/stall?ms=N     pause sending for N ms (all clients)
  GET /control/disconnect     close every open /stream connection now
  GET /control/corrupt?n=K    next K parts carry garbage bytes labelled image/jpeg
  GET /control/large?n=K      next K parts are large noisy JPEGs (4096x2304)
  GET /control/state          JSON counters
Run:  python3 -I mjpeg_server.py --port 0 --fps 30 --size 1280x720 --run-id RUN --log sent.ndjson
Prints one JSON line {"ready":true,"pid":..,"port":..,"run_id":..} on stdout when listening.
"""

import argparse
import hashlib
import io
import json
import os
import random
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

from PIL import Image, ImageDraw, ImageFont

BOUNDARY = "loomframe"


class State:
    def __init__(self, args):
        self.args = args
        self.lock = threading.Lock()
        self.seq = 0
        self.stall_until = 0.0
        self.corrupt = 0
        self.large = 0
        self.generation = 0  # bumped by /control/disconnect
        self.clients = 0
        self.next_client = 0
        self.log = open(args.log, "a", buffering=1) if args.log else None
        w, h = args.size.split("x")
        self.w, self.h = int(w), int(h)
        try:
            self.font = ImageFont.load_default(size=max(18, self.h // 12))
            self.small = ImageFont.load_default(size=max(12, self.h // 30))
        except TypeError:
            self.font = self.small = ImageFont.load_default()

    def record(self, **kv):
        if self.log:
            kv["mono_ns"] = time.monotonic_ns()
            kv["utc_ns"] = time.time_ns()
            self.log.write(json.dumps(kv, separators=(",", ":")) + "\n")


def draw_frame(st, seq):
    img = Image.new("RGB", (st.w, st.h), (16, 24, 40))
    d = ImageDraw.Draw(img)
    x = (seq * 13) % st.w
    d.rectangle([x, st.h - st.h // 8, x + st.w // 10, st.h - 4], fill=(240, 180, 40))
    d.text((24, 24), f"RUN {st.args.run_id}", fill=(255, 255, 255), font=st.small)
    d.text((24, st.h // 4), f"F {seq:06d}", fill=(120, 255, 160), font=st.font)
    d.text((24, st.h // 2), time.strftime("%H:%M:%S") + f".{int(time.time()*1000)%1000:03d}",
           fill=(200, 200, 255), font=st.small)
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=85)
    return buf.getvalue()


def large_frame(st, seq):
    img = Image.effect_noise((4096, 2304), 64).convert("RGB")
    ImageDraw.Draw(img).text((40, 40), f"RUN {st.args.run_id} LARGE {seq}", fill=(255, 0, 0), font=st.font)
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=95)
    return buf.getvalue()


def make_handler(st):
    class H(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):  # quiet; NDJSON log is the record
            pass

        def _json(self, code, obj):
            b = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(b)

        def do_GET(self):
            u = urlparse(self.path)
            q = parse_qs(u.query)
            n = lambda k, d: int(q.get(k, [d])[0])
            if u.path == "/stream":
                return self.stream()
            with st.lock:
                if u.path == "/control/stall":
                    st.stall_until = time.monotonic() + n("ms", 3000) / 1000
                    st.record(event="control", action="stall", ms=n("ms", 3000))
                elif u.path == "/control/disconnect":
                    st.generation += 1
                    st.record(event="control", action="disconnect")
                elif u.path == "/control/corrupt":
                    st.corrupt += n("n", 3)
                    st.record(event="control", action="corrupt", n=n("n", 3))
                elif u.path == "/control/large":
                    st.large += n("n", 1)
                    st.record(event="control", action="large", n=n("n", 1))
                elif u.path != "/control/state":
                    return self._json(404, {"error": "not_found"})
                state = {"seq": st.seq, "clients": st.clients, "generation": st.generation,
                         "corrupt_pending": st.corrupt, "large_pending": st.large,
                         "stalled": time.monotonic() < st.stall_until, "run_id": st.args.run_id}
            self._json(200, state)

        def stream(self):
            with st.lock:
                st.clients += 1
                st.next_client += 1
                client = st.next_client
                gen = st.generation
            st.record(event="connect", client=client)
            period = 1.0 / st.args.fps
            nxt = time.monotonic()
            reason = "client_closed"
            try:
                self.send_response(200)
                self.send_header("Content-Type", f"multipart/x-mixed-replace;boundary={BOUNDARY}")
                self.send_header("Cache-Control", "no-store")
                self.send_header("Connection", "close")
                self.end_headers()
                while True:
                    with st.lock:
                        if st.generation != gen:
                            reason = "server_disconnect"
                            break
                        stalled = time.monotonic() < st.stall_until
                    if stalled:
                        time.sleep(0.01)
                        nxt = time.monotonic()
                        continue
                    with st.lock:
                        st.seq += 1
                        seq = st.seq
                        kind = "normal"
                        if st.corrupt > 0:
                            st.corrupt -= 1
                            kind = "corrupt"
                        elif st.large > 0:
                            st.large -= 1
                            kind = "large"
                    if kind == "corrupt":
                        data = b"\xff\xd8" + os.urandom(20000) + b"\xff\xd9"
                    elif kind == "large":
                        data = large_frame(st, seq)
                    else:
                        data = draw_frame(st, seq)
                    head = (f"--{BOUNDARY}\r\nContent-Type: image/jpeg\r\n"
                            f"Content-Length: {len(data)}\r\n\r\n").encode()
                    self.wfile.write(head + data + b"\r\n")
                    self.wfile.flush()
                    st.record(event="sent", client=client, producer_seq=seq, kind=kind,
                              bytes=len(data), source_sha256=hashlib.sha256(data).hexdigest())
                    nxt += period
                    delay = nxt - time.monotonic()
                    if delay > 0:
                        time.sleep(delay)
                    else:
                        nxt = time.monotonic()
            except (BrokenPipeError, ConnectionResetError, OSError):
                reason = "client_closed"
            finally:
                with st.lock:
                    st.clients -= 1
                st.record(event="disconnect", client=client, reason=reason)
                self.close_connection = True

    return H


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--fps", type=float, default=30.0)
    ap.add_argument("--size", default="1280x720")
    ap.add_argument("--run-id", default=f"R{random.randrange(16**6):06X}")
    ap.add_argument("--log", default="")
    args = ap.parse_args()
    st = State(args)
    srv = ThreadingHTTPServer(("127.0.0.1", args.port), make_handler(st))
    srv.daemon_threads = True
    port = srv.server_address[1]
    st.record(event="ready", pid=os.getpid(), port=port, run_id=args.run_id, fps=args.fps, size=args.size)
    print(json.dumps({"ready": True, "pid": os.getpid(), "port": port, "run_id": args.run_id,
                      "fps": args.fps, "size": args.size}), flush=True)
    try:
        srv.serve_forever(poll_interval=0.1)
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()
        sys.stdout.flush()


if __name__ == "__main__":
    main()
