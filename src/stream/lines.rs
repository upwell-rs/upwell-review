//! Reassembles streamed text deltas into complete lines so drafts are saved as they arrive.

use std::collections::HashMap;
use std::hash::Hash;
use std::io;

use super::DraftSink;

/// Open, partial lines of streamed text, one buffer per stream (message, reasoning block, step).
#[derive(Debug)]
pub struct LineBuffers<K> {
    open: HashMap<K, String>,
}

impl<K: Eq + Hash> LineBuffers<K> {
    /// Creates empty buffers.
    pub fn new() -> Self {
        Self {
            open: HashMap::new(),
        }
    }

    /// Appends a delta to the stream `key` and scans every line it completes.
    pub fn push(&mut self, key: K, delta: &str, sink: &mut DraftSink) -> io::Result<()> {
        let buffer = self.open.entry(key).or_default();

        buffer.push_str(delta);

        while let Some(end) = buffer.find('\n') {
            let line: String = buffer.drain(..=end).collect();

            sink.scan(&line)?;
        }

        Ok(())
    }

    /// Scans and drops the unfinished last line of the stream `key`.
    pub fn finish(&mut self, key: &K, sink: &mut DraftSink) -> io::Result<()> {
        match self.open.remove(key) {
            Some(rest) => sink.scan(&rest),
            None => Ok(()),
        }
    }

    /// Scans and drops the unfinished last line of every stream.
    pub fn finish_all(&mut self, sink: &mut DraftSink) -> io::Result<()> {
        for (_, rest) in self.open.drain() {
            sink.scan(&rest)?;
        }

        Ok(())
    }
}
