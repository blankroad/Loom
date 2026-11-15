//! Launch configuration: which Loom home, which daemon binary, optional UI dir.
//!
//! Resolution is pure (env lookup + executable path injected) so it is unit-testable.
//! - home: `LOOM_HOME`, else `$HOME/.loom`, else `%USERPROFILE%\.loom` (same order as the `loom`
//!   CLI and `loomd`; a Windows app started from Explorer/Start normally has no `HOME`). Empty
//!   values count as unset.
//! - daemon binary: `LOOM_DESKTOP_DAEMON_BIN` (exact, absolute), else `loomd` (`loomd.exe` on
//!   Windows) next to the app executable, where Tauri's `externalBin` places the bundled daemon:
//!   `Loom.app/Contents/MacOS/loomd`, `<install dir>\loomd.exe`, `/usr/bin/loomd`,
//!   `$APPDIR/usr/bin/loomd` (AppImage; see `appimage`). Never searched on `PATH`.
//! - UI dir: `LOOM_DESKTOP_UI_DIR` (absolute) is passed to `loomd --ui-dir` only when this app
//!   starts the daemon; default `None` = the daemon's embedded UI.
//! - startup timeout: `LOOM_DESKTOP_STARTUP_TIMEOUT_MS`, clamped to 1..=120 s, default 20 s.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ENV_DAEMON_BIN: &str = "LOOM_DESKTOP_DAEMON_BIN";
pub const ENV_UI_DIR: &str = "LOOM_DESKTOP_UI_DIR";
pub const ENV_TIMEOUT_MS: &str = "LOOM_DESKTOP_STARTUP_TIMEOUT_MS";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Bundled daemon file name for the platform this app was built for.
pub const DAEMON_FILE_NAME: &str = if cfg!(windows) { "loomd.exe" } else { "loomd" };

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchConfig {
    pub home: PathBuf,
    pub daemon_bin: PathBuf,
    pub ui_dir: Option<PathBuf>,
    pub startup_timeout: Duration,
}

impl LaunchConfig {
    pub fn connect_options(&self) -> loom_desktop_host::ConnectOptions {
        loom_desktop_host::ConnectOptions {
            home: self.home.clone(),
            daemon_bin: self.daemon_bin.clone(),
            ui_dir: self.ui_dir.clone(),
            startup_timeout: self.startup_timeout,
            daemon_env: Vec::new(),
        }
    }
}

fn non_empty(v: Option<OsString>) -> Option<OsString> {
    v.filter(|s| !s.is_empty())
}

fn absolute(name: &str, v: OsString) -> Result<PathBuf, String> {
    let p = PathBuf::from(v);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(format!("{name} must be an absolute path (got {})", p.display()))
    }
}

