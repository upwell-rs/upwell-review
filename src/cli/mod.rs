//! Runs the parsed command line against the real `git`, `gh` and provider CLIs.

mod args;

#[cfg(test)]
mod tests;

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::assets::{Asset, Assets};
use crate::config::{self, Mixes, Plan, Requested};
use crate::doctor::{self, Subject};
use crate::drafts;
use crate::exit::Failure;
use crate::git::GitCli;
use crate::github::GhCli;
use crate::provider::ProviderKind;
use crate::review::{self, CliStages, Console, Context, Request};
use crate::schema;
use crate::tools::{self, Tools};

pub use args::{Cli, Command, DoctorArgs, Global, ReviewArgs, VerifyArgs};

/// Runs `cli`, reading the environment through `env`.
pub fn run<O: Write, E: Write>(
    cli: Cli,
    env: &impl Fn(&str) -> Option<String>,
    console: &mut Console<O, E>,
) -> Result<(), Failure> {
    let Cli { global, command } = cli;

    match command {
        Command::Review(args) => {
            let request = review_request(&global, args, env)?;

            run_review(&global, &request, console)
        }
        Command::Verify(args) => {
            let request = verify_request(&global, args, env)?;

            run_review(&global, &request, console)
        }
        Command::Doctor(args) => run_doctor(&global, &args, env, console),
        Command::Drafts { file } => {
            let drafts = drafts::read_jsonl::<Value>(&file)?;

            write!(console.out, "{}", drafts::pretty(&drafts))?;
            Ok(())
        }
        Command::Validate { schema, answer } => validate(&global, &schema, &answer, console),
        Command::Mixes => list_mixes(&global, env, console),
    }
}

/// Builds the review of `args`: options beat the legacy positions, which beat the mix and the defaults.
fn review_request(
    global: &Global,
    args: ReviewArgs,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<Request, Failure> {
    let ReviewArgs {
        pr,
        worktree,
        model_arg,
        effort_arg,
        provider_arg,
        focus_arg,
        verify_provider_arg,
        verify_model_arg,
        verify_effort_arg,
        run,
        focus,
        verify_only,
    } = args;
    let discovery = Requested {
        provider: run.provider.or(provider_arg),
        model: run.model.or(model_arg),
        effort: run.effort.or(effort_arg),
    };
    let verify = Requested {
        provider: run.verify_provider.or(verify_provider_arg),
        model: run.verify_model.or(verify_model_arg),
        effort: run.verify_effort.or(verify_effort_arg),
    };
    let plan = resolve(global, run.mix.as_deref(), discovery, verify, env)?;

    Ok(Request {
        pr,
        worktree,
        plan,
        focus: focus
            .or(focus_arg)
            .filter(|path| !path.as_os_str().is_empty()),
        verify_only: verify_only.filter(|path| !path.as_os_str().is_empty()),
        dry_run: run.dry_run,
    })
}

/// Builds the resumed review of `args`.
fn verify_request(
    global: &Global,
    args: VerifyArgs,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<Request, Failure> {
    let VerifyArgs {
        pr,
        worktree,
        drafts,
        run,
    } = args;
    let discovery = Requested {
        provider: run.provider,
        model: run.model,
        effort: run.effort,
    };
    let verify = Requested {
        provider: run.verify_provider,
        model: run.verify_model,
        effort: run.verify_effort,
    };
    let plan = resolve(global, run.mix.as_deref(), discovery, verify, env)?;

    Ok(Request {
        pr,
        worktree,
        plan,
        focus: None,
        verify_only: Some(drafts),
        dry_run: run.dry_run,
    })
}

fn resolve(
    global: &Global,
    mix: Option<&str>,
    discovery: Requested,
    verify: Requested,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<Plan, Failure> {
    let Some(name) = mix else {
        return Ok(config::plan(discovery, verify, None, env)?);
    };
    let mixes = load_mixes(global, env)?;

    Ok(config::plan(
        discovery,
        verify,
        Some(mixes.get(name)?),
        env,
    )?)
}

fn run_review<O: Write, E: Write>(
    global: &Global,
    request: &Request,
    console: &mut Console<O, E>,
) -> Result<(), Failure> {
    let tools = tools_of(global);
    let assets = Assets::new(global.prompts_dir.to_owned())?;
    let out = out_dir(global);
    let git = GitCli::new(tools, &request.worktree);
    let github = GhCli::new(tools, global.repo.to_owned());
    let context = Context {
        git: &git,
        github: &github,
        stages: &CliStages,
        assets: &assets,
        out: &out,
    };

    Ok(review::review(&context, request, console)?)
}

fn run_doctor<O: Write, E: Write>(
    global: &Global,
    args: &DoctorArgs,
    env: &impl Fn(&str) -> Option<String>,
    console: &mut Console<O, E>,
) -> Result<(), Failure> {
    let tools = tools_of(global);
    let providers = match args.only.as_slice() {
        [] => ProviderKind::ALL.to_vec(),
        only => only
            .iter()
            .map(|name| name.parse())
            .collect::<Result<Vec<ProviderKind>, _>>()?,
    };
    let config = load_mixes(global, env);
    let assets = Assets::new(global.prompts_dir.to_owned());
    let git = args
        .worktree
        .as_deref()
        .map(|worktree| GitCli::new(tools, worktree));
    let github = GhCli::new(tools, global.repo.to_owned());
    let checks = doctor::run(&Subject {
        git: git.as_ref(),
        github: &github,
        find: tools::find_in_path,
        mise: tools.mise,
        providers: &providers,
        config: config.as_ref(),
        assets: assets.as_ref(),
    });
    let failed = doctor::failures(&checks);

    write!(console.out, "{}", doctor::render(&checks))?;

    if failed > 0 {
        return Err(Failure::Doctor(failed));
    }

    Ok(())
}

fn validate<O: Write, E: Write>(
    global: &Global,
    name: &str,
    answer: &Path,
    console: &mut Console<O, E>,
) -> Result<(), Failure> {
    let schema = match Asset::schema_named(name) {
        Some(asset) => Assets::new(global.prompts_dir.to_owned())?.schema(asset)?,
        None if Path::new(name).is_file() => schema::load::<Value>(
            Path::new(name),
            &Value::Object(Default::default()),
            "schema",
        )
        .map_err(Failure::SchemaFile)?,
        None => return Err(Failure::UnknownSchema(name.to_owned())),
    };

    schema::load::<Value>(answer, &schema, "answer")?;
    writeln!(
        console.out,
        "{} matches the {name} schema",
        answer.display()
    )?;
    Ok(())
}

fn list_mixes<O: Write, E: Write>(
    global: &Global,
    env: &impl Fn(&str) -> Option<String>,
    console: &mut Console<O, E>,
) -> Result<(), Failure> {
    let mixes = load_mixes(global, env)?;
    let source = mixes.source().map_or_else(
        || "built-in mixes (no providers file)".to_owned(),
        |path| format!("built-in mixes and {}", path.display()),
    );

    writeln!(console.out, "{source}")?;

    for (name, mix) in mixes.iter() {
        writeln!(
            console.out,
            "{name}: discovery {}; verify {}",
            mix.discovery, mix.verify
        )?;
    }

    Ok(())
}

fn load_mixes(
    global: &Global,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<Mixes, config::ConfigError> {
    Mixes::load(config::providers_path(global.providers.to_owned(), env).as_deref())
}

fn tools_of(global: &Global) -> Tools {
    Tools {
        mise: !global.no_mise,
    }
}

fn out_dir(global: &Global) -> PathBuf {
    global
        .out
        .to_owned()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| env::temp_dir().join("upwell-review"))
}
