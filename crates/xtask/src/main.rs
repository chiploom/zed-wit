mod cli;
mod dependency_policy;
mod licenses;
mod release;
mod repository_policy;
mod tasks;
mod util;
mod zed_gui;
mod zed_hosted;
mod zed_smoke;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}
