use std::cell::RefCell;
use std::path::Path;

use super::*;
use crate::testing::{FakeGit, FakeGitHub};

fn found(program: &str) -> Option<PathBuf> {
    (program != "agy").then(|| Path::new("/usr/bin").join(program))
}

fn outcome<'a>(checks: &'a [Check], name: &str) -> &'a Outcome {
    &checks
        .iter()
        .find(|check| check.name == name)
        .expect("check is reported")
        .outcome
}

fn broken_prompts_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");

    std::fs::write(dir.path().join("review-schema.json"), "{").expect("broken override");
    dir
}

#[test]
fn a_ready_machine_passes_every_check() {
    let git = FakeGit::default();
    let github = FakeGitHub::default();
    let mixes = Mixes::load(None).expect("built-in mixes");
    let assets = Assets::default();
    let checks = run(&Subject {
        git: Some(&git),
        github: &github,
        find: found,
        mise: true,
        providers: &[ProviderKind::Codex, ProviderKind::Copilot],
        config: Ok(&mixes),
        assets: Ok(&assets),
    });
    let text = render(&checks);

    assert_eq!(failures(&checks), 0, "{text}");
    assert_eq!(
        outcome(&checks, "codex CLI (codex)"),
        &Outcome::Passed("/usr/bin/codex".to_owned())
    );
    assert!(text.contains("ok   mise: /usr/bin/mise"), "{text}");
    assert!(
        text.contains("ok   prompts and schemas: 9 loaded"),
        "{text}"
    );
}

#[test]
fn missing_clis_a_logged_out_gh_and_a_dirty_branch_checkout_fail() {
    let git = FakeGit {
        detached: false,
        dirty: RefCell::new(true),
        ..FakeGit::default()
    };
    let github = FakeGitHub {
        authenticated: false,
        ..FakeGitHub::default()
    };
    let mixes = Mixes::load(None).expect("built-in mixes");
    let assets = Assets::default();
    let checks = run(&Subject {
        git: Some(&git),
        github: &github,
        find: found,
        mise: false,
        providers: &ProviderKind::ALL,
        config: Ok(&mixes),
        assets: Ok(&assets),
    });
    let text = render(&checks);

    assert_eq!(failures(&checks), 4, "{text}");
    assert!(
        text.contains("FAIL gemini CLI (agy): agy is not on PATH"),
        "{text}"
    );
    assert!(
        text.contains("FAIL gh auth: gh auth status failed: not logged in"),
        "{text}"
    );
    assert!(text.contains("FAIL worktree clean"), "{text}");
    assert!(text.contains("FAIL worktree detached"), "{text}");
    assert!(!text.contains("mise"), "{text}");
}

#[test]
fn a_broken_providers_file_or_schema_override_fails_and_a_missing_worktree_is_skipped() {
    let dir = broken_prompts_dir();
    let github = FakeGitHub::default();
    let error = Mixes::load(Some(&dir.path().join("providers.toml"))).expect_err("missing file");
    let assets = Assets::new(Some(dir.path().to_owned())).expect("prompts dir");
    let checks = run(&Subject::<FakeGit, _, _> {
        git: None,
        github: &github,
        find: found,
        mise: false,
        providers: &[],
        config: Err(&error),
        assets: Ok(&assets),
    });

    assert_eq!(
        outcome(&checks, "worktree"),
        &Outcome::Skipped("no worktree given".to_owned())
    );
    assert!(matches!(
        outcome(&checks, "providers file"),
        Outcome::Failed(_)
    ));
    assert!(matches!(
        outcome(&checks, "prompts and schemas"),
        Outcome::Failed(_)
    ));
    assert_eq!(failures(&checks), 2);
}
