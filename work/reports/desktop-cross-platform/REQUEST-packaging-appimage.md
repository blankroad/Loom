# REQUEST (CI → ROOT / packaging): AppImage sidecar check — cause is patchelf RUNPATH, not strip

From: CI worker, Docker Ubuntu 22.04 arm64 run (`/tmp/loom-ci-ubuntu/out/05-dist.log`, step 05 rc=1).

## Finding

`bundle: appimage:sidecar FAIL sha256 bfce1276… != staged 836863dd…`. The deb sidecar check PASSed (byte-identical).

Inspection of the produced `Loom_0.1.0_aarch64.AppImage` (`--appimage-extract`, same cache volume):

| | staged `loomd-aarch64-unknown-linux-gnu` | AppImage `usr/bin/loomd` |
|---|---|---|
| sha256 | 836863dd3ea3a59e78adf6a49fde7b1bdca8ef998b19c490039768d60afb1069 | bfce127679787f42964a6cb6a88ff612744dec999d2065ea18a14c84681b85aa |
| size | 23 984 192 | 23 994 993 (+10 801) |
| `file` | not stripped | **not stripped** |
| RUNPATH | none | **`$ORIGIN/../lib`** (also set on `loom-desktop`) |
| LOAD segments | 2 | 3 (patchelf relocates `.dynamic`/`.dynstr`/`.dynsym`) |
| `.text` / `.rodata` / `.data` / `.data.rel.ro` sha256 | — | **identical** |
| GNU build-id | a3356944fb69a4d61af7f7b4770aa7dd39e31923 | **identical** |
| `package.mjs verify` (embedded /ui vs ui/dist) | PASS | **PASS** (6/6 files, CSP ok) |

So linuxdeploy **patchelf-adds a RUNPATH** to every ELF in `usr/bin`. It does not strip, so `NO_STRIP=true` will
**not** restore byte identity. (`strip` and `strip --strip-all` of the staged binary do not reproduce the
AppImage bytes either.)

## Suggested fix (packaging owner decides; CI does not touch packaging files)

Use an AppImage-only identity rule that mirrors the macOS signing rule (LC_UUID + `__text`/`__const`):

1. GNU build-id equal, and sha256 of `.text`, `.rodata`, `.data.rel.ro`, `.data` equal to the staged binary.
   Use portable Node ELF section parsing (no binutils needed), or `objcopy --only-section` when available.
2. The only allowed dynamic-section difference is a `DT_RUNPATH` of `$ORIGIN/../lib`.
3. `verify` of the extracted `usr/bin/loomd` (already implemented) must PASS.

Keep the byte-equality rule for deb, NSIS and app. If you keep `NO_STRIP=true` as a default, do so for linuxdeploy's
strip of bundled libs, not as the fix for this check.

CI reruns only the packaging steps on the cached volume once packaging marks the fix READY.

## Update — NO_STRIP tested, ROOT decision

`NO_STRIP=true node desktop/scripts/package.mjs bundle --bundles appimage` (cached volume, staged `check` rc=0):
bundle rc=1, `appimage:sidecar FAIL`. The AppImage `loomd` is **again sha256 bfce1276…** (same bytes as without
NO_STRIP), with RUNPATH `$ORIGIN/../lib`. NO_STRIP cannot preserve the bytes.

ROOT decision (2026-11-15): Ubuntu release path is **`.deb` only** (packaging owner changes the Linux default).
AppImage is documented as experimental/unverified, not supported. The strict identical-byte checks stay, and no
ELF-canonical validator is to be built. The proposal above is withdrawn.
