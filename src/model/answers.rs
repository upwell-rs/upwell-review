//! The final answers of the stages and the review that is posted.

use serde::{Deserialize, Serialize};

use super::Finding;

/// The discovery stage's final answer: a done marker that carries no findings.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoneMarker {
    /// Always `DISCOVERY_DONE`.
    pub status: DoneStatus,
    /// The number of drafts the model says it printed.
    pub drafts: u64,
    /// What the model reviewed.
    pub summary: String,
    /// What the model could not check.
    pub unchecked: String,
}

/// The only status a discovery done marker may carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoneStatus {
    /// Discovery went through the whole diff.
    #[serde(rename = "DISCOVERY_DONE")]
    DiscoveryDone,
}

/// The verifier's final answer: one decision per draft id.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyAnswer {
    /// The summary for the PR author.
    pub summary: String,
    /// What the verifier could not check.
    pub unchecked: String,
    /// One decision per draft id.
    pub decisions: Vec<Decision>,
}

/// The verifier's verdict on one draft with the finding fields it returned.
#[derive(Debug, Serialize, Deserialize)]
pub struct Decision {
    /// The draft id (`D1`..`Dn`).
    pub id: String,
    /// Keep, modify or drop.
    pub decision: DecisionKind,
    /// The evidence for the decision.
    pub reason: String,
    /// The echoed (keep, drop) or corrected (modify) finding.
    #[serde(flatten)]
    pub finding: Finding,
}

/// What the verifier decided for a draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionKind {
    /// The draft holds as written and is posted verbatim.
    Keep,
    /// The defect is real; the verifier's corrected fields are posted.
    Modify,
    /// The draft does not hold and is not posted.
    Drop,
}

impl DecisionKind {
    /// Returns the schema spelling of the decision.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Modify => "modify",
            Self::Drop => "drop",
        }
    }
}

/// The review that is posted: the verified findings and the summary.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    /// Whether any finding survived.
    pub verdict: Verdict,
    /// The summary for the PR author.
    pub summary: String,
    /// The findings to post.
    pub findings: Vec<Finding>,
    /// What could not be checked.
    pub unchecked: String,
}

/// Whether a review reports findings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Nothing held up.
    #[serde(rename = "NO_FINDINGS")]
    NoFindings,
    /// At least one verified finding.
    #[serde(rename = "FINDINGS")]
    Findings,
}

impl Verdict {
    /// Returns the verdict as the review body prints it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoFindings => "NO FINDINGS",
            Self::Findings => "FINDINGS",
        }
    }
}
