use super::*;

#[test]
fn codex_drafts_are_found_after_prose_and_across_lines_and_the_template_is_ignored() {
    let stage = Stage::new();
    let (result, log) = stage.stream(&Codex, CODEX);
    let saved = stage.saved();

    result.expect("codex stream is fine");

    assert_eq!(log, CODEX.as_bytes());
    assert_eq!(
        titles(&saved),
        [
            "Shutdown hook runs twice",
            "Env override ignores empty strings",
            "Typo in error message"
        ]
    );
    assert_eq!(saved[1]["path"], "crates/upwell-config/src/load.rs");
    assert_eq!(saved[1]["line"], 41);
}

#[test]
fn codex_drafts_in_the_written_answer_are_saved() {
    let stage = Stage::new();
    let draft = r#"UPWELL_FINDING_DRAFT {"severity":"major","path":"a.rs","line":2,"title":"only in the answer"}"#;

    fs::write(stage.path("answer.json"), draft).expect("answer written");

    let (result, _) = stage.stream(&Codex, "no drafts in the stream\n");

    result.expect("codex stream is fine");
    assert_eq!(titles(&stage.saved()), ["only in the answer"]);
}

#[test]
fn copilot_claude_reasoning_only_and_nit_status_drafts_are_saved_once() {
    let stage = Stage::new();
    let (result, _) = stage.stream(&Copilot, COPILOT_CLAUDE);
    let saved = stage.saved();

    result.expect("copilot stream is fine");

    assert_eq!(
        titles(&saved),
        [
            "Publisher holds the bus lock across await",
            "Doc comment names the old trait"
        ]
    );
    assert_eq!(saved[1]["severity"], "nit");
    assert_eq!(saved[1]["kind"], "nit");
    assert_eq!(saved[1]["status"], "draft");
    assert_eq!(
        stage.answer(),
        json!({"status":"DISCOVERY_DONE","drafts":2,"summary":"Reviewed the topic bus and codec.","unchecked":"Did not run the tests."})
    );
    assert!(
        fs::read_to_string(stage.path("answer.response.md"))
            .expect("response saved")
            .starts_with("```json")
    );
}

#[test]
fn a_failed_copilot_run_keeps_its_drafts_and_reports_the_exit_code() {
    let stage = Stage::new();
    let (result, _) = stage.stream(&Copilot, COPILOT_FAILED);
    let error = result.expect_err("exit code 1 fails");

    assert!(matches!(&error, StreamError::Unsuccessful { detail, .. } if detail == "exit code 1"));
    assert_eq!(titles(&stage.saved()), ["t"]);
    assert!(!stage.path("answer.json").exists());
}

#[test]
fn a_copilot_stream_without_a_result_fails() {
    let stage = Stage::new();
    let (result, _) = stage.stream(&Copilot, "{\"type\":\"session.start\",\"data\":{}}\n");

    assert!(matches!(
        result,
        Err(StreamError::NoResult {
            provider: "copilot"
        })
    ));
}

#[test]
fn agy_text_and_thinking_drafts_are_saved_and_the_structured_answer_written() {
    let stage = Stage::new();
    let (result, _) = stage.stream(&Gemini, AGY);

    result.expect("agy stream is fine");

    assert_eq!(
        titles(&stage.saved()),
        ["Fallback route shadows /health", "Layer order reversed"]
    );
    assert_eq!(stage.answer()["drafts"], 2);
}

#[test]
fn agy_without_structured_output_falls_back_to_the_last_json_block() {
    let stage = Stage::new();
    let (result, _) = stage.stream(&Gemini, AGY_UNSTRUCTURED);

    result.expect("agy stream is fine");

    assert_eq!(stage.answer()["decisions"][0]["id"], "D1");
}

#[test]
fn an_unsuccessful_agy_result_saves_the_response_and_fails() {
    let stage = Stage::new();
    let (result, _) = stage.stream(&Gemini, AGY_FAILED);

    assert!(
        matches!(result, Err(StreamError::Unsuccessful { detail, .. }) if detail == "status QUOTA_EXCEEDED")
    );
    assert_eq!(
        fs::read_to_string(stage.path("answer.response.md")).expect("saved"),
        "I hit a quota."
    );
}

#[test]
fn an_answer_without_any_json_fails() {
    let stage = Stage::new();
    let fixture = r#"{"event":"result","result":{"status":"SUCCESS","response":"no json"}}"#;
    let (result, _) = stage.stream(&Gemini, fixture);

    assert!(matches!(
        result,
        Err(StreamError::NoAnswer {
            provider: "agy",
            ..
        })
    ));
}
