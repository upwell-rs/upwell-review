#![forbid(unsafe_code)]
//! `upwell-review`: an independent, read-only, two-stage pull request review for `upwell-rs/upwell`.

mod assets;
mod cli;
mod config;
mod diffmap;
mod doctor;
mod drafts;
mod exit;
mod git;
mod github;
mod guard;
mod lock;
mod model;
mod post;
mod provider;
mod review;
mod schema;
mod stream;
#[cfg(test)]
mod testing;
mod tools;
mod verify;

use std::env;
use std::io;
use std::process::ExitCode;

use clap::Parser;

use cli::Cli;
use exit::Exit;
use review::Console;

/// Parses the command line, runs it and maps the outcome to the documented exit code. Usage errors
/// exit 1 (clap's own 2 means a stale tree here). A usage message that cannot be printed is ignored:
/// the exit code still reports the failure.
fn main() -> ExitCode {
    let env = |name: &str| env::var(name).ok();
    let mut console = Console {
        out: io::stdout(),
        err: io::stderr(),
    };
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit = if error.use_stderr() {
                Exit::Error
            } else {
                Exit::Ok
            };
            let _ = error.print();

            return exit.into();
        }
    };

    let Err(failure) = cli::run(cli, &env, &mut console) else {
        return Exit::Ok.into();
    };

    eprintln!("upwell-review: {failure}");

    if let Some(hint) = failure.hint() {
        eprintln!("{hint}");
    }

    failure.exit().into()
}
