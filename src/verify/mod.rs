//! Turns the verifier's decisions into the review to post, and fails closed on anything that could lose
//! or invent a finding.

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::assets::{Asset, AssetError, Assets};
use crate::diffmap::{self, LineMap};
use crate::drafts::{self, DraftsError};
use crate::model::{
    Decision, DecisionKind, DiscoveryStage, DoneMarker, Finding, Origin, Review, Stages, Verdict,
    VerificationStage, VerifyAnswer, VerifyInput,
};
use crate::schema::{self, SchemaError};

/// Why a verification answer or discovery marker must not be posted, or why its results could not be saved.
#[derive(Debug, Error)]
pub enum VerifyError {
    /// The verifier's answer file is missing or not JSON.
    #[error("verification answer unreadable ({0})")]
    Unreadable(SchemaError),
    /// The verifier's answer breaks the schema.
    #[error("{0}")]
    Answer(SchemaError),
    /// A decision names an id that is not a draft.
    #[error("decision for unknown draft id '{0}': the verifier may not add findings")]
    UnknownId(String),
    /// A draft has two decisions.
    #[error("more than one decision for draft {0}")]
    Duplicate(String),
    /// Drafts without a decision.
    #[error(
        "no decision for draft(s) {}: every draft needs one, so a left-out draft is never treated as dropped",
        .0.join(", ")
    )]
    Missing(Vec<String>),
    /// A kept or modified finding points outside the diff.
    #[error("{id} ({decision}) targets {path}:{line}, which is not a line of the PR diff", decision = .decision.as_str())]
    OffDiff {
        /// The draft id.
        id: String,
        /// Keep or modify.
        decision: DecisionKind,
        /// The finding's path.
        path: String,
        /// The finding's line.
        line: u32,
    },
    /// The built review breaks the review schema.
    #[error("{0}")]
    Review(String),
    /// The discovery answer is not a done marker.
    #[error("discovery answer unreadable ({0}); a run without drafts needs its done marker")]
    NoMarker(SchemaError),
    /// The discovery marker is not valid.
    #[error("{0}")]
    Marker(SchemaError),
    /// The done marker reports more drafts than were saved.
    #[error(
        "the discovery done marker reports {reported} draft(s) but only {saved} were saved; drafts may have been lost, so nothing is verified or posted"
    )]
    Lost {
        /// Drafts the marker reports.
        reported: u64,
        /// Drafts saved.
        saved: usize,
    },
    /// A schema could not be loaded.
    #[error(transparent)]
    Asset(#[from] AssetError),
    /// A result file could not be written.
    #[error(transparent)]
    Drafts(#[from] DraftsError),
}

impl VerifyError {
    /// Returns whether the error rejects the answer (as opposed to failing to read assets or write files).
    pub const fn rejects(&self) -> bool {
        !matches!(self, Self::Asset(_) | Self::Drafts(_))
    }
}

/// Where the verification results go.
#[derive(Debug)]
pub struct Outputs<'a> {
    /// The run directory (`review.json`, `stages.json`, `verification.jsonl`).
    pub run_dir: &'a Path,
    /// The per-head mirror the verification records are appended to.
    pub mirror: &'a Path,
}

/// One line of `verification.jsonl`: what the verifier decided for one draft.
#[derive(Debug, Serialize)]
struct Record<'a> {
    id: &'a str,
    draft: &'a Finding,
    origin: &'a Origin,
    decision: DecisionKind,
    reason: &'a str,
    #[serde(rename = "final", skip_serializing_if = "Option::is_none")]
    posted: Option<&'a Finding>,
    run: &'a Path,
    verifier: &'a Origin,
}

/// The outcome of a verification: the review to post and the stages of its footer.
#[derive(Debug)]
pub struct Verified {
    /// The verified review.
    pub review: Review,
    /// The discovery and verification runs behind it.
    pub stages: Stages,
}

/// Validates the verifier's answer against the drafts and the diff, then writes `review.json`,
/// `stages.json` and `verification.jsonl` (also appended to the mirror) and returns the review.
pub fn apply(
    assets: &Assets,
    drafts: &[VerifyInput],
    answer: &Path,
    visible: &LineMap,
    outputs: &Outputs<'_>,
    verifier: &Origin,
    resumed: bool,
) -> Result<Verified, VerifyError> {
    let answer: VerifyAnswer = schema::load(
        answer,
        &assets.schema(Asset::VerifySchema)?,
        "verification",
    )
    .map_err(|error| match error {
        SchemaError::Read { .. } | SchemaError::Parse { .. } => VerifyError::Unreadable(error),
        other => VerifyError::Answer(other),
    })?;
    let decisions = decisions_by_id(&answer, drafts)?;
    let mut records = Vec::with_capacity(drafts.len());

    for (draft, decision) in drafts.iter().zip(&decisions) {
        records.push(record(draft, decision, visible, outputs.run_dir, verifier)?);
    }

    let review = Review {
        verdict: if records.iter().any(|record| record.posted.is_some()) {
            Verdict::Findings
        } else {
            Verdict::NoFindings
        },
        findings: records
            .iter()
            .filter_map(|record| record.posted)
            .cloned()
            .collect(),
        summary: answer.summary.to_owned(),
        unchecked: answer.unchecked.to_owned(),
    };
    let stages = Stages {
        discovery: DiscoveryStage {
            origins: origins(drafts),
            drafts: drafts.len(),
            resumed,
        },
        verification: Some(VerificationStage {
            run: verifier.to_owned(),
            keep: count(&records, DecisionKind::Keep),
            modify: count(&records, DecisionKind::Modify),
            drop: count(&records, DecisionKind::Drop),
        }),
    };

    check_review(assets, &review)?;
    drafts::write_jsonl(&outputs.run_dir.join("verification.jsonl"), &records, false)?;
    drafts::write_jsonl(outputs.mirror, &records, true)?;
    write_results(outputs.run_dir, &review, &stages)?;

    Ok(Verified { review, stages })
}

