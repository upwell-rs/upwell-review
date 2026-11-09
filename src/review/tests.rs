use std::cell::RefCell;
use std::io;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::config::StageRun;
use crate::drafts::DraftsError;
use crate::guard::GuardError;
use crate::lock::LockError;
use crate::stream::MARKER;
use crate::testing::{FakeGit, FakeGitHub, HEAD, draft, draft_with, write};
use crate::verify::VerifyError;

const PR: u64 = 7;

/// A scripted provider: discovery streams `found` and answers `marker`; verification keeps every draft.
#[derive(Default)]
struct FakeStages<'a> {
    found: Vec<Value>,
    marker: Option<Value>,
    fail: Option<Stage>,
    dirty: Option<&'a FakeGit>,
    calls: RefCell<Vec<(Stage, ProviderKind, String)>>,
}

impl FakeStages<'_> {
    fn stages(&self) -> Vec<Stage> {
        self.calls
            .borrow()
            .iter()
            .map(|(stage, ..)| *stage)
            .collect()
    }

    fn answer(&self, stage: Stage, call: &Invocation<'_>) -> Value {
        if stage == Stage::Discovery {
            return self
                .marker
                .clone()
                .unwrap_or_else(|| marker(self.found.len()));
        }

        let inputs = drafts::read_jsonl::<Value>(&call.run.join("verify-input.jsonl"))
            .expect("verify input");
        let decisions = (1..=inputs.len())
            .map(|id| {
                draft_with(json!({"id": format!("D{id}"), "decision": "keep", "reason": "checked"}))
            })
            .collect::<Vec<_>>();

        if let Some(git) = self.dirty {
            *git.dirty.borrow_mut() = true;
        }

        json!({"summary": "s", "unchecked": "", "decisions": decisions})
    }
}

impl StageRunner for FakeStages<'_> {
    fn run(
        &self,
        kind: ProviderKind,
        call: &Invocation<'_>,
        _log: &Path,
        sink: &mut DraftSink,
    ) -> Result<(), ProviderError> {
        let stage = match call.answer.file_name().and_then(|name| name.to_str()) {
            Some("discovery.json") => Stage::Discovery,
            _ => Stage::Verification,
        };

        self.calls
            .borrow_mut()
            .push((stage, kind, call.model.to_owned()));

        if stage == Stage::Discovery {
            for found in &self.found {
                sink.scan(&format!("{MARKER} {found}"))
                    .expect("draft is saved");
            }
        }

        if self.fail == Some(stage) {
            return Err(ProviderError::Spawn {
                cli: kind.cli(),
                source: io::Error::other("gone"),
            });
        }

        fs::write(call.answer, self.answer(stage, call).to_string()).expect("answer is written");

        Ok(())
    }
}

/// The checkout, the PR and the output root of one test review.
struct Harness {
    out: TempDir,
    worktree: TempDir,
    git: FakeGit,
    github: FakeGitHub,
    assets: Assets,
}

impl Harness {
    fn new() -> Self {
        Self {
            out: tempfile::tempdir().expect("out dir"),
            worktree: tempfile::tempdir().expect("worktree"),
            git: FakeGit::default(),
            github: FakeGitHub::default(),
            assets: Assets::default(),
        }
    }

    fn request(&self) -> Request {
        Request {
            pr: PR,
            worktree: self.worktree.path().to_owned(),
            plan: Plan {
                discovery: stage_run(ProviderKind::Copilot, "m"),
                verify: stage_run(ProviderKind::Codex, "v"),
            },
            focus: None,
            verify_only: None,
            dry_run: false,
        }
    }

    fn review(
        &self,
        stages: &FakeStages<'_>,
        request: &Request,
    ) -> (Result<(), ReviewError>, String, String) {
        let context = Context {
            git: &self.git,
            github: &self.github,
            stages,
            assets: &self.assets,
            out: self.out.path(),
        };
        let mut console = Console {
            out: Vec::new(),
            err: Vec::new(),
        };
        let result = review(&context, request, &mut console);

        (
            result,
            String::from_utf8_lossy(&console.out).into_owned(),
            String::from_utf8_lossy(&console.err).into_owned(),
        )
    }
}

