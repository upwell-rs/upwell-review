//! Why a review stopped, with the hint printed after the message.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::assets::AssetError;
use crate::drafts::DraftsError;
use crate::guard::GuardError;
use crate::lock::LockError;
use crate::post::PostError;
use crate::provider::{ProviderError, ProviderKind};
use crate::tools::ToolError;
use crate::verify::VerifyError;

/// Which of the two stages a provider run belonged to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Discovery: drafts findings.
    Discovery,
    /// Verification: keeps, modifies or drops drafts.
    Verification,
}

impl Stage {
    /// Returns the stage name used in messages and file names.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Verification => "verification",
        }
    }
}

/// A failed provider run of one stage.
#[derive(Debug, Error)]
#[error(
    "the {} run ({provider}) failed ({source}); not posting (see {}; the full answer text, if any, is in {})",
    stage.name(),
    log.display(),
    response.display()
)]
pub struct ProviderFailure {
    /// The stage.
    pub stage: Stage,
    /// The provider.
    pub provider: ProviderKind,
    /// The failure.
    pub source: ProviderError,
    /// The run log.
    pub log: PathBuf,
    /// The saved response text.
    pub response: PathBuf,
    /// Where the drafts are and how to resume.
    pub hint: String,
}

/// Why a review stopped without posting.
#[derive(Debug, Error)]
pub enum ReviewError {
    /// A focus or drafts file is missing, or a drafts file holds nothing to verify.
    #[error("{0}")]
    Usage(String),
    /// A file of the run could not be written or read.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
    /// A prompt or schema could not be loaded.
    #[error(transparent)]
    Asset(#[from] AssetError),
    /// The model lock is held or could not be taken.
    #[error("{source}")]
    Lock {
        /// The lock failure.
        source: LockError,
        /// How to resume the run later.
        hint: Option<String>,
    },
    /// `git` or `gh` failed.
    #[error(transparent)]
    Tool(#[from] ToolError),
    /// The tree is not, or no longer, the PR.
    #[error(transparent)]
    Guard(#[from] GuardError),
    /// A drafts file is unreadable or belongs to another PR or head.
    #[error(transparent)]
    Drafts(#[from] DraftsError),
    /// A provider run failed.
    #[error(transparent)]
    Provider(Box<ProviderFailure>),
    /// An answer or marker must not be posted.
    #[error("not posting: {source}")]
    Invalid {
        /// The violation.
        source: Box<VerifyError>,
        /// What was kept and how to go on.
        hint: String,
    },
    /// Verification results could not be saved.
    #[error(transparent)]
    Verify(Box<VerifyError>),
    /// The review could not be posted.
    #[error(transparent)]
    Post(#[from] PostError),
}

impl ReviewError {
    /// Returns the lines printed after the message: what was kept and how to resume.
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::Lock { hint, .. } => hint.as_deref(),
            Self::Provider(failure) => Some(&failure.hint),
            Self::Invalid { hint, .. } => Some(hint),
            _ => None,
        }
    }

    /// Wraps a verification failure: a rejected answer carries `hint`, a write failure does not.
    pub fn verify(source: VerifyError, hint: impl FnOnce() -> String) -> Self {
        if source.rejects() {
            return Self::Invalid {
                source: Box::new(source),
                hint: hint(),
            };
        }

        Self::Verify(Box::new(source))
    }
}

impl From<LockError> for ReviewError {
    fn from(source: LockError) -> Self {
        Self::Lock { source, hint: None }
    }
}
