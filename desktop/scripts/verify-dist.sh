#!/usr/bin/env bash
# Verify a distributable Loom.app (default desktop/dist/Loom.app) and write a provenance manifest.
#   desktop/scripts/verify-dist.sh [<Loom.app>] [--manifest FILE]
# Strict: valid code signature (codesign --verify --deep --strict), no QA WebDriver plugin, bundled
# loomd embeds current ui/dist (executed under a throwaway LOOM_HOME). Binary identity across signing:
# LC_UUID + sha256 of the __TEXT,__text and __TEXT,__const sections (signing does not touch them) of
# the bundled loomd vs the staged pre-sign loomd. Manifest records pre-sign and final hashes.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
app="${1:-$repo/desktop/dist/Loom.app}"; [[ "$app" == --manifest ]] && app="$repo/desktop/dist/Loom.app"
app="${app%/}"
manifest="$repo/work/reports/tauri-desktop/qa/evidence/dist-manifest.json"
[[ "${2:-}" == --manifest ]] && manifest="$3"; [[ "${1:-}" == --manifest ]] && manifest="$2"
stamp="$repo/desktop/src-tauri/binaries/loomd.stamp.json"
triple="$(rustc --print host-tuple 2>/dev/null || rustc -Vv | awk '/^host:/{print $2}')"
staged="$repo/desktop/src-tauri/binaries/loomd-$triple"
[[ -d "$app" ]] || { echo "verify-dist: no $app" >&2; exit 2; }

uuid() { dwarfdump --uuid "$1" 2>/dev/null | awk '{print $2}' | head -1; }
sect() { # sha256 of one Mach-O section's bytes
  python3 -I - "$1" "$2" "$3" <<'PY'
import struct, sys, hashlib
path, seg, sec = sys.argv[1:]
b = open(path, 'rb').read()
magic, = struct.unpack_from('<I', b, 0)
assert magic == 0xfeedfacf, 'thin 64-bit Mach-O expected'
ncmds, = struct.unpack_from('<I', b, 16); off = 32
for _ in range(ncmds):
    cmd, size = struct.unpack_from('<II', b, off)
    if cmd == 0x19:  # LC_SEGMENT_64
        segname = b[off+8:off+24].rstrip(b'\0').decode(); nsects, = struct.unpack_from('<I', b, off+64)
        p = off + 72
        for _ in range(nsects):
            sname = b[p:p+16].rstrip(b'\0').decode(); sg = b[p+16:p+32].rstrip(b'\0').decode()
            addr, sz, foff = struct.unpack_from('<QQI', b, p+32)
            if sg == seg and sname == sec:
                print(hashlib.sha256(b[foff:foff+sz]).hexdigest()); sys.exit(0)
            p += 80
    off += size
print('absent')
PY
}

QA_REQUIRE_SIGNATURE=1 "$here/verify-bundle.sh" "$app" --json-out "$manifest.bundle.json"; vb=$?
d="$app/Contents/MacOS/loomd"
ident=PASS
for s in "__TEXT __text" "__TEXT __const"; do
  a="$(sect "$staged" $s)"; b="$(sect "$d" $s)"
  [[ "$a" == "$b" && "$a" != absent ]] || ident=FAIL
  echo "verify-dist: loomd ${s// /,} staged=$a bundled=$b"
done
[[ "$(uuid "$staged")" == "$(uuid "$d")" ]] || ident=FAIL
echo "verify-dist: loomd LC_UUID staged=$(uuid "$staged") bundled=$(uuid "$d") → identity $ident"
exe="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
python3 -I - "$manifest" "$app" "$exe" "$stamp" "$staged" "$d" "$ident" "$vb" "$(uuid "$d")" "$(uuid "$app/Contents/MacOS/$exe")" <<'PY'
import json, sys, hashlib, subprocess, datetime
man, app, exe, stamp, staged, d, ident, vb, duuid, auuid = sys.argv[1:]
h = lambda p: hashlib.sha256(open(p,'rb').read()).hexdigest()
pl = lambda k: subprocess.run(['/usr/libexec/PlistBuddy','-c',f'Print :{k}',f'{app}/Contents/Info.plist'],capture_output=True,text=True).stdout.strip()
sig = subprocess.run(['codesign','-dv',app],capture_output=True,text=True).stderr
st = json.load(open(stamp))
json.dump({
  "app": app, "bundle_id": pl("CFBundleIdentifier"), "version": pl("CFBundleShortVersionString"),
  "main_executable": {"name": exe, "sha256": h(f"{app}/Contents/MacOS/{exe}"), "lc_uuid": auuid},
  "loomd": {"pre_sign_staged_sha256": h(staged), "stamp_sha256": st.get("loomd_sha256"), "bundled_sha256": h(d),
            "lc_uuid": duuid, "profile": st.get("profile"), "source": st.get("source_path"), "baseline": st.get("baseline"),
            "identity_text_const_uuid": ident},
  "ui_dist_digest": st.get("ui_dist_digest"),
  "signature": [l for l in sig.splitlines() if l.startswith(("Signature","TeamIdentifier","CodeDirectory"))],
  "verify_bundle_exit": int(vb),
  "verified_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")}, open(man,"w"), indent=2)
print("verify-dist: manifest", man)
PY
[[ $vb -eq 0 && $ident == PASS ]] && { echo "verify-dist: PASS"; exit 0; } || { echo "verify-dist: FAIL"; exit 1; }
