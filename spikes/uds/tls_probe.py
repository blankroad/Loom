"""Reproduce the pinned doipclient default verified-TLS configuration failure.

A loopback TCP acceptor is sufficient: failure precedes the TLS handshake.
No certificate verification is disabled, and no real ECU is contacted.
"""
import json
import socket
import ssl
import threading
import time
from pathlib import Path

from doipclient import DoIPClient


def probe():
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(3)
    port = listener.getsockname()[1]

    def accept():
        try:
            with listener.accept()[0] as conn:
                conn.settimeout(3)
                try:
                    conn.recv(1024)
                except socket.timeout:
                    pass
        finally:
            listener.close()

    worker = threading.Thread(target=accept)
    worker.start()
    result = {"checked_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "library": "doipclient 1.3.0", "openssl": ssl.OPENSSL_VERSION,
              "case": "use_secure=True to loopback listener; fails before TLS handshake"}
    try:
        client = DoIPClient("127.0.0.1", 0x1001, tcp_port=port, use_secure=True)
        client.close()
        result["unexpected_success"] = True
    except Exception as exc:
        result.update(exception_type=type(exc).__name__, exception=str(exc))
    worker.join(timeout=4)
    assert not worker.is_alive()
    assert result.get("exception_type") == "ValueError", result
    assert result["exception"] == "check_hostname requires server_hostname", result
    return result


if __name__ == "__main__":
    result = probe()
    dest = Path(__file__).parent / "evidence" / "tls-default-probe.json"
    dest.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
