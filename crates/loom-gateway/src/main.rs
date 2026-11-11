//! `loomd` binary: `loomd` / `loomd run` = the real daemon; `--fixture-replay` = UI dev server.

use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use loom_gateway::daemon::{self, Config};
use loom_gateway::home::Home;
use loom_gateway::replay::{self, ReplayConfig, Scenario, Source};
use loom_proto::event::Env;
use loom_proto::rpc::Rights;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum RightsArg {
    Watch,
    Operate,
    Approve,
}

impl From<RightsArg> for Rights {
    fn from(r: RightsArg) -> Rights {
        match r {
            RightsArg::Watch => Rights::Watch,
            RightsArg::Operate => Rights::Operate,
            RightsArg::Approve => Rights::Approve,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum EnvArg {
    Sim,
    Bench,
    Vehicle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Mode {
    /// Run the daemon (same as no argument).
    Run,
}

/// Loom daemon.
#[derive(Debug, Parser)]
#[command(name = "loomd", version)]
struct Args {
    /// `run` (default).
    #[arg(value_enum)]
    mode: Option<Mode>,
    /// Serve the contract v1 stream from generated events (`builtin`) or a fixture directory.
    #[arg(long, value_name = "DIR|builtin")]
    fixture_replay: Option<String>,
    /// Port (daemon default 47123 or OS-assigned if taken; replay default 4790; 0 = any).
    #[arg(long)]
    port: Option<u16>,
    /// Serve the web UI from this directory.
    #[arg(long)]
    ui_dir: Option<PathBuf>,
    /// Environment badge (default: `[env] env` of loom.toml, else bench).
    #[arg(long, value_enum)]
    env: Option<EnvArg>,
    /// Project policy file (default `$LOOM_HOME/loom.toml`).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Rights of viewer cookies issued by bootstrap (daemon), or of every viewer (replay).
    #[arg(long, value_enum)]
    rights: Option<RightsArg>,
    /// Extra allowed browser origin (repeatable), e.g. http://localhost:5173 for Vite.
    #[arg(long = "allow-origin")]
    allow_origin: Vec<String>,
    /// Replay: time scale of generators.
    #[arg(long, default_value_t = 1.0)]
    speed: f64,
    /// Replay: built-in scenario.
    #[arg(long, value_enum, default_value_t = Scenario::Normal)]
    scenario: Scenario,
    /// Replay: answer viewer write RPCs with simulated rows.
    #[arg(long)]
    echo_writes: bool,
}

fn runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| eprintln!("loomd: runtime: {e}"))
        .ok()
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    // `loomd worker <kind> --instance … --producer-epoch … --sock-dir …` (spawned by the gateway):
    // the worker SDK parses its own flags, so this goes before clap.
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).map(String::as_str) == Some("worker") {
        let kind = raw.get(2).cloned().unwrap_or_default();
        // returns only for an unknown kind
        let _ = loom_gateway::domains::worker_main(&kind);
        eprintln!("loomd worker: unknown worker kind {kind:?}");
        return ExitCode::from(2);
    }
    let args = Args::parse();
    let Some(rt) = runtime() else {
        return ExitCode::from(1);
    };
    match args.fixture_replay.clone() {
        Some(src) => rt.block_on(run_replay(args, src)),
        None => rt.block_on(run_daemon(args)),
    }
}

async fn run_daemon(args: Args) -> ExitCode {
    let home = match Home::from_env() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("loomd: {e}");
            return ExitCode::from(2);
        }
    };
    let mut cfg = Config::new(home);
    cfg.port = args.port;
    cfg.ui_dir = args.ui_dir;
    cfg.allow_origins = args.allow_origin;
    cfg.bootstrap_rights = args.rights.map_or(Rights::Approve, Into::into);
    cfg.env = args.env.map(|e| match e {
        EnvArg::Sim => Env::Sim,
        EnvArg::Bench => Env::Bench,
        EnvArg::Vehicle => Env::Vehicle,
    });
    cfg.config_path = args.config;
    cfg.print_url = std::io::stdout().is_terminal();
    match daemon::start(cfg).await {
        Ok(r) => {
            eprintln!(
                "loomd: listening on 127.0.0.1:{} (daemon.json in {})",
                r.addr.port(),
                r.daemon.cfg.home.run_dir().display()
            );
            let d = r.daemon.clone();
            tokio::select! {
                _ = r.server => {}
                _ = tokio::signal::ctrl_c() => eprintln!("loomd: shutting down (Ctrl-C)"),
                () = sigterm() => eprintln!("loomd: shutting down (SIGTERM)"),
                () = d.wait_shutdown() => {
                    eprintln!("loomd: shutting down (core.shutdown)");
                    // let the reply to `core.shutdown` leave the socket
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
            // Graceful: shutdown latch to workers, release them now (no orphan wait), seal the
            // journal. The user's STOP state (latch.json) is untouched.
            let summary = d.shutdown_gracefully().await;
            eprintln!("loomd: workers released {summary}");
            // Remove the discovery file only if it is still ours.
            let home = &d.cfg.home;
            if loom_gateway::home::DaemonInfo::read(home).is_ok_and(|i| i.pid == std::process::id()) {
                let _ = std::fs::remove_file(home.daemon_json());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("loomd: cannot start: {e}");
            ExitCode::from(if e.kind() == std::io::ErrorKind::AddrInUse {
                9
            } else {
                1
            })
        }
    }
}

/// SIGTERM (Unix); never resolves elsewhere.
async fn sigterm() {
    #[cfg(unix)]
    {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
            return;
        }
    }
    std::future::pending::<()>().await;
}

async fn run_replay(args: Args, src: String) -> ExitCode {
    let cfg = ReplayConfig {
        source: if src == "builtin" {
            Source::Builtin
        } else {
            Source::Dir(PathBuf::from(src))
        },
        port: args.port.unwrap_or(4790),
        speed: args.speed,
        scenario: args.scenario,
        echo_writes: args.echo_writes,
        rights: args.rights.map_or(Rights::Operate, Into::into),
        allow_origins: args.allow_origin,
    };
    match replay::serve(cfg).await {
        Ok((addr, handle)) => {
            eprintln!("loomd: fixture replay: NO AUTH, 127.0.0.1 only — not for real devices");
            eprintln!(
                "loomd: open http://localhost:{}/  (WebSocket ws://localhost:{}/rpc)",
                addr.port(),
                addr.port()
            );
            tokio::select! {
                _ = handle => {}
                _ = tokio::signal::ctrl_c() => eprintln!("loomd: shutting down"),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("loomd: cannot start fixture replay: {e}");
            ExitCode::from(1)
        }
    }
}
