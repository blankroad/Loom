#!/usr/bin/env bash
# Runs INSIDE loom-ci-ubuntu2204:tauri (see run-ubuntu.sh). The repo is mounted read-only at /source.
# A filtered snapshot is rsynced into the cache volume (/cache/src-snapshot). Caches, node_modules,
# target dirs, work/ and spikes/ are never copied from the host. Every build and output path is in /cache
# or /out. Each step's real exit code is appended to /out/steps.tsv (name, rc, required|optional, secs, cmd).
# The wrapper exit code is not used as evidence. An optional step failing does not stop the run, but it is
# still a failure: it is never a PASS.
#   bash /ubuntu-build.sh           full run
#   bash /ubuntu-build.sh compare   only: hash the CURRENT /source and diff against /out/snapshot.sha256
#   bash /ubuntu-build.sh smoke     fresh container, NO rebuild: install the already-built /out/dist/*.deb and
#                                   rerun the install/identity/launch smoke + drift (steps-smoke.tsv, smoke-*.log)
set -uo pipefail
SRC=/source W=/cache/src-snapshot OUT=/out
mkdir -p "$W" "$OUT"

# Provenance file list (same rule for the snapshot and for the live /source): build inputs only.
hash_tree() { # hash_tree <root>  → "sha256  relpath" lines, sorted
  local p roots=()   # only inputs that exist (find exits 1 on a missing root, e.g. no ui/public)
  for p in Cargo.toml Cargo.lock rustfmt.toml crates ui/src ui/public ui/index.html ui/package.json ui/pnpm-lock.yaml \
      ui/vite.config.ts ui/tsconfig.json ui/tsconfig.app.json ui/tsconfig.node.json desktop; do [[ -e $1/$p ]] && roots+=("$p"); done
  (cd "$1" && find "${roots[@]}" \
      \( -path desktop/node_modules -o -path desktop/src-tauri/binaries -o -path desktop/src-tauri/gen/schemas \
         -o -path desktop/dist -o -name node_modules -o -name .DS_Store \) -prune -o -type f -print0 \
    | LC_ALL=C sort -z | xargs -0 sha256sum)
}
drift() { # compare the built snapshot with the live source tree (rc 1 = the build is stale vs the tree now)
  hash_tree "$SRC" > "$OUT/source-now.sha256"
  if diff "$OUT/snapshot.sha256" "$OUT/source-now.sha256" > "$OUT/snapshot-drift.diff"; then
    echo "snapshot == current /source ($(wc -l < "$OUT/snapshot.sha256") files)"; return 0
  fi
  echo "DRIFT: files changed since the snapshot:"; grep -E '^[<>]' "$OUT/snapshot-drift.diff" | awk '{print $1, $3}' | sort -u -k2; return 1
}
MODE=${1:-build}
if [[ $MODE == compare ]]; then drift; exit $?; fi
STEPS="$OUT/steps.tsv" PFX=""
[[ $MODE == smoke ]] && STEPS="$OUT/steps-smoke.tsv" PFX="smoke-"
: > "$STEPS"
export CARGO_HOME=/cache/cargo LOOM_STAGE_TARGET_DIR=/cache/target LOOM_DESKTOP_TARGET_DIR=/cache/desktop-target
export CI=true CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" npm_config_cache=/cache/npm
export PNPM_STORE_DIR=/cache/pnpm-store
mkdir -p "$CARGO_HOME"; [[ -e $CARGO_HOME/bin ]] || cp -a /opt/cargo/bin "$CARGO_HOME/bin" 2>/dev/null || true
export PATH=/opt/cargo/bin:$PATH

step() { # step <name> <must:1|0> cmd...
  local name=$1 must=$2; shift 2
  local t0=$SECONDS; echo "::: $name: $*"
  "$@" > "$OUT/$PFX$name.log" 2>&1; local rc=$?
  local kind=required; [[ $must -eq 1 ]] || kind=optional
  printf '%s\t%s\t%s\t%ss\t%s\n' "$name" "$rc" "$kind" "$((SECONDS - t0))" "$*" >> "$STEPS"
  echo "::: $name rc=$rc [$kind] ($((SECONDS - t0))s)"; tail -5 "$OUT/$PFX$name.log"
  if [[ $rc -ne 0 && $must -eq 1 ]]; then echo "::: required step $name failed"; exit "$rc"; fi
  return 0
}

if [[ $MODE == build ]]; then
step 00-snapshot 1 rsync -a --delete \
  --exclude=/.git/ --exclude=/target/ --exclude=/spikes/ --exclude=/work/ --exclude=/.worktrees/ \
  --exclude=node_modules/ --exclude=/ui/dist/ --exclude=/ui/test-results/ --exclude=/desktop/dist/ \
  --exclude=/desktop/src-tauri/binaries/ --exclude=/desktop/src-tauri/gen/schemas/ \
  --exclude=/.loom/ --exclude=/.cargo/ --exclude=/.cargo-home/ --exclude=/.venv/ --exclude=.DS_Store \
  "$SRC/" "$W/"
cd "$W"
# provenance of the exact snapshot that was built (dirty tree, no .git)
provenance() { hash_tree "$W" > "$OUT/snapshot.sha256" && sha256sum "$OUT/snapshot.sha256" && wc -l < "$OUT/snapshot.sha256" \
  && rustc -Vv && node --version && pnpm --version && head -2 /etc/os-release && dpkg -s libwebkit2gtk-4.1-dev | grep -E "^Version"; }
