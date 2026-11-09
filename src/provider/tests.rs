use std::ffi::OsStr;
use std::path::PathBuf;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::model::{Origin, Stamp};

const CODEX: &str = include_str!("tests/fixtures/codex-discovery.txt");
const COPILOT_CLAUDE: &str = include_str!("tests/fixtures/copilot-claude.jsonl");
const COPILOT_FAILED: &str = include_str!("tests/fixtures/copilot-failed.jsonl");
const AGY: &str = include_str!("tests/fixtures/agy-discovery.jsonl");
const AGY_UNSTRUCTURED: &str = include_str!("tests/fixtures/agy-verify-unstructured.jsonl");
const AGY_FAILED: &str = include_str!("tests/fixtures/agy-failed.jsonl");

/// A run directory with the files of one stage.
struct Stage {
    dir: TempDir,
}

impl Stage {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");

        fs::write(dir.path().join("prompt.md"), "review this").expect("prompt written");

        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn sink(&self) -> DraftSink {
        let stamp = Stamp {
            pr: 412,
            head: "a1b2c3d4e5f6".to_owned(),
            origin: Origin {
                provider: "p".to_owned(),
                model: "m".to_owned(),
                effort: "e".to_owned(),
            },
        };

        DraftSink::saving(&[&self.path("drafts.jsonl")], stamp).expect("sink opens")
    }

