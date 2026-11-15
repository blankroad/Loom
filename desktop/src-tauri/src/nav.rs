//! Navigation policy for every webview this app creates.
//!
//! - the local launch page (Tauri asset protocol) is always allowed;
//! - the selected daemon origin `http://localhost:<port>` is allowed once connected;
//! - other `http(s)` URLs never load in-app. Only an explicit new-window request (`target=_blank`,
//!   `window.open`, which WKWebView gates on a user gesture) hands them to the default browser;
//!   a top-level navigation (click without target, script, meta refresh, redirect) is just denied;
//! - every other scheme (file:, data:, javascript:, custom…) is blocked.

use tauri::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// Load in this webview.
    Allow,
    /// Hand to the system browser, do not load in-app.
    External,
    /// Refuse silently.
    Block,
}

/// True for this app's bundled assets (`tauri://localhost` on macOS, `http(s)://tauri.localhost`
/// on other platforms).
pub fn is_app_asset(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

/// True for the exact canonical daemon origin selected by this launch.
pub fn is_daemon_origin(url: &Url, daemon_port: Option<u16>) -> bool {
    match daemon_port {
        Some(port) => {
            url.scheme() == "http"
                && url.host_str() == Some("localhost")
                && url.port_or_known_default() == Some(port)
                && url.username().is_empty()
                && url.password().is_none()
        }
        None => false,
    }
}

/// What a webview does with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Load in the requesting webview.
    Load,
    /// Open another app window on the daemon origin (same policy, shared cookie store).
    OpenChild,
    /// Open in the system browser.
    OpenExternal,
    /// Refuse.
    Deny,
}

/// Top-level navigation of an existing webview: never spawns anything.
pub fn navigation_action(url: &Url, daemon_port: Option<u16>) -> Action {
    match classify(url, daemon_port) {
        Nav::Allow => Action::Load,
        Nav::External | Nav::Block => Action::Deny,
    }
}

/// New-window request (`target=_blank` / `window.open`).
pub fn new_window_action(url: &Url, daemon_port: Option<u16>) -> Action {
    if is_daemon_origin(url, daemon_port) {
        return Action::OpenChild;
    }
    match classify(url, daemon_port) {
        Nav::External => Action::OpenExternal,
        Nav::Allow | Nav::Block => Action::Deny,
    }
}

/// Download request (`<a download>`, REC/export `blob:` links, `Content-Disposition: attachment`).
/// Only content produced by the selected daemon page is saved: a `blob:` URL whose origin is the
/// daemon origin, an in-page `data:` URL while connected, or the daemon origin itself. WKWebView
/// cancels downloads outright when no handler is installed.
pub fn download_allowed(url: &Url, daemon_port: Option<u16>) -> bool {
    match url.scheme() {
        // `blob:http://localhost:<port>/<uuid>` → the path is the creator's origin URL
        "blob" => Url::parse(url.path()).is_ok_and(|inner| is_daemon_origin(&inner, daemon_port)),
        "data" => daemon_port.is_some(),
        _ => is_daemon_origin(url, daemon_port),
    }
}

/// The bundled launch page: `tauri://localhost` on macOS/Linux, `http://tauri.localhost` on
/// Windows (WebView2 custom-scheme convention used by Tauri 2).
pub fn launch_page_url() -> Url {
    #[cfg(any(target_os = "windows", target_os = "android"))]
    let s = "http://tauri.localhost/index.html";
    #[cfg(not(any(target_os = "windows", target_os = "android")))]
    let s = "tauri://localhost/index.html";
    Url::parse(s).expect("static launch page URL")
}

pub fn classify(url: &Url, daemon_port: Option<u16>) -> Nav {
    if is_app_asset(url) || is_daemon_origin(url, daemon_port) || url.as_str() == "about:blank" {
        return Nav::Allow;
    }
    match url.scheme() {
        "http" | "https" if url.host_str().is_some_and(|h| !h.is_empty()) => Nav::External,
        _ => Nav::Block,
    }
}

