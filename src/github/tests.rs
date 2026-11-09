use serde_json::json;

use super::*;

#[test]
fn gh_pr_view_output_decodes() {
    let output = r#"{"baseRefName":"release/1.0.0","baseRefOid":"b4se","headRefOid":"h3ad","title":"feat: <b>x</b>"}"#;
    let pr: PullRequest = serde_json::from_str(output).expect("gh pr view shape");

    assert_eq!(pr.base_ref_name, "release/1.0.0");
    assert_eq!(pr.head_ref_oid, "h3ad");
}

#[test]
fn slurped_file_pages_decode_with_and_without_patches() {
    let output = r#"[[{"sha":"1","filename":"a.rs","status":"modified","patch":"@@ -1 +1 @@\n-a\n+b"}],[{"filename":"logo.png","status":"added"}]]"#;
    let pages: Vec<Vec<PrFile>> = serde_json::from_str(output).expect("slurped pages");

    assert_eq!(pages[0][0].patch.as_deref(), Some("@@ -1 +1 @@\n-a\n+b"));
    assert_eq!(pages[1][0].patch, None);
}

#[test]
fn the_payload_is_a_comment_review_on_the_right_side() {
    let payload = ReviewPayload {
        commit_id: "h3ad".to_owned(),
        event: ReviewEvent::Comment,
        body: "body".to_owned(),
        comments: vec![InlineComment {
            path: "a.rs".to_owned(),
            line: 2,
            side: Side::Right,
            body: "c".to_owned(),
        }],
    };

    assert_eq!(
        serde_json::to_value(&payload).expect("serializes"),
        json!({"commit_id": "h3ad", "event": "COMMENT", "body": "body", "comments": [{"path": "a.rs", "line": 2, "side": "RIGHT", "body": "c"}]})
    );
}
