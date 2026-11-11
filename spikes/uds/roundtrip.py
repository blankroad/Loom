"""Planning-only feasibility probe. No hardware, public interfaces, or production API.

Run from repository root: spikes/uds/.venv/bin/python spikes/uds/roundtrip.py
"""
import collections
import importlib.metadata
import json
import logging
import multiprocessing as mp
import platform
import socket
import struct
import threading
import time
from pathlib import Path

import can
import isotp
import udsoncan
from doipclient import DoIPClient
from doipclient.connectors import DoIPClientUDSConnector
from udsoncan.client import Client
from udsoncan.connections import BaseConnection
from udsoncan.exceptions import TimeoutException

VIN = b"LOOMSIM0000000001"
REQUEST = bytes.fromhex("22F190")
RESPONSE = bytes.fromhex("62F190") + VIN


def summary(values):
    vals = sorted(values)
    return {"n": len(vals), "min_ms": vals[0], "median_ms": vals[len(vals)//2],
            "p95_ms": vals[int((len(vals)-1)*.95)],
            "p99_ms": vals[int((len(vals)-1)*.99)], "max_ms": vals[-1]}


def config():
    cfg = dict(udsoncan.configs.default_client_config)
    cfg.update(data_identifiers={0xF190: udsoncan.AsciiCodec(17),
                                 0xF191: udsoncan.AsciiCodec(17),
                                 0xF192: udsoncan.AsciiCodec(17)},
               p2_timeout=.05, p2_star_timeout=5, request_timeout=2)
    return cfg


def can_daemon(pipe):
    """Both virtual bus handles and ISO-TP nodes live in this child process."""
    faults, counts = [], collections.Counter()
    stop = threading.Event()
    def trace(msg):
        if msg.data:
            counts[{0: "SF", 1: "FF", 2: "CF", 3: "FC"}.get(msg.data[0] >> 4, "other")] += 1
    buses = [can.Bus(interface="virtual", channel="loom-uds-spike", receive_own_messages=False)
             for _ in range(2)]
    notifiers = [can.Notifier(bus, [trace], timeout=.01) for bus in buses]
    stacks = [isotp.NotifierBasedCanStack(
        bus=bus, notifier=notifier,
        address=isotp.Address(isotp.AddressingMode.Normal_11bits, txid=tx, rxid=rx),
        params={"blocking_send": True, "stmin": 0, "blocksize": 8},
        error_handler=lambda err: faults.append(str(err)))
        for bus, notifier, tx, rx in zip(buses, notifiers, [0x7E0, 0x7E8], [0x7E8, 0x7E0])]
    for stack in stacks:
        stack.start()
    def ecu():
        while not stop.is_set():
            data = stacks[1].recv(block=True, timeout=.05)
            if data is None:
                continue
            data = bytes(data)
            if data == REQUEST:
                stacks[1].send(RESPONSE, send_timeout=1)
            elif data == bytes.fromhex("22F191"):
                for _ in range(2):
                    stacks[1].send(bytes.fromhex("7F2278"), send_timeout=1)
                    time.sleep(.03)
                stacks[1].send(bytes.fromhex("62F191") + VIN, send_timeout=1)
            elif data == bytes.fromhex("22F192"):
                pass  # Deliberate P2 timeout.
            else:
                stacks[1].send(bytes([0x7F, data[0], 0x11]), send_timeout=1)
    thread = threading.Thread(target=ecu)
    thread.start()
    pipe.send({"ready": True})
    try:
        while True:
            op, arg = pipe.recv()
            if op == "ping":
                pipe.send(arg)
            elif op == "send":
                stacks[0].send(arg, send_timeout=1)
                pipe.send({"tx_complete_ns": time.monotonic_ns()})
            elif op == "recv":
                data = stacks[0].recv(block=True, timeout=arg)
                pipe.send(None if data is None else bytes(data))
            elif op == "drain":
                while stacks[0].recv() is not None:
                    pass
                pipe.send(True)
            elif op == "close":
                pipe.send({"frame_counts_on_bus_rx": dict(counts), "isotp_errors": faults})
                break
    finally:
        stop.set()
        thread.join(timeout=2)
        for stack in stacks:
            stack.stop()
        for notifier in notifiers:
            notifier.stop()
        for bus in buses:
            bus.shutdown()
        pipe.close()


class PipePduConnection(BaseConnection):
    """Six BaseConnection methods over bounded local process IPC (not JSON-RPC)."""
    def __init__(self, pipe):
        super().__init__("planning-spike")
        self.pipe, self.opened = pipe, False
    def call(self, op, arg=None):
        self.pipe.send((op, arg))
        if not self.pipe.poll(3):
            raise TimeoutException("spike IPC deadline")
        return self.pipe.recv()
    def open(self):
        self.opened = True
        return self
    def close(self):
        self.opened = False
    def is_open(self):
        return self.opened
    def specific_send(self, payload, timeout=None):
        self.call("send", bytes(payload))
    def specific_wait_frame(self, timeout=None):
        data = self.call("recv", min(timeout if timeout is not None else 1, 1))
        if data is None:
            raise TimeoutException("spike PDU timeout")
        return data
    def empty_rxqueue(self):
        self.call("drain")


def can_probe():
    parent, child = mp.get_context("spawn").Pipe()
    proc = mp.get_context("spawn").Process(target=can_daemon, args=(child,))
    proc.start()
    assert parent.poll(10) and parent.recv()["ready"]
    conn = PipePduConnection(parent)
    result = {}
    try:
        samples = []
        for i in range(500):
            t = time.perf_counter_ns()
            assert conn.call("ping", i) == i
            samples.append((time.perf_counter_ns()-t)/1e6)
        result["pipe_ping_rtt"] = summary(samples)
        cfg = config()
        pending = []
        cfg["nrc78_callback"] = lambda: pending.append(time.monotonic_ns())
        with Client(conn, config=cfg) as client:
            samples = []
            for _ in range(100):
                t = time.perf_counter_ns()
                response = client.read_data_by_identifier(0xF190)
                assert response.service_data.values[0xF190] == VIN.decode()
                samples.append((time.perf_counter_ns()-t)/1e6)
            result["ipc_isotp_uds_roundtrip"] = summary(samples)
            t = time.perf_counter_ns()
            response = client.read_data_by_identifier(0xF191)
            assert response.service_data.values[0xF191] == VIN.decode()
            assert len(pending) == 2
            result["pending"] = {"nrc78_count": len(pending), "elapsed_ms": (time.perf_counter_ns()-t)/1e6}
            t = time.perf_counter_ns()
            try:
                client.read_data_by_identifier(0xF192)
                raise AssertionError("P2 timeout was expected")
            except TimeoutException:
                result["p2_timeout"] = {"observed": True, "elapsed_ms": (time.perf_counter_ns()-t)/1e6}
        result.update(conn.call("close"))
        assert not result["isotp_errors"]
        assert result["frame_counts_on_bus_rx"]["FC"] >= 101
    finally:
        proc.join(timeout=5)
        if proc.is_alive():
            proc.terminate()
            proc.join()
        parent.close()
    assert proc.exitcode == 0, proc.exitcode
    return result


def recv_exact(sock, size):
    data = b""
    while len(data) < size:
        chunk = sock.recv(size-len(data))
        if not chunk:
            raise EOFError("entity peer closed")
        data += chunk
    return data


def packet(kind, payload):
    return struct.pack("!BBHI", 2, 0xFD, kind, len(payload)) + payload


def doip_probe():
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(5)
    port = listener.getsockname()[1]
    events, failures = [], []
    def entity():
        try:
            with listener.accept()[0] as sock:
                sock.settimeout(3)
                sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                while True:
                    try:
                        version, inverse, kind, size = struct.unpack("!BBHI", recv_exact(sock, 8))
                    except EOFError:
                        return
                    assert version ^ inverse == 0xFF and size <= 4096
                    payload = recv_exact(sock, size)
                    events.append({"kind": hex(kind), "payload_hex": payload.hex()})
                    if kind == 0x0005:
                        tester = struct.unpack("!H", payload[:2])[0]
                        assert payload[2] == 0
                        sock.sendall(packet(0x0006, struct.pack("!HHBI", tester, 0x1000, 0x10, 0)))
                    elif kind == 0x8001:
                        source, target = struct.unpack("!HH", payload[:4])
                        assert target == 0x1001
                        ack = packet(0x8002, struct.pack("!HHB", target, source, 0))
                        if payload[4:] == bytes.fromhex("22F192"):
                            sock.sendall(ack)
                            continue  # ACK is not a UDS response; test P2 enforcement.
                        assert payload[4:] == REQUEST
                        response = packet(0x8001, struct.pack("!HH", target, source) + RESPONSE)
                        # Coalescing ACK and response tests the library stream parser.
                        sock.sendall(ack + response)
                    else:
                        raise AssertionError(hex(kind))
        except Exception as exc:
            failures.append(repr(exc))
    thread = threading.Thread(target=entity)
    thread.start()
    client = None
    try:
        t = time.perf_counter_ns()
        client = DoIPClient("127.0.0.1", 0x1001, tcp_port=port, client_logical_address=0x0E00)
        activation_ms = (time.perf_counter_ns()-t)/1e6
        conn = DoIPClientUDSConnector(client, close_connection=False)
        cfg = config()
        with Client(conn, config=cfg) as uds_client:
            samples = []
            for _ in range(100):
                t = time.perf_counter_ns()
                response = uds_client.read_data_by_identifier(0xF190)
                assert response.service_data.values[0xF190] == VIN.decode()
                samples.append((time.perf_counter_ns()-t)/1e6)
            t = time.perf_counter_ns()
            try:
                uds_client.read_data_by_identifier(0xF192)
                raise AssertionError("silent DoIP ECU must time out")
            except TimeoutException:
                silent_timeout = {"configured_p2_ms": 50,
                                  "elapsed_ms": (time.perf_counter_ns()-t)/1e6}
        return {"bind": "127.0.0.1", "port": port, "activation_ms": activation_ms,
                "uds_roundtrip": summary(samples), "routing_activation_count": sum(e["kind"] == "0x5" for e in events),
                "diagnostic_count": sum(e["kind"] == "0x8001" for e in events),
                "first_events": events[:2], "silent_response_timeout": silent_timeout,
                "failures": failures}
    finally:
        if client is not None:
            client.close()
        thread.join(timeout=5)
        listener.close()
        assert not thread.is_alive() and not failures, failures


if __name__ == "__main__":
    logging.basicConfig(level=logging.CRITICAL)
    result = {"checked_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "platform": platform.platform(), "python": platform.python_version(),
              "versions": {name: importlib.metadata.version(name) for name in
                           ("udsoncan", "doipclient", "can-isotp", "python-can")},
              "request_hex": REQUEST.hex(), "response_hex": RESPONSE.hex(),
              "can": can_probe(), "doip": doip_probe()}
    path = Path(__file__).parent / "evidence" / "roundtrip.json"
    path.parent.mkdir(exist_ok=True)
    path.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
