//! The providers file of named mixes and the choice of each stage's provider, model and effort.
//!
//! ```toml
//! [mix.risky]
//! discovery = { provider = "copilot", model = "claude-opus-5.5", effort = "high" }
//! verify = { provider = "codex", model = "gpt-6.1-terra" }
//! ```

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::provider::{ProviderKind, UnknownProvider};

/// The discovery provider when nothing else chooses one.
pub const DEFAULT_PROVIDER: ProviderKind = ProviderKind::Codex;

/// The discovery model when nothing else chooses one.
pub const DEFAULT_MODEL: &str = "gpt-6.1-terra";

/// The effort when nothing else chooses one.
pub const DEFAULT_EFFORT: &str = "medium";

/// The environment variable that names the providers file.
pub const PROVIDERS_ENV: &str = "UPWELL_REVIEW_PROVIDERS";

/// Why the providers file or a mix could not be used.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("providers file {}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
    /// The file is not a valid providers file.
    #[error("providers file {}: {source}", path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// The TOML failure.
        source: Box<toml::de::Error>,
    },
    /// No mix has the name.
    #[error("unknown mix '{name}'; known mixes: {known}")]
    UnknownMix {
        /// The requested name.
        name: String,
        /// The known names, comma separated.
        known: String,
    },
}

/// One stage's provider, model and effort.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageRun {
    /// The provider.
    pub provider: ProviderKind,
    /// The model id.
    pub model: String,
    /// The reasoning effort.
    #[serde(default = "default_effort")]
    pub effort: String,
}

impl fmt::Display for StageRun {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {} ({})",
            self.provider, self.model, self.effort
        )
    }
}

/// A named pair of discovery and verification runs.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mix {
    /// The discovery run.
    pub discovery: StageRun,
    /// The verification run.
    pub verify: StageRun,
}

/// The providers file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvidersFile {
    #[serde(default)]
    mix: BTreeMap<String, Mix>,
}

/// The built-in mixes, overridden and extended by the providers file.
#[derive(Debug)]
pub struct Mixes {
    mixes: BTreeMap<String, Mix>,
    source: Option<PathBuf>,
}

impl Mixes {
    /// Returns the built-in mixes plus those of the providers file at `path`, when given.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let mut mixes = built_in();
        let Some(path) = path else {
            return Ok(Self {
                mixes,
                source: None,
            });
        };
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        let file: ProvidersFile = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source: Box::new(source),
        })?;

        mixes.extend(file.mix);

        Ok(Self {
            mixes,
            source: Some(path.to_owned()),
        })
    }

    /// Returns the mix called `name`.
    pub fn get(&self, name: &str) -> Result<&Mix, ConfigError> {
        self.mixes.get(name).ok_or_else(|| ConfigError::UnknownMix {
            name: name.to_owned(),
            known: self
                .mixes
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        })
    }

    /// Returns every mix by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Mix)> {
        self.mixes.iter().map(|(name, mix)| (name.as_str(), mix))
    }

    /// Returns the providers file the mixes were read from, if any.
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }
}

/// Returns the providers file to read: `explicit` (the flag or `UPWELL_REVIEW_PROVIDERS`), else
/// `<config dir>/upwell-review/providers.toml` when it exists.
pub fn providers_path(
    explicit: Option<PathBuf>,
    env: impl Fn(&str) -> Option<String>,
) -> Option<PathBuf> {
    let config_dir = env("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| Path::new(&home).join(".config"))
        });

    if explicit.is_some() {
        return explicit;
    }

    config_dir
        .map(|dir| dir.join("upwell-review").join("providers.toml"))
        .filter(|path| path.is_file())
}

/// One stage's provider, model and effort as given on the command line; any may be left out.
#[derive(Debug, Default)]
pub struct Requested {
    /// The provider name.
    pub provider: Option<String>,
    /// The model id.
    pub model: Option<String>,
    /// The effort.
    pub effort: Option<String>,
}

/// Both stages' runs, resolved.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// The discovery run.
    pub discovery: StageRun,
    /// The verification run.
    pub verify: StageRun,
}

/// Resolves both stages.
///
/// Discovery: the request, else the mix, else the defaults. Verification: the request, else the mix,
/// else `UPWELL_VERIFY_PROVIDER`/`_MODEL`/`_EFFORT`, else the discovery value. Empty values count as unset.
pub fn plan(
    discovery: Requested,
    verify: Requested,
    mix: Option<&Mix>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Plan, UnknownProvider> {
    let pick = |value: Option<String>| value.filter(|value| !value.is_empty());
    let discovery = StageRun {
        provider: pick(discovery.provider)
            .or_else(|| mix.map(|mix| mix.discovery.provider.name().to_owned()))
            .map_or(Ok(DEFAULT_PROVIDER), |name| name.parse())?,
        model: pick(discovery.model)
            .or_else(|| mix.map(|mix| mix.discovery.model.to_owned()))
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned()),
        effort: pick(discovery.effort)
            .or_else(|| mix.map(|mix| mix.discovery.effort.to_owned()))
            .unwrap_or_else(|| DEFAULT_EFFORT.to_owned()),
    };
    let verify = StageRun {
        provider: pick(verify.provider)
            .or_else(|| mix.map(|mix| mix.verify.provider.name().to_owned()))
            .or_else(|| pick(env("UPWELL_VERIFY_PROVIDER")))
            .map_or(Ok(discovery.provider), |name| name.parse())?,
        model: pick(verify.model)
            .or_else(|| mix.map(|mix| mix.verify.model.to_owned()))
            .or_else(|| pick(env("UPWELL_VERIFY_MODEL")))
            .unwrap_or_else(|| discovery.model.to_owned()),
        effort: pick(verify.effort)
            .or_else(|| mix.map(|mix| mix.verify.effort.to_owned()))
            .or_else(|| pick(env("UPWELL_VERIFY_EFFORT")))
            .unwrap_or_else(|| discovery.effort.to_owned()),
    };

    Ok(Plan { discovery, verify })
}

fn default_effort() -> String {
    DEFAULT_EFFORT.to_owned()
}

fn built_in() -> BTreeMap<String, Mix> {
    let run = |provider, model: &str, effort: &str| StageRun {
        provider,
        model: model.to_owned(),
        effort: effort.to_owned(),
    };

    BTreeMap::from([
        (
            "risky".to_owned(),
            Mix {
                discovery: run(ProviderKind::Copilot, "claude-opus-5.5", "high"),
                verify: run(ProviderKind::Codex, "gpt-6.1-terra", "medium"),
            },
        ),
        (
            "cheap".to_owned(),
            Mix {
                discovery: run(ProviderKind::Copilot, "gpt-6.1-terra", "medium"),
                verify: run(ProviderKind::Codex, "gpt-6.1-terra", "medium"),
            },
        ),
    ])
}
