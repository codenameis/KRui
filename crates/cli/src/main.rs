//! Entry point for the `krui` developer CLI (`krui serve`).

#![forbid(unsafe_code)]

use krui_cli::{parse_cli_args, run_serve, CliParseError};
use std::process::ExitCode;

const CLI_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn main() -> ExitCode {
    let args = std::env::args().skip(1);
    let opts = match parse_cli_args(args) {
        Ok(o) => o,
        Err(CliParseError::HelpRequested(msg)) => {
            println!("{msg}");
            return ExitCode::SUCCESS;
        }
        Err(CliParseError::InvalidArgs(msg)) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let result = match std::thread::Builder::new()
        .name("krui-cli".to_string())
        .stack_size(CLI_WORKER_STACK_BYTES)
        .spawn(move || run_serve(opts))
    {
        Ok(handle) => match handle.join() {
            Ok(res) => res,
            Err(_) => Err("`krui serve` worker thread panicked".to_string()),
        },
        Err(err) => Err(format!("Failed to spawn `krui serve` worker: {err}")),
    };
    if let Err(err) = result {
        eprintln!("[krui serve] Error: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
