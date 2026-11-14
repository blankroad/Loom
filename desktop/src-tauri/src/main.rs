//! Loom desktop: a native macOS window hosting the real Loom UI served by `loomd`.
//!
//! Flow: the main window opens on a local launch page (bundled `frontend/`). A worker thread runs
//! `loom_desktop_host::connect` — reuse the daemon of the selected `LOOM_HOME`, or start the
//! bundled/explicit `loomd` detached — and on success navigates the same window to the fresh
//! viewer URL on the canonical `http://localhost:<port>` origin. On failure the launch page shows
//! the redacted, actionable error and a Retry button. Closing the window or quitting the app never
//! stops the daemon (the host crate spawns it detached; nothing here kills it). If the daemon
//! restarts later, Connection > Reconnect (⇧⌘R) returns to the launch page and connects afresh.
//!
//! Security: the viewer URL / token are never logged or shown; only the local launch page has IPC
//! (capabilities/launch.json, no remote URLs); navigation is limited to the app assets and the
//! exact daemon origin (see `nav`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(feature = "e2e", not(debug_assertions)))]
compile_error!("feature `e2e` (in-app WebDriver server) is test-only; never build it in release");

mod config;
mod nav;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, Runtime, State, Url, WebviewUrl, WebviewWindow};

use crate::config::LaunchConfig;
use crate::nav::Action;

const MAIN: &str = "main";

/// What the launch page renders. Never contains credentials.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
enum LaunchView {
    Connecting {
        home: String,
    },
    Failed {
        message: String,
        home: String,
        daemon_bin: String,
    },
    Connected {
        home: String,
        port: u16,
        pid: u32,
        started: bool,
    },
}

struct Launch {
    config: Result<LaunchConfig, String>,
    view: Mutex<LaunchView>,
    /// Daemon port selected by the last successful connect; gates navigation.
    port: Arc<Mutex<Option<u16>>>,
    running: AtomicBool,
}

