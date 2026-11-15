#!/usr/bin/env python3
"""QA watch-viewer fixture for the browser side of parity checks (loom skill ui.md, "Isolated simulator UI
acceptance"). Only for a daemon QA itself started in its own LOOM_HOME.

  python3 -I watch_url.py <LOOM_HOME> <OUT_FILE> [--next ROUTE]

Reads the agent token from <LOOM_HOME>/run/daemon.json inside this process (never argv/env/output), requests
GET /api/v1/bootstrap/new?rights=watch on the canonical origin, and writes the one-time viewer URL to
OUT_FILE (mode 0600). The consumer (an Ego script) reads and deletes it. Prints only rights/TTL."""
import json, os, sys, urllib.parse, urllib.request

home, out = sys.argv[1], sys.argv[2]
nxt = sys.argv[sys.argv.index("--next") + 1] if "--next" in sys.argv else None
d = json.load(open(os.path.join(home, "run", "daemon.json")))
q = "rights=watch" + (f"&next={urllib.parse.quote(nxt, safe='/?=&')}" if nxt else "")
req = urllib.request.Request(f"http://127.0.0.1:{d['port']}/api/v1/bootstrap/new?{q}",
                             headers={"Authorization": f"Bearer {d['token']}", "Host": f"localhost:{d['port']}"})
r = json.load(urllib.request.urlopen(req, timeout=5))
del d
if r.get("rights") != "watch":
    sys.exit(f"refusing: daemon issued rights={r.get('rights')!r}, expected watch")
fd = os.open(out, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    f.write(r["url"])
print(json.dumps({"ok": True, "rights": r["rights"], "expires_in_ms": r.get("expires_in_ms"), "out": out}))
