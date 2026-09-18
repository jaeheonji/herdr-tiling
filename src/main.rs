mod action;
mod config;
mod features;
pub mod herdr;
mod preset;
pub mod rebuild;
pub mod tree;

use std::process::ExitCode;

use action::Action;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("herdr-tiling: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let action = Action::parse(std::env::args_os().skip(1))?;
    herdr::environment::validate(&action)?;
    features::run(action)
}
