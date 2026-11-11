//! `loom` binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    ExitCode::from(loom_cli::run(&args, &mut std::io::stdout()))
}
