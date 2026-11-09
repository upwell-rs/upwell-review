//! `upwell-review doctor`: checks the provider CLIs, `git`, `gh` and its login, the providers file and
//! the worktree before a review needs them.

#[cfg(test)]
mod tests;

use std::fmt::Write;
use std::path::PathBuf;

use crate::assets::{Asset, AssetError, Assets};
use crate::config::{ConfigError, Mixes};
use crate::git::Git;
use crate::github::GitHub;
use crate::provider::ProviderKind;

/// The result of one check.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The check passed; the text says what was found.
    Passed(String),
    /// The check failed; the text says why.
    Failed(String),
    /// The check did not apply.
    Skipped(String),
}

/// One named check and its result.
#[derive(Debug, PartialEq, Eq)]
pub struct Check {
    /// What was checked.
    pub name: String,
    /// The result.
    pub outcome: Outcome,
}

/// What `doctor` inspects.
#[derive(Debug)]
pub struct Subject<'a, G, H, F> {
    /// The worktree to review, when one was given.
    pub git: Option<&'a G>,
    /// GitHub through `gh`.
    pub github: &'a H,
    /// Finds an executable on `PATH`.
    pub find: F,
    /// Whether `git` and `gh` run through `mise exec --`.
    pub mise: bool,
    /// The providers whose CLIs must be installed.
    pub providers: &'a [ProviderKind],
    /// The loaded providers file.
    pub config: Result<&'a Mixes, &'a ConfigError>,
    /// The prompts and schemas, with any `--prompts-dir` overrides.
    pub assets: Result<&'a Assets, &'a AssetError>,
}

/// Runs every check.
pub fn run<G: Git, H: GitHub, F: Fn(&str) -> Option<PathBuf>>(
    subject: &Subject<'_, G, H, F>,
) -> Vec<Check> {
    let mut checks = Vec::new();
    let tools = ["git", "gh"]
        .into_iter()
        .chain(subject.mise.then_some("mise"));

    for kind in subject.providers {
        checks.push(installed(
            &format!("{kind} CLI ({})", kind.cli()),
            kind.cli(),
            &subject.find,
        ));
    }

    for tool in tools {
        checks.push(installed(tool, tool, &subject.find));
    }

    checks.push(check(
        "gh auth",
        subject
            .github
            .auth_status()
            .map(|()| Outcome::Passed("logged in".to_owned())),
    ));
    checks.push(Check {
        name: "providers file".to_owned(),
        outcome: match subject.config {
            Ok(mixes) => Outcome::Passed(mixes.source().map_or_else(
                || "none; built-in mixes only".to_owned(),
                |path| path.display().to_string(),
            )),
            Err(error) => Outcome::Failed(error.to_string()),
        },
    });
    checks.push(check(
        "prompts and schemas",
        subject
            .assets
            .map_err(AssetError::to_string)
            .and_then(|assets| load_all(assets).map_err(|error| error.to_string())),
    ));
    checks.extend(worktree(subject.git));
    checks
}

/// Renders the checks, one line each.
pub fn render(checks: &[Check]) -> String {
    let mut text = String::new();

    for check in checks {
        let (label, detail) = match &check.outcome {
            Outcome::Passed(detail) => ("ok  ", detail),
            Outcome::Failed(detail) => ("FAIL", detail),
            Outcome::Skipped(detail) => ("skip", detail),
        };

        let _ = writeln!(text, "{label} {}: {detail}", check.name);
    }

    text
}

/// Returns the number of failed checks.
pub fn failures(checks: &[Check]) -> usize {
    checks
        .iter()
        .filter(|check| matches!(check.outcome, Outcome::Failed(_)))
        .count()
}

fn load_all(assets: &Assets) -> Result<Outcome, AssetError> {
    for asset in Asset::ALL {
        if asset.is_schema() {
            assets.schema(asset)?;
        } else {
            assets.text(asset)?;
        }
    }

    Ok(Outcome::Passed(format!("{} loaded", Asset::ALL.len())))
}

fn installed<F: Fn(&str) -> Option<PathBuf>>(name: &str, program: &str, find: &F) -> Check {
    Check {
        name: name.to_owned(),
        outcome: find(program).map_or_else(
            || Outcome::Failed(format!("{program} is not on PATH")),
            |path| Outcome::Passed(path.display().to_string()),
        ),
    }
}

fn worktree<G: Git>(git: Option<&G>) -> Vec<Check> {
    let Some(git) = git else {
        return vec![Check {
            name: "worktree".to_owned(),
            outcome: Outcome::Skipped("no worktree given".to_owned()),
        }];
    };
    let clean = git.has_tracked_changes().map(|dirty| match dirty {
        true => Outcome::Failed("tracked files have uncommitted changes".to_owned()),
        false => Outcome::Passed("no uncommitted changes to tracked files".to_owned()),
    });
    let detached = git.is_detached().map(|detached| match detached {
        true => Outcome::Passed("HEAD is detached".to_owned()),
        false => Outcome::Failed(
            "HEAD is on a branch; review a detached checkout of the PR head".to_owned(),
        ),
    });

    vec![
        check("worktree clean", clean),
        check("worktree detached", detached),
    ]
}

fn check<E: ToString>(name: &str, outcome: Result<Outcome, E>) -> Check {
    Check {
        name: name.to_owned(),
        outcome: outcome.unwrap_or_else(|error| Outcome::Failed(error.to_string())),
    }
}
