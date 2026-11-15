#!/usr/bin/env python3
"""Independent minimal DoIP (ISO 13400-2) ECU test fixture written by the Opus w26 validator.
NOT Loom code, NOT Loom's internal sim, NOT a certified DoIP stack. stdlib only, 127.0.0.1 only.
Supports: routing activation (0x0005→0x0006), diagnostic message (0x8001 → ACK 0x8002 + response 0x8001),
alive check response. UDS: 22 F190 → 62 F190 + VIN; 22 FFFF → 7F 22 31; 3E 00 → 7E 00; else 7F <sid> 11.
Logs every DoIP message as NDJSON (dir, payload type, hex, wall_ns). Control: touch <out>/disconnect to drop
the TCP connection (server keeps listening)."""
import json, os, pathlib, signal, socket, struct, sys, threading, time
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
VIN = os.environ.get('FIXTURE_VIN', 'OPUSW26DOIP000001').encode()
ECU, TESTER = 0x1001, 0x0E80
lock = threading.Lock(); running = True

def log(**kw):
    kw['wall_ns'] = time.time_ns(); kw['fixturePID'] = os.getpid()
    with lock, (out / 'doip.jsonl').open('a') as f: f.write(json.dumps(kw) + '\n')

def frame(ptype, payload):
    return struct.pack('>BBHI', 2, 0xFD, ptype, len(payload)) + payload

def uds(req):
    if req[:3] == bytes.fromhex('22f190'): return bytes.fromhex('62f190') + VIN
    if req[:3] == bytes.fromhex('22ffff'): return bytes.fromhex('7f2231')
    if req[:2] == bytes.fromhex('3e00'): return bytes.fromhex('7e00')
    if req[:2] == bytes.fromhex('3e80'): return None
    return bytes([0x7F, req[0] if req else 0, 0x11])

def recv_exact(c, n):
    b = b''
    while len(b) < n:
        if (out / 'disconnect').exists(): raise ConnectionError('fixture disconnect requested')
        try: chunk = c.recv(n - len(b))
        except socket.timeout: continue
        if not chunk: raise ConnectionError('peer closed')
        b += chunk
    return b

def send(c, ptype, payload, note):
    data = frame(ptype, payload); c.sendall(data)
    log(dir='tx', ptype=f'0x{ptype:04x}', hex=data.hex(), note=note)

def serve(c, peer):
    log(event='accepted', peer=f'{peer[0]}:{peer[1]}'); c.settimeout(0.2)
    try:
        while running:
            hdr = recv_exact(c, 8); ver, inv, ptype, ln = struct.unpack('>BBHI', hdr)
            body = recv_exact(c, ln) if ln else b''
            log(dir='rx', ptype=f'0x{ptype:04x}', hex=(hdr + body).hex())
            if ptype == 0x0005:
                sa = struct.unpack('>H', body[:2])[0]
                code = 0x10 if sa == TESTER else 0x00
                send(c, 0x0006, struct.pack('>HHB', sa, ECU, code) + b'\0' * 4, f'routing activation code 0x{code:02x}')
            elif ptype == 0x0007:
                send(c, 0x0008, struct.pack('>H', ECU), 'alive check response')
            elif ptype == 0x8001:
                sa, ta = struct.unpack('>HH', body[:4]); req = body[4:]
                send(c, 0x8002, struct.pack('>HHB', ta, sa, 0) + req[:4], 'diag ACK')
                rsp = uds(req)
                if rsp is not None:
                    send(c, 0x8001, struct.pack('>HH', ECU, sa) + rsp, 'uds response ' + rsp[:3].hex())
            else:
                send(c, 0x0000, bytes([0x01]), 'generic NACK unknown payload type')
    except ConnectionError as e:
        log(event='connection_end', reason=str(e))
    finally:
        c.close()
        try: (out / 'disconnect').unlink()
        except FileNotFoundError: pass

def stop(*_):
    global running
    running = False
signal.signal(signal.SIGTERM, stop); signal.signal(signal.SIGINT, stop)
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('127.0.0.1', int(os.environ.get('FIXTURE_PORT','0')))); s.listen(4); s.settimeout(0.2)
port = s.getsockname()[1]
(out / 'ready.json').write_text(json.dumps(dict(port=port, pid=os.getpid(), ecu=hex(ECU), tester=hex(TESTER), vin=VIN.decode())))
log(event='listening', port=port)
print(json.dumps(dict(ready=True, port=port, pid=os.getpid())), flush=True)
while running:
    try: c, peer = s.accept()
    except socket.timeout: continue
    threading.Thread(target=serve, args=(c, peer), daemon=True).start()
s.close(); log(event='closed')
