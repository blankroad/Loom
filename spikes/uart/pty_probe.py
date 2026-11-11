"""Planning-only POSIX PTY probe; never opens a physical UART."""
import hashlib
import json
import os
import platform
import select
import threading
import time

import serial


def main():
    master, slave = os.openpty()
    path = os.ttyname(slave)
    result = {"os": platform.platform(), "python": platform.python_version(),
              "pyserial": serial.VERSION, "port": path}
    port = serial.Serial(path, baudrate=115200, timeout=0.05,
                         write_timeout=1, exclusive=True)
    try:
        binary = bytes(range(256))
        os.write(master, binary)
        received = port.read(len(binary))
        assert received == binary, "PTY -> serial binary mismatch"
        assert port.write(binary) == len(binary)
        returned = bytearray()
        deadline = time.monotonic() + 2
        while len(returned) < len(binary) and time.monotonic() < deadline:
            if select.select([master], [], [], 0.05)[0]:
                returned.extend(os.read(master, len(binary) - len(returned)))
        assert returned == binary, "serial -> PTY binary mismatch"
        result["binary_roundtrip_bytes_each_direction"] = len(binary)

        try:
            other = serial.Serial(path, timeout=0, exclusive=True)
        except serial.SerialException as exc:
            result["second_exclusive_open"] = str(exc)
        else:
            other.close()
            result["second_exclusive_open"] = "unexpectedly accepted"

        for name, action in [
            ("send_break", lambda: port.send_break(0.02)),
            ("break_on_off", lambda: (setattr(port, "break_condition", True),
                                      setattr(port, "break_condition", False))),
            ("dtr_off", lambda: setattr(port, "dtr", False)),
            ("rts_off", lambda: setattr(port, "rts", False)),
        ]:
            started = time.perf_counter()
            try:
                action()
                result[name] = {"api": "accepted", "elapsed_ms": round(
                    (time.perf_counter() - started) * 1000, 3)}
            except (OSError, serial.SerialException) as exc:
                result[name] = {"api": "error", "detail": str(exc)}

        # PTY transport ceiling only: its baud parameter does not pace the bytes.
        payload = bytes(range(256)) * (4 * 1024 * 1024 // 256)
        errors = []
        os.set_blocking(master, False)

        def producer():
            offset = 0
            deadline = time.monotonic() + 10
            try:
                while offset < len(payload) and time.monotonic() < deadline:
                    if select.select([], [master], [], 0.05)[1]:
                        try:
                            offset += os.write(master, payload[offset:offset + 16384])
                        except BlockingIOError:
                            pass
                if offset != len(payload):
                    errors.append("producer timeout")
            except Exception as exc:
                errors.append(str(exc))

        thread = threading.Thread(target=producer, daemon=True)
        started = time.perf_counter()
        thread.start()
        received = bytearray()
        deadline = time.monotonic() + 10
        while len(received) < len(payload) and time.monotonic() < deadline:
            received.extend(port.read(min(16384, len(payload) - len(received))))
        thread.join(timeout=1)
        assert not thread.is_alive() and not errors, errors
        assert received == payload, "bulk RX mismatch or timeout"
        elapsed = time.perf_counter() - started
        result["pty_bulk"] = {"bytes": len(payload), "seconds": round(elapsed, 4),
                              "MiB_per_second": round(len(payload) / elapsed / 2**20, 2),
                              "sha256": hashlib.sha256(received).hexdigest()}
    finally:
        port.close()
        os.close(slave)
        os.close(master)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
