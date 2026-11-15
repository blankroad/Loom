//! UI build identity guard (read-only).
//!
//! The desktop window shows whatever UI the selected daemon embeds. A reused daemon that was
//! started by an older build keeps serving its own (old) UI even after a new app is installed.
//! Before navigating, the app compares the daemon's UI page `/ui/` (exact bytes, canonical host, no
//! redirect following) with the `index.html` embedded in its
//! own bundled loomd (staged by `desktop/scripts/stage-loomd.sh`, proven identical by
//! `verify-loomd.sh`). Vite's `index.html` names every content-hashed asset, so equal bytes mean
//! the same UI build. On a mismatch the launch page shows an actionable error; the running daemon
//! is never stopped automatically.

/// `index.html` (served at `/ui/`) of the UI embedded in this app's bundled loomd (empty if not staged).
pub const EXPECTED_INDEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui-index.html"));

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Same UI build: navigate.
    Match,
    /// This app build carries no identity (unstaged dev build): cannot check.
    Unknown,
    /// Different (or unreadable) UI: do not navigate.
    Mismatch,
}

pub fn verdict(expected: &[u8], served: Option<&[u8]>) -> Verdict {
    match served {
        _ if expected.is_empty() => Verdict::Unknown,
        Some(s) if s == expected => Verdict::Match,
        _ => Verdict::Mismatch,
    }
}

/// Korean, actionable, credential-free message for the launch page error view.
pub fn mismatch_message(pid: u32, port: u16, home: &str, detail: &str) -> String {
    let stop = stop_command(home);
    format!(
        "실행 중인 Loom 데몬이 이 앱과 다른 UI 빌드를 제공하고 있어 화면을 열지 않았습니다.\n\n\
         이 Loom 홈에서 이전 버전의 데몬(pid {pid}, 포트 {port})이 계속 실행 중입니다. \
         앱은 실행 중인 데몬을 자동으로 중지하지 않습니다.\n\n\
         해결: 진행 중인 기록·캡처를 확인한 뒤 {TERMINAL}에서\n  {stop}\n\
         을 실행하고 Retry를 누르세요. 앱이 번들된 현재 버전의 데몬을 시작합니다.\n\n\
         세부: {detail}"
    )
}

/// Korean message when the daemon's UI page could not be read (non-200, timeout): the build is
/// unknown, so it must not be called "older"; the daemon is still never stopped automatically.
pub fn unreadable_message(pid: u32, port: u16, detail: &str) -> String {
    format!(
        "실행 중인 Loom 데몬(pid {pid}, 포트 {port})의 UI 화면을 확인하지 못해 열지 않았습니다. \
         앱은 데몬이 이 앱과 같은 UI 빌드를 제공하는지 확인된 경우에만 화면을 엽니다.\n\n\
         앱은 실행 중인 데몬을 자동으로 중지하지 않으며 진행 중인 기록·캡처는 그대로 유지됩니다. 잠시 후 Retry를 누르세요. \
         계속 실패하면 데몬 로그(이 Loom 홈의 run/loomd.log)를 확인하세요.\n\n세부: {detail}"
    )
}

/// Where the user pastes [`stop_command`].
const TERMINAL: &str = if cfg!(windows) { "PowerShell" } else { "터미널" };

/// Copy-paste command that stops the daemon of `home` in the platform's default shell.
pub fn stop_command(home: &str) -> String {
    if cfg!(windows) { ps_stop_command(home) } else { sh_stop_command(home) }
}

fn sh_stop_command(home: &str) -> String {
    format!("LOOM_HOME={} loom daemon stop", sh_single_quote(home))
}

fn ps_stop_command(home: &str) -> String {
    format!("$env:LOOM_HOME={}; loom daemon stop", ps_single_quote(home))
}

