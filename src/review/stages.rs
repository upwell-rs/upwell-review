//! The discovery and verification stages and the checks between them.

use std::io::Write;
use std::path::Path;

use serde_json::Value;

use super::{
    Console, Context, Pinning, ProviderFailure, ReviewError, Stage, StageRunner, prompt,
    shell_word, write_file,
};
use crate::assets::{Asset, Assets};
use crate::config::StageRun;
use crate::diffmap;
use crate::drafts;
use crate::github::GitHub;
use crate::model::{Origin, Stamp, VerifyInput};
use crate::post;
use crate::provider::Invocation;
use crate::stream::DraftSink;
use crate::verify::{self, Outputs, Verified};

/// Runs discovery, saving every draft as it streams; a provider failure keeps the drafts and says how to
/// verify them later.
pub(super) fn discover<G, H, S: StageRunner, O: Write, E: Write>(
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
pub(super) fn check_saved(assets: &Assets, pinning: &Pinning<'_>) -> Result<(), ReviewError> {
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
pub(super) fn skip_verification<G, H, S, O: Write, E: Write>(
    context: &Context<'_, G, H, S>,
    pinning: &Pinning<'_>,
    verify_only: Option<&Path>,
    console: &mut Console<O, E>,
) -> Result<Verified, ReviewError> {
    let workspace = &pinning.workspace;
    let origin = origin(&pinning.request.plan.discovery);
    let hint = || {
        format!(
            "not posting a clean review: discovery returned no valid done marker or reported \
             drafts none were saved (see {} and {})",
            workspace.file("discovery.log").display(),
            workspace.discovery.display()
        )
    };

    if let Some(path) = verify_only {
        return Err(ReviewError::Usage(format!(
            "drafts file {} holds no drafts; nothing to verify",
            path.display()
        )));
    }

    console.note("discovery saved no drafts; skipping verification");

    verify::empty(context.assets, &workspace.discovery, &workspace.run, origin)
        .map_err(|error| ReviewError::verify(error, hint))
}

/// Runs the fresh verifier on the drafts and applies its decisions.
pub(super) fn verification<G, H, S: StageRunner, O: Write, E: Write>(
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
pub(super) fn publish<G, H: GitHub, S, O: Write, E: Write>(
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
