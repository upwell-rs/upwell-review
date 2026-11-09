use std::fs;

use serde_json::{Value, json};

use super::*;
use crate::model::{Finding, Origin, Severity};

const DRAFT: &str = r#"{"status":"draft","severity":"major","path":"src/lib.rs","line":7,"title":"t","body":"b","suggestion":"s"}"#;

fn stamp() -> Stamp {
    Stamp {
        pr: 42,
        head: "0123456789abcdef".to_owned(),
        origin: Origin {
            provider: "copilot".to_owned(),
            model: "m".to_owned(),
            effort: "high".to_owned(),
        },
    }
}

fn saved_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("drafts file readable")
        .lines()
        .map(|line| serde_json::from_str(line).expect("saved line is JSON"))
        .collect()
}

#[test]
fn a_marker_is_found_anywhere_in_a_line() {
    let text = format!("- note: `{MARKER} {DRAFT}` (more prose)\n");
    let drafts = find_drafts(&text);

    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].finding.path, "src/lib.rs");
    assert_eq!(drafts[0].kind, None);
}

#[test]
fn two_markers_in_one_line_are_both_found() {
    let second = DRAFT.replace("\"line\":7", "\"line\":8");
    let text = format!("{MARKER} {DRAFT} and {MARKER}: {second}");

    assert_eq!(find_drafts(&text).len(), 2);
}

