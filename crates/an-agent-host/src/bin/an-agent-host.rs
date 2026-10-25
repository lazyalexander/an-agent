mod cli;

use an_agent_host::execution::Result;
use std::process::ExitCode;

fn run(args: Vec<String>) -> Result<()> {
    cli::run(args)
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