/// POSIX shell single-quoting for a copy-paste command line: `'` becomes `'\''`; everything else
/// (spaces, `$`, backticks, `\`, newlines) is literal inside single quotes.
fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// PowerShell verbatim (single-quoted) string: every single-quote character — ASCII `'` and the
/// typographic U+2018..U+201B that PowerShell also treats as quotes — is doubled; `$`, backticks
/// and `\` are literal.
fn ps_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// Korean message when this app build itself carries no UI identity (release builds only).
pub fn unknown_message() -> String {
    "이 앱 빌드에 UI 빌드 식별 정보가 없어 데몬 UI가 현재 버전인지 확인할 수 없습니다. \
     `npm --prefix desktop run dist`(desktop/scripts/package.mjs; macOS는 stage-loomd.sh + build-app.sh도 가능)로 앱을 다시 빌드하세요."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts() {
        assert_eq!(verdict(b"<a>", Some(b"<a>")), Verdict::Match);
        assert_eq!(verdict(b"<a>", Some(b"<b>")), Verdict::Mismatch);
        assert_eq!(verdict(b"<a>", None), Verdict::Mismatch);
        assert_eq!(verdict(b"", Some(b"<b>")), Verdict::Unknown);
        assert_eq!(verdict(b"", None), Verdict::Unknown);
    }

    #[test]
    fn message_is_actionable_and_never_auto_stops() {
        let m = mismatch_message(4242, 47123, "/Users/u/.loom", "index.html differs");
        assert!(m.contains("pid 4242") && m.contains("포트 47123"));
        assert!(m.contains(&stop_command("/Users/u/.loom")));
        assert!(m.contains(TERMINAL));
        assert!(m.contains("자동으로 중지하지 않습니다"));
        assert!(!m.contains("#boot=") && !m.to_ascii_lowercase().contains("bearer"));
    }

    #[test]
    fn unreadable_is_not_called_an_old_build() {
        let m = unreadable_message(7, 8, "GET /ui/ returned HTTP 308");
        assert!(m.contains("pid 7") && m.contains("포트 8") && m.contains("HTTP 308"));
        assert!(m.contains("자동으로 중지하지 않으며") && m.contains("기록·캡처는 그대로 유지"));
        assert!(!m.contains("이전 버전") && !m.contains("다른 UI 빌드"));
    }

    #[test]
    fn stop_commands_quote_home_per_shell() {
        assert_eq!(
            sh_stop_command("/Users/u/O'Brien Lab/$x `y` \\z/.loom"),
            r"LOOM_HOME='/Users/u/O'\''Brien Lab/$x `y` \z/.loom' loom daemon stop"
        );
        assert_eq!(
            ps_stop_command("C:\\Users\\O'Brien\u{2019}s Lab\\$x `y`\\.loom"),
            "$env:LOOM_HOME='C:\\Users\\O''Brien\u{2019}\u{2019}s Lab\\$x `y`\\.loom'; loom daemon stop"
        );
    }

    /// The suggested command must survive a real shell: apostrophe + space (and other metacharacters)
    /// in the home path round-trip exactly into LOOM_HOME.
    #[cfg(unix)]
    #[test]
    fn stop_command_quotes_home_for_the_shell() {
        let home = "/Users/u/O'Brien Lab/$x `y` \\z/.loom";
        let m = mismatch_message(1, 2, home, "d");
        assert!(m.contains(r"LOOM_HOME='/Users/u/O'\''Brien Lab/$x `y` \z/.loom' loom daemon stop"));
        let line = m
            .lines()
            .find(|l| l.trim_start().starts_with("LOOM_HOME="))
            .expect("command line");
        let probe = line
            .trim()
            .replace(" loom daemon stop", " /usr/bin/printenv LOOM_HOME");
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&probe)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), format!("{home}\n"));
    }

    /// Windows: the suggested PowerShell line round-trips the home exactly.
    #[cfg(windows)]
    #[test]
    fn stop_command_quotes_home_for_powershell() {
        // ASCII only: console code pages would garble a typographic quote in captured stdout
        // (the U+2019 doubling is covered by `stop_commands_quote_home_per_shell`).
        let home = "C:\\Users\\O'Brien Lab\\$x `y`\\.loom";
        let line = ps_stop_command(home).replace("; loom daemon stop", "; [Console]::Out.Write($env:LOOM_HOME)");
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(&line)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), home);
    }
}
