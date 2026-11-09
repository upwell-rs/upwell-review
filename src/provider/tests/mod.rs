mod commands;
mod runs;
mod streams;

use std::ffi::OsStr;
use std::path::PathBuf;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::model::{Origin, Stamp};

const CODEX: &str = include_str!("fixtures/codex-discovery.txt");
const COPILOT_CLAUDE: &str = include_str!("fixtures/copilot-claude.jsonl");
const COPILOT_FAILED: &str = include_str!("fixtures/copilot-failed.jsonl");
const AGY: &str = include_str!("fixtures/agy-discovery.jsonl");
const AGY_UNSTRUCTURED: &str = include_str!("fixtures/agy-verify-unstructured.jsonl");
const AGY_FAILED: &str = include_str!("fixtures/agy-failed.jsonl");

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