/// Open an explicit external link in the user's default browser. Only absolute `http(s)` URLs
/// with a host ever reach the OS, always as one argument (never through a shell or `cmd /c start`):
/// macOS `/usr/bin/open <url>`; Windows `ShellExecuteW("open", <url>)`; Linux/BSD `xdg-open <url>`
/// (system copy preferred; inside an AppImage the `$APPDIR` environment is stripped first so the
/// browser does not load the app's bundled GTK/GIO modules).
pub fn open_external(url: &Url) {
    if !is_external_link(url) {
        return;
    }
    if let Err(e) = platform_open(url) {
        eprintln!(
            "loom-desktop: could not open external link host {:?}: {e}",
            url.host_str()
        );
    }
}

/// What [`open_external`] accepts: `http(s)` with a non-empty host and no credentials.
pub fn is_external_link(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
}

#[cfg(unix)]
fn spawn_reaped(cmd: &mut std::process::Command) -> std::io::Result<()> {
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    // reap in the background so no zombie is left behind
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_open(url: &Url) -> std::io::Result<()> {
    spawn_reaped(std::process::Command::new("/usr/bin/open").arg(url.as_str()))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_open(url: &Url) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let fixups = crate::appimage::current_env_fixups();
    #[cfg(not(target_os = "linux"))]
    let fixups: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = Vec::new();
    // The distro's xdg-open first (an AppImage may ship its own copy on its PATH).
    let program = ["/usr/bin/xdg-open", "/usr/local/bin/xdg-open"]
        .into_iter()
        .map(std::path::Path::new)
        .find(|p| p.is_file())
        .map_or_else(|| std::path::PathBuf::from("xdg-open"), std::path::Path::to_path_buf);
    let mut cmd = std::process::Command::new(program);
    for (k, v) in fixups {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    spawn_reaped(cmd.arg(url.as_str()))
}

#[cfg(windows)]
fn platform_open(url: &Url) -> std::io::Result<()> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(url.as_str()));
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call; null HWND,
    // parameters and directory are documented as optional. Called on the UI thread (COM is
    // initialised there by the webview), as ShellExecute requires.
    let h = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Documented contract: a value greater than 32 means success.
    if h as isize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("ShellExecuteW returned {}", h as isize)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn app_assets_allowed_before_and_after_connect() {
        for p in [None, Some(47123)] {
            assert_eq!(classify(&u("tauri://localhost/index.html"), p), Nav::Allow);
            assert_eq!(classify(&u("http://tauri.localhost/index.html"), p), Nav::Allow);
            assert_eq!(classify(&u("about:blank"), p), Nav::Allow);
        }
    }

    #[test]
    fn daemon_origin_only_exact_port_after_connect() {
        let ui = u("http://localhost:47123/ui/#boot=abc");
        assert_eq!(classify(&ui, None), Nav::External);
        assert_eq!(classify(&ui, Some(47123)), Nav::Allow);
        assert_eq!(
            classify(&u("http://localhost:47124/ui/"), Some(47123)),
            Nav::External
        );
        // not the canonical origin
        assert_eq!(
            classify(&u("http://127.0.0.1:47123/ui/"), Some(47123)),
            Nav::External
        );
        assert_eq!(
            classify(&u("https://localhost:47123/ui/"), Some(47123)),
            Nav::External
        );
        assert_eq!(
            classify(&u("http://user:pw@localhost:47123/ui/"), Some(47123)),
            Nav::External
        );
        assert_eq!(classify(&u("http://localhost/ui/"), Some(80)), Nav::Allow);
    }

    #[test]
    fn external_links_and_blocked_schemes() {
        assert_eq!(classify(&u("https://example.com/docs"), Some(1)), Nav::External);
        for s in [
            "file:///etc/passwd",
            "data:text/html,hi",
            "javascript:alert(1)",
            "ftp://example.com/",
            "mailto:a@example.com",
            "tauri://evil/index.html",
            "loom://x",
        ] {
            assert_eq!(classify(&u(s), Some(47123)), Nav::Block, "{s}");
        }
    }

    #[test]
    fn top_level_navigation_never_opens_the_browser() {
        let p = Some(47123);
        assert_eq!(
            navigation_action(&u("http://localhost:47123/ui/"), p),
            Action::Load
        );
        assert_eq!(navigation_action(&launch_page_url(), p), Action::Load);
        assert_eq!(navigation_action(&launch_page_url(), None), Action::Load);
        // script/redirect/meta-refresh or plain link to another site: denied, nothing spawned
        assert_eq!(navigation_action(&u("https://example.com/"), p), Action::Deny);
        // a restarted daemon on another port is not followed (use Connection > Reconnect)
        assert_eq!(
            navigation_action(&u("http://localhost:50000/ui/"), p),
            Action::Deny
        );
        assert_eq!(navigation_action(&u("file:///etc/hosts"), p), Action::Deny);
    }

    #[test]
    fn new_windows_open_child_external_or_deny() {
        let p = Some(47123);
        assert_eq!(
            new_window_action(&u("http://localhost:47123/api/a"), p),
            Action::OpenChild
        );
        assert_eq!(
            new_window_action(&u("https://example.com/docs"), p),
            Action::OpenExternal
        );
        assert_eq!(
            new_window_action(&u("http://localhost:47123/ui/"), None),
            Action::OpenExternal
        );
        assert_eq!(new_window_action(&launch_page_url(), p), Action::Deny);
        assert_eq!(new_window_action(&u("about:blank"), p), Action::Deny);
        assert_eq!(new_window_action(&u("data:text/html,x"), p), Action::Deny);
        assert_eq!(new_window_action(&u("javascript:alert(1)"), p), Action::Deny);
    }

    #[test]
    fn only_plain_http_links_reach_the_os() {
        assert!(is_external_link(&u("https://example.com/docs?a=1&b=2")));
        assert!(is_external_link(&u("http://example.com/")));
        for s in [
            "https://user:pw@example.com/",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "ms-settings:display",
            "search-ms:query=x",
            "data:text/html,x",
            "mailto:a@example.com",
        ] {
            assert!(!is_external_link(&u(s)), "{s}");
        }
    }

    /// The OS sees one argument per URL (argv element / `lpFile`, never a shell): whitespace,
    /// quotes and newlines are percent-encoded by URL serialization, so the value cannot be split.
    #[test]
    fn external_url_serialization_has_no_whitespace_or_quotes() {
        let url = u("https://example.com/a b\"c'd`e?x=1&y=$(z)|w;q");
        let s = url.as_str();
        assert!(!s.contains([' ', '"', '`', '\n', '\t']), "{s}");
    }

    #[test]
    fn downloads_only_from_daemon_page() {
        let p = Some(47123);
        assert!(download_allowed(
            &u("blob:http://localhost:47123/0b6c1f2e-9a4d-4c55-8f7e-1d2c3b4a5f60"),
            p
        ));
        assert!(download_allowed(&u("http://localhost:47123/api/export?x=1"), p));
        assert!(download_allowed(&u("data:text/plain;base64,aGk="), p));
        assert!(!download_allowed(&u("blob:http://localhost:47999/0b6c"), p));
        assert!(!download_allowed(&u("blob:https://example.com/0b6c"), p));
        assert!(!download_allowed(&u("blob:tauri://localhost/0b6c"), p));
        assert!(!download_allowed(&u("https://example.com/f.bin"), p));
        assert!(!download_allowed(&u("file:///etc/hosts"), p));
        assert!(!download_allowed(&u("data:text/plain,hi"), None));
        assert!(!download_allowed(&u("blob:http://localhost:47123/0b6c"), None));
    }
}
