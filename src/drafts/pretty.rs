//! The human listing of a drafts file (`upwell-review drafts <file>`).

use std::fmt::Write;

use serde::Deserialize;
use serde_json::Value;

use crate::model::Severity;

/// Renders drafts ordered by severity, path and line, tolerating hand-edited records with odd field types.
pub fn pretty(drafts: &[Value]) -> String {
    let mut sorted: Vec<&Value> = drafts.iter().collect();
    let mut text = format!("{} draft(s), unverified\n", drafts.len());

    sorted.sort_by_key(|draft| sort_key(draft));

    for (number, draft) in sorted.iter().enumerate() {
        let _ = write!(
            text,
            "\n[{}] {}:{}  {}  {}\n    {}\n",
            number + 1,
            shown(draft.get("path")),
            shown(draft.get("line")),
            shown(draft.get("severity")),
            shown(draft.get("title")),
            draft
                .get("body")
                .map_or_else(String::new, |body| shown(Some(body))),
        );

        if let Some(suggestion) = draft.get("suggestion").filter(|value| truthy(value)) {
            let _ = writeln!(text, "    fix: {}", shown(Some(suggestion)));
        }
    }

    text
}

fn sort_key(draft: &Value) -> (u8, String, i64) {
    let rank = draft
        .get("severity")
        .and_then(|value| Severity::deserialize(value).ok())
        .map_or(9, |severity| severity as u8);
    let path = draft
        .get("path")
        .map_or_else(String::new, |path| shown(Some(path)));
    let line = draft.get("line").and_then(Value::as_i64).unwrap_or(0);

    (rank, path, line)
}

/// Renders a field like Python's `str()`: strings bare, a missing field as `None`, anything else as JSON.
fn shown(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_owned(),
        Some(Value::String(text)) => text.to_owned(),
        Some(other) => other.to_string(),
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
        Value::Number(number) => number.as_f64() != Some(0.0),
    }
}
