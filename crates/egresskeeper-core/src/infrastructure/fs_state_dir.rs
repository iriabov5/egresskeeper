//! Реализация порта [`StateDirectory`] поверх локальной файловой системы.

use std::path::{Path, PathBuf};

use crate::application::ports::StateDirectory;
use crate::domain::error::EgressError;

/// Каталог состояния приложения в локальной файловой системе.
///
/// Путь приходит из composition root: ядро не знает, как платформа выбирает
/// каталог данных приложения.
#[derive(Debug, Clone)]
pub struct FsStateDirectory {
    path: PathBuf,
}

impl FsStateDirectory {
    /// Создаёт порт для заданного пути.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Путь, с которым работает порт.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl StateDirectory for FsStateDirectory {
    fn ensure(&self) -> Result<PathBuf, EgressError> {
        if self.path.as_os_str().is_empty() {
            return Err(EgressError::validation("state_dir", "must not be empty"));
        }

        std::fs::create_dir_all(&self.path)
            .map_err(|source| EgressError::StateDirUnavailable { source })?;

        let metadata = std::fs::metadata(&self.path)
            .map_err(|source| EgressError::StateDirUnavailable { source })?;

        if !metadata.is_dir() {
            return Err(EgressError::StateDirUnavailable {
                source: std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "state directory path exists and is not a directory",
                ),
            });
        }

        Ok(self.path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;

    #[test]
    fn empty_path_is_rejected_as_validation_error() {
        let directory = FsStateDirectory::new(PathBuf::new());

        let error = directory.ensure().expect_err("empty path must be rejected");

        assert_eq!(error.code(), ErrorCode::Validation);
    }

    #[test]
    fn path_accessor_returns_configured_path() {
        let directory = FsStateDirectory::new("/tmp/egresskeeper");

        assert_eq!(directory.path(), Path::new("/tmp/egresskeeper"));
    }
}
