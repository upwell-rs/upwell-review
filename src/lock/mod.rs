//! The per-model lock: one review per provider and model at a time.
//!
//! An exclusive `flock(2)` (`File::try_lock`) on `<out>/locks/<provider>--<model>.lock`, compatible
//! with the legacy scripts' lock. The kernel releases it however the process ends, so there is no
//! stale lock to take over or delete; the holder's pid is recorded next to it for the refusal message.
//! The file is opened close-on-exec, so provider processes never inherit it.

#[cfg(test)]
mod tests;

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use thiserror::Error;

use crate::provider::ProviderKind;

/// Why the lock was not taken.
#[derive(Debug, Error)]
pub enum LockError {
    /// The lock file could not be opened or locked.
    #[error("lock {}: {source}", path.display())]
    Io {
        /// The lock file.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
    /// Another review holds the lock.
    #[error(
        "another review with {provider}/{model} is already running (pid {holder}, lock {}); run one instance at a time",
        path.display()
    )]
    Busy {
        /// The provider.
        provider: ProviderKind,
        /// The model.
        model: String,
        /// The holder's pid, or `unknown`.
        holder: String,
        /// The lock file.
        path: PathBuf,
    },
}

/// A held per-model lock; dropping it releases the lock.
#[derive(Debug)]
pub struct ModelLock {
    root: PathBuf,
    provider: ProviderKind,
    model: String,
    _file: File,
}

impl ModelLock {
    /// Takes the lock of `provider` and `model` under `<root>/locks`, or fails when it is held.
    pub fn acquire(root: &Path, provider: ProviderKind, model: &str) -> Result<Self, LockError> {
        let path = root
            .join("locks")
            .join(format!("{provider}--{}.lock", model.replace('/', "_")));
        let pid_path = path.with_extension("lock.pid");
        let io_error = |source| LockError::Io {
            path: path.to_owned(),
            source,
        };
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io_error)?;

        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let holder = fs::read_to_string(&pid_path)
                    .map(|pid| pid.trim().to_owned())
                    .ok();

                return Err(LockError::Busy {
                    provider,
                    model: model.to_owned(),
                    holder: holder
                        .filter(|pid| !pid.is_empty())
                        .unwrap_or_else(|| "unknown".to_owned()),
                    path,
                });
            }
            Err(TryLockError::Error(source)) => return Err(io_error(source)),
        }

        fs::write(&pid_path, format!("{}\n", process::id())).map_err(io_error)?;

        Ok(Self {
            root: root.to_owned(),
            provider,
            model: model.to_owned(),
            _file: file,
        })
    }

    /// Returns whether this lock already covers `provider` and `model`.
    pub fn covers(&self, provider: ProviderKind, model: &str) -> bool {
        self.provider == provider && self.model == model
    }

    /// Holds the lock of `provider` and `model` instead, unless this lock already covers them; the old
    /// lock is released once the new one is held.
    pub fn switch(&mut self, provider: ProviderKind, model: &str) -> Result<(), LockError> {
        if self.covers(provider, model) {
            return Ok(());
        }

        *self = Self::acquire(&self.root, provider, model)?;

        Ok(())
    }
}
