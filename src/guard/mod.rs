//! The freshness guards: the review must be of exactly the PR head, and nothing may move while it runs.

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::git::Git;
use crate::github::GitHub;
use crate::tools::ToolError;

/// Why the reviewed tree is not, or is no longer, the PR.
#[derive(Debug, Error)]
pub enum GuardError {
    /// The worktree is not at the PR head.
    #[error(
        "worktree head {local} differs from the PR head {remote}; refusing to review a stale tree"
    )]
    Stale {
        /// The local head.
        local: String,
        /// The PR head.
        remote: String,
    },
    /// Tracked files have uncommitted changes.
    #[error(
        "worktree has uncommitted changes to tracked files; refusing to review something other than the PR"
    )]
    Dirty,
    /// The local head or tree changed during the run.
    #[error("the local checkout changed while the review ran; not posting (see {})", run.display())]
    CheckoutChanged {
        /// The run directory.
        run: PathBuf,
    },
    /// The PR head moved during the run.
    #[error("the PR head moved to {current} while the review ran; not posting a stale review (see {})", run.display())]
    HeadMoved {
        /// The new PR head.
        current: String,
        /// The run directory.
        run: PathBuf,
    },
    /// The PR base moved during the run.
    #[error(
        "the PR base moved to {current} while the review ran; not posting a review of a changed diff (see {})",
        run.display()
    )]
    BaseMoved {
        /// The new PR base.
        current: String,
        /// The run directory.
        run: PathBuf,
    },
    /// `git` or `gh` failed.
    #[error(transparent)]
    Tool(#[from] ToolError),
}

/// The head and base a review started on.
#[derive(Debug)]
pub struct Pinned<'a> {
    /// The PR number.
    pub pr: u64,
    /// The reviewed head commit.
    pub head: &'a str,
    /// The PR base commit at the start.
    pub base: &'a str,
    /// The run directory, for messages.
    pub run: &'a Path,
}

/// Checks that the checkout is the clean PR head before anything runs.
pub fn check_start<G: Git>(git: &G, local: &str, remote: &str) -> Result<(), GuardError> {
    if local != remote {
        return Err(GuardError::Stale {
            local: local.to_owned(),
            remote: remote.to_owned(),
        });
    }

    if git.has_tracked_changes()? {
        return Err(GuardError::Dirty);
    }

    Ok(())
}

/// Checks that neither the local checkout, the PR head nor the PR base changed since the start.
pub fn check_fresh<G: Git, H: GitHub>(
    git: &G,
    github: &H,
    pinned: &Pinned<'_>,
) -> Result<(), GuardError> {
    let local = git.head()?;
    let changed = local != pinned.head || git.has_tracked_changes()?;
    let pr = github.pull_request(pinned.pr)?;
    let run = pinned.run.to_owned();

    if changed {
        return Err(GuardError::CheckoutChanged { run });
    }

    if pr.head_ref_oid != pinned.head {
        return Err(GuardError::HeadMoved {
            current: pr.head_ref_oid,
            run,
        });
    }

    if pr.base_ref_oid != pinned.base {
        return Err(GuardError::BaseMoved {
            current: pr.base_ref_oid,
            run,
        });
    }

    Ok(())
}
