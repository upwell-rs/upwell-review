//! The two-stage review of one PR: discovery drafts findings, a fresh verifier keeps, modifies or drops
//! them, and only the verified review is posted.

mod error;
mod prompt;
mod stages;
mod workspace;

#[cfg(test)]
mod tests;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::assets::Assets;
use crate::config::Plan;
use crate::drafts;
use crate::git::Git;
use crate::github::GitHub;
use crate::guard::{self, Pinned};
use crate::lock::ModelLock;
use crate::provider::{self, Invocation, ProviderError, ProviderKind};
use crate::stream::DraftSink;

pub use error::{ProviderFailure, ReviewError, Stage};
pub use workspace::{Workspace, resume_hint, shell_word};

use prompt::PrFacts;
use stages::{check_saved, discover, publish, skip_verification, verification};

/// Runs one provider session of a stage; the seam tests replace.
pub trait StageRunner {
    /// Runs `kind` on `call`, appending its output to `log` and its drafts to `sink`.
    fn run(
        &self,
        kind: ProviderKind,
        call: &Invocation<'_>,
        log: &Path,
        sink: &mut DraftSink,
    ) -> Result<(), ProviderError>;
}

/// Runs the real provider CLIs.
#[derive(Debug, Default)]
pub struct CliStages;

impl StageRunner for CliStages {
    fn run(
        &self,
        kind: ProviderKind,
        call: &Invocation<'_>,
        log: &Path,
        sink: &mut DraftSink,
    ) -> Result<(), ProviderError> {
        provider::run_stage(kind, call, log, sink)
    }
}

/// What to review and how.
#[derive(Debug)]
pub struct Request {
    /// The PR number.
    pub pr: u64,
    /// The read-only checkout of the PR head.
    pub worktree: PathBuf,
    /// Both stages' provider, model and effort.
    pub plan: Plan,
    /// A file whose text restricts discovery.
    pub focus: Option<PathBuf>,
    /// A saved drafts file to verify instead of running discovery.
    pub verify_only: Option<PathBuf>,
    /// Print the review instead of posting it.
    pub dry_run: bool,
}

/// The tools and places a review uses.
#[derive(Debug)]
pub struct Context<'a, G, H, S> {
    /// The PR checkout.
    pub git: &'a G,
    /// The PR on GitHub.
    pub github: &'a H,
    /// The provider runner.
    pub stages: &'a S,
    /// The prompts and schemas.
    pub assets: &'a Assets,
    /// The output root (locks, mirrors and run directories).
    pub out: &'a Path,
}

/// Where a review reports: results on `out`, progress and warnings on `err`.
#[derive(Debug)]
pub struct Console<O, E> {
    /// Standard output.
    pub out: O,
    /// Standard error.
    pub err: E,
}

impl<O: Write, E: Write> Console<O, E> {
    /// Prints a result line. A closed terminal must not abort a review whose record is the run
    /// directory, so write failures are ignored.
    fn say(&mut self, text: &str) {
        let _ = writeln!(self.out, "{text}");
    }

    /// Prints a progress or warning line; write failures are ignored as in [`Console::say`].
    fn note(&mut self, text: &str) {
        let _ = writeln!(self.err, "{text}");
    }
}

/// The state of a review once the PR, the checkout and the diff are pinned.
struct Pinning<'a> {
    request: &'a Request,
    workspace: Workspace,
    head: String,
    facts_base: String,
    base_tip: String,
    merge_base: String,
    title: String,
    pr_base: String,
    diff: String,
}

impl Pinning<'_> {
    fn facts(&self) -> PrFacts<'_> {
        PrFacts {
            pr: self.request.pr,
            head: &self.head,
            base: &self.facts_base,
            base_tip: &self.base_tip,
            merge_base: &self.merge_base,
            title: &self.title,
            diff: &self.workspace.diff,
        }
    }

    fn pinned(&self) -> Pinned<'_> {
        Pinned {
            pr: self.request.pr,
            head: &self.head,
            base: &self.pr_base,
            run: &self.workspace.run,
        }
    }

    fn resume_hint(&self, drafts: &Path) -> String {
        resume_hint(
            self.request.pr,
            &self.request.worktree,
            drafts,
            &self.request.plan,
        )
    }
}