impl Launch {
    fn home_label(&self) -> String {
        self.config
            .as_ref()
            .map(|c| c.home.display().to_string())
            .unwrap_or_default()
    }
    fn set(&self, v: LaunchView) {
        *self.view.lock().unwrap_or_else(|p| p.into_inner()) = v;
    }
    fn get(&self) -> LaunchView {
        self.view.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

#[tauri::command]
fn launch_state(launch: State<'_, Arc<Launch>>) -> LaunchView {
    launch.get()
}

/// Retry after a failure (no-op while an attempt is running or once connected; after a
/// successful connect, recovery is the native Connection > Reconnect menu item).
#[tauri::command]
fn retry(app: AppHandle, launch: State<'_, Arc<Launch>>) -> LaunchView {
    if matches!(launch.get(), LaunchView::Failed { .. }) {
        start_attempt(&app, launch.inner().clone());
    }
    launch.get()
}

fn start_attempt<R: Runtime>(app: &AppHandle<R>, launch: Arc<Launch>) {
    if launch.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let config = match &launch.config {
        Ok(c) => c.clone(),
        Err(e) => {
            launch.set(LaunchView::Failed {
                message: format!("Launch configuration error: {e}"),
                home: launch.home_label(),
                daemon_bin: String::new(),
            });
            launch.running.store(false, Ordering::SeqCst);
            return;
        }
    };
    launch.set(LaunchView::Connecting {
        home: launch.home_label(),
    });
    let app = app.clone();
    std::thread::Builder::new()
        .name("loom-connect".into())
        .spawn(move || {
            let outcome = connect_and_open(&app, &launch, &config);
            if let Err(message) = outcome {
                eprintln!("loom-desktop: connect failed: {message}");
                launch.set(LaunchView::Failed {
                    message,
                    home: config.home.display().to_string(),
                    daemon_bin: config.daemon_bin.display().to_string(),
                });
            }
            launch.running.store(false, Ordering::SeqCst);
        })
        .expect("spawn connect thread");
}

fn connect_and_open<R: Runtime>(
    app: &AppHandle<R>,
    launch: &Launch,
    config: &LaunchConfig,
) -> Result<(), String> {
    let session = loom_desktop_host::connect(&config.connect_options()).map_err(|e| e.to_string())?;
    let url = Url::parse(session.viewer_url())
        .map_err(|_| "The daemon returned an invalid viewer URL; retry.".to_string())?;
    if !nav::is_daemon_origin(&url, Some(session.port())) {
        return Err(format!(
            "The daemon viewer URL is not on the expected origin http://localhost:{}; retry.",
            session.port()
        ));
    }
    *launch.port.lock().unwrap_or_else(|p| p.into_inner()) = Some(session.port());
    eprintln!(
        "loom-desktop: {} loomd pid {} on port {} (home {})",
        if session.started() { "started" } else { "reusing" },
        session.pid(),
        session.port(),
        config.home.display()
    );
    let window = app
        .get_webview_window(MAIN)
        .ok_or_else(|| "The Loom window was closed before the daemon was ready.".to_string())?;
    launch.set(LaunchView::Connected {
        home: config.home.display().to_string(),
        port: session.port(),
        pid: session.pid(),
        started: session.started(),
    });
    window
        .navigate(url)
        .map_err(|e| format!("Could not open the Loom UI in the window: {e}"))?;
    // `session` drops here: it holds no process handle and never stops the daemon.
    Ok(())
}

/// Apply the navigation / new-window policy (see `nav`) to a webview window builder.
fn with_policy<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: AppHandle<R>,
    port: Arc<Mutex<Option<u16>>>,
) -> WebviewWindowBuilder<'a, R, M> {
    let nav_port = port.clone();
    builder
        .on_navigation(move |url| {
            let p = *nav_port.lock().unwrap_or_else(|e| e.into_inner());
            match nav::navigation_action(url, p) {
                Action::Load => true,
                _ => {
                    eprintln!(
                        "loom-desktop: denied top-level navigation (scheme {:?}, host {:?})",
                        url.scheme(),
                        url.host_str()
                    );
                    false
                }
            }
        })
        .on_new_window(move |url, features| {
            let p = *port.lock().unwrap_or_else(|e| e.into_inner());
            match nav::new_window_action(&url, p) {
                // Same-origin pop-ups (e.g. daemon artifact links) open in another app window that
                // shares the WKWebView configuration (and thus the viewer cookie) and this policy.
                Action::OpenChild => match open_child(&app, port.clone(), url, features) {
                    Some(window) => NewWindowResponse::Create { window },
                    None => NewWindowResponse::Deny,
                },
                Action::OpenExternal => {
                    nav::open_external(&url);
                    NewWindowResponse::Deny
                }
                Action::Load | Action::Deny => NewWindowResponse::Deny,
            }
        })
}

/// Create the main window on the bundled launch page.
fn create_main<R: Runtime, M: Manager<R>>(
    manager: &M,
    app: &AppHandle<R>,
    port: Arc<Mutex<Option<u16>>>,
) -> tauri::Result<WebviewWindow<R>> {
    let builder = WebviewWindowBuilder::new(manager, MAIN, WebviewUrl::App("index.html".into()))
        .title("Loom")
        .inner_size(1440.0, 900.0)
        .min_inner_size(900.0, 600.0)
        .background_color(tauri::window::Color(0x0f, 0x11, 0x15, 0xff));
    // Test-only (e2e debug builds): keep the window above others so macOS occlusion does not hide
    // the page (WebKit stops requestAnimationFrame for a fully covered window). Opt-in per run.
    #[cfg(all(feature = "e2e", debug_assertions))]
    let builder = builder.always_on_top(std::env::var_os("LOOM_DESKTOP_QA_ON_TOP").is_some_and(|v| v == "1"));
    with_policy(builder, app.clone(), port).build()
}

