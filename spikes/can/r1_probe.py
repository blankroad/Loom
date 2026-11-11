"""R1 planning evidence only; in-process virtual CAN is not a hardware timing test."""
import importlib.metadata as md
import json
import math
import platform
import statistics
import sys
import threading
import time
from pathlib import Path

import can
import cantools
import isotp

DBC = '''VERSION ""
NS_ :
BS_:
BU_: ECU
BO_ 256 Engine: 8 ECU
 SG_ RPM : 0|16@1+ (0.25,0) [0|16383.75] "rpm" ECU
 SG_ Temp : 16|8@1+ (1,-40) [-40|215] "degC" ECU
 SG_ State : 24|8@1+ (1,0) [0|255] "" ECU
'''
db = cantools.database.load_string(DBC, database_format="dbc")
message = db.get_message_by_name("Engine")
encoded = message.encode({"RPM": 1500, "Temp": 80, "State": 1})


def stats(values):
    ordered = sorted(values)
    return {"n": len(values), "p50": statistics.median(values),
            "p95": ordered[math.ceil(len(values) * .95) - 1],
            "p99": ordered[math.ceil(len(values) * .99) - 1], "max": max(values)}


def stress(stop):
    while not stop.is_set():
        for _ in range(1000):
            message.decode(encoded, decode_choices=False)


def cyclic(period, seconds):
    tx = can.Bus(interface="virtual", channel="r1-periodic", ignore_config=True)
    rx = can.Bus(interface="virtual", channel="r1-periodic", ignore_config=True)
    task = tx.send_periodic(can.Message(arbitration_id=256, data=encoded,
                                       is_extended_id=False), period)
    times = []
    deadline = time.perf_counter() + seconds
    try:
        while time.perf_counter() < deadline:
            if rx.recv(timeout=.1) is not None:
                times.append(time.perf_counter())
    finally:
        task.stop()
        tx.shutdown()
        rx.shutdown()
    intervals = [(b - a) * 1000 for a, b in zip(times, times[1:])]
    return {"period_ms": period * 1000, "interval_ms": stats(intervals),
            "absolute_interval_error_ms": stats([abs(v - period * 1000) for v in intervals])}


def transport(n=50):
    buses = [can.Bus(interface="virtual", channel="r1-isotp", ignore_config=True)
             for _ in range(2)]
    first_frames = []
    flow_controls = []
    for bus in buses:
        original = bus.send

        def timed_send(msg, timeout=None, original=original):
            now = time.perf_counter_ns()
            if msg.data and msg.data[0] >> 4 == 1:
                first_frames.append(now)
            if msg.data and msg.data[0] >> 4 == 3:
                flow_controls.append(now)
            return original(msg, timeout=timeout)

        bus.send = timed_send
    notifiers = [can.Notifier(bus, [], timeout=.01) for bus in buses]
    params = {"stmin": 0, "blocksize": 0, "blocking_send": True,
              "rx_flowcontrol_timeout": 1000, "rx_consecutive_frame_timeout": 1000}
    errors = []
    stacks = [isotp.NotifierBasedCanStack(bus=bus, notifier=notifier,
              address=isotp.Address(isotp.AddressingMode.Normal_11bits,
              txid=tx, rxid=rx), params=params,
              error_handler=lambda e: errors.append(str(e)))
              for bus, notifier, tx, rx in zip(buses, notifiers, [0x700, 0x708], [0x708, 0x700])]
    for stack in stacks:
        stack.start()
    stopped = threading.Event()
    server_errors = []

    def echo():
        while not stopped.is_set():
            data = stacks[1].recv(block=True, timeout=.1)
            if data is not None:
                try:
                    stacks[1].send(data, send_timeout=2)
                except Exception as exc:
                    server_errors.append(str(exc))

    server = threading.Thread(target=echo)
    server.start()
    payload = bytes(range(256))
    rtts = []
    try:
        for _ in range(n):
            begin = time.perf_counter_ns()
            stacks[0].send(payload, send_timeout=2)
            response = stacks[0].recv(block=True, timeout=2)
            if response != payload:
                raise RuntimeError("PDU round trip mismatch or timeout")
            rtts.append((time.perf_counter_ns() - begin) / 1e6)
    finally:
        stopped.set()
        server.join(timeout=3)
        for stack in stacks:
            stack.stop()
        for notifier in notifiers:
            notifier.stop()
        for bus in buses:
            bus.shutdown()
    if len(first_frames) != len(flow_controls):
        raise RuntimeError("FF/FC sample mismatch")
    return {"pdu_bytes": 256, "stmin": 0, "blocksize": 0,
            "round_trip_ms": stats(rtts),
            "ff_send_to_fc_send_ms": stats([(b - a) / 1e6 for a, b in zip(first_frames, flow_controls)]),
            "errors": errors + server_errors}


def main():
    result = {"environment": {"python": sys.version, "platform": platform.platform(),
              "machine": platform.machine(), "cpu": platform.processor(),
              "packages": {n: md.version(n) for n in ["python-can", "can-isotp", "cantools"]}},
              "method": "One process, two virtual buses, wall-clock perf_counter; no hardware or arbitration; one run per condition."}
    for mode in ["idle", "gil_decode_load"]:
        stop = threading.Event()
        worker = None
        if mode != "idle":
            worker = threading.Thread(target=stress, args=(stop,))
            worker.start()
        try:
            result[mode] = {"cyclic": [cyclic(.001, .7), cyclic(.01, 1.2)],
                            "isotp": transport()}
        finally:
            stop.set()
            if worker:
                worker.join()
    n = 200000
    begin = time.perf_counter()
    for _ in range(n):
        message.decode(encoded, decode_choices=False)
    elapsed = time.perf_counter() - begin
    result["decode"] = {"frames": n, "signals": 3, "elapsed_seconds": elapsed,
                        "frames_per_second": n / elapsed}
    event = {"seq": 1, "ts": {"mono_ns": "1000000000", "wall_ns": "1794220000000000000"},
             "domain": "can", "channel": "ch0", "dir": "rx", "origin": "device",
             "payload": {"id": 256, "data_hex": encoded.hex(), "fd": False}}
    batch = [dict(event, seq=i) for i in range(200)]
    begin = time.perf_counter()
    for _ in range(2000):
        serialized = json.dumps(batch, separators=(",", ":"))
    elapsed = time.perf_counter() - begin
    result["json_batch"] = {"batch_frames": 200, "batch_bytes": len(serialized.encode()),
                            "ms_per_batch": elapsed / 2000 * 1000,
                            "estimated_bytes_per_second_at_5000fps": len(serialized.encode()) * 25,
                            "serialize_cpu_ms_per_second_at_5000fps": elapsed / 2000 * 1000 * 25,
                            "excludes": "WebSocket, fan-out, browser, decode, storage"}
    output = Path(__file__).with_name("r1-results.json")
    output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