fn stage_run(provider: ProviderKind, model: &str) -> StageRun {
    StageRun {
        provider,
        model: model.to_owned(),
        effort: "high".to_owned(),
    }
}

fn marker(drafts: usize) -> Value {
    json!({"status": "DISCOVERY_DONE", "drafts": drafts, "summary": "s", "unchecked": ""})
}

fn found() -> Value {
    draft_with(json!({"status": "draft"}))
}

fn stamped(head: &str) -> Value {
    draft_with(
        json!({"status": "draft", "pr": PR, "head": head, "provider": "copilot", "model": "m", "effort": "high"}),
    )
}

#[test]
fn a_discovery_without_drafts_posts_a_clean_review_without_verifying() {
    let harness = Harness::new();
    let stages = FakeStages::default();
    let (result, out, err) = harness.review(&stages, &harness.request());
    let posted = harness.github.posted.borrow();

    result.expect("clean review is posted");
    assert_eq!(stages.stages(), [Stage::Discovery]);
    assert!(
        err.contains("discovery saved no drafts; skipping verification"),
        "{err}"
    );
    assert!(out.contains("posted review of #7 at a1b2c3d4"), "{out}");
    assert_eq!(posted.len(), 1);
    assert!(
        posted[0].body.contains("**Verdict:** NO FINDINGS"),
        "{}",
        posted[0].body
    );
}

#[test]
fn kept_drafts_are_verified_by_a_fresh_run_and_posted_inline() {
    let harness = Harness::new();
    let stages = FakeStages {
        found: vec![found()],
        ..FakeStages::default()
    };
    let (result, out, err) = harness.review(&stages, &harness.request());
    let posted = harness.github.posted.borrow();
    let calls = stages.calls.borrow();

    result.expect("review is posted");
    assert_eq!(
        *calls,
        [
            (Stage::Discovery, ProviderKind::Copilot, "m".to_owned()),
            (Stage::Verification, ProviderKind::Codex, "v".to_owned())
        ]
    );
    assert!(
        err.contains("verifying 1 draft(s) with codex/v in a fresh run"),
        "{err}"
    );
    assert!(
        out.contains("verification: 1 kept, 0 modified, 0 dropped"),
        "{out}"
    );
    assert_eq!(posted[0].comments.len(), 1);
    assert_eq!(posted[0].commit_id, HEAD);
}

#[test]
fn a_marker_reporting_more_drafts_than_were_saved_is_not_posted() {
    let harness = Harness::new();
    let stages = FakeStages {
        marker: Some(marker(2)),
        ..FakeStages::default()
    };
    let (result, ..) = harness.review(&stages, &harness.request());
    let error = result.expect_err("lost drafts fail closed");
    let hint = error.hint().unwrap_or_default().to_owned();

    assert!(
        matches!(&error, ReviewError::Invalid { source, .. } if matches!(**source, VerifyError::Lost { .. })),
        "{error}"
    );
    assert!(hint.contains("upwell-review verify 7"), "{hint}");
    assert_eq!(stages.stages(), [Stage::Discovery]);
    assert!(harness.github.posted.borrow().is_empty());
}

#[test]
fn a_checkout_changed_during_verification_is_not_posted() {
    let harness = Harness::new();
    let stages = FakeStages {
        found: vec![found()],
        dirty: Some(&harness.git),
        ..FakeStages::default()
    };
    let (result, ..) = harness.review(&stages, &harness.request());
    let error = result.expect_err("changed checkout is refused");

    assert!(
        matches!(
            error,
            ReviewError::Guard(GuardError::CheckoutChanged { .. })
        ),
        "{error}"
    );
    assert!(harness.github.posted.borrow().is_empty());
}

