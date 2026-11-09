use super::*;

fn root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");

    fs::create_dir(dir.path().join("locks")).expect("locks dir");
    dir
}

#[test]
fn a_held_lock_refuses_a_second_review_of_the_same_model() {
    let root = root();
    let held =
        ModelLock::acquire(root.path(), ProviderKind::Codex, "gpt-6.1-terra").expect("first lock");
    let error =
        ModelLock::acquire(root.path(), ProviderKind::Codex, "gpt-6.1-terra").expect_err("busy");
    let message = error.to_string();

    assert!(held.covers(ProviderKind::Codex, "gpt-6.1-terra"));
    assert!(matches!(error, LockError::Busy { .. }));
    assert!(
        message.contains(&format!("pid {}", process::id())),
        "{message}"
    );
}

#[test]
fn other_models_and_released_locks_are_free() {
    let root = root();
    let held =
        ModelLock::acquire(root.path(), ProviderKind::Copilot, "org/model").expect("first lock");

    ModelLock::acquire(root.path(), ProviderKind::Codex, "org/model")
        .expect("other provider is free");
    drop(held);
    ModelLock::acquire(root.path(), ProviderKind::Copilot, "org/model")
        .expect("released lock is free");
}

#[test]
fn switching_releases_the_old_lock_and_holds_the_new_one() {
    let root = root();
    let mut held = ModelLock::acquire(root.path(), ProviderKind::Copilot, "a").expect("first lock");

    held.switch(ProviderKind::Copilot, "a")
        .expect("already covered");
    held.switch(ProviderKind::Codex, "b").expect("switched");

    assert!(held.covers(ProviderKind::Codex, "b"));
    ModelLock::acquire(root.path(), ProviderKind::Copilot, "a").expect("old lock is released");
    ModelLock::acquire(root.path(), ProviderKind::Codex, "b").expect_err("new lock is held");
}
