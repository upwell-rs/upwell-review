use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::model::{DiscoveryStage, Origin, Severity, Verdict, VerificationStage};
use crate::testing::{FakeGitHub, HEAD};

fn finding(severity: Severity, path: &str, line: u32, suggestion: &str) -> Finding {
    Finding {
        severity,
        path: path.to_owned(),
        line,
        title: "t".to_owned(),
        body: "b".to_owned(),
        suggestion: suggestion.to_owned(),
    }
}

fn origin(provider: &str) -> Origin {
    Origin {
        provider: provider.to_owned(),
        model: "m".to_owned(),
        effort: "high".to_owned(),
    }
}

fn verified() -> Stages {
    Stages {
        discovery: DiscoveryStage {
            origins: vec![origin("copilot"), origin("copilot"), origin("x")],
            drafts: 2,
            resumed: true,
        },
        verification: Some(VerificationStage {
            run: origin("codex"),
            keep: 1,
            modify: 1,
            drop: 0,
        }),
    }
}

fn review() -> Review {
    Review {
        verdict: Verdict::Findings,
        summary: "sum".to_owned(),
        findings: vec![
            finding(Severity::Nit, "src/lib.rs", 1, " "),
            finding(Severity::Major, "src/other.rs", 4, "do x"),
        ],
        unchecked: "the docs".to_owned(),
    }
}

#[test]
fn compose_anchors_visible_lines_and_lists_the_rest_in_the_body() {
    let review = review();
    let files = FakeGitHub::default().files;
    let composed = compose(&review, HEAD, &verified(), &files);
    let expected = concat!(
        "## Independent review (Copilot, x, verified by Codex) of `a1b2c3d4`\n\n**Verdict:** FINDINGS\n\nsum",
        "\n\n### Findings outside the changed lines\n",
        "\n- **[major]** `src/other.rs:4` t: b Suggested fix: do x",
        "\n\n_Could not check:_ the docs",
        "\n\n---\n_Discovery: Copilot `m`, effort high; Copilot `m`, effort high; x `m`, effort high: 2 draft(s), ",
        "verified from a saved drafts file. Verification (fresh run that saw only the drafts, the diff and the code): ",
        "Codex `m`, effort high: 1 kept, 1 modified, 0 dropped. ",
        "Read-only sandbox. Replaces CodeRabbit while the organisation is on the Free tier. Each inline comment is a ",
        "resolvable thread; the author answers and resolves it after pushing. This is not a bot approval._",
    );

    assert_eq!(composed.payload.body, expected);
    assert_eq!(composed.loose.len(), 1);
    assert_eq!(
        serde_json::to_value(&composed.payload.comments).expect("comments serialize"),
        json!([{"path": "src/lib.rs", "line": 1, "side": "RIGHT", "body": "**[nit] t**\n\nb"}])
    );
}

#[test]
fn a_review_without_drafts_says_verification_was_skipped() {
    let review = Review {
        verdict: Verdict::NoFindings,
        summary: "s".to_owned(),
        findings: Vec::new(),
        unchecked: String::new(),
    };
    let stages = Stages {
        discovery: DiscoveryStage {
            origins: vec![origin("codex")],
            drafts: 0,
            resumed: false,
        },
        verification: None,
    };
    let body = compose(&review, HEAD, &stages, &[]).payload.body;

    assert!(
        body.starts_with(
            "## Independent review (Codex) of `a1b2c3d4`\n\n**Verdict:** NO FINDINGS\n\ns\n\n---\n"
        ),
        "{body}"
    );
    assert!(
        body.contains(": 0 draft(s). Verification: skipped (no drafts). Read-only"),
        "{body}"
    );
}

#[test]
fn a_suggested_fix_is_appended_to_the_inline_comment() {
    let text = body::comment(&finding(Severity::Blocker, "a", 1, "use y"));

    assert_eq!(text, "**[blocker] t**\n\nb\n\n**Suggested fix:** use y");
}

#[test]
fn a_dry_run_writes_the_payload_and_posts_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let github = FakeGitHub::default();
    let report = post(&github, 7, HEAD, &review(), &verified(), dir.path(), true).expect("dry run");

    assert!(
        report.starts_with(
            "DRY RUN, not posted: verdict FINDINGS, 1 inline, 1 in the body\n## Independent"
        ),
        "{report}"
    );
    assert!(
        report.ends_with("\n--- inline src/lib.rs:1\n**[nit] t**\n\nb\n"),
        "{report}"
    );
    assert!(dir.path().join("payload.json").exists());
    assert!(github.posted.borrow().is_empty());
}

#[test]
fn a_posted_review_is_a_comment_on_the_head() {
    let dir = TempDir::new().expect("temp dir");
    let github = FakeGitHub::default();
    let report = post(&github, 7, HEAD, &review(), &verified(), dir.path(), false).expect("posted");
    let posted = github.posted.borrow();

    assert_eq!(
        report,
        "posted review of #7 at a1b2c3d4: 1 inline, 1 in the body, https://github.com/upwell-rs/upwell/pull/1#pullrequestreview-1\n"
    );
    assert_eq!(posted[0].commit_id, HEAD);
    assert_eq!(posted[0].event, ReviewEvent::Comment);
    assert!(!dir.path().join("payload.json").exists());
}