#[test]
fn a_status_word_becomes_the_severity_and_is_kept_as_kind() {
    let text =
        format!(r#"{MARKER} {{"status":"nit","path":"a.rs","line":"3","title":"t","body":"b"}}"#);
    let drafts = find_drafts(&text);

    assert_eq!(drafts[0].finding.severity, Severity::Nit);
    assert_eq!(drafts[0].finding.line, 3);
    assert_eq!(drafts[0].finding.suggestion, "");
    assert_eq!(drafts[0].kind.as_deref(), Some("nit"));
}

#[test]
fn an_explicit_severity_wins_over_the_status_word() {
    let text = format!(
        r#"{MARKER} {{"status":"nit","severity":"minor","path":"a.rs","line":3,"title":"t"}}"#
    );
    let drafts = find_drafts(&text);

    assert_eq!(drafts[0].finding.severity, Severity::Minor);
    assert_eq!(drafts[0].kind.as_deref(), Some("nit"));
}

#[test]
fn an_unknown_severity_word_is_kept_as_kind() {
    let text = format!(r#"{MARKER} {{"severity":"important","path":"a.rs","line":3,"title":"t"}}"#);
    let drafts = find_drafts(&text);

    assert_eq!(drafts[0].finding.severity, Severity::Minor);
    assert_eq!(drafts[0].kind.as_deref(), Some("important"));
}

#[test]
fn the_prompt_template_and_incomplete_drafts_are_rejected() {
    let cases = [
        r#"{"status":"draft","severity":"major","path":"<repo-relative path>","line":0,"title":"...","body":"...","suggestion":"..."}"#,
        r#"{"severity":"major","path":"a.rs","line":0,"title":"t"}"#,
        r#"{"severity":"major","path":" ","line":1,"title":"t"}"#,
        r#"{"severity":"major","line":1,"title":"t"}"#,
        r#"{"severity":"major","path":"a.rs","title":"t"}"#,
        r#"{"severity":"major","path":"a.rs","line":1}"#,
        r#"{"severity":"major","path":"a.rs","line":-1,"title":"t"}"#,
        r#"{"severity":"major","path":"a.rs","line":1,"title":5}"#,
        "not json",
    ];

    for case in cases {
        assert!(
            find_drafts(&format!("{MARKER} {case}")).is_empty(),
            "{case} must be rejected"
        );
    }

    assert!(find_drafts("I will print UPWELL_FINDING_DRAFT lines as I go").is_empty());
}

#[test]
fn draft_json_may_span_several_lines() {
    let text = format!(
        "{MARKER} {{\n  \"severity\": \"blocker\",\n  \"path\": \"./a.rs\",\n  \"line\": 9,\n  \"title\": \"t\"\n}}\n"
    );
    let drafts = find_drafts(&text);

    assert_eq!(drafts[0].finding.path, "a.rs");
    assert_eq!(drafts[0].finding.severity, Severity::Blocker);
}

#[test]
fn a_saving_sink_stamps_and_deduplicates_by_content() {
    let dir = tempfile::tempdir().expect("temp dir");
    let drafts = dir.path().join("drafts.jsonl");
    let mirror = dir.path().join("mirror.jsonl");
    let mut sink = DraftSink::saving(&[&drafts, &mirror], stamp()).expect("sink opens");
    let nit = DRAFT.replace("\"status\":\"draft\"", "\"status\":\"nit\"");

    sink.scan(&format!("{MARKER} {DRAFT}")).expect("scan");
    sink.scan(&format!("again: {MARKER} {DRAFT}"))
        .expect("scan");
    sink.scan(&format!("{MARKER} {nit}")).expect("scan");

    let saved = saved_lines(&drafts);

    assert_eq!(sink.saved(), 1);
    assert_eq!(saved, saved_lines(&mirror));
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["status"], "draft");
    assert_eq!(saved[0]["pr"], 42);
    assert_eq!(saved[0]["head"], "0123456789abcdef");
    assert_eq!(saved[0]["provider"], "copilot");
    assert!(saved[0].get("kind").is_none());
}

#[test]
fn a_disabled_sink_saves_nothing() {
    let mut sink = DraftSink::disabled();

    sink.scan(&format!("{MARKER} {DRAFT}")).expect("scan");

    assert_eq!(sink.saved(), 0);
}

#[test]
fn line_buffers_scan_a_draft_split_across_deltas() {
    let dir = tempfile::tempdir().expect("temp dir");
    let drafts = dir.path().join("drafts.jsonl");
    let mut sink = DraftSink::saving(&[&drafts], stamp()).expect("sink opens");
    let mut buffers = LineBuffers::new();
    let text = format!("intro\n{MARKER} {DRAFT}");
    let (first, second) = text.split_at(20);

    buffers.push("m1", first, &mut sink).expect("push");
    buffers.push("m1", second, &mut sink).expect("push");

    assert_eq!(sink.saved(), 0);

    buffers.finish_all(&mut sink).expect("finish");

    assert_eq!(sink.saved(), 1);
}

#[test]
fn read_lines_tees_raw_bytes_and_decodes_lossily() {
    let input: &[u8] = b"one\r\ntwo \xff\nthree";
    let mut log = Vec::new();
    let mut seen = Vec::new();

    read_lines(input, &mut log, |line| {
        seen.push(line.to_owned());
        Ok(())
    })
    .expect("read");

    assert_eq!(log, input);
    assert_eq!(seen, ["one", "two \u{fffd}", "three"]);
}

#[test]
fn the_fallback_answer_is_the_last_parsable_json_fence() {
    let response = "```rust\nfn x() {}\n```\nfirst:\n```json\n{\"a\": 1}\n```\nlast:\n```json \n{\"a\": 2}\n```\n```json\nnot json\n```\n";

    assert_eq!(fallback_answer(response), Some(json!({"a": 2})));
}

#[test]
fn without_a_fence_the_last_line_object_is_the_answer() {
    let response = "{\"a\": 1}\nprose {\"inline\": true}\n{\"a\": 3} trailing\n";

    assert_eq!(fallback_answer(response), Some(json!({"a": 3})));
    assert_eq!(fallback_answer("no json here"), None);
}

#[test]
fn normalized_findings_round_trip_through_the_saved_format() {
    let finding = Finding {
        severity: Severity::Nit,
        path: "a.rs".to_owned(),
        line: 1,
        title: "t".to_owned(),
        body: String::new(),
        suggestion: String::new(),
    };
    let stamp = stamp();
    let record = SavedDraft {
        finding: &finding,
        status: "draft",
        kind: Some("nit"),
        stamp: &stamp,
    };
    let value = serde_json::to_value(&record).expect("serializes");

    assert_eq!(value["kind"], "nit");
    assert_eq!(value["severity"], "nit");
    assert_eq!(value["model"], "m");
}