/// Connection > Reconnect: forget the pinned daemon origin, return the main window to the local
/// launch page and run a fresh connect (reuse or start the daemon, mint a new viewer bootstrap).
/// Needed after the daemon restarted: old viewer cookies die with it and the port may change.
fn reconnect<R: Runtime>(app: &AppHandle<R>) {
    let launch = app.state::<Arc<Launch>>().inner().clone();
    if launch.running.load(Ordering::SeqCst) {
        return; // an attempt is already in flight
    }
    *launch.port.lock().unwrap_or_else(|p| p.into_inner()) = None;
    launch.set(LaunchView::Connecting {
        home: launch.home_label(),
    });
    let shown = match app.get_webview_window(MAIN) {
        Some(w) => w.navigate(nav::launch_page_url()).map(|_| w),
        None => create_main(app, app, launch.port.clone()),
    };
    match shown {
        Ok(w) => {
            let _ = w.show();
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("loom-desktop: reconnect could not show the launch page: {e}"),
    }
    eprintln!("loom-desktop: reconnect requested");
    start_attempt(app, launch);
}

const MENU_RECONNECT: &str = "loom.reconnect";

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    let reconnect = MenuItem::with_id(app, MENU_RECONNECT, "Reconnect", true, Some("CmdOrCtrl+Shift+R"))?;
    let connection = Submenu::with_items(app, "Connection", true, &[&reconnect])?;
    menu.append(&connection)?;
    Ok(menu)
}

fn open_child<R: Runtime>(
    app: &AppHandle<R>,
    port: Arc<Mutex<Option<u16>>>,
    url: Url,
    features: tauri::webview::NewWindowFeatures,
) -> Option<WebviewWindow<R>> {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let label = format!("child-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .window_features(features)
        .title("Loom")
        .on_document_title_changed(|w, title| {
            let _ = w.set_title(&title);
        });
    with_policy(builder, app.clone(), port)
        .build()
        .map_err(|e| eprintln!("loom-desktop: could not open child window: {e}"))
        .ok()
}

fn main() {
    let config = config::resolve(|k| std::env::var_os(k), std::env::current_exe().ok().as_deref());
    let launch = Arc::new(Launch {
        view: Mutex::new(LaunchView::Connecting { home: String::new() }),
        config,
        port: Arc::new(Mutex::new(None)),
        running: AtomicBool::new(false),
    });

    let state = launch.clone();
    let builder = tauri::Builder::default();
    // Test-only (feature `e2e`, debug builds only, never in dist/release): in-app W3C WebDriver
    // server on 127.0.0.1:<port>, started only when a port is named in the environment. It drives
    // the WKWebView via WebKit script messages; no Tauri IPC capability is granted for it.
    #[cfg(all(feature = "e2e", debug_assertions))]
    let builder = match ["LOOM_DESKTOP_QA_WEBDRIVER_PORT", "TAURI_WEBDRIVER_PORT"]
        .iter()
        .find_map(|k| std::env::var(k).ok()?.trim().parse::<u16>().ok())
    {
        Some(port) => {
            eprintln!("loom-desktop: e2e WebDriver server on 127.0.0.1:{port} (test build)");
            builder.plugin(tauri_plugin_wdio_webdriver::init_with_port(port))
        }
        None => builder,
    };
    builder
        .manage(launch.clone())
        .invoke_handler(tauri::generate_handler![launch_state, retry])
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if event.id() == MENU_RECONNECT {
                reconnect(app);
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            create_main(app, &handle, state.port.clone())?;
            start_attempt(&handle, state.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Loom desktop")
        .run(|app, event| {
            // Once, on the user's explicit launch: bring the main window forward and make it key
            // (activates the app). No later focus changes except the user's own Reconnect.
            if let tauri::RunEvent::Ready = event
                && let Some(w) = app.get_webview_window(MAIN)
            {
                let _ = w.show();
                let _ = w.set_focus();
            }
        });
}
