//! Draft findings: as parsed from a stream, as saved, as read back and as handed to the verifier.

use serde::{Deserialize, Serialize};

use super::{Finding, Origin};

/// A draft finding parsed from a provider stream and normalized.
#[derive(Debug, PartialEq, Eq)]
pub struct Draft {
    /// The normalized finding.
    pub finding: Finding,
    /// The model's own status or severity word when it was not the expected one (for example `nit`).
    pub kind: Option<String>,
}

/// The PR and head a draft was found on, plus the discovery run that found it.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    /// The PR number.
    pub pr: u64,
    /// The full head commit id.
    pub head: String,
    /// The discovery provider, model and effort.
    #[serde(flatten)]
    pub origin: Origin,
}

/// One line of `drafts.jsonl` and of the per-head mirror, as written.
#[derive(Debug, Serialize)]
pub struct SavedDraft<'a> {
    /// The normalized finding.
    #[serde(flatten)]
    pub finding: &'a Finding,
    /// Always `draft`, kept for readers of the legacy format.
    pub status: &'static str,
    /// The model's original status or severity word, when it differed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'a str>,
    /// Where and by whom the draft was found.
    #[serde(flatten)]
    pub stamp: &'a Stamp,
}

/// A PR number as stamped: a number, or a string in drafts the legacy scripts saved.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PrRef {
    /// A numeric stamp.
    Number(u64),
    /// A textual stamp.
    Text(String),
}

impl PrRef {
    /// Returns whether the stamp names PR `pr`.
    pub fn is(&self, pr: u64) -> bool {
        match self {
            Self::Number(number) => *number == pr,
            Self::Text(text) => text.trim() == pr.to_string(),
        }
    }
}

/// A draft read back from a drafts file, a mirror or a `verify-input.jsonl`, with optional stamps.
#[derive(Debug, Deserialize)]
pub struct StoredDraft {
    /// The finding fields.
    #[serde(flatten)]
    pub finding: Finding,
    /// The stamped PR number.
    #[serde(default)]
    pub pr: Option<PrRef>,
    /// The stamped head commit id.
    #[serde(default)]
    pub head: Option<String>,
    /// The stamped discovery provider.
    #[serde(default)]
    pub provider: Option<String>,
    /// The stamped discovery model.
    #[serde(default)]
    pub model: Option<String>,
    /// The stamped discovery effort.
    #[serde(default)]
    pub effort: Option<String>,
    /// The discovery run of a `verify-input.jsonl` record.
    #[serde(default)]
    pub origin: Option<Origin>,
}

/// One distinct draft with its id, as handed to the verifier (`verify-input.jsonl`).
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyInput {
    /// The draft id, `D1`..`Dn`.
    pub id: String,
    /// The PR number.
    pub pr: u64,
    /// The full head commit id.
    pub head: String,
    /// The finding fields.
    #[serde(flatten)]
    pub finding: Finding,
    /// The discovery run that found it.
    pub origin: Origin,
}
