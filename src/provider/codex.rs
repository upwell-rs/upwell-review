//! `codex exec`: plain-text output; codex writes the final answer itself (`-o`).

use std::fs;
use std::io::{self, BufRead, Write};
use std::process::Command;

use super::run::prompt_stdin;
use super::{Invocation, Provider, ProviderKind, Transcript};
use crate::stream::{DraftSink, StreamError, read_lines};

/// The `codex` CLI.
#[derive(Debug)]
pub struct Codex;

impl Provider for Codex {
    const KIND: ProviderKind = ProviderKind::Codex;

    fn command(&self, call: &Invocation<'_>) -> io::Result<Command> {
        let mut command = Command::new(Self::KIND.cli());

        command
            .args(["exec", "-m", call.model, "-c"])
            .arg(format!("model_reasoning_effort={}", call.effort))
            .args(["-s", "read-only", "--ephemeral", "--output-schema"])
            .arg(call.schema)
            .arg("-C")
            .arg(call.worktree)
            .arg("-o")
            .arg(call.answer)
            .arg("-")
            .stdin(prompt_stdin(call.prompt)?);

        Ok(command)
    }

    fn parse<R: BufRead, L: Write>(
        &self,
        input: R,
        log: &mut L,
        sink: &mut DraftSink,
    ) -> Result<Transcript, StreamError> {
        let mut text = String::new();

        read_lines(input, log, |line| {
            text.push_str(line);
            text.push('\n');
            sink.scan(line)?;

            Ok(())
        })?;
        sink.scan(&text)?;

        Ok(Transcript::completed())
    }

    /// Scans the answer codex wrote for drafts; a missing answer is reported when it is validated.
    fn finish(
        &self,
        _transcript: Transcript,
        call: &Invocation<'_>,
        sink: &mut DraftSink,
    ) -> Result<(), StreamError> {
        match fs::read_to_string(call.answer) {
            Ok(answer) => sink.scan(&answer)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        Ok(())
    }
}
