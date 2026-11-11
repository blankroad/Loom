//! `loom-schema export|check [--dir <path>]` — run from the workspace root.

use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: loom-schema <export|check> [--dir <path>]   (default dir: {})",
        loom_schema::SCHEMA_DIR
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { return usage() };
    let dir = match args.get(1).map(String::as_str) {
        None => PathBuf::from(loom_schema::SCHEMA_DIR),
        Some("--dir") => match args.get(2) {
            Some(d) => PathBuf::from(d),
            None => return usage(),
        },
        Some(_) => return usage(),
    };
    match cmd.as_str() {
        "export" => match loom_schema::export(&dir) {
            Ok(n) => {
                println!("wrote {n} schema files to {}", dir.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        },
        "check" => match loom_schema::check(&dir) {
            Ok(d) if d.is_clean() => {
                println!("schema up to date ({})", dir.display());
                ExitCode::SUCCESS
            }
            Ok(d) => {
                eprintln!(
                    "schema drift in {}: missing {:?}, changed {:?}, stale {:?}\nrun: cargo run -p loom-schema -- export",
                    dir.display(),
                    d.missing,
                    d.changed,
                    d.stale
                );
                ExitCode::from(1)
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        },
        _ => usage(),
    }
}
