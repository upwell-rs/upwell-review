use serde_json::json;

use super::*;
use crate::assets::{Asset, Assets};
use crate::model::{DoneMarker, Review, VerifyAnswer};

fn schema(asset: Asset) -> Value {
    Assets::default()
        .schema(asset)
        .expect("embedded schema parses")
}

fn finding() -> Value {
    json!({"severity": "major", "path": "src/lib.rs", "line": 3, "title": "t", "body": "b", "suggestion": ""})
}

#[test]
fn a_valid_done_marker_decodes() {
    let value = json!({"status": "DISCOVERY_DONE", "drafts": 2, "summary": "s", "unchecked": "u"});
    let marker: DoneMarker =
        check(value, &schema(Asset::DiscoverySchema), "discovery").expect("valid marker");

    assert_eq!(marker.drafts, 2);
}

#[test]
fn violations_name_the_first_offending_field() {
    let cases = [
        (json!([]), "answer: expected object"),
        (
            json!({"status": "DISCOVERY_DONE", "drafts": 1, "summary": "s"}),
            "answer: missing unchecked",
        ),
        (
            json!({"status": "DONE", "drafts": 1, "summary": "s", "unchecked": ""}),
            "answer.status: \"DONE\" not in [\"DISCOVERY_DONE\"]",
        ),
        (
            json!({"status": "DISCOVERY_DONE", "drafts": -1, "summary": "s", "unchecked": ""}),
            "answer.drafts: below minimum 0",
        ),
        (
            json!({"status": "DISCOVERY_DONE", "drafts": 1.5, "summary": "s", "unchecked": ""}),
            "answer.drafts: expected integer",
        ),
        (
            json!({"status": "DISCOVERY_DONE", "drafts": true, "summary": "s", "unchecked": ""}),
            "answer.drafts: expected integer",
        ),
        (
            json!({"status": "DISCOVERY_DONE", "drafts": 0, "summary": "s", "unchecked": "", "extra": 1}),
            "answer: unexpected extra",
        ),
    ];

    for (value, expected) in cases {
        assert_eq!(
            violation(&value, &schema(Asset::DiscoverySchema), "answer").as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn array_items_are_validated_with_their_index() {
    let mut bad = finding();

    bad["line"] = json!(0);

    let review = json!({"verdict": "FINDINGS", "summary": "s", "unchecked": "", "findings": [finding(), bad]});

    assert_eq!(
        violation(&review, &schema(Asset::ReviewSchema), "review").as_deref(),
        Some("review.findings[1].line: below minimum 1")
    );
}

#[test]
fn a_verify_answer_decodes_into_typed_decisions() {
    let mut decision = finding();

    decision["id"] = json!("D1");
    decision["decision"] = json!("modify");
    decision["reason"] = json!("r");

    let value = json!({"summary": "s", "unchecked": "", "decisions": [decision]});
    let answer: VerifyAnswer =
        check(value, &schema(Asset::VerifySchema), "verification").expect("valid answer");

    assert_eq!(answer.decisions[0].finding.line, 3);
}

#[test]
fn a_weakened_schema_still_fails_the_typed_shape() {
    let value = json!({"verdict": "MAYBE", "summary": "s", "unchecked": "", "findings": []});
    let error = check::<Review>(value, &json!({}), "review")
        .expect_err("typed decoding rejects the verdict");

    assert!(matches!(error, SchemaError::Shape { .. }));
}
