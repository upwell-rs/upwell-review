use clap::{CommandFactory, Parser};
use serde_json::json;

use super::*;
use crate::config::StageRun;
use crate::exit::Exit;
use crate::testing::{draft, write};

fn parse(arguments: &[&str]) -> Cli {
    Cli::try_parse_from(["upwell-review"].iter().chain(arguments)).expect("arguments parse")
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn request(arguments: &[&str]) -> Result<Request, Failure> {
    let Cli { global, command } = parse(arguments);

    match command {
        Command::Review(args) => review_request(&global, args, &no_env),
        Command::Verify(args) => verify_request(&global, args, &no_env),
        other => panic!("not a review: {other:?}"),
    }
}

fn run_output(arguments: &[&str]) -> (Result<(), Failure>, String) {
    let mut console = Console {
        out: Vec::new(),
        err: Vec::new(),
    };
    let result = run(parse(arguments), &no_env, &mut console);

    (result, String::from_utf8_lossy(&console.out).into_owned())
}

fn stage(provider: ProviderKind, model: &str, effort: &str) -> StageRun {
    StageRun {
        provider,
        model: model.to_owned(),
        effort: effort.to_owned(),
    }
}

#[test]
fn the_command_line_definition_is_consistent() {
    Cli::command().debug_assert();
}

#[test]
fn legacy_positions_apply_and_options_override_them() {
    let request = request(&[
        "review", "12", "/wt", "m1", "low", "copilot", "", "codex", "", "", "--model", "m2",
    ])
    .expect("request");

    assert_eq!(request.pr, 12);
    assert_eq!(
        request.plan.discovery,
        stage(ProviderKind::Copilot, "m2", "low")
    );
    assert_eq!(request.plan.verify, stage(ProviderKind::Codex, "m2", "low"));
    assert_eq!(request.focus, None);
    assert_eq!(request.verify_only, None);
    assert!(!request.dry_run);
}

#[test]
fn a_mix_sets_both_stages_and_options_override_it() {
    let request = request(&[
        "review",
        "1",
        "/wt",
        "--mix",
        "risky",
        "--verify-effort",
        "xhigh",
    ])
    .expect("request");

    assert_eq!(
        request.plan.discovery,
        stage(ProviderKind::Copilot, "claude-opus-5.5", "high")
    );
    assert_eq!(
        request.plan.verify,
        stage(ProviderKind::Codex, "gpt-6.1-terra", "xhigh")
    );
}

#[test]
fn verify_resumes_from_a_drafts_file() {
    let request = request(&[
        "verify",
        "3",
        "/wt",
        "--drafts",
        "d.jsonl",
        "--dry-run",
        "--provider",
        "gemini",
    ])
    .expect("request");

    assert_eq!(request.verify_only, Some(PathBuf::from("d.jsonl")));
    assert!(request.dry_run);
    assert_eq!(request.plan.verify.provider, ProviderKind::Gemini);
}

#[test]
fn unknown_providers_and_mixes_are_refused() {
    let provider = request(&["review", "1", "/wt", "--verify-provider", "claude"])
        .expect_err("unknown provider");
    let mix = request(&["review", "1", "/wt", "--mix", "fancy"]).expect_err("unknown mix");

    assert_eq!(provider.exit(), Exit::UnknownProvider);
    assert_eq!(mix.exit(), Exit::Error);
    assert!(
        mix.to_string().contains("known mixes: cheap, risky"),
        "{mix}"
    );
}

#[test]
fn mixes_lists_the_built_in_mixes() {
    let (result, out) = run_output(&["mixes"]);

    result.expect("mixes listed");
    assert!(out.contains("built-in mixes"), "{out}");
    assert!(
        out.contains(
            "risky: discovery copilot claude-opus-5.5 (high); verify codex gpt-6.1-terra (medium)"
        ),
        "{out}"
    );
}

#[test]
fn drafts_prints_a_drafts_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = write(dir.path(), "drafts.jsonl", &json!([draft()]));
    let (result, out) = run_output(&["drafts", &file.to_string_lossy()]);

    result.expect("drafts listed");
    assert!(out.starts_with("1 draft(s), unverified"), "{out}");
    assert!(out.contains("src/lib.rs:1  major  t"), "{out}");
}

#[test]
fn validate_accepts_a_matching_answer_and_refuses_a_violation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let marker = json!({"status": "DISCOVERY_DONE", "drafts": 0, "summary": "s", "unchecked": ""});
    let good = write(dir.path(), "good.json", &marker);
    let bad = write(dir.path(), "bad.json", &json!({"status": "DONE"}));
    let (accepted, out) = run_output(&["validate", "discovery", &good.to_string_lossy()]);
    let (refused, _) = run_output(&["validate", "discovery", &bad.to_string_lossy()]);
    let (unknown, _) = run_output(&["validate", "nothing", &good.to_string_lossy()]);

    accepted.expect("valid marker");
    assert!(out.contains("matches the discovery schema"), "{out}");
    assert_eq!(refused.expect_err("invalid marker").exit(), Exit::Invalid);
    assert_eq!(unknown.expect_err("unknown schema").exit(), Exit::Error);
}