step 01-provenance 1 provenance
step 02-npm-ci 1 npm ci --prefix desktop
step 03-pnpm-install 1 pnpm --dir ui install --frozen-lockfile
step 04-packaging-tests 1 npm --prefix desktop test
step 05-dist 1 npm --prefix desktop run dist -- --out-dir "$OUT/dist"
step 06-check 1 node desktop/scripts/package.mjs check
step 07-desktop-tests 1 env CARGO_TARGET_DIR=/cache/desktop-target cargo test --locked --manifest-path desktop/Cargo.toml -p loom-desktop-host -p loom-desktop
else
  cd "$W"   # smoke: reuse the snapshot + staged sidecar of the build that produced /out/dist (no rebuild)
fi

# ---- installer smoke (container-local; root inside a throwaway --rm container) ----
# Ubuntu release path is .deb only (ROOT 2026-11-15; AppImage experimental/unverified: linuxdeploy adds RUNPATH).
deb=$(ls "$OUT"/dist/*.deb 2>/dev/null | head -1)
only_deb() { node -e 'const m=require("/out/dist/artifacts.json"); const k=m.artifacts.map(a=>a.kind);
  console.log("verified="+m.verified, "kinds="+k.join(","));
  process.exit(m.verified===true && k.length===1 && k[0]==="deb" ? 0 : 1)'; }
step 08-manifest-deb-only 1 only_deb
step 10-deb-contents 1 dpkg-deb -c "$deb"
step 11-deb-install 1 dpkg -i "$deb"
step 12-deb-verify-loomd 1 node desktop/scripts/package.mjs verify /usr/bin/loomd
step 12b-installed-identity 1 bash -c "sha256sum /usr/bin/loomd $W/desktop/src-tauri/binaries/loomd-*-linux-gnu && cmp /usr/bin/loomd $W/desktop/src-tauri/binaries/loomd-*-linux-gnu"

# The app runs as an unprivileged user (WebKitGTK refuses or degrades as root). The product's sandbox
# settings are not changed.
id loomsmoke >/dev/null 2>&1 || useradd -m -s /bin/bash loomsmoke
launch_smoke() { # launch_smoke <label> <expected daemon exe> <cmd...>: app under Xvfb, private LOOM_HOME.
  # PASS only if the app starts exactly <expected daemon exe>, /ui/ answers 200 and the daemon survives the app exit.
  local label=$1 want=$2; shift 2
  local home; home=$(mktemp -d /tmp/loom-smoke-$label.XXXX); chmod 700 "$home"; chown loomsmoke: "$home"
  runuser -u loomsmoke -- env LOOM_HOME="$home" HOME=/home/loomsmoke \
    dbus-run-session -- xvfb-run -a -s "-screen 0 1280x800x24" "$@" > "/tmp/app-$label.log" 2>&1 &
  local app=$!
  local ok=1
  for _ in $(seq 120); do [[ -s $home/run/daemon.json ]] && { ok=0; break; }; sleep 0.5; done
  sleep 3   # let the app finish bootstrap / navigation
  echo "daemon.json: $([[ $ok -eq 0 ]] && echo present || echo MISSING)"
  if [[ $ok -eq 0 ]]; then
    local dpid; dpid=$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["pid"])' "$home/run/daemon.json")
    # read as the owning user: Docker drops CAP_SYS_PTRACE, so root cannot read another user's /proc/<pid>/exe
    local exe; exe=$(runuser -u loomsmoke -- readlink "/proc/$dpid/exe" 2>/dev/null); echo "daemon pid=$dpid exe=$exe (want $want)"
    [[ $exe == "$want" ]] || { echo "FAIL: daemon is not $want"; ok=1; }
    echo "daemon parent=$(ps -o ppid= -p "$dpid" 2>/dev/null)"
    ls -la "$home/desktop/bin" 2>/dev/null || true
    port=$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["port"])' "$home/run/daemon.json")
    curl -fsS -o /dev/null -w "GET /ui/ -> %{http_code}\n" "http://localhost:$port/ui/" || ok=1
  fi
  if kill -0 "$app" 2>/dev/null && pgrep -u loomsmoke -x loom-desktop >/dev/null; then echo "app alive after start: yes"
  else echo "FAIL: app not running after start"; ok=1; fi
  # quit app; the daemon must survive the app (desktop lifecycle contract)
  pkill -TERM -f "loom-desktop" 2>/dev/null; kill $app 2>/dev/null; sleep 2
  if [[ -n ${dpid:-} ]]; then
    if kill -0 "$dpid" 2>/dev/null; then echo "daemon alive after app exit: yes"; else echo "FAIL: daemon died with the app"; ok=1; fi
    kill -TERM "$dpid" 2>/dev/null
  fi
  pkill -TERM -f -- "--journal-dir $home/" 2>/dev/null || true   # only our private home's workers
  echo "--- app.log (tail)"; tail -20 "/tmp/app-$label.log"; echo "--- loomd.log (tail)"; tail -10 "$home/run/loomd.log" 2>/dev/null
  return $ok
}
step 13-deb-launch-smoke 1 launch_smoke deb /usr/bin/loomd /usr/bin/loom-desktop
step 15-artifacts 1 bash -c 'cat /out/dist/artifacts.json && ls -la /out/dist && sha256sum /out/dist/*.deb'
step 99-snapshot-drift 1 drift
echo "::: done"; cat "$STEPS"
