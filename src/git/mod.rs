//! The local PR checkout, read through `git -C <worktree>`.

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tools::{ToolError, Tools};

/// The git operations a review needs on the PR checkout.
pub trait Git {
    /// Returns the full commit id of `HEAD`.
    fn head(&self) -> Result<String, ToolError>;

    /// Returns whether tracked files have uncommitted changes.
    fn has_tracked_changes(&self) -> Result<bool, ToolError>;

    /// Returns whether `HEAD` is detached.
    fn is_detached(&self) -> Result<bool, ToolError>;

    /// Fetches the PR's base branch into a private ref and returns its tip.
    fn fetch_base(&self, pr: u64, base: &str) -> Result<String, ToolError>;

    /// Returns the merge base of two commits.
    fn merge_base(&self, first: &str, second: &str) -> Result<String, ToolError>;

    /// Returns `git diff <merge_base>...<head>`.
    fn diff(&self, merge_base: &str, head: &str) -> Result<String, ToolError>;
}

/// The `git` CLI on a worktree.
#[derive(Debug)]
pub struct GitCli {
    tools: Tools,
    worktree: PathBuf,
}

impl GitCli {
    /// Reads the checkout at `worktree`.
    pub fn new(tools: Tools, worktree: &Path) -> Self {
        Self {
            tools,
            worktree: worktree.to_owned(),
        }
    }

    fn git(&self, arguments: &[&str]) -> Result<String, ToolError> {
        let mut command: Command = self.tools.command("git");

        command.arg("-C").arg(&self.worktree).args(arguments);

        Tools::run(&mut command, None)
    }

    fn line(&self, arguments: &[&str]) -> Result<String, ToolError> {
        self.git(arguments).map(|output| output.trim().to_owned())
    }
}

impl Git for GitCli {
    fn head(&self) -> Result<String, ToolError> {
        self.line(&["rev-parse", "HEAD"])
    }

    fn has_tracked_changes(&self) -> Result<bool, ToolError> {
        self.git(&["status", "--porcelain", "--untracked-files=no"])
            .map(|output| !output.trim().is_empty())
    }

    fn is_detached(&self) -> Result<bool, ToolError> {
        self.line(&["rev-parse", "--abbrev-ref", "HEAD"])
            .map(|name| name == "HEAD")
    }

    fn fetch_base(&self, pr: u64, base: &str) -> Result<String, ToolError> {
        let base_ref = format!("refs/upwell-review/pr{pr}-base");

        self.git(&[
            "fetch",
            "-q",
            "origin",
            &format!("+refs/heads/{base}:{base_ref}"),
        ])?;
        self.line(&["rev-parse", &base_ref])
    }

    fn merge_base(&self, first: &str, second: &str) -> Result<String, ToolError> {
        self.line(&["merge-base", first, second])
    }

    fn diff(&self, merge_base: &str, head: &str) -> Result<String, ToolError> {
        self.git(&["diff", &format!("{merge_base}...{head}")])
    }
}