#[test]
fn saved_drafts_are_verified_without_a_new_discovery() {
    let harness = Harness::new();
    let drafts = write(
        harness.worktree.path(),
        "saved.jsonl",
        &json!([stamped(HEAD)]),
    );
    let stages = FakeStages::default();
    let request = Request {
        verify_only: Some(drafts),
        ..harness.request()
    };
    let (result, out, _) = harness.review(&stages, &request);

    result.expect("saved drafts are verified and posted");
    assert_eq!(stages.stages(), [Stage::Verification]);
    assert!(out.contains("verification: 1 kept"), "{out}");
    assert_eq!(harness.github.posted.borrow().len(), 1);
}

#[test]
fn drafts_of_another_head_are_refused() {
    let harness = Harness::new();
    let drafts = write(
        harness.worktree.path(),
        "saved.jsonl",
        &json!([stamped("0123456789")]),
    );
    let stages = FakeStages::default();
    let request = Request {
        verify_only: Some(drafts),
        ..harness.request()
    };
    let (result, ..) = harness.review(&stages, &request);
    let error = result.expect_err("foreign drafts are refused");

    assert!(
        matches!(error, ReviewError::Drafts(DraftsError::Foreign { .. })),
        "{error}"
    );
    assert!(stages.calls.borrow().is_empty());
}

#[test]
fn a_dry_run_writes_the_payload_and_posts_nothing() {
    let harness = Harness::new();
    let stages = FakeStages {
        found: vec![draft_with(json!({"status": "nit"}))],
        ..FakeStages::default()
    };
    let request = Request {
        dry_run: true,
        ..harness.request()
    };
    let (result, out, _) = harness.review(&stages, &request);
    let payloads = fs::read_dir(harness.out.path())
        .expect("out dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("payload.json").is_file())
        .count();

    result.expect("dry run succeeds");
    assert!(out.contains("DRY RUN, not posted"), "{out}");
    assert_eq!(payloads, 1);
    assert!(harness.github.posted.borrow().is_empty());
}

#[test]
fn a_busy_model_lock_stops_the_review_before_any_run() {
    let harness = Harness::new();
    let stages = FakeStages::default();

    Workspace::prepare(harness.out.path()).expect("out dirs");

    let _held = ModelLock::acquire(harness.out.path(), ProviderKind::Copilot, "m").expect("lock");
    let (result, ..) = harness.review(&stages, &harness.request());
    let error = result.expect_err("busy lock");

    assert!(
        matches!(
            error,
            ReviewError::Lock {
                source: LockError::Busy { .. },
                ..
            }
        ),
        "{error}"
    );
    assert!(stages.calls.borrow().is_empty());
}

#[test]
fn a_failed_discovery_keeps_its_drafts_and_says_how_to_verify_them() {
    let harness = Harness::new();
    let stages = FakeStages {
        found: vec![found(), draft()],
        fail: Some(Stage::Discovery),
        ..FakeStages::default()
    };
    let (result, ..) = harness.review(&stages, &harness.request());
    let error = result.expect_err("provider failed");
    let hint = error.hint().unwrap_or_default().to_owned();

    assert!(
        matches!(&error, ReviewError::Provider(failure) if failure.stage == Stage::Discovery),
        "{error}"
    );
    assert!(hint.contains("1 local draft finding(s) kept in"), "{hint}");
    assert!(hint.contains("--drafts"), "{hint}");
    assert!(harness.github.posted.borrow().is_empty());
}

#[test]
fn a_missing_focus_file_is_a_usage_error() {
    let harness = Harness::new();
    let stages = FakeStages::default();
    let request = Request {
        focus: Some(harness.worktree.path().join("absent.md")),
        ..harness.request()
    };
    let (result, ..) = harness.review(&stages, &request);
    let error = result.expect_err("missing focus");

    assert!(matches!(error, ReviewError::Usage(_)), "{error}");
    assert!(error.to_string().starts_with("focus file "), "{error}");
}
