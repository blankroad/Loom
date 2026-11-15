//! Linux AppImage support (pure std; compiled and unit-tested on every Unix host).
//!
//! An AppImage runs from a FUSE mount (`$APPDIR`, e.g. `/tmp/.mount_LoomXXXX`) that disappears
//! when the app process exits. The daemon is detached and outlives the app, so it must never run
//! from that mount: it would fault on its next page-in and could not re-exec itself for
//! `loomd worker` children. [`stable_daemon`] copies the bundled `loomd` once into the Loom home
//! (`<home>/desktop/bin/loomd-<fnv64>`) and the app starts that copy instead.
//!
//! A cached copy is reused only when it is a regular file (not a symlink) owned by this user,
//! mode exactly 0755, under directories (home, `desktop`, `desktop/bin`) owned by this user and not
//! group/other-writable (`desktop/bin` is created 0700), and byte-for-byte equal
//! to the bundled daemon. Otherwise it is replaced through a fresh `create_new` temp file in the
//! same directory and an atomic `rename`. The file-name hash only separates builds; it is never
//! trusted on its own.
//!
//! AppRun also points `PATH`, `XDG_DATA_DIRS`, GTK/GIO/GdkPixbuf module variables and friends into
//! `$APPDIR`. [`env_fixups`] drops those entries for processes that outlive or leave the app: the
//! daemon (and so its workers, ffmpeg, sigrok) and the default browser opened for external links.

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

/// `$APPDIR` when this process runs inside an AppImage (`APPIMAGE` and an absolute `APPDIR` set).
pub fn appdir(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    env("APPIMAGE").filter(|v| !v.is_empty())?;
    let dir = PathBuf::from(env("APPDIR").filter(|v| !v.is_empty())?);
    (dir.is_absolute() && dir.parent().is_some()).then_some(dir)
}

/// Environment changes (`None` = remove) that strip every `$APPDIR` entry from `vars`. List
/// values (`:`-separated) keep their entries outside `$APPDIR`; a variable left empty is removed.
pub fn env_fixups(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    appdir: &Path,
) -> Vec<(OsString, Option<OsString>)> {
    let mut out = Vec::new();
    for (k, v) in vars {
        let entries: Vec<&[u8]> = v.as_bytes().split(|b| *b == b':').collect();
        let inside = |e: &&[u8]| !e.is_empty() && Path::new(OsStr::from_bytes(e)).starts_with(appdir);
        if !entries.iter().any(inside) {
            continue;
        }
        let kept: Vec<&[u8]> = entries.into_iter().filter(|e| !inside(e) && !e.is_empty()).collect();
        if kept.is_empty() {
            out.push((k, None));
        } else {
            out.push((k, Some(OsString::from_vec(kept.join(&b':')))));
        }
    }
    out
}

/// [`env_fixups`] for this process's environment, or nothing outside an AppImage.
pub fn current_env_fixups() -> Vec<(OsString, Option<OsString>)> {
    match appdir(|k| std::env::var_os(k)) {
        Some(dir) => env_fixups(std::env::vars_os(), &dir),
        None => Vec::new(),
    }
}

/// FNV-1a 64 (stable across Rust releases; names cache entries only, never trusted alone).
fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// Called only after `symlink_metadata` showed a regular file inside a directory chain only this
/// user can write, so the name cannot be swapped for a link in between.
fn same_bytes(path: &Path, want: &[u8]) -> io::Result<bool> {
    let mut f = fs::File::open(path)?;
    if f.metadata()?.len() != want.len() as u64 {
        return Ok(false);
    }
    let mut got = Vec::with_capacity(want.len());
    f.read_to_end(&mut got)?;
    Ok(got == want)
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, msg)
}

