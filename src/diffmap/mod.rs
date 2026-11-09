//! Maps unified diffs to the new-file (right side) line numbers a review comment may target.
//!
//! A line is commentable when it is an added or a context line of a hunk, the rule GitHub applies to
//! review comments on the RIGHT side.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

/// The commentable right-side lines of every file of a diff, by repo-relative path.
pub type LineMap = BTreeMap<String, BTreeSet<u32>>;

/// Returns whether `line` of `path` is a commentable line of the diff.
pub fn is_visible(map: &LineMap, path: &str, line: u32) -> bool {
    map.get(path).is_some_and(|lines| lines.contains(&line))
}

/// Returns the right-side line numbers of one file's hunks (a GitHub `patch` field).
pub fn patch_lines(patch: &str) -> BTreeSet<u32> {
    let mut lines = BTreeSet::new();
    let mut new = 0;

    for raw in patch.strip_suffix('\n').unwrap_or(patch).split('\n') {
        if let Some(start) = hunk_start(raw) {
            new = start;
            continue;
        }

        if new == 0 || raw.starts_with('-') || raw.starts_with('\\') {
            continue;
        }

        lines.insert(new);
        new += 1;
    }

    lines
}

/// Returns the commentable lines of every file of a full `git diff` output.
pub fn diff_lines(diff: &str) -> LineMap {
    let mut files = LineMap::new();
    let mut path: Option<String> = None;
    let mut patch: Vec<&str> = Vec::new();

    for raw in diff.strip_suffix('\n').unwrap_or(diff).split('\n') {
        if raw.starts_with("diff --git ") {
            if let Some(done) = path.take() {
                files.insert(done, patch_lines(&patch.join("\n")));
            }

            patch.clear();
            continue;
        }

        if patch.is_empty() && raw.starts_with("+++ ") {
            path = new_path(raw);
            continue;
        }

        if raw.starts_with("@@") || !patch.is_empty() {
            patch.push(raw);
        }
    }

    if let Some(done) = path {
        files.insert(done, patch_lines(&patch.join("\n")));
    }

    files
}

/// Decodes the body of a path git C-quoted: octal byte escapes and `\t`, `\"`, `\\` and the like.
pub fn unquote(quoted: &str) -> String {
    let bytes = quoted.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        let escaped = bytes[index] == b'\\';

        if escaped && let Some(byte) = bytes.get(index + 1..index + 4).and_then(octal) {
            decoded.push(byte);
            index += 4;
            continue;
        }

        if escaped && let Some(byte) = bytes.get(index + 1).copied().and_then(simple_escape) {
            decoded.push(byte);
            index += 2;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

/// Returns the repo-relative path of a `+++ b/<path>` header, or `None` for a deleted file.
fn new_path(header: &str) -> Option<String> {
    let raw = header[4..].trim_end_matches(['\r', '\n', '\t']);
    let path = if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        unquote(&raw[1..raw.len() - 1])
    } else {
        raw.to_owned()
    };

    if path == "/dev/null" {
        return None;
    }

    match path.strip_prefix("b/") {
        Some(stripped) => Some(stripped.to_owned()),
        None => Some(path),
    }
}

/// Parses `@@ -<old>[,<count>] +<new>[,<count>] @@` at the start of a line and returns `<new>`.
fn hunk_start(line: &str) -> Option<u32> {
    let rest = skip_range(line.strip_prefix("@@ -")?)?;
    let rest = rest.strip_prefix(" +")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let start = rest[..digits].parse().ok()?;

    skip_range(rest)?.starts_with(" @@").then_some(start)
}

/// Skips `<digits>[,<digits>]` and returns the rest.
fn skip_range(text: &str) -> Option<&str> {
    let rest = skip_digits(text)?;

    match rest.strip_prefix(',') {
        Some(count) => skip_digits(count),
        None => Some(rest),
    }
}

fn skip_digits(text: &str) -> Option<&str> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();

    (digits > 0).then(|| &text[digits..])
}

fn octal(digits: &[u8]) -> Option<u8> {
    if !digits.iter().all(|digit| (b'0'..=b'7').contains(digit)) {
        return None;
    }

    let value = digits
        .iter()
        .fold(0u16, |value, digit| value * 8 + u16::from(digit - b'0'));

    u8::try_from(value).ok()
}

fn simple_escape(escape: u8) -> Option<u8> {
    match escape {
        b'a' => Some(7),
        b'b' => Some(8),
        b't' => Some(9),
        b'n' => Some(10),
        b'v' => Some(11),
        b'f' => Some(12),
        b'r' => Some(13),
        b'"' => Some(b'"'),
        b'\\' => Some(b'\\'),
        _ => None,
    }
}
