use super::*;

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
