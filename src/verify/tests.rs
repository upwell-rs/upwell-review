use std::fs;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::diffmap::diff_lines;
use crate::model::Severity;
use crate::testing::{DIFF, HEAD, draft, draft_with, write};

fn origin() -> Origin {
    Origin {
        provider: "codex".into(),
        model: "m".into(),
        effort: "e".into(),
    }
}

fn decision(id: &str) -> Value {
    draft_with(json!({"id": id, "decision": "keep", "reason": "checked"}))
}

fn marker(drafts: u64) -> Value {
    json!({"status": "DISCOVERY_DONE", "drafts": drafts, "summary": "s", "unchecked": ""})
}

fn inputs() -> Vec<VerifyInput> {
    ["D1", "D2"]
        .into_iter()
        .map(|id| VerifyInput {
            id: id.to_owned(),
            pr: 7,
            head: HEAD.to_owned(),
            finding: serde_json::from_value(draft()).expect("fixture is a finding"),
            origin: origin(),
        })
        .collect()
}

fn apply_answer(dir: &TempDir, decisions: Value) -> Result<Verified, VerifyError> {
    let answer = write(
        dir.path(),
        "verify.json",
        &json!({"summary": "s", "unchecked": "", "decisions": decisions}),
    );
    let mirror = dir.path().join("mirror.jsonl");
    let outputs = Outputs {
        run_dir: dir.path(),
        mirror: &mirror,
    };

    apply(
        &Assets::default(),
        &inputs(),
        &answer,
        &diff_lines(DIFF),
        &outputs,
        &origin(),
        false,
    )
}

fn review(dir: &TempDir) -> Review {
    let text = fs::read_to_string(dir.path().join("review.json")).expect("review.json written");

    serde_json::from_str(&text).expect("review.json is a review")
}

#[test]
fn a_done_marker_reporting_unsaved_drafts_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let path = write(dir.path(), "discovery.json", &marker(2));
    let error =
        empty(&Assets::default(), &path, dir.path(), origin()).expect_err("drafts were lost");

    assert!(
        error
            .to_string()
            .contains("reports 2 draft(s) but only 0 were saved"),
        "{error}"
    );
    assert!(!dir.path().join("review.json").exists());
}

#[test]
fn a_done_marker_reporting_more_drafts_than_were_saved_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let path = write(dir.path(), "discovery.json", &marker(3));
    let marker = done_marker(&Assets::default(), &path).expect("valid marker");
    let error = check_saved(&marker, 1).expect_err("drafts were lost");

    assert!(
        error
            .to_string()
            .contains("reports 3 draft(s) but only 1 were saved"),
        "{error}"
    );
}

#[test]
fn a_done_marker_matching_the_saved_drafts_is_accepted() {
    let dir = TempDir::new().expect("temp dir");
    let path = write(dir.path(), "discovery.json", &marker(1));
    let marker = done_marker(&Assets::default(), &path).expect("valid marker");

    check_saved(&marker, 1).expect("nothing was lost");
}

#[test]
fn a_done_marker_without_drafts_posts_a_clean_review() {
    let dir = TempDir::new().expect("temp dir");
    let path = write(dir.path(), "discovery.json", &marker(0));
    let stages = empty(&Assets::default(), &path, dir.path(), origin())
        .expect("clean review")
        .stages;

    assert_eq!(review(&dir).verdict, Verdict::NoFindings);
    assert_eq!(stages.verification, None);
    assert_eq!(stages.discovery.origins, [origin()]);
}

#[test]
fn a_missing_or_invalid_done_marker_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let missing = empty(
        &Assets::default(),
        &dir.path().join("none.json"),
        dir.path(),
        origin(),
    );
    let path = write(
        dir.path(),
        "discovery.json",
        &json!({"status": "DONE", "drafts": 0, "summary": "", "unchecked": ""}),
    );
    let invalid = empty(&Assets::default(), &path, dir.path(), origin());

    assert!(matches!(missing, Err(VerifyError::NoMarker(_))));
    assert!(matches!(invalid, Err(VerifyError::Marker(_))));
}

#[test]
fn an_answer_leaving_out_drafts_is_refused() {
    for decisions in [json!([]), json!([decision("D1")])] {
        let dir = TempDir::new().expect("temp dir");
        let error = apply_answer(&dir, decisions).expect_err("a draft has no decision");

        assert!(
            error.to_string().starts_with("no decision for draft"),
            "{error}"
        );
        assert!(!dir.path().join("review.json").exists());
    }
}

#[test]
fn an_answer_deciding_every_draft_is_applied() {
    let dir = TempDir::new().expect("temp dir");
    let stages = apply_answer(&dir, json!([decision("D1"), decision("D2")]))
        .expect("applied")
        .stages;
    let records = fs::read_to_string(dir.path().join("mirror.jsonl")).expect("mirror appended");

    assert_eq!(review(&dir).findings.len(), 2);
    assert_eq!(stages.verification.map(|run| run.keep), Some(2));
    assert_eq!(records.lines().count(), 2);
}

#[test]
fn unknown_and_duplicate_ids_are_refused() {
    let dir = TempDir::new().expect("temp dir");
    let unknown = apply_answer(
        &dir,
        json!([decision("D1"), decision("D2"), decision("D3")]),
    );
    let duplicate = apply_answer(
        &dir,
        json!([decision("D1"), decision("D1"), decision("D2")]),
    );

    assert_eq!(
        unknown.expect_err("D3 is new").to_string(),
        "decision for unknown draft id 'D3': the verifier may not add findings"
    );
    assert_eq!(
        duplicate.expect_err("D1 twice").to_string(),
        "more than one decision for draft D1"
    );
}

#[test]
fn a_modified_finding_outside_the_diff_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let mut moved = decision("D2");

    moved["decision"] = json!("modify");
    moved["line"] = json!(9);

    let error =
        apply_answer(&dir, json!([decision("D1"), moved])).expect_err("line 9 is not in the diff");

    assert_eq!(
        error.to_string(),
        "D2 (modify) targets src/lib.rs:9, which is not a line of the PR diff"
    );
}

#[test]
fn modify_posts_the_corrected_fields_and_drop_posts_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let mut modified = decision("D1");
    let mut dropped = decision("D2");

    modified["decision"] = json!("modify");
    modified["severity"] = json!("minor");
    dropped["decision"] = json!("drop");
    dropped["reason"] = json!("  ");

    apply_answer(&dir, json!([modified, dropped])).expect("applied");

    let records =
        fs::read_to_string(dir.path().join("verification.jsonl")).expect("records written");
    let records: Vec<Value> = records
        .lines()
        .map(|line| serde_json::from_str(line).expect("record"))
        .collect();

    assert_eq!(review(&dir).findings[0].severity, Severity::Minor);
    assert_eq!(records[1]["reason"], "(no reason given)");
    assert_eq!(records[1].get("final"), None);
    assert_eq!(records[0]["verifier"]["provider"], "codex");
}

#[test]
fn an_unreadable_answer_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let mirror = dir.path().join("mirror.jsonl");
    let outputs = Outputs {
        run_dir: dir.path(),
        mirror: &mirror,
    };
    let result = apply(
        &Assets::default(),
        &inputs(),
        &dir.path().join("verify.json"),
        &diff_lines(DIFF),
        &outputs,
        &origin(),
        false,
    );
    let error = result.expect_err("no answer");

    assert!(
        error
            .to_string()
            .starts_with("verification answer unreadable"),
        "{error}"
    );
    assert!(error.rejects());
}
