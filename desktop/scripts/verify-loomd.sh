#!/usr/bin/env bash
# Prove a loomd binary embeds exactly the current ui/dist (artifact identity, not mtime).
#
#   desktop/scripts/verify-loomd.sh [<loomd>]   (default: staged desktop/src-tauri/binaries/loomd-<triple>)
#   desktop/scripts/verify-loomd.sh --app <Loom.app>   (verifies <app>/Contents/MacOS/loomd)
#
# Starts `<loomd> run --port 0` under a fresh private LOOM_HOME (mktemp, 0700), waits for
# run/daemon.json, GETs every ui/dist file under http://localhost:PORT/ui/<path> and compares bytes,
# checks the CSP header, then SIGTERMs ONLY the pid it spawned (never another daemon) and removes the
# temp home. The discovery token is never printed. When the verified binary is the staged one and its
# sha256 matches loomd.stamp.json, the stamp gets "embedded_ui_verified": true.
# Exit: 0 identical, 1 mismatch/failure, 2 usage.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
triple="$(rustc --print host-tuple 2>/dev/null || rustc -Vv | awk '/^host:/{print $2}')"
stamp="$repo/desktop/src-tauri/binaries/loomd.stamp.json"
bin="$repo/desktop/src-tauri/binaries/loomd-$triple"
case "${1:-}" in
  --app) bin="${2:?--app needs a path}/Contents/MacOS/loomd" ;;
  -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
  "") ;;
  *) bin="$1" ;;
esac
[[ -x "$bin" ]] || { echo "verify-loomd: not executable: $bin" >&2; exit 2; }
dist="$repo/ui/dist"
[[ -f "$dist/index.html" ]] || { echo "verify-loomd: $dist/index.html missing" >&2; exit 2; }

home="$(mktemp -d "${TMPDIR:-/tmp}/loom-verify.XXXXXX")"; chmod 700 "$home"
pid=""
cleanup() {
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    for _ in $(seq 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$pid" 2>/dev/null && kill -KILL "$pid" 2>/dev/null || true
  fi
  # workers of our daemon carry our private journal path; reap any left behind
  pgrep -f -- "--journal-dir $home/" >/dev/null 2>&1 && pkill -TERM -f -- "--journal-dir $home/" || true
  rm -rf "$home"
}
trap cleanup EXIT

LOOM_HOME="$home" "$bin" run --port 0 >"$home/loomd.out" 2>&1 &
pid=$!
for _ in $(seq 150); do
  [[ -s "$home/run/daemon.json" ]] && break
  kill -0 "$pid" 2>/dev/null || { echo "verify-loomd: loomd exited early:" >&2; tail -20 "$home/loomd.out" >&2; exit 1; }
  sleep 0.1
done
[[ -s "$home/run/daemon.json" ]] || { echo "verify-loomd: no run/daemon.json within 15s" >&2; exit 1; }
port="$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["port"])' "$home/run/daemon.json")"
dpid="$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["pid"])' "$home/run/daemon.json")"
[[ "$dpid" == "$pid" ]] || echo "verify-loomd: note: discovery pid $dpid != spawned $pid"

total=0 bad=0
while IFS= read -r -d '' f; do
  rel="${f#"$dist"/}"
  total=$((total+1))
  if ! curl -fsS --max-time 5 "http://localhost:$port/ui/$rel" -o "$home/got" || ! cmp -s "$home/got" "$f"; then
    bad=$((bad+1)); [[ $bad -le 5 ]] && echo "verify-loomd: MISMATCH /ui/$rel" >&2
  fi
done < <(find "$dist" -type f -print0)
hdr="$(curl -fsS --max-time 5 -D - -o /dev/null "http://localhost:$port/ui/")"
csp_ok=no; grep -qi "^content-security-policy: default-src 'self'" <<<"$hdr" && csp_ok=yes

bin_sha="$(shasum -a 256 "$bin" | cut -c1-64)"
echo "verify-loomd: binary $bin sha256=$bin_sha"
echo "verify-loomd: /ui files compared=$total mismatched=$bad csp=$csp_ok port=$port"
if [[ $bad -ne 0 || $total -eq 0 || $csp_ok != yes ]]; then
  echo "verify-loomd: FAIL (binary does not embed current ui/dist)"; exit 1
fi
if [[ -f "$stamp" ]]; then
  python3 -I - "$stamp" "$bin_sha" "$total" <<'PY'
import json, sys, datetime
path, sha, n = sys.argv[1:]
s = json.load(open(path))
if s.get("loomd_sha256") == sha:
    s.update(embedded_ui_verified=True, embedded_ui_files=int(n),
             embedded_ui_verified_at=datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"))
    json.dump(s, open(path, "w"), indent=2)
    print("verify-loomd: stamp updated (embedded_ui_verified=true)")
PY
fi
echo "verify-loomd: PASS"
