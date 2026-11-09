//! Turns a verified review into a GitHub review: inline comments on diff lines, the rest in the body,
//! and a footer naming both stages.

mod body;

#[cfg(test)]
mod tests;

use std::fmt::Write;
use std::path::Path;

use thiserror::Error;

use crate::diffmap::{self, LineMap};
use crate::github::{GitHub, InlineComment, PrFile, ReviewEvent, ReviewPayload, Side};
use crate::model::{Finding, Review, Stages};
use crate::tools::ToolError;
use crate::verify::{self, VerifyError};

/// Why a review was not posted.
#[derive(Debug, Error)]
pub enum PostError {
    /// `gh` failed.
    #[error(transparent)]
    Tool(#[from] ToolError),
    /// The dry-run payload could not be written.
    #[error(transparent)]
    Write(#[from] VerifyError),
}

/// The review to create, with the findings that could not be anchored on a diff line.
#[derive(Debug)]
pub struct Composed<'a> {
    /// The payload for GitHub.
    pub payload: ReviewPayload,
    /// The findings listed in the body instead of inline.
    pub loose: Vec<&'a Finding>,
}

/// Builds the payload: findings ordered by severity, inline where GitHub shows the line, else in the body.
pub fn compose<'a>(
    review: &'a Review,
    head: &str,
    stages: &Stages,
    files: &[PrFile],
) -> Composed<'a> {
    let visible: LineMap = files
        .iter()
        .map(|file| {
            (
                file.filename.to_owned(),
                diffmap::patch_lines(file.patch.as_deref().unwrap_or("")),
            )
        })
        .collect();
    let mut findings: Vec<&Finding> = review.findings.iter().collect();
    let mut comments = Vec::new();
    let mut loose = Vec::new();

    findings.sort_by_key(|finding| finding.severity);

    for finding in findings {
        if diffmap::is_visible(&visible, &finding.path, finding.line) {
            comments.push(InlineComment {
                path: finding.path.to_owned(),
                line: finding.line,
                side: Side::Right,
                body: body::comment(finding),
            });
        } else {
            loose.push(finding);
        }
    }

    let payload = ReviewPayload {
        commit_id: head.to_owned(),
        event: ReviewEvent::Comment,
        body: body::review(review, head, stages, &loose),
        comments,
    };

    Composed { payload, loose }
}

/// Posts the review of PR `pr` at `head`, or with `dry_run` writes `payload.json` into `run_dir`;
/// returns the report to print.
pub fn post(
    github: &impl GitHub,
    pr: u64,
    head: &str,
    review: &Review,
    stages: &Stages,
    run_dir: &Path,
    dry_run: bool,
) -> Result<String, PostError> {
    let files = github.pr_files(pr)?;
    let Composed { payload, loose } = compose(review, head, stages, &files);
    let inline = payload.comments.len();

    if dry_run {
        verify::write_json(&run_dir.join("payload.json"), &payload)?;

        return Ok(dry_run_report(review, &payload, loose.len()));
    }

    let posted = github.post_review(pr, &payload)?;

    Ok(format!(
        "posted review of #{pr} at {}: {inline} inline, {} in the body, {}\n",
        short(head),
        loose.len(),
        posted.html_url
    ))
}

/// Returns the first eight characters of a commit id.
pub fn short(head: &str) -> &str {
    head.get(..8).unwrap_or(head)
}

fn dry_run_report(review: &Review, payload: &ReviewPayload, loose: usize) -> String {
    let mut text = format!(
        "DRY RUN, not posted: verdict {}, {} inline, {loose} in the body\n{}\n",
        review.verdict.as_str(),
        payload.comments.len(),
        payload.body
    );

    for comment in &payload.comments {
        let _ = write!(
            text,
            "\n--- inline {}:{}\n{}\n",
            comment.path, comment.line, comment.body
        );
    }

    text
}
