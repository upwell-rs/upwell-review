//! The two-stage review of one PR: discovery drafts findings, a fresh verifier keeps, modifies or drops
//! them, and only the verified review is posted.

mod error;
mod prompt;
mod workspace;

#[cfg(test)]
mod tests;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::assets::{Asset, Assets};
use crate::config::{Plan, StageRun};
use crate::diffmap;
use crate::drafts;
use crate::git::Git;
use crate::github::GitHub;
use crate::guard::{self, Pinned};
use crate::lock::ModelLock;
use crate::model::{Origin, Stamp, VerifyInput};
use crate::post;
use crate::provider::{self, Invocation, ProviderError, ProviderKind};
use crate::stream::DraftSink;
use crate::verify::{self, Outputs, Verified};

pub use error::{ProviderFailure, ReviewError, Stage};
pub use workspace::{Workspace, resume_hint, shell_word};

use prompt::PrFacts;

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

/// Runs discovery, saving every draft as it streams; a provider failure keeps the drafts and says how to
/// verify them later.
fn discover<G, H, S: StageRunner, O: Write, E: Write>(
    context: &Context<'_, G, H, S>,
    pinning: &Pinning<'_>,
    focus: Option<&str>,
    console: &mut Console<O, E>,
) -> Result<(), ReviewError> {
    let run = &pinning.request.plan.discovery;
    let workspace = &pinning.workspace;
    let stamp = Stamp {
        pr: pinning.request.pr,
        head: pinning.head.to_owned(),
        origin: origin(run),
    };
    let prompt_path = workspace.file("discovery-prompt.md");
    let log = workspace.file("discovery.log");
    let prompt = prompt::discovery(context.assets, &pinning.facts(), run.provider, focus)?;
    let stamp_path = workspace.file("stamp.json");
    let stamp_text = serde_json::to_string(&stamp).map_err(|source| ReviewError::Io {
        path: stamp_path.to_owned(),
        source: source.into(),
    })?;

    write_file(&stamp_path, &stamp_text)?;
    write_file(&prompt_path, &prompt)?;

    let schema = context
        .assets
        .materialize(Asset::DiscoverySchema, &workspace.run)?;
    let mut sink =
        DraftSink::saving(&[&workspace.drafts, &workspace.mirror], stamp).map_err(|source| {
            ReviewError::Io {
                path: workspace.drafts.to_owned(),
                source,
            }
        })?;
    let call = invocation(pinning, run, &schema, &prompt_path, &workspace.discovery);

    if let Err(source) = context.stages.run(run.provider, &call, &log, &mut sink) {
        let hint = format!(
            "{} local draft finding(s) kept in {} (mirror: {})\ninspect with: upwell-review drafts {}\n{}",
            sink.saved(),
            workspace.drafts.display(),
            workspace.mirror.display(),
            shell_word(&workspace.drafts.to_string_lossy()),
            pinning.resume_hint(&workspace.drafts)
        );

        return Err(ReviewError::Provider(Box::new(ProviderFailure {
            stage: Stage::Discovery,
            provider: run.provider,
            source,
            response: call.response_path(),
            log,
            hint,
        })));
    }

    if let Err(error) = verify::done_marker(context.assets, &workspace.discovery) {
        console.note(&error.to_string());
        console.note(&format!(
            "the discovery answer is not a valid done marker (see {}); continuing with the saved drafts",
            log.display()
        ));
    }

    Ok(())
}

/// Fails closed when a valid done marker reports more drafts than discovery saved.
fn check_saved(assets: &Assets, pinning: &Pinning<'_>) -> Result<(), ReviewError> {
    let workspace = &pinning.workspace;
    let hint = || {
        format!(
            "run dir {} kept; after inspecting {} you can resume it\n{}",
            workspace.run.display(),
            workspace.drafts.display(),
            pinning.resume_hint(&workspace.drafts)
        )
    };
    let marker = match verify::done_marker(assets, &workspace.discovery) {
        Ok(marker) => marker,
        Err(error) if error.rejects() => return Ok(()),
        Err(error) => return Err(ReviewError::Verify(Box::new(error))),
    };
    let saved = drafts::read_jsonl::<Value>(&workspace.drafts)?.len();

    verify::check_saved(&marker, saved).map_err(|error| ReviewError::verify(error, hint))
}