    fn saved(&self) -> Vec<Value> {
        fs::read_to_string(self.path("drafts.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("saved draft is JSON"))
            .collect()
    }

    fn answer(&self) -> Value {
        let text = fs::read_to_string(self.path("answer.json")).expect("answer written");

        serde_json::from_str(&text).expect("answer is JSON")
    }

    /// Parses `fixture` as `provider`'s output, then finishes the stage as a real run would.
    fn stream<P: Provider>(
        &self,
        provider: &P,
        fixture: &str,
    ) -> (Result<(), StreamError>, Vec<u8>) {
        let (prompt, answer, schema) = (
            self.path("prompt.md"),
            self.path("answer.json"),
            self.path("s.json"),
        );
        let call = self.call(&prompt, &answer, &schema);
        let mut sink = self.sink();
        let mut log = Vec::new();
        let result = provider
            .parse(fixture.as_bytes(), &mut log, &mut sink)
            .and_then(|transcript| provider.finish(transcript, &call, &mut sink));

        (result, log)
    }

    fn call<'a>(&'a self, prompt: &'a Path, answer: &'a Path, schema: &'a Path) -> Invocation<'a> {
        Invocation {
            model: "the-model",
            effort: "high",
            schema,
            prompt,
            answer,
            worktree: Path::new("/work/tree"),
            run: self.dir.path(),
        }
    }
}

fn titles(saved: &[Value]) -> Vec<&str> {
    saved
        .iter()
        .map(|draft| draft["title"].as_str().expect("title"))
        .collect()
}

fn arguments(command: &Command) -> Vec<&str> {
    command
        .get_args()
        .map(|argument| argument.to_str().expect("utf-8 argument"))
        .collect()
}

fn has_pair(arguments: &[&str], flag: &str, value: &str) -> bool {
    arguments.windows(2).any(|pair| pair == [flag, value])
}

#[test]
fn provider_names_parse_and_unknown_names_are_refused() {
    assert_eq!(
        "gemini".parse::<ProviderKind>().expect("known"),
        ProviderKind::Gemini
    );
    assert_eq!(
        "claude"
            .parse::<ProviderKind>()
            .expect_err("unknown")
            .to_string(),
        "unknown provider 'claude'; expected codex, gemini or copilot"
    );
    assert_eq!(ProviderKind::Gemini.recorded_effort("high"), GEMINI_EFFORT);
    assert_eq!(ProviderKind::Copilot.recorded_effort("high"), "high");
    assert_eq!(ProviderKind::label_of("codex"), "Codex");
    assert_eq!(ProviderKind::label_of("unknown"), "unknown");
}

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

#[test]
fn the_codex_command_is_read_only_and_ephemeral() {
    let stage = Stage::new();
    let (prompt, answer, schema) = (
        stage.path("prompt.md"),
        stage.path("answer.json"),
        stage.path("s.json"),
    );
    let command = Codex
        .command(&stage.call(&prompt, &answer, &schema))
        .expect("command");
    let arguments = arguments(&command);

    assert_eq!(command.get_program(), "codex");
    assert_eq!(arguments[..3], ["exec", "-m", "the-model"]);
    assert!(has_pair(&arguments, "-c", "model_reasoning_effort=high"));
    assert!(has_pair(&arguments, "-s", "read-only"));
    assert!(arguments.contains(&"--ephemeral"));
    assert!(has_pair(&arguments, "-C", "/work/tree"));
    assert_eq!(arguments.last(), Some(&"-"));
}

#[test]
fn the_copilot_command_allows_only_read_only_tools() {
    let stage = Stage::new();
    let (prompt, answer, schema) = (
        stage.path("prompt.md"),
        stage.path("answer.json"),
        stage.path("s.json"),
    );
    let command = Copilot
        .command(&stage.call(&prompt, &answer, &schema))
        .expect("command");
    let arguments = arguments(&command);
    let tools = arguments
        .iter()
        .position(|argument| *argument == "--available-tools")
        .expect("tool list");

    assert_eq!(command.get_program(), "copilot");
    assert_eq!(command.get_current_dir(), Some(Path::new("/work/tree")));
    assert!(has_pair(&arguments, "-p", "review this"));
    assert!(has_pair(&arguments, "--reasoning-effort", "high"));
    assert_eq!(
        arguments[tools + 1..tools + 1 + copilot::TOOLS.len()],
        copilot::TOOLS
    );
    assert!(arguments.contains(&"--disable-builtin-mcps"));
    assert!(arguments.contains(&"--no-ask-user"));

    for denied in [
        "--deny-tool=shell",
        "--deny-tool=write",
        "--deny-tool=memory",
        "--deny-tool=url",
    ] {
        assert!(arguments.contains(&denied), "{denied} missing");
    }

    for server in ["MCP_DOCKER", "playwright", "railway"] {
        assert!(
            has_pair(&arguments, "--disable-mcp-server", server),
            "{server} not disabled"
        );
    }

    assert!(
        !arguments
            .iter()
            .any(|argument| argument.starts_with("--allow-all"))
    );
}

#[test]
fn the_agy_command_runs_in_plan_mode_with_the_mcp_allow_list() {
    let stage = Stage::new();
    let (prompt, answer, schema) = (
        stage.path("prompt.md"),
        stage.path("answer.json"),
        stage.path("s.json"),
    );
    let command = Gemini
        .command(&stage.call(&prompt, &answer, &schema))
        .expect("command");
    let arguments = arguments(&command);
    let run = stage.dir.path().to_str().expect("utf-8 temp dir");

    assert_eq!(command.get_program(), OsStr::new("agy"));
    assert!(has_pair(&arguments, "--mode", "plan"));
    assert!(arguments.contains(&"--sandbox"));
    assert!(has_pair(&arguments, "--output-format", "stream-json"));
    assert!(has_pair(&arguments, "--settings", gemini::SETTINGS));
    assert!(has_pair(&arguments, "--add-dir", "/work/tree"));
    assert!(has_pair(&arguments, "--add-dir", run));
    assert!(has_pair(&arguments, "-p", "review this"));
}

/// A provider whose command runs a shell script (or a missing program), parsed as copilot output.
struct Replay {
    program: &'static str,
    script: String,
}

impl Provider for Replay {
    const KIND: ProviderKind = ProviderKind::Copilot;

    fn command(&self, _call: &Invocation<'_>) -> io::Result<Command> {
        let mut command = Command::new(self.program);

        command.args(["-c", &self.script]);

        Ok(command)
    }

    fn parse<R: BufRead, L: Write>(
        &self,
        input: R,
        log: &mut L,
        sink: &mut DraftSink,
    ) -> Result<Transcript, StreamError> {
        Copilot.parse(input, log, sink)
    }
}

#[test]
fn a_run_merges_stderr_into_the_stream_and_reports_a_failed_exit() {
    let stage = Stage::new();
    let fixture = stage.path("fixture.jsonl");
    let (prompt, answer, schema) = (
        stage.path("prompt.md"),
        stage.path("answer.json"),
        stage.path("s.json"),
    );
    let call = stage.call(&prompt, &answer, &schema);
    let script = format!(
        "cat '{}'; echo 'UPWELL_FINDING_DRAFT {{\"severity\":\"major\",\"path\":\"e.rs\",\"line\":1,\"title\":\"from stderr\"}}' >&2; exit 3",
        fixture.display()
    );
    let mut sink = stage.sink();

    fs::write(&fixture, COPILOT_CLAUDE).expect("fixture written");
    fs::write(&answer, "stale").expect("stale answer");

    let error = run::run(
        &Replay {
            program: "sh",
            script,
        },
        &call,
        &stage.path("stage.log"),
        &mut sink,
    )
    .expect_err("exit 3 fails");
    let log = fs::read_to_string(stage.path("stage.log")).expect("log written");

    assert!(
        matches!(error, ProviderError::Exited { cli: "copilot", status } if status.code() == Some(3))
    );
    assert!(log.contains("from stderr"));
    assert_eq!(stage.saved().len(), 3);
    assert_eq!(stage.answer()["status"], "DISCOVERY_DONE");
}

#[test]
fn a_missing_cli_is_a_spawn_error() {
    let stage = Stage::new();
    let (prompt, answer, schema) = (
        stage.path("prompt.md"),
        stage.path("answer.json"),
        stage.path("s.json"),
    );
    let call = stage.call(&prompt, &answer, &schema);
    let replay = Replay {
        program: "upwell-review-test-missing-cli",
        script: String::new(),
    };
    let mut sink = DraftSink::disabled();
    let result = run::run(&replay, &call, &stage.path("stage.log"), &mut sink);

    assert!(matches!(
        result,
        Err(ProviderError::Spawn { cli: "copilot", .. })
    ));
}
