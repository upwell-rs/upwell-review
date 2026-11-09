//! The footer data that names both stages of a review (`stages.json`).

use serde::{Deserialize, Serialize};

use super::Origin;

/// Which runs found and verified the posted findings.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stages {
    /// The discovery runs the drafts came from.
    pub discovery: DiscoveryStage,
    /// The verification run; `None` when there were no drafts to verify.
    pub verification: Option<VerificationStage>,
}

/// The discovery side of the footer.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryStage {
    /// The distinct discovery runs, in first-seen order.
    pub origins: Vec<Origin>,
    /// The number of distinct drafts verified.
    pub drafts: usize,
    /// Whether the drafts came from a saved file (verification only).
    pub resumed: bool,
}

/// The verification side of the footer.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationStage {
    /// The verifier's provider, model and effort.
    #[serde(flatten)]
    pub run: Origin,
    /// Drafts kept verbatim.
    pub keep: usize,
    /// Drafts posted with corrections.
    pub modify: usize,
    /// Drafts dropped.
    pub drop: usize,
}
