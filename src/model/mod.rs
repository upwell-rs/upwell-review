//! Typed JSON shapes shared by the stages: findings, drafts, answers and footer data.

mod answers;
mod draft;
mod stages;

use std::fmt;

use serde::{Deserialize, Serialize};

pub use answers::{Decision, DecisionKind, DoneMarker, DoneStatus, Review, Verdict, VerifyAnswer};
pub use draft::{Draft, PrRef, SavedDraft, Stamp, StoredDraft, VerifyInput};
pub use stages::{DiscoveryStage, Stages, VerificationStage};

/// How serious a finding is, ordered from the most to the least severe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Must be fixed before merging.
    Blocker,
    /// A real defect or a violated design goal.
    Major,
    /// A small defect.
    Minor,
    /// Style or taste.
    Nit,
}

impl Severity {
    /// Returns the schema spelling of the severity.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocker => "blocker",
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Nit => "nit",
        }
    }

    /// Maps a severity or status word a model wrote to a severity, accepting common synonyms.
    pub fn from_word(word: &str) -> Option<Self> {
        match word.trim().to_ascii_lowercase().as_str() {
            "blocker" | "critical" => Some(Self::Blocker),
            "major" | "high" | "bug" | "error" => Some(Self::Major),
            "minor" | "medium" | "warning" => Some(Self::Minor),
            "nit" | "nitpick" | "low" | "style" | "info" => Some(Self::Nit),
            _ => None,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One review finding, anchored at a line of the new version of a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Finding {
    /// How serious the defect is.
    pub severity: Severity,
    /// The repo-relative path of the file.
    pub path: String,
    /// The line in the new version of the file.
    pub line: u32,
    /// A one-line title.
    pub title: String,
    /// The self-contained explanation.
    pub body: String,
    /// The suggested fix; may be empty.
    pub suggestion: String,
}

/// The provider, model and effort of one stage run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The provider name (`codex`, `gemini` or `copilot`).
    pub provider: String,
    /// The model id.
    pub model: String,
    /// The recorded reasoning effort.
    pub effort: String,
}
