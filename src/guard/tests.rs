use std::path::Path;

use super::*;
use crate::testing::{BASE, FakeGit, FakeGitHub, HEAD};

fn pinned() -> Pinned<'static> {
    Pinned {
        pr: 7,
        head: HEAD,
        base: BASE,
        run: Path::new("/run"),
    }
}

#[test]
fn a_clean_checkout_of_the_pr_head_passes() {
    let git = FakeGit::default();

    check_start(&git, HEAD, HEAD).expect("clean PR head");
    check_fresh(&git, &FakeGitHub::default(), &pinned()).expect("nothing moved");
}

#[test]
fn a_stale_or_dirty_checkout_is_refused_at_the_start() {
    let git = FakeGit::default();

    assert!(matches!(
        check_start(&git, HEAD, "other"),
        Err(GuardError::Stale { .. })
    ));

    *git.dirty.borrow_mut() = true;

    assert!(matches!(
        check_start(&git, HEAD, HEAD),
        Err(GuardError::Dirty)
    ));
}

#[test]
fn a_local_head_change_or_edit_during_the_run_is_a_checkout_change() {
    let git = FakeGit::default();
    let github = FakeGitHub::default();

    *git.head.borrow_mut() = "moved".to_owned();

    assert!(matches!(
        check_fresh(&git, &github, &pinned()),
        Err(GuardError::CheckoutChanged { .. })
    ));

    *git.head.borrow_mut() = HEAD.to_owned();
    *git.dirty.borrow_mut() = true;

    assert!(matches!(
        check_fresh(&git, &github, &pinned()),
        Err(GuardError::CheckoutChanged { .. })
    ));
}

#[test]
fn a_moved_pr_head_or_base_is_refused_after_the_local_check() {
    let git = FakeGit::default();
    let github = FakeGitHub::default();

    github.pr.borrow_mut().base_ref_oid = "new-base".to_owned();

    assert!(
        matches!(check_fresh(&git, &github, &pinned()), Err(GuardError::BaseMoved { current, .. }) if current == "new-base")
    );

    github.pr.borrow_mut().head_ref_oid = "new-head".to_owned();

    assert!(matches!(
        check_fresh(&git, &github, &pinned()),
        Err(GuardError::HeadMoved { .. })
    ));

    *git.head.borrow_mut() = "moved".to_owned();

    assert!(matches!(
        check_fresh(&git, &github, &pinned()),
        Err(GuardError::CheckoutChanged { .. })
    ));
}