pub fn resolve(
    env: impl Fn(&str) -> Option<OsString>,
    current_exe: Option<&Path>,
) -> Result<LaunchConfig, String> {
    let home = match non_empty(env("LOOM_HOME")) {
        Some(h) => PathBuf::from(h),
        None => match non_empty(env("HOME")).or_else(|| non_empty(env("USERPROFILE"))) {
            Some(h) => PathBuf::from(h).join(".loom"),
            None => return Err("none of LOOM_HOME, HOME or USERPROFILE is set".into()),
        },
    };
    let daemon_bin = match non_empty(env(ENV_DAEMON_BIN)) {
        Some(v) => absolute(ENV_DAEMON_BIN, v)?,
        None => current_exe
            .and_then(Path::parent)
            .map(|dir| dir.join(DAEMON_FILE_NAME))
            .ok_or_else(|| {
                format!("cannot locate the bundled loomd; set {ENV_DAEMON_BIN} to an absolute path")
            })?,
    };
    let ui_dir = non_empty(env(ENV_UI_DIR))
        .map(|v| absolute(ENV_UI_DIR, v))
        .transpose()?;
    let startup_timeout = match non_empty(env(ENV_TIMEOUT_MS)) {
        None => DEFAULT_TIMEOUT,
        Some(v) => {
            let ms: u64 = v
                .to_str()
                .and_then(|s| s.trim().parse().ok())
                .ok_or_else(|| format!("{ENV_TIMEOUT_MS} must be a number of milliseconds"))?;
            Duration::from_millis(ms.clamp(1_000, 120_000))
        }
    };
    Ok(LaunchConfig {
        home,
        daemon_bin,
        ui_dir,
        startup_timeout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| m.get(k).cloned()
    }

    const EXE: &str = "/Applications/Loom.app/Contents/MacOS/loom-desktop";

    #[test]
    fn defaults_use_home_dot_loom_and_bundled_daemon() {
        let c = resolve(env_of(&[("HOME", "/Users/u")]), Some(Path::new(EXE))).unwrap();
        assert_eq!(c.home, PathBuf::from("/Users/u/.loom"));
        assert_eq!(
            c.daemon_bin,
            Path::new("/Applications/Loom.app/Contents/MacOS").join(DAEMON_FILE_NAME)
        );
        assert_eq!(c.ui_dir, None);
        assert_eq!(c.startup_timeout, DEFAULT_TIMEOUT);
    }

    /// An absolute path on the platform running the test (`C:` drive prefix on Windows).
    fn abs(p: &str) -> String {
        if cfg!(windows) { format!("C:{p}") } else { p.to_string() }
    }

    #[test]
    fn loom_home_and_explicit_binary_win() {
        let (home, bin, ui) = (abs("/tmp/iso"), abs("/opt/loom/loomd"), abs("/opt/loom/ui"));
        let c = resolve(
            env_of(&[
                ("HOME", "/Users/u"),
                ("LOOM_HOME", &home),
                (ENV_DAEMON_BIN, &bin),
                (ENV_UI_DIR, &ui),
                (ENV_TIMEOUT_MS, "5000"),
            ]),
            Some(Path::new(EXE)),
        )
        .unwrap();
        assert_eq!(c.home, PathBuf::from(home));
        assert_eq!(c.daemon_bin, PathBuf::from(bin));
        assert_eq!(c.ui_dir, Some(PathBuf::from(ui)));
        assert_eq!(c.startup_timeout, Duration::from_secs(5));
    }

    #[test]
    fn empty_values_are_ignored() {
        let c = resolve(
            env_of(&[
                ("HOME", "/h"),
                ("LOOM_HOME", ""),
                (ENV_DAEMON_BIN, ""),
                (ENV_UI_DIR, ""),
            ]),
            Some(Path::new(EXE)),
        )
        .unwrap();
        assert_eq!(c.home, PathBuf::from("/h/.loom"));
        assert!(c.daemon_bin.ends_with(Path::new("MacOS").join(DAEMON_FILE_NAME)));
    }

    #[test]
    fn relative_paths_are_rejected() {
        let e = resolve(env_of(&[("HOME", "/h"), (ENV_DAEMON_BIN, "loomd")]), None).unwrap_err();
        assert!(e.contains(ENV_DAEMON_BIN), "{e}");
        let e = resolve(
            env_of(&[("HOME", "/h"), (ENV_UI_DIR, "ui/dist")]),
            Some(Path::new(EXE)),
        )
        .unwrap_err();
        assert!(e.contains(ENV_UI_DIR), "{e}");
    }

    /// Windows GUI launch: no `HOME`, only `USERPROFILE` (same fallback as `loomd`/`loom`).
    #[test]
    fn userprofile_is_the_fallback_after_home() {
        let c = resolve(env_of(&[("USERPROFILE", "/win/u")]), Some(Path::new(EXE))).unwrap();
        assert_eq!(c.home, PathBuf::from("/win/u/.loom"));
        let c = resolve(
            env_of(&[("HOME", "/h"), ("USERPROFILE", "/win/u")]),
            Some(Path::new(EXE)),
        )
        .unwrap();
        assert_eq!(c.home, PathBuf::from("/h/.loom"), "HOME wins, as in the CLI");
        let c = resolve(
            env_of(&[("HOME", ""), ("USERPROFILE", "/win/u")]),
            Some(Path::new(EXE)),
        )
        .unwrap();
        assert_eq!(c.home, PathBuf::from("/win/u/.loom"));
    }

    #[test]
    fn bundled_daemon_name_has_the_platform_suffix() {
        let c = resolve(env_of(&[("HOME", "/h")]), Some(Path::new(EXE))).unwrap();
        let name = c.daemon_bin.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, format!("loomd{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(DAEMON_FILE_NAME, name);
    }

    #[test]
    fn missing_home_and_exe_are_errors() {
        assert!(resolve(env_of(&[]), Some(Path::new(EXE))).is_err());
        assert!(resolve(env_of(&[("HOME", "/h")]), None).is_err());
    }

    #[test]
    fn timeout_is_clamped_and_validated() {
        let t = |v: &str| {
            resolve(
                env_of(&[("HOME", "/h"), (ENV_TIMEOUT_MS, v)]),
                Some(Path::new(EXE)),
            )
            .map(|c| c.startup_timeout)
        };
        assert_eq!(t("1").unwrap(), Duration::from_secs(1));
        assert_eq!(t("999999999").unwrap(), Duration::from_secs(120));
        assert!(t("soon").is_err());
    }
}
