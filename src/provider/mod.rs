//! The provider CLIs (`codex`, `agy` for gemini, `copilot`): their exact read-only invocations and
//! output stream parsers.

mod codex;
mod copilot;
mod gemini;
mod run;

#[cfg(test)]
mod tests;

use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, de};
use serde_json::Value;
use thiserror::Error;

use crate::stream::{DraftSink, StreamError, fallback_answer};

pub use codex::Codex;
pub use copilot::Copilot;
pub use gemini::Gemini;
pub use run::run_stage;

/// The effort recorded for gemini runs, whose models carry their effort in the model name.
pub const GEMINI_EFFORT: &str = "set by the model name";

/// One of the supported provider CLIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    /// OpenAI `codex exec`.
    Codex,
    /// Google's `agy` CLI.
    Gemini,
    /// GitHub `copilot`.
    Copilot,
}

impl ProviderKind {
    /// Every provider, for `doctor`.
    pub const ALL: [Self; 3] = [Self::Codex, Self::Gemini, Self::Copilot];

    /// Returns the provider name used on the command line and in stamps.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Copilot => "copilot",
        }
    }

    /// Returns the name the review title and footer use.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Gemini => "Gemini",
            Self::Copilot => "Copilot",
        }
    }

    /// Returns the executable the provider runs.
    pub const fn cli(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Gemini => "agy",
            Self::Copilot => "copilot",
        }
    }

    /// Returns the effort to record for a run with `effort`.
    pub fn recorded_effort(self, effort: &str) -> &str {
        match self {
            Self::Gemini => GEMINI_EFFORT,
            Self::Codex | Self::Copilot => effort,
        }
    }

    /// Returns the label of a recorded provider name, or the name itself when it is not known.
    pub fn label_of(name: &str) -> &str {
        name.parse::<Self>().map_or(name, |kind| kind.label())
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// A provider name that is not `codex`, `gemini` or `copilot`.
#[derive(Debug, Error)]
#[error("unknown provider '{0}'; expected codex, gemini or copilot")]
pub struct UnknownProvider(pub String);

impl FromStr for ProviderKind {
    type Err = UnknownProvider;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "copilot" => Ok(Self::Copilot),
            other => Err(UnknownProvider(other.to_owned())),
        }
    }
}

impl<'de> Deserialize<'de> for ProviderKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;

        name.parse().map_err(de::Error::custom)
    }
}

/// Everything one stage run needs: the model, the files and the directories it may read.
#[derive(Debug)]
pub struct Invocation<'a> {
    /// The model id.
    pub model: &'a str,
    /// The reasoning effort.
    pub effort: &'a str,
    /// The answer schema file.
    pub schema: &'a Path,
    /// The prompt file.
    pub prompt: &'a Path,
    /// Where the final answer is written.
    pub answer: &'a Path,
    /// The read-only PR checkout.
    pub worktree: &'a Path,
    /// The run directory (diff, prompts, logs).
    pub run: &'a Path,
}

impl Invocation<'_> {
    /// Returns where the full answer text is saved: `<answer stem>.response.md`.
    pub fn response_path(&self) -> PathBuf {
        self.answer.with_extension("response.md")
    }
}

/// How a provider stream ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The final result event reported success.
    Completed,
    /// The stream ended without a result event.
    Missing,
    /// The result event reported a failure; the text names the status.
    Failed(String),
}

/// What a provider stream yielded besides its drafts.
#[derive(Debug)]
pub struct Transcript {
    /// The final answer text.
    pub response: String,
    /// The provider's structured answer, when it gave one.
    pub structured: Option<Value>,
    /// How the stream ended.
    pub outcome: Outcome,
}

impl Transcript {
    /// A transcript of a stream whose answer the provider writes itself (codex `-o`).
    pub fn completed() -> Self {
        Self {
            response: String::new(),
            structured: None,
            outcome: Outcome::Completed,
        }
    }
}

/// Why a stage run failed.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// The provider could not be started.
    #[error("could not start {cli}: {source}")]
    Spawn {
        /// The executable.
        cli: &'static str,
        /// The spawn failure.
        source: io::Error,
    },
    /// The provider exited unsuccessfully.
    #[error("{cli} exited with {status}")]
    Exited {
        /// The executable.
        cli: &'static str,
        /// Its exit status.
        status: ExitStatus,
    },
    /// The output stream gave no usable answer.
    #[error("{source} ({cli} exited with {status})")]
    Stream {
        /// The executable.
        cli: &'static str,
        /// The provider's exit status.
        status: ExitStatus,
        /// The stream failure.
        source: StreamError,
    },
}

/// A provider CLI: builds its read-only command and parses its own output stream.
pub trait Provider {
    /// Which provider this is.
    const KIND: ProviderKind;

    /// Builds the command of one fresh, read-only session; the prompt goes on stdin or in an argument.
    fn command(&self, call: &Invocation<'_>) -> io::Result<Command>;

    /// Reads the merged stdout and stderr, copies it to `log` and hands every draft to `sink`.
    fn parse<R: BufRead, L: Write>(
        &self,
        input: R,
        log: &mut L,
        sink: &mut DraftSink,
    ) -> Result<Transcript, StreamError>;

    /// Saves the full response, scans it for drafts and writes the final answer.
    fn finish(
        &self,
        transcript: Transcript,
        call: &Invocation<'_>,
        sink: &mut DraftSink,
    ) -> Result<(), StreamError> {
        write_answer(Self::KIND.cli(), transcript, call, sink)
    }
}

/// Saves the response text, scans it for drafts and writes the structured answer, else the last JSON
/// block of the response.
fn write_answer(
    cli: &'static str,
    transcript: Transcript,
    call: &Invocation<'_>,
    sink: &mut DraftSink,
) -> Result<(), StreamError> {
    let saved = call.response_path();

    fs::write(&saved, &transcript.response)?;
    sink.scan(&transcript.response)?;

    match transcript.outcome {
        Outcome::Missing => return Err(StreamError::NoResult { provider: cli }),
        Outcome::Failed(detail) => {
            return Err(StreamError::Unsuccessful {
                provider: cli,
                detail,
                saved,
            });
        }
        Outcome::Completed => {}
    }

    let answer = match transcript.structured {
        Some(answer) => answer,
        None => {
            let Some(answer) = fallback_answer(&transcript.response) else {
                return Err(StreamError::NoAnswer {
                    provider: cli,
                    saved,
                });
            };

            eprintln!(
                "{cli} gave no structured_output; using the last JSON block of {}",
                saved.display()
            );
            answer
        }
    };

    fs::write(call.answer, answer.to_string())?;

    Ok(())
}

/// Reads the prompt file for providers that take the prompt as an argument.
fn prompt_text(call: &Invocation<'_>) -> io::Result<String> {
    fs::read_to_string(call.prompt)
}
