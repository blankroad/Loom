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

/// The bundled launch page (Tauri asset protocol on macOS).
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

/// Open an explicit external link in the user's default browser (macOS `open`). Only
/// `http(s)` URLs ever reach here; the URL is passed as one argv element, never through a shell.
pub fn open_external(url: &Url) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("/usr/bin/open")
        .arg(url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            // reap in the background so no zombie is left behind
            std::thread::spawn(move || child.wait());
        });
    #[cfg(not(target_os = "macos"))]
    let r: std::io::Result<()> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "external links: macOS only",
    ));
    if let Err(e) = r {
        eprintln!(
            "loom-desktop: could not open external link host {:?}: {e}",
            url.host_str()
        );
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
}