/// The daemon binary to start: `src` itself unless it lives inside `appdir`, else a verified copy
/// under `<home>/desktop/bin` (see module docs).
pub fn stable_daemon(src: &Path, appdir: &Path, home: &Path) -> io::Result<PathBuf> {
    if !src.starts_with(appdir) {
        return Ok(src.to_path_buf());
    }
    let bytes = fs::read(src)?;
    let dir = home.join("desktop").join("bin");
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
    let target = dir.join(format!("loomd-{:016x}", fnv64(&bytes)));

    // Temp file first (O_CREAT|O_EXCL: never follows or reuses an existing name, link or not):
    // proves the directory is writable and tells us our own uid.
    let tmp = dir.join(format!(
        ".loomd-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&tmp)?;
    let result = (|| {
        let uid = f.metadata()?.uid();
        // Only this user may rename or replace anything on the way to the cached binary.
        for d in [home, dir.parent().unwrap_or(&dir), &dir] {
            let m = fs::symlink_metadata(d)?;
            if !m.is_dir() || m.uid() != uid || m.mode() & 0o022 != 0 {
                return Err(bad(format!(
                    "{} must be a directory owned by this user and not writable by group/others",
                    d.display()
                )));
            }
        }
        if let Ok(m) = fs::symlink_metadata(&target)
            && m.file_type().is_file()
            && m.uid() == uid
            && m.mode() & 0o7777 == 0o755
            && same_bytes(&target, &bytes)?
        {
            return Ok(false); // verified cached copy
        }
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        fs::rename(&tmp, &target)?;
        Ok(true)
    })();
    drop(f);
    match result {
        Ok(true) => Ok(target),
        Ok(false) => {
            let _ = fs::remove_file(&tmp);
            Ok(target)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir(tag: &str) -> TempDir {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("loom-appimage-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        TempDir(fs::canonicalize(&p).unwrap())
    }

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), v.into())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn appdir_requires_appimage_and_absolute_appdir() {
        let a = "/tmp/.mount_LoomAB";
        assert_eq!(appdir(env_of(&[("APPIMAGE", "/h/Loom.AppImage"), ("APPDIR", a)])), Some(a.into()));
        assert_eq!(appdir(env_of(&[("APPDIR", a)])), None);
        assert_eq!(appdir(env_of(&[("APPIMAGE", "x"), ("APPDIR", "rel")])), None);
        assert_eq!(appdir(env_of(&[("APPIMAGE", "x"), ("APPDIR", "/")])), None);
        assert_eq!(appdir(env_of(&[("APPIMAGE", ""), ("APPDIR", a)])), None);
    }

    #[test]
    fn fixups_strip_only_appdir_entries() {
        let d = Path::new("/tmp/.mount_LoomAB");
        let vars = [
            ("PATH", "/tmp/.mount_LoomAB/usr/bin:/usr/local/bin:/usr/bin"),
            ("GDK_PIXBUF_MODULE_FILE", "/tmp/.mount_LoomAB/usr/lib/loaders.cache"),
            ("XDG_DATA_DIRS", "/tmp/.mount_LoomAB/usr/share::/usr/share"),
            ("APPDIR", "/tmp/.mount_LoomAB"),
            ("HOME", "/home/u"),
            ("LOOK_ALIKE", "/tmp/.mount_LoomABC/usr/bin"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let mut f = env_fixups(vars, d);
        f.sort();
        let s = |v: &str| Some(OsString::from(v));
        assert_eq!(
            f,
            vec![
                ("APPDIR".into(), None),
                ("GDK_PIXBUF_MODULE_FILE".into(), None),
                ("PATH".into(), s("/usr/local/bin:/usr/bin")),
                ("XDG_DATA_DIRS".into(), s("/usr/share")),
            ]
        );
    }

    #[test]
    fn outside_appdir_is_used_as_is_without_copy() {
        let t = tempdir("outside");
        let src = t.0.join("loomd");
        fs::write(&src, b"bin").unwrap();
        let got = stable_daemon(&src, Path::new("/tmp/.mount_none"), &t.0.join("home")).unwrap();
        assert_eq!(got, src);
        assert!(!t.0.join("home").exists());
    }

    #[test]
    fn copies_once_then_reuses_only_a_verified_copy() {
        let t = tempdir("copy");
        let appdir = t.0.join("mount");
        fs::create_dir_all(appdir.join("usr/bin")).unwrap();
        let src = appdir.join("usr/bin/loomd");
        fs::write(&src, b"#!/bin/sh\necho daemon v1\n").unwrap();
        let home = t.0.join("home");

        let a = stable_daemon(&src, &appdir, &home).unwrap();
        assert!(a.starts_with(home.join("desktop/bin")));
        assert_eq!(fs::read(&a).unwrap(), fs::read(&src).unwrap());
        assert_eq!(fs::metadata(&a).unwrap().mode() & 0o7777, 0o755);
        assert_eq!(fs::metadata(a.parent().unwrap()).unwrap().mode() & 0o777, 0o700);
        let ino = fs::metadata(&a).unwrap().ino();

        // Verified cache: same inode reused, no temp files left behind.
        assert_eq!(stable_daemon(&src, &appdir, &home).unwrap(), a);
        assert_eq!(fs::metadata(&a).unwrap().ino(), ino);
        assert_eq!(fs::read_dir(a.parent().unwrap()).unwrap().count(), 1);

        // Same name, tampered bytes (same length) → replaced atomically.
        fs::write(&a, b"#!/bin/sh\necho daemon XX\n").unwrap();
        assert_eq!(stable_daemon(&src, &appdir, &home).unwrap(), a);
        assert_eq!(fs::read(&a).unwrap(), fs::read(&src).unwrap());

        // Wrong mode → replaced.
        fs::set_permissions(&a, fs::Permissions::from_mode(0o775)).unwrap();
        stable_daemon(&src, &appdir, &home).unwrap();
        assert_eq!(fs::metadata(&a).unwrap().mode() & 0o7777, 0o755);

        // Symlink planted at the name → replaced by a regular file, link target untouched.
        let decoy = t.0.join("decoy");
        fs::write(&decoy, b"decoy").unwrap();
        fs::remove_file(&a).unwrap();
        std::os::unix::fs::symlink(&decoy, &a).unwrap();
        stable_daemon(&src, &appdir, &home).unwrap();
        assert!(fs::symlink_metadata(&a).unwrap().file_type().is_file());
        assert_eq!(fs::read(&decoy).unwrap(), b"decoy");

        // A new build gets a new name; the old copy (maybe still running) is left alone.
        fs::write(&src, b"#!/bin/sh\necho daemon v2\n").unwrap();
        let b = stable_daemon(&src, &appdir, &home).unwrap();
        assert_ne!(a, b);
        assert!(a.exists() && b.exists());
    }

    #[test]
    fn loose_cache_directory_is_refused() {
        let t = tempdir("loose");
        let appdir = t.0.join("mount");
        fs::create_dir_all(&appdir).unwrap();
        let src = appdir.join("loomd");
        fs::write(&src, b"bin").unwrap();
        let home = t.0.join("home");
        let dir = home.join("desktop/bin");
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
        let e = stable_daemon(&src, &appdir, &home).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "temp file cleaned up");
    }
}