/// Reviews the PR: discovery (unless verifying saved drafts), the freshness and fail-closed checks,
/// verification, and the post (or dry run).
pub fn review<G: Git, H: GitHub, S: StageRunner, O: Write, E: Write>(
    context: &Context<'_, G, H, S>,
    request: &Request,
    console: &mut Console<O, E>,
) -> Result<(), ReviewError> {
    let focus = read_focus(request.focus.as_deref())?;
    let verify_only = request
        .verify_only
        .as_deref()
        .map(absolute_drafts)
        .transpose()?;
    let plan = &request.plan;

    Workspace::prepare(context.out)?;

    let mut lock = match verify_only {
        Some(_) => ModelLock::acquire(context.out, plan.verify.provider, &plan.verify.model)?,
        None => ModelLock::acquire(context.out, plan.discovery.provider, &plan.discovery.model)?,
    };
    let pinning = pin(context, request)?;
    let source = match &verify_only {
        Some(path) => path.to_owned(),
        None => {
            discover(context, &pinning, focus.as_deref(), console)?;
            pinning.workspace.drafts.to_owned()
        }
    };

    guard::check_fresh(context.git, context.github, &pinning.pinned())?;

    if verify_only.is_none() {
        check_saved(context.assets, &pinning)?;
    }

    let drafts = drafts::prepare(&source, request.pr, &pinning.head)?;

    drafts::write_jsonl(&pinning.workspace.verify_input, &drafts, false)?;

    let verified = if drafts.is_empty() {
        skip_verification(context, &pinning, verify_only.as_deref(), console)?
    } else {
        console.note(&format!(
            "verifying {} draft(s) with {}/{} in a fresh run",
            drafts.len(),
            plan.verify.provider,
            plan.verify.model
        ));

        if verify_only.is_none() {
            lock.switch(plan.verify.provider, &plan.verify.model)
                .map_err(|source| ReviewError::Lock {
                    source,
                    hint: Some(pinning.resume_hint(&pinning.workspace.drafts)),
                })?;
        }

        let verified = verification(context, &pinning, &drafts, verify_only.is_some(), console)?;

        guard::check_fresh(context.git, context.github, &pinning.pinned())?;
        verified
    };

    publish(context, &pinning, &verified, console)
}

/// Reads the PR and the checkout, checks they match, creates the run directory and saves the diff.
fn pin<'a, G: Git, H: GitHub, S>(
    context: &Context<'_, G, H, S>,
    request: &'a Request,
) -> Result<Pinning<'a>, ReviewError> {
    let run = Workspace::create(context.out, request.pr)?;
    let head = context.git.head()?;
    let pr = context.github.pull_request(request.pr)?;
    let workspace = Workspace::new(context.out, run, request.pr, &head);

    guard::check_start(context.git, &head, &pr.head_ref_oid)?;

    let base_tip = context.git.fetch_base(request.pr, &pr.base_ref_name)?;
    let merge_base = context.git.merge_base(&base_tip, &head)?;
    let diff = context.git.diff(&merge_base, &head)?;

    write_file(&workspace.diff, &diff)?;

    Ok(Pinning {
        request,
        workspace,
        head,
        facts_base: pr.base_ref_name,
        base_tip,
        merge_base,
        title: pr.title,
        pr_base: pr.base_ref_oid,
        diff,
    })
}

fn read_focus(path: Option<&Path>) -> Result<Option<String>, ReviewError> {
    let Some(path) = path else {
        return Ok(None);
    };

    if !path.is_file() {
        return Err(ReviewError::Usage(format!(
            "focus file {} does not exist",
            path.display()
        )));
    }

    fs::read_to_string(path)
        .map(Some)
        .map_err(|source| ReviewError::Io {
            path: path.to_owned(),
            source,
        })
}

fn absolute_drafts(path: &Path) -> Result<PathBuf, ReviewError> {
    if !path.is_file() {
        return Err(ReviewError::Usage(format!(
            "drafts file {} does not exist",
            path.display()
        )));
    }

    path.canonicalize().map_err(|source| ReviewError::Io {
        path: path.to_owned(),
        source,
    })
}

fn write_file(path: &Path, text: &str) -> Result<(), ReviewError> {
    fs::write(path, text).map_err(|source| ReviewError::Io {
        path: path.to_owned(),
        source,
    })
}