/// Writes the clean review of a discovery that saved no drafts, after its done marker confirms none.
fn skip_verification<G, H, S, O: Write, E: Write>(
    context: &Context<'_, G, H, S>,
    pinning: &Pinning<'_>,
    verify_only: Option<&Path>,
    console: &mut Console<O, E>,
) -> Result<Verified, ReviewError> {
    let workspace = &pinning.workspace;

    if let Some(path) = verify_only {
        return Err(ReviewError::Usage(format!(
            "drafts file {} holds no drafts; nothing to verify",
            path.display()
        )));
    }

    console.note("discovery saved no drafts; skipping verification");

    verify::empty(context.assets, &workspace.discovery, &workspace.run, origin(&pinning.request.plan.discovery)).map_err(
        |error| {
            ReviewError::verify(error, || {
                format!(
                    "not posting a clean review: discovery returned no valid done marker or reported drafts none were saved (see {} and {})",
                    workspace.file("discovery.log").display(),
                    workspace.discovery.display()
                )
            })
        },
    )
}

/// Runs the fresh verifier on the drafts and applies its decisions.
fn verification<G, H, S: StageRunner, O: Write, E: Write>(
    context: &Context<'_, G, H, S>,
    pinning: &Pinning<'_>,
    drafts: &[VerifyInput],
    resumed: bool,
    console: &mut Console<O, E>,
) -> Result<Verified, ReviewError> {
    let run = &pinning.request.plan.verify;
    let workspace = &pinning.workspace;
    let prompt_path = workspace.file("verify-prompt.md");
    let log = workspace.file("verify.log");
    let prompt = prompt::verification(
        context.assets,
        &pinning.facts(),
        run.provider,
        &drafts::block(drafts),
    )?;
    let outputs = Outputs {
        run_dir: &workspace.run,
        mirror: &workspace.verification_mirror,
    };

    write_file(&prompt_path, &prompt)?;

    let schema = context
        .assets
        .materialize(Asset::VerifySchema, &workspace.run)?;
    let call = invocation(
        pinning,
        run,
        &schema,
        &prompt_path,
        &workspace.verify_answer,
    );

    if let Err(source) = context
        .stages
        .run(run.provider, &call, &log, &mut DraftSink::disabled())
    {
        return Err(ReviewError::Provider(Box::new(ProviderFailure {
            stage: Stage::Verification,
            provider: run.provider,
            source,
            response: call.response_path(),
            hint: pinning.resume_hint(&workspace.verify_input),
            log,
        })));
    }

    let visible = diffmap::diff_lines(&pinning.diff);
    let verified = verify::apply(
        context.assets,
        drafts,
        &workspace.verify_answer,
        &visible,
        &outputs,
        &origin(run),
        resumed,
    )
    .map_err(|error| {
        ReviewError::verify(error, || {
            format!(
                "the verification answer is not valid; not posting (see {})\n{}",
                log.display(),
                pinning.resume_hint(&workspace.verify_input)
            )
        })
    })?;
    let counts = verified.stages.verification.as_ref();

    console.say(&format!(
        "verification: {} kept, {} modified, {} dropped",
        counts.map_or(0, |run| run.keep),
        counts.map_or(0, |run| run.modify),
        counts.map_or(0, |run| run.drop)
    ));

    Ok(verified)
}

/// Names the result files and posts the review (or prints it in a dry run).
fn publish<G, H: GitHub, S, O: Write, E: Write>(
    context: &Context<'_, G, H, S>,
    pinning: &Pinning<'_>,
    verified: &Verified,
    console: &mut Console<O, E>,
) -> Result<(), ReviewError> {
    let workspace = &pinning.workspace;

    console.say(&format!(
        "review {}; verification {} (mirror: {})",
        workspace.file("review.json").display(),
        workspace.file("verification.jsonl").display(),
        workspace.verification_mirror.display()
    ));

    let report = post::post(
        context.github,
        pinning.request.pr,
        &pinning.head,
        &verified.review,
        &verified.stages,
        &workspace.run,
        pinning.request.dry_run,
    )?;

    console.say(report.trim_end_matches('\n'));

    Ok(())
}

fn invocation<'a>(
    pinning: &'a Pinning<'_>,
    run: &'a StageRun,
    schema: &'a Path,
    prompt: &'a Path,
    answer: &'a Path,
) -> Invocation<'a> {
    Invocation {
        model: &run.model,
        effort: &run.effort,
        schema,
        prompt,
        answer,
        worktree: &pinning.request.worktree,
        run: &pinning.workspace.run,
    }
}

/// Returns the origin a stage records: gemini runs record that the model name sets the effort.
fn origin(run: &StageRun) -> Origin {
    Origin {
        provider: run.provider.name().to_owned(),
        model: run.model.to_owned(),
        effort: run.provider.recorded_effort(&run.effort).to_owned(),
    }
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
