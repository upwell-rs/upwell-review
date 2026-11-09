//! Runs the built binary and checks its exit codes, output and environment handling.

use std::process::{Command, Output};

use tempfile::TempDir;

/// A scratch home and output directory for one binary run.
struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("temp dir"),
        }
    }

    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).to_string_lossy().into_owned()
    }

    fn write(&self, name: &str, text: &str) -> String {
        let path = self.path(name);

        std::fs::write(&path, text).expect("fixture written");
        path
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_upwell-review"));

        command
            .env_clear()
            .env("HOME", self.dir.path())
            .env("UPWELL_REVIEW_OUT", self.dir.path().join("out"));
        command
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.command()
            .args(arguments)
            .output()
            .expect("binary runs")
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exited normally")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_and_version_succeed() {
    let sandbox = Sandbox::new();
    let help = sandbox.run(&["--help"]);
    let version = sandbox.run(&["--version"]);

    assert_eq!(code(&help), 0, "{}", stderr(&help));
    assert!(stdout(&help).contains("doctor"), "{}", stdout(&help));
    assert_eq!(code(&version), 0);
    assert!(stdout(&version).starts_with("upwell-review "));
}

#[test]
fn usage_errors_exit_one_like_the_legacy_script() {
    let sandbox = Sandbox::new();
    let missing = sandbox.run(&["review"]);
    let not_a_number = sandbox.run(&["review", "twelve", "/wt"]);

    assert_eq!(code(&missing), 1, "{}", stderr(&missing));
    assert_eq!(code(&not_a_number), 1, "{}", stderr(&not_a_number));
}

#[test]
fn an_unknown_provider_exits_eight_before_touching_git() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["review", "1", "/nonexistent", "--provider", "claude"]);

    assert_eq!(code(&output), 8, "{}", stderr(&output));
    assert!(
        stderr(&output).starts_with("upwell-review: "),
        "{}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("claude"), "{}", stderr(&output));
}

#[test]
fn legacy_environment_variables_select_the_verifier() {
    let sandbox = Sandbox::new();
    let output = sandbox
        .command()
        .args(["review", "1", "/nonexistent"])
        .env("UPWELL_VERIFY_PROVIDER", "nope")
        .output()
        .expect("binary runs");

    assert_eq!(code(&output), 8, "{}", stderr(&output));
}

#[test]
fn the_legacy_dry_run_variable_is_accepted() {
    let sandbox = Sandbox::new();
    let output = sandbox
        .command()
        .args(["review", "1", "/nonexistent", "--provider", "claude"])
        .env("UPWELL_REVIEW_DRY_RUN", "1")
        .output()
        .expect("binary runs");

    assert_eq!(code(&output), 8, "{}", stderr(&output));
}

#[test]
fn a_missing_focus_or_drafts_file_exits_one() {
    let sandbox = Sandbox::new();
    let focus = sandbox.path("focus.md");
    let drafts = sandbox.path("drafts.jsonl");
    let no_focus = sandbox.run(&["review", "1", "/nonexistent", "--focus", &focus]);
    let no_drafts = sandbox.run(&["verify", "1", "/nonexistent", "--drafts", &drafts]);

    assert_eq!(code(&no_focus), 1, "{}", stderr(&no_focus));
    assert!(
        stderr(&no_focus).contains("focus.md"),
        "{}",
        stderr(&no_focus)
    );
    assert_eq!(code(&no_drafts), 1, "{}", stderr(&no_drafts));
    assert!(
        stderr(&no_drafts).contains("drafts.jsonl"),
        "{}",
        stderr(&no_drafts)
    );
}

#[test]
fn mixes_reads_the_providers_file_from_the_environment() {
    let sandbox = Sandbox::new();
    let file = sandbox.write(
        "providers.toml",
        "[mix.deep]\ndiscovery = { provider = \"codex\", model = \"big\", effort = \"xhigh\" }\n\
         verify = { provider = \"gemini\", model = \"pro\" }\n",
    );
    let output = sandbox
        .command()
        .arg("mixes")
        .env("UPWELL_REVIEW_PROVIDERS", &file)
        .output()
        .expect("binary runs");

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains(&file), "{}", stdout(&output));
    assert!(
        stdout(&output).contains("deep: discovery codex big (xhigh); verify gemini pro (medium)"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_broken_providers_file_exits_one_and_names_the_file() {
    let sandbox = Sandbox::new();
    let file = sandbox.write("providers.toml", "[mix.x]\ndiscovery = 3\n");
    let output = sandbox.run(&["--providers", &file, "mixes"]);

    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(stderr(&output).contains(&file), "{}", stderr(&output));
}

#[test]
fn validate_exits_zero_or_three() {
    let sandbox = Sandbox::new();
    let good = sandbox.write(
        "good.json",
        r#"{"verdict": "NO_FINDINGS", "summary": "s", "unchecked": "", "findings": []}"#,
    );
    let bad = sandbox.write("bad.json", r#"{"verdict": "MAYBE"}"#);
    let accepted = sandbox.run(&["validate", "review", &good]);
    let refused = sandbox.run(&["validate", "review", &bad]);

    assert_eq!(code(&accepted), 0, "{}", stderr(&accepted));
    assert!(stdout(&accepted).contains("matches the review schema"));
    assert_eq!(code(&refused), 3, "{}", stderr(&refused));
}

#[test]
fn drafts_prints_a_saved_drafts_file() {
    let sandbox = Sandbox::new();
    let file = sandbox.write(
        "drafts.jsonl",
        concat!(
            r#"{"id": "D1", "path": "src/lib.rs", "line": 4, "severity": "minor", "#,
            r#""title": "off by one", "body": "b", "evidence": "e"}"#,
            "\n"
        ),
    );
    let output = sandbox.run(&["drafts", &file]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).contains("src/lib.rs:4  minor  off by one"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn doctor_refuses_an_unknown_provider() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["doctor", "--provider", "claude"]);

    assert_eq!(code(&output), 8, "{}", stderr(&output));
}

#[test]
fn the_binary_does_not_need_a_config_directory() {
    let sandbox = Sandbox::new();
    let output = sandbox
        .command()
        .env_remove("HOME")
        .arg("mixes")
        .output()
        .expect("binary runs");

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stdout(&output).starts_with("built-in mixes (no providers file)"),
        "{}",
        stdout(&output)
    );
}
