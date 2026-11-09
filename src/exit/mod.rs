//! The documented exit codes and the top-level failure each one is mapped from.

#[cfg(test)]
mod tests;

use std::io;
use std::process::ExitCode;

use thiserror::Error;

use crate::assets::AssetError;
use crate::config::ConfigError;
use crate::drafts::DraftsError;
use crate::guard::GuardError;
use crate::lock::LockError;
use crate::provider::UnknownProvider;
use crate::review::ReviewError;
use crate::schema::SchemaError;

/// How `upwell-review` exits; the codes are the legacy script's, plus 11 for `doctor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    /// Done: the review was posted (or printed in a dry run), or the command succeeded.
    Ok = 0,
    /// A usage error, a missing file, or a failed `git`/`gh`/file operation.
    Error = 1,
    /// The worktree head is not the PR head.
    Stale = 2,
    /// An answer, marker or review is invalid, or the done marker reports more drafts than were saved.
    Invalid = 3,
    /// The worktree has uncommitted changes to tracked files.
    Dirty = 4,
    /// The PR head moved during the run, or the drafts belong to another PR or head.
    HeadMoved = 5,
    /// The PR base moved during the run.
    BaseMoved = 6,
    /// The local checkout (head or tracked files) changed during the run.
    CheckoutChanged = 7,
    /// A provider name is not `codex`, `gemini` or `copilot`.
    UnknownProvider = 8,
    /// Another review holds the model's lock.
    Locked = 9,
    /// A provider run failed.
    ProviderFailed = 10,
    /// A `doctor` check failed.
    DoctorFailed = 11,
}

impl Exit {
    /// Returns the process exit status.
    pub const fn code(self) -> u8 {
        self as u8
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit.code())
    }
}

/// Why a command failed, carrying the exit code and the hint printed after the message.
#[derive(Debug, Error)]
pub enum Failure {
    /// The review stopped.
    #[error(transparent)]
    Review(#[from] ReviewError),
    /// The providers file or a mix is unusable.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A provider name is unknown.
    #[error(transparent)]
    Provider(#[from] UnknownProvider),
    /// A prompt or schema could not be loaded.
    #[error(transparent)]
    Asset(#[from] AssetError),
    /// A drafts file could not be read.
    #[error(transparent)]
    Drafts(#[from] DraftsError),
    /// `validate` found the answer unreadable or invalid.
    #[error(transparent)]
    Schema(#[from] SchemaError),
    /// `validate` was given neither a schema name nor a schema file.
    #[error("unknown schema '{0}'; expected discovery, verify, review or a schema file")]
    UnknownSchema(String),
    /// `validate` was given a schema file that is not JSON.
    #[error("schema file {0}")]
    SchemaFile(SchemaError),
    /// Some `doctor` checks failed.
    #[error("{0} doctor check(s) failed")]
    Doctor(usize),
    /// Standard output could not be written.
    #[error("cannot write the output: {0}")]
    Output(#[from] io::Error),
}

impl Failure {
    /// Returns the exit code of this failure.
    pub fn exit(&self) -> Exit {
        match self {
            Self::Review(error) => review_exit(error),
            Self::Provider(_) => Exit::UnknownProvider,
            Self::Drafts(error) => drafts_exit(error),
            Self::Schema(SchemaError::Read { .. }) => Exit::Error,
            Self::Schema(_) => Exit::Invalid,
            Self::Doctor(_) => Exit::DoctorFailed,
            Self::Config(_)
            | Self::Asset(_)
            | Self::UnknownSchema(_)
            | Self::SchemaFile(_)
            | Self::Output(_) => Exit::Error,
        }
    }

    /// Returns the lines printed after the message: what was kept and how to resume.
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::Review(error) => error.hint(),
            _ => None,
        }
    }
}

fn review_exit(error: &ReviewError) -> Exit {
    match error {
        ReviewError::Lock {
            source: LockError::Busy { .. },
            ..
        } => Exit::Locked,
        ReviewError::Guard(error) => guard_exit(error),
        ReviewError::Drafts(error) => drafts_exit(error),
        ReviewError::Provider(_) => Exit::ProviderFailed,
        ReviewError::Invalid { .. } => Exit::Invalid,
        ReviewError::Usage(_)
        | ReviewError::Io { .. }
        | ReviewError::Asset(_)
        | ReviewError::Lock { .. }
        | ReviewError::Tool(_)
        | ReviewError::Verify(_)
        | ReviewError::Post(_) => Exit::Error,
    }
}

fn guard_exit(error: &GuardError) -> Exit {
    match error {
        GuardError::Stale { .. } => Exit::Stale,
        GuardError::Dirty => Exit::Dirty,
        GuardError::CheckoutChanged { .. } => Exit::CheckoutChanged,
        GuardError::HeadMoved { .. } => Exit::HeadMoved,
        GuardError::BaseMoved { .. } => Exit::BaseMoved,
        GuardError::Tool(_) => Exit::Error,
    }
}

fn drafts_exit(error: &DraftsError) -> Exit {
    match error {
        DraftsError::Foreign { .. } => Exit::HeadMoved,
        DraftsError::Io { .. } | DraftsError::Parse { .. } => Exit::Error,
    }
}
