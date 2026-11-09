//! Saved drafts: reading them back, proving they belong to the reviewed head, numbering them for
//! the verifier and printing them for a human.

mod pretty;

#[cfg(test)]
mod tests;

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::model::{Finding, Origin, PrRef, StoredDraft, VerifyInput};

pub use pretty::pretty;

/// The line that opens the drafts block of the verification prompt.
pub const BEGIN: &str = "<<<UPWELL_DRAFTS_BEGIN>>>";

/// The line that closes the drafts block of the verification prompt.
pub const END: &str = "<<<UPWELL_DRAFTS_END>>>";

/// The origin value recorded when a draft does not say who found it.
const UNKNOWN: &str = "unknown";

/// Why a drafts file could not be used.
#[derive(Debug, Error)]
pub enum DraftsError {
    /// The file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
    /// A line is not a draft.
    #[error("{}:{line}: not a draft ({source})", path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// The 1-based line number.
        line: usize,
        /// The decoding failure.
        source: serde_json::Error,
    },
    /// A draft was found on another PR or head.
    #[error("draft {path}:{line} is for PR {found_pr} head {found_head}, not #{pr} at {head}")]
    Foreign {
        /// The draft's file path.
        path: String,
        /// The draft's line.
        line: u32,
        /// The PR the draft names, or `?`.
        found_pr: String,
        /// The head the draft names, or `None`.
        found_head: String,
        /// The reviewed PR.
        pr: u64,
        /// The reviewed head.
        head: String,
    },
}

/// Reads a JSON Lines file, skipping blank lines.
pub fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, DraftsError> {
    let text = fs::read_to_string(path).map_err(|source| DraftsError::Io {
        path: path.to_owned(),
        source,
    })?;

    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|source| DraftsError::Parse {
                path: path.to_owned(),
                line: index + 1,
                source,
            })
        })
        .collect()
}

/// Writes (or appends) records as JSON Lines.
pub fn write_jsonl<T: Serialize>(
    path: &Path,
    records: &[T],
    append: bool,
) -> Result<(), DraftsError> {
    let io_error = |source| DraftsError::Io {
        path: path.to_owned(),
        source,
    };
    let mut text = Vec::new();

    for record in records {
        serde_json::to_writer(&mut text, record).map_err(|source| io_error(source.into()))?;
        text.push(b'\n');
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(io_error)?;

    file.write_all(&text).map_err(io_error)
}

/// Returns the distinct drafts of `path` numbered `D1`..`Dn`, after checking that every draft was found
/// on PR `pr` at `head`: by its stamp, else by the `pr<N>-<head8>.jsonl` mirror file name.
pub fn prepare(path: &Path, pr: u64, head: &str) -> Result<Vec<VerifyInput>, DraftsError> {
    let named = path
        .file_name()
        .and_then(|name| mirror_name(&name.to_string_lossy()));
    let mut prepared = Vec::new();

    for draft in read_jsonl::<StoredDraft>(path)? {
        let (same_pr, found) = match (&draft.head, &named) {
            (Some(stamped), _) => (
                draft.pr.as_ref().is_some_and(|stamp| stamp.is(pr)),
                Some(stamped.as_str()),
            ),
            (None, Some((named_pr, named_head))) => (*named_pr == pr, Some(named_head.as_str())),
            (None, None) => (false, None),
        };

        if !same_pr || !found.is_some_and(|found| !found.is_empty() && head.starts_with(found)) {
            return Err(foreign(&draft, found, pr, head));
        }

        if prepared
            .iter()
            .any(|known: &VerifyInput| known.finding == draft.finding)
        {
            continue;
        }

        let origin = draft.origin.unwrap_or_else(|| Origin {
            provider: draft.provider.unwrap_or_else(|| UNKNOWN.to_owned()),
            model: draft.model.unwrap_or_else(|| UNKNOWN.to_owned()),
            effort: draft.effort.unwrap_or_else(|| UNKNOWN.to_owned()),
        });

        prepared.push(VerifyInput {
            id: format!("D{}", prepared.len() + 1),
            pr,
            head: head.to_owned(),
            finding: draft.finding,
            origin,
        });
    }

    Ok(prepared)
}

/// Returns the drafts as id-tagged JSON lines between the delimiters, with `<` escaped so a draft's text
/// cannot forge the end delimiter.
pub fn block(drafts: &[VerifyInput]) -> String {
    let mut text = format!("{BEGIN}\n");

    for draft in drafts {
        let claim = Claim {
            id: &draft.id,
            finding: &draft.finding,
        };
        let line = serde_json::to_value(&claim)
            .map(|value| value.to_string())
            .unwrap_or_default();

        text.push_str(&line.replace('<', "\\u003c"));
        text.push('\n');
    }

    text.push_str(END);
    text.push('\n');
    text
}

/// One draft as the verifier sees it: its id and the finding fields, nothing about who found it.
#[derive(Debug, Serialize)]
struct Claim<'a> {
    id: &'a str,
    #[serde(flatten)]
    finding: &'a Finding,
}

fn foreign(draft: &StoredDraft, found: Option<&str>, pr: u64, head: &str) -> DraftsError {
    let found_pr = match &draft.pr {
        Some(PrRef::Number(number)) => number.to_string(),
        Some(PrRef::Text(text)) => text.to_owned(),
        None => "?".to_owned(),
    };

    DraftsError::Foreign {
        path: draft.finding.path.to_owned(),
        line: draft.finding.line,
        found_pr,
        found_head: found.unwrap_or("None").to_owned(),
        pr,
        head: head.to_owned(),
    }
}

/// Parses a mirror file name `…pr<N>-<8 hex>.jsonl` into the PR and the short head.
fn mirror_name(name: &str) -> Option<(u64, String)> {
    let stem = name.strip_suffix(".jsonl")?;
    let (rest, short) = stem.rsplit_once('-')?;
    let digits = rest.len() - rest.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let hex = short.len() == 8
        && short
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));

    if !hex || digits == 0 || !rest[..rest.len() - digits].ends_with("pr") {
        return None;
    }

    Some((rest[rest.len() - digits..].parse().ok()?, short.to_owned()))
}
