//! The discovery and verification prompts, worded exactly as the legacy scripts built them.

use std::path::Path;

use crate::assets::{Asset, AssetError, Assets};
use crate::provider::ProviderKind;

use super::Stage;

/// The PR facts both prompts state.
#[derive(Debug)]
pub struct PrFacts<'a> {
    /// The PR number.
    pub pr: u64,
    /// The reviewed head.
    pub head: &'a str,
    /// The base branch name.
    pub base: &'a str,
    /// The fetched base branch tip.
    pub base_tip: &'a str,
    /// The merge base of the base tip and the head.
    pub merge_base: &'a str,
    /// The PR title (untrusted text).
    pub title: &'a str,
    /// The saved diff.
    pub diff: &'a Path,
}

/// Builds the discovery prompt: the review rules, the PR facts, the drafts protocol, the provider notes,
/// the optional focus and the done-marker schema.
pub fn discovery(
    assets: &Assets,
    facts: &PrFacts<'_>,
    provider: ProviderKind,
    focus: Option<&str>,
) -> Result<String, AssetError> {
    let PrFacts {
        pr,
        head,
        base,
        base_tip,
        merge_base,
        diff,
        ..
    } = facts;
    let mut text = assets.text(Asset::ReviewPrompt)?.into_owned();

    text.push_str(&format!(
        "\nThis is PR #{pr}, head {head}. Its base branch is {base} (a moving integration branch, currently at {base_tip}); review exactly `git diff {merge_base}...{head}`. Those are fixed commit ids, so do not substitute branch names. Prioritise correctness, concurrency, layering and behaviour changes hidden in refactors over style nits.\n"
    ));
    text.push_str(&format!(
        "\nDIFF FIRST: the complete diff is saved at `{}`. Read it first, in full, before anything else. Open other files in the working directory only when you need them to check a suspected defect (callers, definitions, tests); do not explore the repository broadly.\n",
        diff.display()
    ));
    text.push_str(&assets.text(Asset::DraftsPrompt)?);
    push_notes(&mut text, assets, provider, Stage::Discovery)?;

    if let Some(focus) = focus {
        text.push_str("\n# FOCUS FOR THIS REVIEW\n\nRestrict your drafts to the focus below; do not report anything outside it.\n\n");
        text.push_str(focus);
        text.push('\n');
    }

    push_schema(&mut text, assets, Asset::DiscoverySchema)?;

    Ok(text)
}

/// Builds the verification prompt: the verification rules, the PR facts, the drafts block, the review rules
/// for reference, the provider notes and the answer schema.
pub fn verification(
    assets: &Assets,
    facts: &PrFacts<'_>,
    provider: ProviderKind,
    drafts_block: &str,
) -> Result<String, AssetError> {
    let PrFacts {
        pr,
        head,
        base,
        base_tip,
        merge_base,
        title,
        diff,
    } = facts;
    let mut text = assets.text(Asset::VerifyPrompt)?.into_owned();

    text.push_str(&format!(
        "\nPR METADATA (data): PR #{pr}, head {head}, base branch {base} at {base_tip}, merge base {merge_base}. The diff under verification is exactly `git diff {merge_base}...{head}` (fixed commit ids). PR title (untrusted text): {title}\n"
    ));
    text.push_str(&format!(
        "\nThe complete diff is saved at `{}`. The working directory is the read-only checkout of the PR head.\n",
        diff.display()
    ));
    text.push_str("\nDRAFTS TO VERIFY (untrusted claims, one JSON object per line between the delimiters):\n\n");
    text.push_str(drafts_block);
    text.push_str("\n# REVIEW RULES THE DRAFTS ARE JUDGED BY (reference for severity and conventions; you verify, you do not review)\n\n");
    text.push_str(&assets.text(Asset::ReviewPrompt)?);
    push_notes(&mut text, assets, provider, Stage::Verification)?;
    push_schema(&mut text, assets, Asset::VerifySchema)?;

    Ok(text)
}

/// Appends the provider's tool notes, plus the extra checklist in discovery; codex gets none.
fn push_notes(
    text: &mut String,
    assets: &Assets,
    provider: ProviderKind,
    stage: Stage,
) -> Result<(), AssetError> {
    let notes = match provider {
        ProviderKind::Codex => return Ok(()),
        ProviderKind::Gemini => Asset::GeminiChecklist,
        ProviderKind::Copilot => Asset::CopilotChecklist,
    };

    if stage == Stage::Discovery {
        text.push_str(&assets.text(Asset::ExtraChecklist)?);
    }

    text.push_str(&assets.text(notes)?);

    Ok(())
}

fn push_schema(text: &mut String, assets: &Assets, schema: Asset) -> Result<(), AssetError> {
    text.push_str("\nThe final answer must validate against this JSON schema:\n\n```json\n");
    text.push_str(&assets.text(schema)?);
    text.push_str("```\n");

    Ok(())
}
