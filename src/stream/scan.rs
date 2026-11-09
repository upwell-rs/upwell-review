//! Finds `UPWELL_FINDING_DRAFT` lines anywhere in text and normalizes what the models wrote.

use serde::Deserialize;

use crate::model::{Draft, Finding, Severity};

/// The marker that starts a draft line.
pub const MARKER: &str = "UPWELL_FINDING_DRAFT";

/// The draft fields as a model may write them: any status, a missing severity, a quoted line number.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawDraft {
    status: Option<String>,
    severity: Option<String>,
    path: Option<String>,
    line: Option<RawLine>,
    title: Option<String>,
    body: Option<String>,
    suggestion: Option<String>,
}

/// A line number written as a number or as a string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawLine {
    Number(u64),
    Text(String),
}

/// Returns every valid draft in `text`, in order. A marker may appear anywhere in a line (after a
/// bullet, inside backticks, in reasoning prose) and its JSON may span several lines.
pub fn find_drafts(text: &str) -> Vec<Draft> {
    text.match_indices(MARKER)
        .filter_map(|(index, _)| payload(&text[index + MARKER.len()..]))
        .filter_map(normalize)
        .collect()
}

/// Decodes the JSON object that follows a marker, ignoring whatever text comes after it.
fn payload(rest: &str) -> Option<RawDraft> {
    let start =
        rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '=' | '`' | '*'));

    if !start.starts_with('{') {
        return None;
    }

    serde_json::Deserializer::from_str(start)
        .into_iter::<RawDraft>()
        .next()?
        .ok()
}

/// Turns what a model wrote into a draft, or rejects it (the prompt's template line, no path, no line).
///
/// Any status is accepted: `draft` is the protocol, but some models write the severity there
/// (`"status":"nit"`); such a word becomes the severity when none is given and is kept as `kind`.
fn normalize(raw: RawDraft) -> Option<Draft> {
    let path = raw.path?;
    let path = path.trim();
    let path = path.strip_prefix("./").unwrap_or(path);
    let line = match raw.line? {
        RawLine::Number(number) => number,
        RawLine::Text(text) => text.trim().parse().ok()?,
    };
    let status = raw
        .status
        .filter(|status| !status.trim().is_empty() && !status.trim().eq_ignore_ascii_case("draft"));
    let named = raw.severity.as_deref().and_then(Severity::from_word);
    let severity = named
        .or_else(|| status.as_deref().and_then(Severity::from_word))
        .unwrap_or(Severity::Minor);
    let kind = status.or_else(|| raw.severity.filter(|_| named.is_none()));

    if path.is_empty() || path.starts_with('<') || line == 0 {
        return None;
    }

    let finding = Finding {
        severity,
        path: path.to_owned(),
        line: u32::try_from(line).ok()?,
        title: raw.title?,
        body: raw.body.unwrap_or_default(),
        suggestion: raw.suggestion.unwrap_or_default(),
    };

    Some(Draft { finding, kind })
}
