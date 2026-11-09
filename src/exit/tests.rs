use std::path::PathBuf;

use super::*;
use crate::provider::{ProviderError, ProviderKind};
use crate::review::{ProviderFailure, Stage};
use crate::verify::VerifyError;

fn run() -> PathBuf {
    PathBuf::from("/tmp/run")
}

fn guard(error: GuardError) -> Failure {
    Failure::Review(ReviewError::Guard(error))
}

#[test]
fn every_failure_maps_to_its_documented_exit_code() {
    let busy = LockError::Busy {
        provider: ProviderKind::Codex,
        model: "m".to_owned(),
        holder: "1".to_owned(),
        path: run(),
    };
    let lock_io = LockError::Io {
        path: run(),
        source: io::Error::other("denied"),
    };
    let provider = ProviderFailure {
        stage: Stage::Discovery,
        provider: ProviderKind::Copilot,
        source: ProviderError::Spawn {
            cli: "copilot",
            source: io::Error::other("gone"),
        },
        log: run(),
        response: run(),
        hint: String::new(),
    };
    let lost = VerifyError::Lost {
        reported: 2,
        saved: 1,
    };
    let foreign = DraftsError::Foreign {
        path: "src/lib.rs".to_owned(),
        line: 1,
        found_pr: "8".to_owned(),
        found_head: "None".to_owned(),
        pr: 7,
        head: "a1".to_owned(),
    };
    let cases = [
        (Failure::Review(ReviewError::Usage("u".to_owned())), 1),
        (
            Failure::Review(ReviewError::Lock {
                source: lock_io,
                hint: None,
            }),
            1,
        ),
        (
            guard(GuardError::Stale {
                local: "a".to_owned(),
                remote: "b".to_owned(),
            }),
            2,
        ),
        (Failure::Review(ReviewError::verify(lost, String::new)), 3),
        (Failure::Schema(SchemaError::Violation("x".to_owned())), 3),
        (guard(GuardError::Dirty), 4),
        (
            guard(GuardError::HeadMoved {
                current: "c".to_owned(),
                run: run(),
            }),
            5,
        ),
        (Failure::Drafts(foreign), 5),
        (
            guard(GuardError::BaseMoved {
                current: "c".to_owned(),
                run: run(),
            }),
            6,
        ),
        (guard(GuardError::CheckoutChanged { run: run() }), 7),
        (Failure::Provider(UnknownProvider("x".to_owned())), 8),
        (
            Failure::Review(ReviewError::Lock {
                source: busy,
                hint: None,
            }),
            9,
        ),
        (
            Failure::Review(ReviewError::Provider(Box::new(provider))),
            10,
        ),
        (Failure::Doctor(1), 11),
    ];

    for (failure, code) in cases {
        assert_eq!(failure.exit().code(), code, "{failure}");
    }
}

#[test]
fn only_review_failures_carry_a_hint() {
    let invalid = ReviewError::verify(
        VerifyError::Lost {
            reported: 1,
            saved: 0,
        },
        || "resume with this".to_owned(),
    );

    assert_eq!(Failure::Review(invalid).hint(), Some("resume with this"));
    assert_eq!(Failure::Doctor(2).hint(), None);
}
