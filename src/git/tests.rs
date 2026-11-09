use std::fs;
use std::process::Command;

use super::*;

fn git(dir: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(arguments)
        .output()
        .expect("git runs")
        .status;

    assert!(status.success(), "git {arguments:?} failed");
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");

    git(dir.path(), &["init", "-q", "-b", "main"]);
    fs::write(dir.path().join("lib.rs"), "one\n").expect("file written");
    git(dir.path(), &["add", "lib.rs"]);
    git(dir.path(), &["commit", "-q", "-m", "first"]);
    dir
}

#[test]
fn head_cleanliness_and_detachment_are_read_from_the_checkout() {
    let dir = repo();
    let cli = GitCli::new(Tools { mise: false }, dir.path());

    assert_eq!(cli.head().expect("head").len(), 40);
    assert!(!cli.has_tracked_changes().expect("status"));
    assert!(!cli.is_detached().expect("branch"));

    fs::write(dir.path().join("new.rs"), "untracked\n").expect("file written");

    assert!(
        !cli.has_tracked_changes()
            .expect("untracked files do not count")
    );

    fs::write(dir.path().join("lib.rs"), "two\n").expect("file written");

    assert!(cli.has_tracked_changes().expect("status"));

    git(dir.path(), &["checkout", "-q", "--detach"]);

    assert!(cli.is_detached().expect("detached"));
}

#[test]
fn the_three_dot_diff_is_against_the_merge_base() {
    let dir = repo();
    let cli = GitCli::new(Tools { mise: false }, dir.path());
    let base = cli.head().expect("head");

    fs::write(dir.path().join("lib.rs"), "one\ntwo\n").expect("file written");
    git(dir.path(), &["commit", "-q", "-am", "second"]);

    let head = cli.head().expect("head");
    let merge_base = cli.merge_base(&base, &head).expect("merge base");
    let diff = cli.diff(&merge_base, &head).expect("diff");

    assert_eq!(merge_base, base);
    assert!(diff.contains("+two"), "{diff}");
}
