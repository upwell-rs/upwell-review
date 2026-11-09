//! Starting `git` and `gh`, by default through `mise exec --` so the repo's tool versions and
//! identity apply, and capturing their output.

#[cfg(test)]
mod tests;

use std::env;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;

use thiserror::Error;

/// Why a `git` or `gh` command failed.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The command could not be started.
    #[error("could not run {command}: {source}")]
    Spawn {
        /// The command line.
        command: String,
        /// The spawn failure.
        source: io::Error,
    },
    /// The command exited unsuccessfully.
    #[error("{command} failed: {stderr}")]
    Failed {
        /// The command line.
        command: String,
        /// Its trimmed stderr.
        stderr: String,
    },
    /// The command printed something that is not the expected JSON.
    #[error("{command} printed unexpected output: {source}")]
    Output {
        /// The command line.
        command: String,
        /// The decoding failure.
        source: serde_json::Error,
    },
}

/// How `git` and `gh` are started.
#[derive(Clone, Copy, Debug)]
pub struct Tools {
    /// Whether to wrap them in `mise exec --`.
    pub mise: bool,
}

impl Tools {
    /// Builds a command for `program`, wrapped in `mise exec --` when enabled.
    pub fn command(self, program: &str) -> Command {
        if !self.mise {
            return Command::new(program);
        }

        let mut command = Command::new("mise");

        command.args(["exec", "--", program]);
        command
    }

    /// Runs `command`, feeding it `stdin`, and returns its stdout; fails with its stderr when it fails.
    pub fn run(command: &mut Command, stdin: Option<&[u8]>) -> Result<String, ToolError> {
        let output = match spawn(command, stdin) {
            Ok(output) => output,
            Err(source) => {
                return Err(ToolError::Spawn {
                    command: command_line(command),
                    source,
                });
            }
        };

        if !output.status.success() {
            return Err(ToolError::Failed {
                command: command_line(command),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Runs `command` and decodes its stdout as JSON.
    pub fn json<T: serde::de::DeserializeOwned>(
        command: &mut Command,
        stdin: Option<&[u8]>,
    ) -> Result<T, ToolError> {
        let stdout = Self::run(command, stdin)?;

        serde_json::from_str(&stdout).map_err(|source| ToolError::Output {
            command: command_line(command),
            source,
        })
    }
}

/// Returns the path of `program` when it is an executable file on `PATH`.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;

    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

fn spawn(command: &mut Command, stdin: Option<&[u8]>) -> io::Result<Output> {
    let input = if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = command
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let Some((bytes, mut pipe)) = stdin.zip(child.stdin.take()) else {
        return child.wait_with_output();
    };

    thread::scope(|scope| {
        let writer = scope.spawn(move || pipe.write_all(bytes));
        let output = child.wait_with_output()?;
        let written = writer
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("stdin writer panicked")));

        if output.status.success() {
            written?;
        }

        Ok(output)
    })
}

/// Returns the program and its first arguments for messages, without the `mise exec --` wrapper.
fn command_line(command: &Command) -> String {
    let program = std::iter::once(command.get_program());
    let mut words: Vec<_> = program
        .chain(command.get_args())
        .map(OsStr::to_string_lossy)
        .collect();

    if words.len() > 3 && words[0] == "mise" && words[1] == "exec" && words[2] == "--" {
        words.drain(..3);
    }

    words.truncate(4);
    words.join(" ")
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}
