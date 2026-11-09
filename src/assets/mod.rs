//! The prompts, checklists and schemas, embedded at build time and overridable per file.

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

/// One embedded prompt, checklist or schema file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asset {
    /// The review rules (`review-prompt.md`).
    ReviewPrompt,
    /// The discovery protocol (`review-drafts-prompt.md`).
    DraftsPrompt,
    /// The verification rules (`verify-prompt.md`).
    VerifyPrompt,
    /// The discovery checklist for the gemini and copilot providers.
    ExtraChecklist,
    /// The gemini (`agy`) tool notes.
    GeminiChecklist,
    /// The copilot tool notes.
    CopilotChecklist,
    /// The discovery done-marker schema.
    DiscoverySchema,
    /// The verifier answer schema.
    VerifySchema,
    /// The posted review schema.
    ReviewSchema,
}

impl Asset {
    /// Every asset, for listing and tests.
    pub const ALL: [Self; 9] = [
        Self::ReviewPrompt,
        Self::DraftsPrompt,
        Self::VerifyPrompt,
        Self::ExtraChecklist,
        Self::GeminiChecklist,
        Self::CopilotChecklist,
        Self::DiscoverySchema,
        Self::VerifySchema,
        Self::ReviewSchema,
    ];

    /// Returns the file name, which is also the name an override must have.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::ReviewPrompt => "review-prompt.md",
            Self::DraftsPrompt => "review-drafts-prompt.md",
            Self::VerifyPrompt => "verify-prompt.md",
            Self::ExtraChecklist => "review-extra-checklist.md",
            Self::GeminiChecklist => "review-gemini-checklist.md",
            Self::CopilotChecklist => "review-copilot-checklist.md",
            Self::DiscoverySchema => "discovery-schema.json",
            Self::VerifySchema => "verify-schema.json",
            Self::ReviewSchema => "review-schema.json",
        }
    }

    /// Returns the copy compiled into the binary.
    pub const fn embedded(self) -> &'static str {
        match self {
            Self::ReviewPrompt => include_str!("../../prompts/review-prompt.md"),
            Self::DraftsPrompt => include_str!("../../prompts/review-drafts-prompt.md"),
            Self::VerifyPrompt => include_str!("../../prompts/verify-prompt.md"),
            Self::ExtraChecklist => include_str!("../../prompts/review-extra-checklist.md"),
            Self::GeminiChecklist => include_str!("../../prompts/review-gemini-checklist.md"),
            Self::CopilotChecklist => include_str!("../../prompts/review-copilot-checklist.md"),
            Self::DiscoverySchema => include_str!("../../schemas/discovery-schema.json"),
            Self::VerifySchema => include_str!("../../schemas/verify-schema.json"),
            Self::ReviewSchema => include_str!("../../schemas/review-schema.json"),
        }
    }

    /// Maps a schema name (`discovery`, `verify` or `review`) to its asset.
    pub fn schema_named(name: &str) -> Option<Self> {
        match name {
            "discovery" => Some(Self::DiscoverySchema),
            "verify" | "verification" => Some(Self::VerifySchema),
            "review" => Some(Self::ReviewSchema),
            _ => None,
        }
    }
}

/// Why an asset could not be loaded.
#[derive(Debug, Error)]
pub enum AssetError {
    /// The override directory does not exist.
    #[error("prompts dir {} is not a directory", .0.display())]
    MissingDir(PathBuf),
    /// An override file could not be read, or a materialized copy written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The I/O failure.
        source: io::Error,
    },
    /// A schema is not valid JSON.
    #[error("schema {name} is not valid JSON: {source}")]
    Schema {
        /// The schema file name.
        name: &'static str,
        /// The JSON failure.
        source: serde_json::Error,
    },
}

/// Where assets come from: the embedded copies, each overridable by a same-named file in a directory.
#[derive(Debug, Default)]
pub struct Assets {
    dir: Option<PathBuf>,
}

impl Assets {
    /// Uses the embedded assets, overridden by the files in `dir` when given.
    pub fn new(dir: Option<PathBuf>) -> Result<Self, AssetError> {
        if let Some(dir) = &dir
            && !dir.is_dir()
        {
            return Err(AssetError::MissingDir(dir.to_owned()));
        }

        Ok(Self { dir })
    }

    /// Returns the text of an asset: the override when one exists, else the embedded copy.
    pub fn text(&self, asset: Asset) -> Result<Cow<'static, str>, AssetError> {
        let Some(path) = self.override_path(asset) else {
            return Ok(Cow::Borrowed(asset.embedded()));
        };

        fs::read_to_string(&path)
            .map(Cow::Owned)
            .map_err(|source| AssetError::Io { path, source })
    }

    /// Returns a schema asset parsed as JSON.
    pub fn schema(&self, asset: Asset) -> Result<Value, AssetError> {
        let text = self.text(asset)?;

        serde_json::from_str(&text).map_err(|source| AssetError::Schema {
            name: asset.file_name(),
            source,
        })
    }

    /// Writes an asset to `dir` (providers that take a schema file need one on disk) and returns its path.
    pub fn materialize(&self, asset: Asset, dir: &Path) -> Result<PathBuf, AssetError> {
        let path = dir.join(asset.file_name());
        let text = self.text(asset)?;

        if let Err(source) = fs::write(&path, text.as_bytes()) {
            return Err(AssetError::Io { path, source });
        }

        Ok(path)
    }

    fn override_path(&self, asset: Asset) -> Option<PathBuf> {
        let path = self.dir.as_ref()?.join(asset.file_name());

        path.is_file().then_some(path)
    }
}
