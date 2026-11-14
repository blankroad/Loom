#!/usr/bin/env bash
# Static + identity checks of a built Loom.app. Read-only except a throwaway LOOM_HOME (verify-loomd).
#   desktop/scripts/verify-bundle.sh <path/to/Loom.app> [--json-out FILE]
# Checks: Info.plist identity, main executable, Contents/MacOS/loomd == staged sha256 (stamp),
# loomd embeds current ui/dist (verify-loomd.sh --app), arch, code signature state (reported, not
# faked), and that no credentials/discovery files/home data were bundled.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
app="${1:?usage: verify-bundle.sh <Loom.app> [--json-out FILE]}"; app="${app%/}"
json_out=""; [[ "${2:-}" == --json-out ]] && json_out="${3:?}"
stamp="$repo/desktop/src-tauri/binaries/loomd.stamp.json"
rows=() fail=0
row() { rows+=("$1|$2|$3"); [[ "$2" == FAIL ]] && fail=1; return 0; }
plist="$app/Contents/Info.plist"
pl() { /usr/libexec/PlistBuddy -c "Print :$1" "$plist" 2>/dev/null; }

[[ -f "$plist" ]] && row info-plist PASS "$plist" || { row info-plist FAIL "missing"; }
id="$(pl CFBundleIdentifier)"; [[ "$id" == local.loom.desktop ]] && row bundle-id PASS "$id" || row bundle-id FAIL "got '${id}'"
exe="$(pl CFBundleExecutable)"
[[ -n "$exe" && -x "$app/Contents/MacOS/$exe" ]] && row main-exe PASS "Contents/MacOS/$exe ($(lipo -archs "$app/Contents/MacOS/$exe" 2>/dev/null))" || row main-exe FAIL "CFBundleExecutable '${exe}' not executable"
row version INFO "$(pl CFBundleShortVersionString) min macOS $(pl LSMinimumSystemVersion)"

d="$app/Contents/MacOS/loomd"
if [[ -x "$d" ]]; then
  sha="$(shasum -a 256 "$d" | cut -c1-64)"
  want="$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1])).get("loomd_sha256",""))' "$stamp" 2>/dev/null)"
  prov="$(python3 -I -c 'import json,sys;s=json.load(open(sys.argv[1]));print(s.get("profile"),s.get("source_path"),"baseline",s.get("baseline"))' "$stamp" 2>/dev/null)"
  staged="$(dirname "$stamp")/loomd-$(rustc --print host-tuple 2>/dev/null || rustc -Vv | awk '/^host:/{print $2}')"
  if [[ "$sha" == "$want" ]]; then
    row loomd-identity PASS "sha256 $sha == stamp ($prov)"
  elif [[ -f "$staged" && "$(shasum -a 256 "$staged" | cut -c1-64)" == "$want" ]]; then
    # Tauri re-signs nested code (ad-hoc, hardened runtime). Ad-hoc signing is deterministic, so
    # re-signing the staged binary under the same name must reproduce the bundled bytes exactly.
    t="$(mktemp -d)"; cp "$staged" "$t/loomd"
    flags="$(codesign -dv "$d" 2>&1 | grep -o 'flags=0x[0-9a-f]*([^)]*)')"
    opts=(); [[ "$flags" == *runtime* ]] && opts=(--options runtime)
    codesign --force --sign - ${opts[@]+"${opts[@]}"} "$t/loomd" 2>/dev/null
    if cmp -s "$t/loomd" "$d"; then
      row loomd-identity PASS "bundled $sha == ad-hoc re-sign of staged $want ($flags; $prov)"
    else
      row loomd-identity FAIL "bundled $sha is not the staged $want (even after deterministic re-sign)"
    fi
    rm -rf "$t"
  else
    row loomd-identity FAIL "bundled $sha != staged ${want:-<no stamp>}"
  fi
  if out="$("$here/verify-loomd.sh" "$d" 2>&1)"; then row loomd-embedded-ui PASS "$(grep 'files compared' <<<"$out")"; else row loomd-embedded-ui FAIL "$(tail -3 <<<"$out" | tr '\n' ' ')"; fi
else
  row loomd-identity FAIL "Contents/MacOS/loomd missing (externalBin not bundled)"
fi

leaks="$(find "$app" \( -name daemon.json -o -name 'loomd.log' -o -name '*.journal' -o -name 'policy.toml' -o -name 'loom.toml' \) 2>/dev/null | head -5)"
[[ -z "$leaks" ]] && row no-home-data PASS "no daemon.json/logs/journals/policy in bundle" || row no-home-data FAIL "$leaks"
if grep -rIl --exclude-dir=MacOS -e '#boot=' -e 'Bearer ' "$app" >/dev/null 2>&1; then row no-credentials FAIL "boot fragment/Bearer literal in bundle resources"; else row no-credentials PASS "no '#boot=' / 'Bearer ' literals in resources"; fi

n="$(grep -c -a 'wdio-webdriver\|tauri_plugin_wdio' "$app/Contents/MacOS/$exe" 2>/dev/null || true)"
if [[ "${n:-0}" -eq 0 ]]; then row no-qa-webdriver PASS "no embedded WebDriver plugin in main executable"
elif [[ -n "${QA_ALLOW_WEBDRIVER:-}" ]]; then row qa-webdriver INFO "WebDriver plugin compiled in ($n hits): TEST BUILD (feature e2e), never distribute"
else row no-qa-webdriver FAIL "WebDriver plugin compiled in ($n hits) — test-only build must not be distributed"; fi
if cs="$(codesign --verify --deep --strict "$app" 2>&1)"; then
  row codesign PASS "valid ($(codesign -dv "$app" 2>&1 | grep -E '^Signature|^TeamIdentifier' | tr '\n' ' '))"
else
  row codesign "${QA_REQUIRE_SIGNATURE:+FAIL}${QA_REQUIRE_SIGNATURE:-WARN}" "not valid/strict: $(tr '\n' ' ' <<<"$cs" | cut -c1-160) — local unsigned/ad-hoc build; Gatekeeper will block distribution, local launch OK"
fi
row size INFO "$(du -sh "$app" | cut -f1)"

printf '%s\n' "${rows[@]}" | awk -F'|' '{printf "%-18s %-5s %s\n",$1,$2,$3}'
if [[ -n "$json_out" ]]; then
  printf '%s\n' "${rows[@]}" | python3 -I -c 'import json,sys;print(json.dumps({"app":sys.argv[1],"checks":[dict(zip(("check","result","detail"),l.rstrip("\n").split("|",2))) for l in sys.stdin]},indent=1))' "$app" >"$json_out"
fi
[[ $fail -eq 0 ]] && echo "verify-bundle: PASS" || echo "verify-bundle: FAIL"
exit $fail
