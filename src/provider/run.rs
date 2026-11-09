//! Runs one provider session with its stdout and stderr merged into one stream, like `2>&1 |`.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader};
use std::path::Path;
use std::process::Stdio;

use super::{Codex, Copilot, Gemini, Invocation, Provider, ProviderError, ProviderKind};
use crate::stream::DraftSink;

/// Runs one fresh, read-only session of `kind`, appending its raw output to `log` and every draft it
/// prints to `sink`, then writes its final answer to `call.answer`.
pub fn run_stage(
    kind: ProviderKind,
    call: &Invocation<'_>,
    log: &Path,
    sink: &mut DraftSink,
) -> Result<(), ProviderError> {
    match kind {
        ProviderKind::Codex => run(&Codex, call, log, sink),
        ProviderKind::Gemini => run(&Gemini, call, log, sink),
        ProviderKind::Copilot => run(&Copilot, call, log, sink),
    }
}

/// Spawns the provider, streams its merged output through its parser and waits for it.
///
/// The parser runs to the end even when the provider fails, so the drafts, the log and the response
/// text survive a crash or a usage limit.
pub(super) fn run<P: Provider>(
    provider: &P,
    call: &Invocation<'_>,
    log: &Path,
    sink: &mut DraftSink,
) -> Result<(), ProviderError> {
    let cli = P::KIND.cli();
    let spawn = |source| ProviderError::Spawn { cli, source };
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(spawn)?;
    let (reader, writer) = io::pipe().map_err(spawn)?;

    remove_stale(call.answer).map_err(spawn)?;

    let mut command = provider.command(call).map_err(spawn)?;

    command
        .stdout(writer.try_clone().map_err(spawn)?)
        .stderr(writer);

    let mut child = command.spawn().map_err(spawn)?;

    drop(command);

    let streamed = provider
        .parse(BufReader::new(reader), &mut log, sink)
        .and_then(|transcript| provider.finish(transcript, call, sink));
    let status = child.wait().map_err(spawn)?;

    match streamed {
        Err(source) => Err(ProviderError::Stream {
            cli,
            status,
            source,
        }),
        Ok(()) if !status.success() => Err(ProviderError::Exited { cli, status }),
        Ok(()) => Ok(()),
    }
}

fn remove_stale(answer: &Path) -> io::Result<()> {
    match std::fs::remove_file(answer) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Opens the prompt file as the provider's stdin (codex reads the prompt from `-`).
pub(super) fn prompt_stdin(prompt: &Path) -> io::Result<Stdio> {
    File::open(prompt).map(Stdio::from)
}
