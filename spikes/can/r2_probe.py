"""R2 review evidence: software send timing and BLF close/abrupt-exit behavior."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import threading
import time

import can

from r1_probe import stats, stress

ROOT = Path(__file__).resolve().parent
N_FRAMES = 100000


def write_blf(path, abrupt=False):
    writer = can.BLFWriter(path)
    begin = time.perf_counter()
    for i in range(N_FRAMES):
        writer.on_message_received(can.Message(
            timestamp=1794220000 + i / 5000,
            arbitration_id=0x100 + i % 32,
            is_extended_id=False, channel=0,
            data=i.to_bytes(4, "little") + b"\x01\x02\x03\x04"))
    if abrupt:
        os._exit(0)  # Intentionally skip Python/BLF close in this probe child only.
    writer.stop()
    return time.perf_counter() - begin


def read_count(path):
    count = 0
    error = None
    try:
        with can.BLFReader(path) as reader:
            for _ in reader:
                count += 1
    except Exception as exc:
        error = str(exc)
    return {"readable_frames": count, "missing_frames": N_FRAMES - count,
            "read_error": error, "bytes": path.stat().st_size}


def send_timing(loaded):
    stop = threading.Event()
    worker = None
    if loaded:
        worker = threading.Thread(target=stress, args=(stop,))
        worker.start()
    bus = can.Bus(interface="virtual", channel="r2-send-hook", ignore_config=True)
    original = bus.send
    observations = {0x100: [], 0x101: []}

    def observed(msg, timeout=None):
        observations[msg.arbitration_id].append(time.perf_counter_ns())
        return original(msg, timeout=timeout)

    bus.send = observed
    tasks = []
    try:
        for identifier in observations:
            tasks.append(bus.send_periodic(can.Message(arbitration_id=identifier,
                is_extended_id=False, data=b"\x01"), .01))
        time.sleep(1.5)
    finally:
        for task in tasks:
            task.stop()
        bus.shutdown()
        stop.set()
        if worker:
            worker.join()
    result = {}
    for identifier, times in observations.items():
        intervals = [(b - a) / 1e6 for a, b in zip(times, times[1:])]
        result[hex(identifier)] = {"interval_ms": stats(intervals),
            "absolute_interval_error_ms": stats([abs(x - 10) for x in intervals])}
    return result


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "--abrupt-child":
        write_blf(ROOT / "r2-abrupt.blf", abrupt=True)
        raise RuntimeError("abrupt child unexpectedly returned")
    result = {"environment": {"python": sys.version, "platform": platform.platform(),
               "python_can": can.__version__},
              "method": "Single run; software send hook, two 10ms tasks; not wire timing or Windows evidence."}
    for name in ["r1-sol.md", "r1-opus.md"]:
        p = ROOT.parent.parent / "docs" / "plan" / "can" / name
        result.setdefault("r1_sha256", {})[name] = hashlib.sha256(p.read_bytes()).hexdigest()
    result["send_idle"] = send_timing(False)
    result["send_gil_decode_load"] = send_timing(True)
    normal = ROOT / "r2-closed.blf"
    elapsed = write_blf(normal)
    result["blf_closed"] = dict(read_count(normal), write_close_seconds=elapsed,
                                 frames_per_second=N_FRAMES / elapsed)
    subprocess.run([sys.executable, str(Path(__file__).resolve()), "--abrupt-child"], check=True)
    result["blf_abrupt"] = read_count(ROOT / "r2-abrupt.blf")
    result["blf_limitations"] = "No fsync/power-loss guarantee; no full receive/IPC/UI pipeline; ordinary disk and zlib; no 10s/4MB explicit flush."
    (ROOT / "r2-results.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
