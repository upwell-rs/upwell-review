//! The review body, inline comment texts and the stages footer, worded as the legacy poster did.

use std::fmt::Write;

use crate::model::{Finding, Origin, Review, Stages};
use crate::provider::ProviderKind;

use super::short;

/// The closing sentences of every review footer.
const NOTICE: &str = "Read-only sandbox. Replaces CodeRabbit while the organisation is on the Free tier. Each inline comment is a resolvable thread; the author answers and resolves it after pushing. This is not a bot approval._";

/// Returns the text of an inline comment.
pub fn comment(finding: &Finding) -> String {
    let mut text = format!(
        "**[{}] {}**\n\n{}",
        finding.severity, finding.title, finding.body
    );

    if !finding.suggestion.trim().is_empty() {
        let _ = write!(text, "\n\n**Suggested fix:** {}", finding.suggestion);
    }

    text
}

/// Returns the review body: title, verdict, summary, findings outside the diff, what could not be checked
/// and the footer.
pub fn review(review: &Review, head: &str, stages: &Stages, loose: &[&Finding]) -> String {
    let mut text = format!(
        "## Independent review ({}) of `{}`\n\n**Verdict:** {}\n\n{}",
        title_label(stages),
        short(head),
        review.verdict.label(),
        review.summary
    );

    if !loose.is_empty() {
        text.push_str("\n\n### Findings outside the changed lines\n");
    }

    for finding in loose {
        let _ = write!(
            text,
            "\n- **[{}]** `{}:{}` {}: {}",
            finding.severity, finding.path, finding.line, finding.title, finding.body
        );

        if !finding.suggestion.trim().is_empty() {
            let _ = write!(text, " Suggested fix: {}", finding.suggestion);
        }
    }

    if !review.unchecked.trim().is_empty() {
        let _ = write!(text, "\n\n_Could not check:_ {}", review.unchecked);
    }

    text.push_str(&footer(stages));
    text
}

/// Names both stages' provider, model and effort with the draft and decision counts.
fn footer(stages: &Stages) -> String {
    let discovery = &stages.discovery;
    let found: Vec<String> = discovery.origins.iter().map(run_label).collect();
    let resumed = if discovery.resumed {
        ", verified from a saved drafts file"
    } else {
        ""
    };
    let mut text = format!(
        "\n\n---\n_Discovery: {}: {} draft(s){resumed}. ",
        found.join("; "),
        discovery.drafts
    );

    match &stages.verification {
        None => text.push_str("Verification: skipped (no drafts). "),
        Some(verification) => {
            let _ = write!(
                text,
                "Verification (fresh run that saw only the drafts, the diff and the code): {}: {} kept, {} modified, {} dropped. ",
                run_label(&verification.run),
                verification.keep,
                verification.modify,
                verification.drop
            );
        }
    }

    text.push_str(NOTICE);
    text
}

fn title_label(stages: &Stages) -> String {
    let mut names: Vec<&str> = Vec::new();

    for origin in &stages.discovery.origins {
        let name = ProviderKind::label_of(&origin.provider);

        if !names.contains(&name) {
            names.push(name);
        }
    }

    let mut label = names.join(", ");

    if let Some(verification) = &stages.verification {
        let _ = write!(
            label,
            ", verified by {}",
            ProviderKind::label_of(&verification.run.provider)
        );
    }

    label
}

fn run_label(run: &Origin) -> String {
    format!(
        "{} `{}`, effort {}",
        ProviderKind::label_of(&run.provider),
        run.model,
        run.effort
    )
}
