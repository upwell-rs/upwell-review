//! Validation of answers against the subset of JSON Schema the review schemas use, then typed decoding.

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// Why an answer was not accepted.
#[derive(Debug, Error)]
pub enum SchemaError {
    /// The answer file could not be read.
    #[error("{}: {source}", path.display())]
    Read {
        /// The answer file.
        path: PathBuf,
        /// The I/O failure.
        source: std::io::Error,
    },
    /// The answer is not JSON.
    #[error("{}: {source}", path.display())]
    Parse {
        /// The answer file.
        path: PathBuf,
        /// The JSON failure.
        source: serde_json::Error,
    },
    /// The answer breaks the schema; the text names the first violation.
    #[error("{0}")]
    Violation(String),
    /// The answer matches the schema but not the typed shape (a weakened override schema).
    #[error("{at}: {source}")]
    Shape {
        /// Where in the answer.
        at: String,
        /// The decoding failure.
        source: serde_json::Error,
    },
}

/// Returns the first violation of `value` against `schema` as text, or `None` when it matches.
pub fn violation(value: &Value, schema: &Value, at: &str) -> Option<String> {
    let kind = schema.get("type").and_then(Value::as_str);

    if let Some(kind) = kind
        && !has_type(value, kind)
    {
        return Some(format!("{at}: expected {kind}"));
    }

    if let Some(choices) = schema.get("enum").and_then(Value::as_array)
        && !choices.contains(value)
    {
        return Some(format!(
            "{at}: {value} not in {}",
            Value::Array(choices.to_vec())
        ));
    }

    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
        && value.as_f64().is_some_and(|number| number < minimum)
    {
        return Some(format!("{at}: below minimum {}", schema["minimum"]));
    }

    match kind {
        Some("object") => object_violation(value, schema, at),
        Some("array") => array_violation(value, schema, at),
        _ => None,
    }
}

/// Validates `value` against `schema`, then decodes it into `T`.
pub fn check<T: DeserializeOwned>(
    value: Value,
    schema: &Value,
    at: &str,
) -> Result<T, SchemaError> {
    if let Some(found) = violation(&value, schema, at) {
        return Err(SchemaError::Violation(found));
    }

    serde_json::from_value(value).map_err(|source| SchemaError::Shape {
        at: at.to_owned(),
        source,
    })
}

/// Reads a JSON answer file, validates it against `schema` and decodes it into `T`.
pub fn load<T: DeserializeOwned>(path: &Path, schema: &Value, at: &str) -> Result<T, SchemaError> {
    let text = fs::read_to_string(path).map_err(|source| SchemaError::Read {
        path: path.to_owned(),
        source,
    })?;
    let value = serde_json::from_str(&text).map_err(|source| SchemaError::Parse {
        path: path.to_owned(),
        source,
    })?;

    check(value, schema, at)
}

fn has_type(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn object_violation(value: &Value, schema: &Value, at: &str) -> Option<String> {
    let empty = serde_json::Map::new();
    let object = value.as_object()?;
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    for name in required.iter().filter_map(Value::as_str) {
        if !object.contains_key(name) {
            return Some(format!("{at}: missing {name}"));
        }
    }

    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        for name in object.keys() {
            if !properties.contains_key(name) {
                return Some(format!("{at}: unexpected {name}"));
            }
        }
    }

    properties
        .iter()
        .filter_map(|(name, sub)| object.get(name).map(|field| (name, field, sub)))
        .find_map(|(name, field, sub)| violation(field, sub, &format!("{at}.{name}")))
}

fn array_violation(value: &Value, schema: &Value, at: &str) -> Option<String> {
    let items = schema.get("items")?;

    value
        .as_array()?
        .iter()
        .enumerate()
        .find_map(|(index, item)| violation(item, items, &format!("{at}[{index}]")))
}
