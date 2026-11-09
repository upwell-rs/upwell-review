//! The pull request on GitHub, read and reviewed through the `gh` CLI.

#[cfg(test)]
mod tests;

use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::tools::{ToolError, Tools};

/// The repository reviewed by default.
pub const DEFAULT_REPO: &str = "upwell-rs/upwell";

/// The PR fields a review needs (`gh pr view --json`).
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// The base branch name.
    pub base_ref_name: String,
    /// The head commit id.
    pub head_ref_oid: String,
    /// The base commit id.
    pub base_ref_oid: String,
    /// The PR title (untrusted text).
    pub title: String,
}

/// One changed file of a PR with its patch (absent for binary or huge files).
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct PrFile {
    /// The new path.
    pub filename: String,
    /// The unified diff hunks.
    #[serde(default)]
    pub patch: Option<String>,
}

/// The review to create (`POST /repos/{repo}/pulls/{pr}/reviews`).
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPayload {
    /// The reviewed head commit.
    pub commit_id: String,
    /// Always a plain comment, never an approval or a change request.
    pub event: ReviewEvent,
    /// The review body.
    pub body: String,
    /// The inline comments.
    pub comments: Vec<InlineComment>,
}

/// The kind of review created; the tool only ever comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewEvent {
    /// A review that neither approves nor requests changes.
    #[serde(rename = "COMMENT")]
    Comment,
}

/// An inline comment on a line of the new version of a file.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineComment {
    /// The file.
    pub path: String,
    /// The line in the new version.
    pub line: u32,
    /// Always the new version.
    pub side: Side,
    /// The comment text.
    pub body: String,
}

/// The diff side a comment is anchored on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    /// The new version of the file.
    #[serde(rename = "RIGHT")]
    Right,
}

/// The created review.
#[derive(Debug, Default, Deserialize)]
pub struct PostedReview {
    /// The review's web address.
    #[serde(default)]
    pub html_url: String,
}

/// The GitHub operations a review needs.
pub trait GitHub {
    /// Reads the PR's base, head and title.
    fn pull_request(&self, pr: u64) -> Result<PullRequest, ToolError>;

    /// Lists the PR's changed files with their patches, all pages.
    fn pr_files(&self, pr: u64) -> Result<Vec<PrFile>, ToolError>;

    /// Creates a review on the PR.
    fn post_review(&self, pr: u64, payload: &ReviewPayload) -> Result<PostedReview, ToolError>;

    /// Checks that `gh` is logged in.
    fn auth_status(&self) -> Result<(), ToolError>;
}

/// The `gh` CLI on one repository.
#[derive(Debug)]
pub struct GhCli {
    tools: Tools,
    repo: String,
}

impl GhCli {
    /// Talks to `repo` (`owner/name`).
    pub fn new(tools: Tools, repo: String) -> Self {
        Self { tools, repo }
    }

    fn gh(&self, arguments: &[&str]) -> Command {
        let mut command = self.tools.command("gh");

        command.args(arguments);
        command
    }
}

impl GitHub for GhCli {
    fn pull_request(&self, pr: u64) -> Result<PullRequest, ToolError> {
        let pr = pr.to_string();
        let fields = "baseRefName,headRefOid,baseRefOid,title";

        Tools::json(
            &mut self.gh(&["pr", "view", &pr, "--repo", &self.repo, "--json", fields]),
            None,
        )
    }

    fn pr_files(&self, pr: u64) -> Result<Vec<PrFile>, ToolError> {
        let path = format!("repos/{}/pulls/{pr}/files", self.repo);
        let pages: Vec<Vec<PrFile>> =
            Tools::json(&mut self.gh(&["api", "--paginate", "--slurp", &path]), None)?;

        Ok(pages.into_iter().flatten().collect())
    }

    fn post_review(&self, pr: u64, payload: &ReviewPayload) -> Result<PostedReview, ToolError> {
        let path = format!("repos/{}/pulls/{pr}/reviews", self.repo);
        let mut command = self.gh(&["api", "-X", "POST", &path, "--input", "-"]);
        let body = serde_json::to_vec(payload).map_err(|source| ToolError::Output {
            command: "review payload".to_owned(),
            source,
        })?;

        Tools::json(&mut command, Some(&body))
    }

    fn auth_status(&self) -> Result<(), ToolError> {
        Tools::run(&mut self.gh(&["auth", "status"]), None).map(drop)
    }
}
