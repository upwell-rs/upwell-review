use super::*;

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
