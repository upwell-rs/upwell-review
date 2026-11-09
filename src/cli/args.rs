//! The command line: global options, the subcommands and their arguments.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::config::PROVIDERS_ENV;
use crate::github::DEFAULT_REPO;

/// An independent, read-only, two-stage pull request review for `upwell-rs/upwell`.
#[derive(Debug, Parser)]
#[command(name = "upwell-review", version, about, long_about = None)]
pub struct Cli {
    /// The options every subcommand takes.
    #[command(flatten)]
    pub global: Global,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The options every subcommand takes.
#[derive(Debug, Args)]
pub struct Global {
    /// Output root for locks, draft mirrors and run directories [default: $TMPDIR/upwell-review]
    #[arg(long, global = true, env = "UPWELL_REVIEW_OUT", value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Directory whose files override the embedded prompts and schemas of the same name
    #[arg(long, global = true, value_name = "DIR")]
    pub prompts_dir: Option<PathBuf>,
    /// Providers file of named mixes [default: $XDG_CONFIG_HOME/upwell-review/providers.toml]
    #[arg(long, global = true, env = PROVIDERS_ENV, value_name = "FILE")]
    pub providers: Option<PathBuf>,
    /// The GitHub repository of the PR
    #[arg(long, global = true, default_value = DEFAULT_REPO, value_name = "OWNER/NAME")]
    pub repo: String,
    /// Run git and gh directly instead of through `mise exec --`
    #[arg(long, global = true)]
    pub no_mise: bool,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Review a PR: discovery drafts findings, a fresh verifier checks them, the result is posted
    Review(ReviewArgs),
    /// Verify a saved drafts file of the current PR head and post the result
    Verify(VerifyArgs),
    /// Check the provider CLIs, git, gh and its login, the providers file and the worktree
    Doctor(DoctorArgs),
    /// List the drafts of a drafts file, most severe first
    Drafts {
        /// The drafts file (`drafts.jsonl` or a mirror)
        file: PathBuf,
    },
    /// Validate an answer file against a schema
    Validate {
        /// `discovery`, `verify`, `review`, or a schema file
        schema: String,
        /// The JSON answer file
        answer: PathBuf,
    },
    /// List the provider mixes
    Mixes,
}

/// The provider, model and effort of both stages.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Discovery provider: codex, gemini or copilot [default: codex]
    #[arg(long)]
    pub provider: Option<String>,
    /// Discovery model [default: gpt-6.1-terra]
    #[arg(long)]
    pub model: Option<String>,
    /// Discovery reasoning effort [default: medium]
    #[arg(long)]
    pub effort: Option<String>,
    /// Verification provider [default: $UPWELL_VERIFY_PROVIDER, else the discovery provider]
    #[arg(long)]
    pub verify_provider: Option<String>,
    /// Verification model [default: $UPWELL_VERIFY_MODEL, else the discovery model]
    #[arg(long)]
    pub verify_model: Option<String>,
    /// Verification effort [default: $UPWELL_VERIFY_EFFORT, else the discovery effort]
    #[arg(long)]
    pub verify_effort: Option<String>,
    /// A named provider mix (see `upwell-review mixes`); explicit options override it
    #[arg(long)]
    pub mix: Option<String>,
    /// Run everything except the post and print the would-be review
    #[arg(
        long,
        env = "UPWELL_REVIEW_DRY_RUN",
        value_parser = clap::builder::FalseyValueParser::new()
    )]
    pub dry_run: bool,
}

/// `review`: the legacy positional arguments, each overridden by its option.
#[derive(Debug, Args)]
pub struct ReviewArgs {
    /// The PR number
    pub pr: u64,
    /// A clean, detached checkout of the PR head
    pub worktree: PathBuf,
    /// Discovery model (legacy position)
    #[arg(value_name = "MODEL")]
    pub model_arg: Option<String>,
    /// Discovery effort (legacy position)
    #[arg(value_name = "EFFORT")]
    pub effort_arg: Option<String>,
    /// Discovery provider (legacy position)
    #[arg(value_name = "PROVIDER")]
    pub provider_arg: Option<String>,
    /// Focus file (legacy position)
    #[arg(value_name = "FOCUS", value_parser = optional_path())]
    pub focus_arg: Option<PathBuf>,
    /// Verification provider (legacy position)
    #[arg(value_name = "VERIFY_PROVIDER")]
    pub verify_provider_arg: Option<String>,
    /// Verification model (legacy position)
    #[arg(value_name = "VERIFY_MODEL")]
    pub verify_model_arg: Option<String>,
    /// Verification effort (legacy position)
    #[arg(value_name = "VERIFY_EFFORT")]
    pub verify_effort_arg: Option<String>,
    /// The stages' providers, models and efforts
    #[command(flatten)]
    pub run: RunArgs,
    /// A file whose text narrows what discovery looks at
    #[arg(long, value_name = "FILE", value_parser = optional_path())]
    pub focus: Option<PathBuf>,
    /// Skip discovery and verify this saved drafts file (same as `verify --drafts`)
    #[arg(
        long,
        env = "UPWELL_REVIEW_VERIFY_ONLY",
        value_name = "DRAFTS",
        value_parser = optional_path()
    )]
    pub verify_only: Option<PathBuf>,
}

/// `verify`: resume a review from its saved drafts.
#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// The PR number
    pub pr: u64,
    /// A clean, detached checkout of the PR head
    pub worktree: PathBuf,
    /// The drafts file to verify (`drafts.jsonl`, a mirror or `verify-input.jsonl`)
    #[arg(long, value_name = "FILE")]
    pub drafts: PathBuf,
    /// The stages' providers, models and efforts
    #[command(flatten)]
    pub run: RunArgs,
}

/// `doctor`: what to check.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// The worktree to check for a clean, detached checkout
    pub worktree: Option<PathBuf>,
    /// Only check these provider CLIs [default: all]
    #[arg(long = "provider", value_name = "PROVIDER")]
    pub only: Vec<String>,
}

/// Parses a path that may be empty, which the legacy interface uses for "none".
fn optional_path() -> impl clap::builder::TypedValueParser<Value = PathBuf> {
    use clap::builder::TypedValueParser;

    clap::builder::OsStringValueParser::new().map(PathBuf::from)
}