/// Reads the discovery answer as a done marker.
pub fn done_marker(assets: &Assets, path: &Path) -> Result<DoneMarker, VerifyError> {
    schema::load(path, &assets.schema(Asset::DiscoverySchema)?, "discovery").map_err(|error| {
        match error {
            SchemaError::Read { .. } | SchemaError::Parse { .. } => VerifyError::NoMarker(error),
            other => VerifyError::Marker(other),
        }
    })
}

/// Fails when a done marker reports more drafts than were saved.
pub fn check_saved(marker: &DoneMarker, saved: usize) -> Result<(), VerifyError> {
    if marker.drafts > saved as u64 {
        return Err(VerifyError::Lost {
            reported: marker.drafts,
            saved,
        });
    }

    Ok(())
}

/// Writes the `NO_FINDINGS` review and the stages of a discovery that saved no drafts; the discovery
/// answer must be a done marker that reports none.
pub fn empty(
    assets: &Assets,
    discovery: &Path,
    run_dir: &Path,
    origin: Origin,
) -> Result<Verified, VerifyError> {
    let marker = done_marker(assets, discovery)?;

    check_saved(&marker, 0)?;

    let review = Review {
        verdict: Verdict::NoFindings,
        summary: marker.summary,
        findings: Vec::new(),
        unchecked: marker.unchecked,
    };
    let stages = Stages {
        discovery: DiscoveryStage {
            origins: vec![origin],
            drafts: 0,
            resumed: false,
        },
        verification: None,
    };

    write_results(run_dir, &review, &stages)?;

    Ok(Verified { review, stages })
}

/// Pairs every draft with its decision, in draft order; rejects unknown and duplicate ids and a draft
/// left out.
fn decisions_by_id<'a>(
    answer: &'a VerifyAnswer,
    drafts: &[VerifyInput],
) -> Result<Vec<&'a Decision>, VerifyError> {
    let mut found: Vec<Option<&Decision>> = vec![None; drafts.len()];

    for decision in &answer.decisions {
        let Some(index) = drafts.iter().position(|draft| draft.id == decision.id) else {
            return Err(VerifyError::UnknownId(decision.id.to_owned()));
        };

        if found[index].replace(decision).is_some() {
            return Err(VerifyError::Duplicate(decision.id.to_owned()));
        }
    }

    let missing: Vec<String> = drafts
        .iter()
        .zip(&found)
        .filter(|(_, decision)| decision.is_none())
        .map(|(draft, _)| draft.id.to_owned())
        .collect();

    if !missing.is_empty() {
        return Err(VerifyError::Missing(missing));
    }

    Ok(found.into_iter().flatten().collect())
}

fn record<'a>(
    draft: &'a VerifyInput,
    decision: &'a Decision,
    visible: &LineMap,
    run: &'a Path,
    verifier: &'a Origin,
) -> Result<Record<'a>, VerifyError> {
    let reason = match decision.reason.trim() {
        "" => "(no reason given)",
        reason => reason,
    };
    let posted = match decision.decision {
        DecisionKind::Keep => Some(&draft.finding),
        DecisionKind::Modify => Some(&decision.finding),
        DecisionKind::Drop => None,
    };

    if let Some(finding) = posted
        && !diffmap::is_visible(visible, &finding.path, finding.line)
    {
        return Err(VerifyError::OffDiff {
            id: draft.id.to_owned(),
            decision: decision.decision,
            path: finding.path.to_owned(),
            line: finding.line,
        });
    }

    Ok(Record {
        id: &draft.id,
        draft: &draft.finding,
        origin: &draft.origin,
        decision: decision.decision,
        reason,
        posted,
        run,
        verifier,
    })
}

fn origins(drafts: &[VerifyInput]) -> Vec<Origin> {
    let mut distinct: Vec<Origin> = Vec::new();

    for draft in drafts {
        if !distinct.contains(&draft.origin) {
            distinct.push(draft.origin.to_owned());
        }
    }

    distinct
}

fn count(records: &[Record<'_>], kind: DecisionKind) -> usize {
    records
        .iter()
        .filter(|record| record.decision == kind)
        .count()
}

fn check_review(assets: &Assets, review: &Review) -> Result<(), VerifyError> {
    let value = serde_json::to_value(review).map_err(|source| SchemaError::Shape {
        at: "review".to_owned(),
        source,
    });
    let value = value.map_err(VerifyError::Answer)?;

    match schema::violation(&value, &assets.schema(Asset::ReviewSchema)?, "review") {
        Some(found) => Err(VerifyError::Review(found)),
        None => Ok(()),
    }
}

fn write_results(run_dir: &Path, review: &Review, stages: &Stages) -> Result<(), VerifyError> {
    write_json(&run_dir.join("review.json"), review)?;
    write_json(&run_dir.join("stages.json"), stages)
}

/// Writes a value as indented JSON.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), VerifyError> {
    let io_error = |source| DraftsError::Io {
        path: PathBuf::from(path),
        source,
    };
    let text = serde_json::to_string_pretty(value).map_err(|source| io_error(source.into()))?;

    fs::write(path, text).map_err(|source| VerifyError::Drafts(io_error(source)))
}
