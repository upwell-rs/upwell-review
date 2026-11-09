//! Test doubles for `git` and `gh`, and fixture files.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::git::Git;
use crate::github::{GitHub, PostedReview, PrFile, PullRequest, ReviewPayload};
use crate::tools::ToolError;

/// The head every fake starts at.
pub const HEAD: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

/// The PR base every fake starts at.
pub const BASE: &str = "ba5eba5eba5eba5eba5eba5eba5eba5eba5eba5e";

/// A diff of one changed line, `src/lib.rs:1`.
pub const DIFF: &str = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,1 +1,1 @@\n-x\n+y\n";

/// The legacy fixture draft, at `src/lib.rs:1`.
pub fn draft() -> Value {
    json!({"severity": "major", "path": "src/lib.rs", "line": 1, "title": "t", "body": "b", "suggestion": "s"})
}

/// Returns the fixture draft with extra fields merged in.
pub fn draft_with(extra: Value) -> Value {
    let mut value = draft();

    if let (Some(fields), Value::Object(extra)) = (value.as_object_mut(), extra) {
        fields.extend(extra);
    }

    value
}

/// Writes `value` to `dir/name`: one JSON line per array item for `.jsonl`, else JSON.
pub fn write(dir: &Path, name: &str, value: &Value) -> PathBuf {
    let path = dir.join(name);
    let text = match (name.ends_with(".jsonl"), value) {
        (true, Value::Array(items)) => items.iter().map(|item| format!("{item}\n")).collect(),
        _ => value.to_string(),
    };

    fs::write(&path, text).expect("fixture file is written");
    path
}

/// A scripted checkout whose state a test may change mid-run.
#[derive(Debug)]
pub struct FakeGit {
    /// The current head.
    pub head: RefCell<String>,
    /// Whether tracked files are modified.
    pub dirty: RefCell<bool>,
    /// Whether `HEAD` is detached.
    pub detached: bool,
    /// The diff `diff` returns.
    pub diff: String,
}

impl Default for FakeGit {
    fn default() -> Self {
        Self {
            head: RefCell::new(HEAD.to_owned()),
            dirty: RefCell::new(false),
            detached: true,
            diff: DIFF.to_owned(),
        }
    }
}

impl Git for FakeGit {
    fn head(&self) -> Result<String, ToolError> {
        Ok(self.head.borrow().to_owned())
    }

    fn has_tracked_changes(&self) -> Result<bool, ToolError> {
        Ok(*self.dirty.borrow())
    }

    fn is_detached(&self) -> Result<bool, ToolError> {
        Ok(self.detached)
    }

    fn fetch_base(&self, _pr: u64, _base: &str) -> Result<String, ToolError> {
        Ok(BASE.to_owned())
    }

    fn merge_base(&self, first: &str, _second: &str) -> Result<String, ToolError> {
        Ok(first.to_owned())
    }

    fn diff(&self, _merge_base: &str, _head: &str) -> Result<String, ToolError> {
        Ok(self.diff.to_owned())
    }
}

/// A scripted GitHub that records the reviews posted to it.
#[derive(Debug)]
pub struct FakeGitHub {
    /// The PR as `gh pr view` reports it now.
    pub pr: RefCell<PullRequest>,
    /// The PR's files.
    pub files: Vec<PrFile>,
    /// Whether `gh auth status` succeeds.
    pub authenticated: bool,
    /// Every posted review.
    pub posted: RefCell<Vec<ReviewPayload>>,
}

impl Default for FakeGitHub {
    fn default() -> Self {
        let pr = PullRequest {
            base_ref_name: "release/1.0.0".to_owned(),
            head_ref_oid: HEAD.to_owned(),
            base_ref_oid: BASE.to_owned(),
            title: "feat: a change".to_owned(),
        };
        let files = vec![PrFile {
            filename: "src/lib.rs".to_owned(),
            patch: Some("@@ -1,1 +1,1 @@\n-x\n+y".to_owned()),
        }];

        Self {
            pr: RefCell::new(pr),
            files,
            authenticated: true,
            posted: RefCell::new(Vec::new()),
        }
    }
}

impl GitHub for FakeGitHub {
    fn pull_request(&self, _pr: u64) -> Result<PullRequest, ToolError> {
        let pr = self.pr.borrow();

        Ok(PullRequest {
            base_ref_name: pr.base_ref_name.to_owned(),
            head_ref_oid: pr.head_ref_oid.to_owned(),
            base_ref_oid: pr.base_ref_oid.to_owned(),
            title: pr.title.to_owned(),
        })
    }

    fn pr_files(&self, _pr: u64) -> Result<Vec<PrFile>, ToolError> {
        Ok(self
            .files
            .iter()
            .map(|file| PrFile {
                filename: file.filename.to_owned(),
                patch: file.patch.to_owned(),
            })
            .collect())
    }

    fn post_review(&self, _pr: u64, payload: &ReviewPayload) -> Result<PostedReview, ToolError> {
        let copy = serde_json::to_value(payload)
            .and_then(serde_json::from_value)
            .expect("payload round-trips");

        self.posted.borrow_mut().push(copy);

        Ok(PostedReview {
            html_url: "https://github.com/upwell-rs/upwell/pull/1#pullrequestreview-1".to_owned(),
        })
    }

    fn auth_status(&self) -> Result<(), ToolError> {
        if self.authenticated {
            return Ok(());
        }

        Err(ToolError::Failed {
            command: "gh auth status".to_owned(),
            stderr: "not logged in".to_owned(),
        })
    }
}
