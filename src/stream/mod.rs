//! Draft collection from provider output: marker scanning, normalization, deduplication and saving.

mod answer;
mod lines;
mod scan;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{Draft, SavedDraft, Stamp};

pub use answer::fallback_answer;
pub use lines::LineBuffers;
pub use scan::{MARKER, find_drafts};

/// Why a provider's output stream did not yield a usable answer.
#[derive(Debug, Error)]
pub enum StreamError {
    /// The log, the drafts files or the answer could not be written.
    #[error("could not write the run log, drafts or answer: {0}")]
    Io(#[from] io::Error),
    /// The stream ended without its final result event.
    #[error("{provider} ended without a result event")]
    NoResult {
        /// The provider CLI.
        provider: &'static str,
    },
    /// The provider reported a failed run.
    #[error("{provider} result was not successful ({detail}); full response saved in {}", saved.display())]
    Unsuccessful {
        /// The provider CLI.
        provider: &'static str,
        /// The status the provider reported.
        detail: String,
        /// Where the response text was saved.
        saved: PathBuf,
    },
    /// The final answer holds neither structured output nor a JSON block.
    #[error("{provider} result without a structured answer; full response saved in {}", saved.display())]
    NoAnswer {
        /// The provider CLI.
        provider: &'static str,
        /// Where the response text was saved.
        saved: PathBuf,
    },
}

/// The drafts files new drafts are appended to and the stamp each saved draft carries.
#[derive(Debug)]
struct DraftStore {
    files: Vec<File>,
    stamp: Stamp,
}

/// Collects drafts from provider text, deduplicated by content, and appends each new one to the drafts files
/// the moment it is seen. A disabled sink (verification) ignores drafts.
#[derive(Debug)]
pub struct DraftSink {
    store: Option<DraftStore>,
    seen: HashSet<String>,
}

impl DraftSink {
    /// A sink that ignores drafts, for the verification stage.
    pub fn disabled() -> Self {
        Self {
            store: None,
            seen: HashSet::new(),
        }
    }

    /// A sink that appends stamped drafts to every file of `paths`.
    pub fn saving(paths: &[&Path], stamp: Stamp) -> io::Result<Self> {
        let files = paths
            .iter()
            .map(|path| OpenOptions::new().create(true).append(true).open(path))
            .collect::<io::Result<Vec<_>>>()?;

        Ok(Self {
            store: Some(DraftStore { files, stamp }),
            seen: HashSet::new(),
        })
    }

    /// Returns the number of distinct drafts saved so far.
    pub fn saved(&self) -> usize {
        self.seen.len()
    }

    /// Finds every draft in `text` and saves the ones not seen before.
    pub fn scan(&mut self, text: &str) -> io::Result<()> {
        if self.store.is_none() || !text.contains(MARKER) {
            return Ok(());
        }

        for draft in find_drafts(text) {
            self.keep(&draft)?;
        }

        Ok(())
    }

    fn keep(&mut self, draft: &Draft) -> io::Result<()> {
        let Some(store) = self.store.as_mut() else {
            return Ok(());
        };
        let key = serde_json::to_string(&draft.finding)?;

        if !self.seen.insert(key) {
            return Ok(());
        }

        let record = SavedDraft {
            finding: &draft.finding,
            status: "draft",
            kind: draft.kind.as_deref(),
            stamp: &store.stamp,
        };
        let mut line = serde_json::to_vec(&record)?;

        line.push(b'\n');

        for file in &mut store.files {
            file.write_all(&line)?;
            file.flush()?;
        }

        Ok(())
    }
}

/// Copies every raw line of `input` to `log` and hands it, decoded lossily, to `each`.
pub fn read_lines<R, L, F>(mut input: R, log: &mut L, mut each: F) -> Result<(), StreamError>
where
    R: BufRead,
    L: Write,
    F: FnMut(&str) -> Result<(), StreamError>,
{
    let mut raw = Vec::new();

    loop {
        raw.clear();

        if input.read_until(b'\n', &mut raw)? == 0 {
            return Ok(());
        }

        log.write_all(&raw)?;
        log.flush()?;
        each(String::from_utf8_lossy(&raw).trim_end_matches(['\n', '\r']))?;
    }
}
