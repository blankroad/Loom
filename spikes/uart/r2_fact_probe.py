"""R2 planning fact checks; opens only an os.openpty-created device."""
import json
import os
import platform
import re
import subprocess
import sys
import time
from unittest.mock import patch

import serial


def main():
    result = {"os": platform.platform(), "python": platform.python_version(),
              "pyserial": serial.VERSION}
    master, slave = os.openpty()
    first = second = None
    try:
        path = os.ttyname(slave)
        first = serial.Serial(path, exclusive=True, timeout=0.01, write_timeout=0.01)
        try:
            other = serial.Serial(path, exclusive=True, timeout=0.01)
        except serial.SerialException as exc:
            result["second_exclusive"] = {"accepted": False, "error": str(exc)}
        else:
            other.close()
            result["second_exclusive"] = {"accepted": True}
        try:
            second = serial.Serial(path, timeout=0.01)
        except serial.SerialException as exc:
            result["second_default"] = {"accepted": False, "error": str(exc)}
        else:
            result["second_default"] = {"accepted": True}

        # Synthetic lower-level stall: not a hardware CTS experiment.
        with patch("serial.serialposix.termios.tcdrain", side_effect=lambda fd: time.sleep(0.08)):
            started = time.perf_counter()
            first.flush()
            result["mock_tcdrain"] = {
                "write_timeout_ms": 10,
                "flush_elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
                "synthetic": True,
            }
    finally:
        if second is not None:
            second.close()
        if first is not None:
            first.close()
        os.close(slave)
        os.close(master)

    result["regex_end_without_newline"] = bool(re.search(r"[#$] $", "root@dut:~# ", re.MULTILINE))
    result["regex_partial_then_extended"] = {
        "first_chunk_matches": bool(re.search(r"OK$", "OK", re.MULTILINE)),
        "extended_stream_matches": bool(re.search(r"OK$", "OKAY", re.MULTILINE)),
    }
    code = "import re; print('READY',flush=True); re.search(r'(a+)+$', 'a'*32+'!')"
    child = subprocess.Popen([sys.executable, "-u", "-c", code], stdout=subprocess.PIPE, text=True)
    try:
        assert child.stdout.readline().strip() == "READY"
        try:
            child.communicate(timeout=0.2)
            result["regex_adversarial"] = {"exceeded_200ms": False, "returncode": child.returncode}
        except subprocess.TimeoutExpired:
            result["regex_adversarial"] = {"exceeded_200ms": True, "child_terminated": True}
    finally:
        if child.poll() is None:
            child.kill()
        child.communicate()
    result["ring_rx_only_minutes"] = {
        "115200_8N1": round(16 * 2**20 / (115200 / 10) / 60, 3),
        "921600_8N1": round(16 * 2**20 / (921600 / 10) / 60, 3),
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
