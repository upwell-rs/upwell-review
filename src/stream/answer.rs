//! Extracts the final JSON answer from a free-text response when a provider gave no structured output.

use serde_json::Value;

const FENCE: &str = "```";

/// Returns the last fenced json block of a response that parses, else the last JSON object that starts a
/// line, else `None`.
pub fn fallback_answer(response: &str) -> Option<Value> {
    fenced_blocks(response)
        .into_iter()
        .rev()
        .find_map(|block| serde_json::from_str(block).ok())
        .or_else(|| last_line_object(response))
}

/// Returns the bodies of the fenced blocks opened by "```" or "```json" (plus optional blanks) and a newline.
fn fenced_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut from = 0;

    while let Some(offset) = text[from..].find(FENCE) {
        let open = from + offset;
        let Some(body) =
            fence_body_start(&text[open + FENCE.len()..]).map(|skip| open + FENCE.len() + skip)
        else {
            from = open + 1;
            continue;
        };
        let Some(length) = text[body..].find(FENCE) else {
            break;
        };

        blocks.push(&text[body..body + length]);
        from = body + length + FENCE.len();
    }

    blocks
}

/// Returns how many bytes of the fence header (`json`, blanks, newline) precede a block body.
fn fence_body_start(header: &str) -> Option<usize> {
    let tagged = header.strip_prefix("json").unwrap_or(header);
    let blanks = tagged.len() - tagged.trim_start_matches([' ', '\t']).len();
    let skipped = header.len() - tagged.len() + blanks;

    header[skipped..].starts_with('\n').then_some(skipped + 1)
}

fn last_line_object(text: &str) -> Option<Value> {
    let starts = text
        .match_indices('{')
        .map(|(index, _)| index)
        .filter(|&index| index == 0 || text.as_bytes()[index - 1] == b'\n');

    starts.rev().find_map(|index| {
        serde_json::Deserializer::from_str(&text[index..])
            .into_iter::<Value>()
            .next()?
            .ok()
    })
}
