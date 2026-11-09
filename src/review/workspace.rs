//! The files of one review: the run directory, the per-head mirrors and the resume command.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Plan;
use crate::post::short;

use super::ReviewError;

/// How often a fresh run directory name is retried when it already exists.
const ATTEMPTS: u32 = 64;

/// The paths of one review run.
#[derive(Debug)]
pub struct Workspace {
    /// The fresh run directory `<out>/pr<N>.<suffix>`.
    pub run: PathBuf,
    /// `pr.diff`.
    pub diff: PathBuf,
    /// The discovery drafts, `drafts.jsonl`.
    pub drafts: PathBuf,
    /// The discovery answer, `discovery.json`.
    pub discovery: PathBuf,
    /// The numbered drafts handed to the verifier, `verify-input.jsonl`.
    pub verify_input: PathBuf,
    /// The verifier's answer, `verify.json`.
    pub verify_answer: PathBuf,
    /// The per-head drafts mirror `<out>/drafts/pr<N>-<head8>.jsonl`.
    pub mirror: PathBuf,
    /// The per-head verification mirror `<out>/drafts/pr<N>-<head8>.verification.jsonl`.
    pub verification_mirror: PathBuf,
}

impl Workspace {
    /// Creates `<out>/locks` and `<out>/drafts`.
    pub fn prepare(out: &Path) -> Result<(), ReviewError> {
        for dir in [out.join("locks"), out.join("drafts")] {
            fs::create_dir_all(&dir).map_err(|source| ReviewError::Io {
                path: dir.to_owned(),
                source,
            })?;
        }

        Ok(())
    }

    /// Creates a fresh run directory `<out>/pr<N>.<suffix>` for PR `pr`.
    pub fn create(out: &Path, pr: u64) -> Result<PathBuf, ReviewError> {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());

        for attempt in 0..ATTEMPTS {
            let suffix =
                (process::id().wrapping_mul(0x9E37_79B1) ^ seed.wrapping_add(attempt)) & 0xFF_FFFF;
            let run = out.join(format!("pr{pr}.{suffix:06x}"));

            match fs::create_dir(&run) {
                Ok(()) => return Ok(run),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(ReviewError::Io { path: run, source }),
            }
        }

        Err(ReviewError::Io {
            path: out.to_owned(),
            source: io::Error::new(io::ErrorKind::AlreadyExists, "no free run directory name"),
        })
    }

    /// Names the files of the run in `run` for PR `pr` at `head`.
    pub fn new(out: &Path, run: PathBuf, pr: u64, head: &str) -> Self {
        let mirrors = out.join("drafts");

        Self {
            diff: run.join("pr.diff"),
            drafts: run.join("drafts.jsonl"),
            discovery: run.join("discovery.json"),
            verify_input: run.join("verify-input.jsonl"),
            verify_answer: run.join("verify.json"),
            mirror: mirrors.join(format!("pr{pr}-{}.jsonl", short(head))),
            verification_mirror: mirrors.join(format!("pr{pr}-{}.verification.jsonl", short(head))),
            run,
        }
    }

    /// Returns the file of the run called `name`.
    pub fn file(&self, name: &str) -> PathBuf {
        self.run.join(name)
    }
}

/// Returns the command that verifies `drafts` later with the same runs.
pub fn resume_hint(pr: u64, worktree: &Path, drafts: &Path, plan: &Plan) -> String {
    let words = [
        "upwell-review".to_owned(),
        "verify".to_owned(),
        pr.to_string(),
        shell_word(&worktree.to_string_lossy()),
        "--drafts".to_owned(),
        shell_word(&drafts.to_string_lossy()),
        format!("--provider {}", plan.discovery.provider),
        format!("--model {}", shell_word(&plan.discovery.model)),
        format!("--effort {}", shell_word(&plan.discovery.effort)),
        format!("--verify-provider {}", plan.verify.provider),
        format!("--verify-model {}", shell_word(&plan.verify.model)),
        format!("--verify-effort {}", shell_word(&plan.verify.effort)),
    ];

    format!("verify these drafts later with: {}", words.join(" "))
}

/// Quotes a word for a POSIX shell when it holds anything but plain path characters.
pub fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@+=,%".contains(c));

    if plain {
        return word.to_owned();
    }

    format!("'{}'", word.replace('\'', r"'\''"))
}
