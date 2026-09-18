//! Use-case «runtime-информация приложения».
//!
//! Инициализация выполняется один раз на старте backend: каталог состояния
//! проверяется и создаётся здесь, а не при каждом вызове команды. Это делает
//! старт детерминированным: если ресурс недоступен, приложение узнаёт об этом
//! сразу, а команды IPC не выполняют ввод-вывод.

use crate::application::ports::StateDirectory;
use crate::domain::error::EgressError;
use crate::domain::runtime::RuntimeOverview;
use crate::domain::time::now_unix_ms;

/// Сервис runtime-информации приложения.
///
/// Держит уже вычисленный снимок состояния и отдаёт его копию, поэтому чтение
/// через IPC не блокирует исполнитель и не зависит от состояния файловой системы
/// после старта.
#[derive(Debug)]
pub struct RuntimeInfoService {
    overview: RuntimeOverview,
}

impl RuntimeInfoService {
    /// Инициализирует сервис: валидирует вход, готовит каталог состояния и
    /// фиксирует момент старта.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`], если версия приложения не задана,
    /// и [`EgressError::StateDirUnavailable`], если каталог состояния недоступен.
    pub fn initialize(
        app_version: impl Into<String>,
        directory: &dyn StateDirectory,
    ) -> Result<Self, EgressError> {
        let app_version = app_version.into();
        if app_version.trim().is_empty() {
            return Err(EgressError::validation("app_version", "must not be empty"));
        }

        let state_dir = directory.ensure()?;
        let overview = RuntimeOverview::new(
            app_version,
            state_dir.to_string_lossy().into_owned(),
            now_unix_ms()?,
        );

        tracing::info!(
            core_version = %overview.core_version,
            os = %overview.os,
            arch = %overview.arch,
            state_dir = %overview.state_dir,
            "runtime info initialized"
        );

        Ok(Self { overview })
    }

    /// Возвращает снимок runtime-информации.
    #[must_use]
    pub fn overview(&self) -> RuntimeOverview {
        self.overview.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::error::ErrorCode;

    /// Тестовый порт: фиксирует вызов и возвращает заранее заданный результат.
    #[derive(Debug)]
    struct StubStateDirectory {
        path: PathBuf,
        failures: bool,
    }

    impl StubStateDirectory {
        fn ready(path: impl Into<PathBuf>) -> Self {
            Self {
                path: path.into(),
                failures: false,
            }
        }

        fn failing() -> Self {
            Self {
                path: PathBuf::from("/unused"),
                failures: true,
            }
        }
    }

    impl StateDirectory for StubStateDirectory {
        fn ensure(&self) -> Result<PathBuf, EgressError> {
            if self.failures {
                return Err(EgressError::StateDirUnavailable {
                    source: std::io::Error::other("stub failure"),
                });
            }

            Ok(self.path.clone())
        }
    }

    #[test]
    fn initialize_reports_state_directory_from_port() {
        let service = RuntimeInfoService::initialize(
            "1.2.3",
            &StubStateDirectory::ready("/tmp/egresskeeper-state"),
        )
        .expect("service initializes");

        assert_eq!(service.overview().state_dir, "/tmp/egresskeeper-state");
        assert_eq!(service.overview().app_version, "1.2.3");
    }

    #[test]
    fn initialize_rejects_empty_app_version() {
        let error = RuntimeInfoService::initialize("   ", &StubStateDirectory::ready("/tmp"))
            .expect_err("empty version must be rejected");

        assert_eq!(error.code(), ErrorCode::Validation);
    }

    #[test]
    fn initialize_propagates_state_directory_failure() {
        let error = RuntimeInfoService::initialize("1.2.3", &StubStateDirectory::failing())
            .expect_err("state directory failure must be propagated");

        assert_eq!(error.code(), ErrorCode::StateDirUnavailable);
    }

    #[test]
    fn initialize_records_start_time_close_to_now() {
        let before = now_unix_ms().expect("clock is available");
        let service = RuntimeInfoService::initialize("1.2.3", &StubStateDirectory::ready("/tmp"))
            .expect("service initializes");
        let after = now_unix_ms().expect("clock is available");

        let started_at = service.overview().started_at_unix_ms;

        assert!((before..=after).contains(&started_at));
    }

    #[test]
    fn overview_returns_independent_snapshots() {
        let service = RuntimeInfoService::initialize("1.2.3", &StubStateDirectory::ready("/tmp"))
            .expect("service initializes");

        let mut first = service.overview();
        first.app_version = "mutated".to_owned();

        assert_eq!(service.overview().app_version, "1.2.3");
    }
}
