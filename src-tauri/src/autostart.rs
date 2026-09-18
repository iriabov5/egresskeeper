//! Запуск приложения при входе пользователя в систему.
//!
//! Источник истины — операционная система, а не настройка приложения: автозапуск
//! можно выключить средствами ОС, и интерфейс обязан показывать факт, а не
//! сохранённое намерение. Порт объявлен здесь, чтобы логику можно было проверить
//! без реальной регистрации в системе.

use egresskeeper_core::EgressError;
use tauri_plugin_autostart::ManagerExt;

/// Управление автозапуском.
pub trait Autostart: std::fmt::Debug + Send + Sync {
    /// Возвращает фактическое состояние автозапуска.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::PlatformFeatureUnavailable`], если состояние
    /// недоступно в текущем окружении.
    fn is_enabled(&self) -> Result<bool, EgressError>;

    /// Включает или выключает автозапуск.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::PlatformFeatureUnavailable`], если изменить
    /// состояние не удалось.
    fn set_enabled(&self, enabled: bool) -> Result<(), EgressError>;
}

/// Автозапуск средствами операционной системы.
#[derive(Debug)]
pub struct SystemAutostart {
    app: tauri::AppHandle,
}

impl SystemAutostart {
    /// Создаёт управление автозапуском для приложения.
    #[must_use]
    pub const fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl Autostart for SystemAutostart {
    fn is_enabled(&self) -> Result<bool, EgressError> {
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|error| unavailable("autostart", &error))
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), EgressError> {
        let manager = self.app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };

        result.map_err(|error| unavailable("autostart", &error))
    }
}

/// Преобразует ошибку плагина в доменную ошибку.
fn unavailable(feature: &'static str, error: &tauri_plugin_autostart::Error) -> EgressError {
    tracing::warn!(feature, error = %error, "platform feature is unavailable");

    EgressError::platform_feature_unavailable(feature)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Автозапуск-заглушка: состояние задаётся тестом.
    #[derive(Debug)]
    struct FakeAutostart {
        enabled: std::sync::Mutex<bool>,
        failure: bool,
    }

    impl FakeAutostart {
        fn disabled() -> Self {
            Self {
                enabled: std::sync::Mutex::new(false),
                failure: false,
            }
        }

        fn failing() -> Self {
            Self {
                enabled: std::sync::Mutex::new(false),
                failure: true,
            }
        }
    }

    impl Autostart for FakeAutostart {
        fn is_enabled(&self) -> Result<bool, EgressError> {
            if self.failure {
                return Err(EgressError::platform_feature_unavailable("autostart"));
            }

            Ok(*self.enabled.lock().expect("lock"))
        }

        fn set_enabled(&self, enabled: bool) -> Result<(), EgressError> {
            if self.failure {
                return Err(EgressError::platform_feature_unavailable("autostart"));
            }

            *self.enabled.lock().expect("lock") = enabled;

            Ok(())
        }
    }

    #[test]
    fn state_round_trips_through_the_port() {
        let autostart = FakeAutostart::disabled();

        assert!(!autostart.is_enabled().expect("state"));
        autostart.set_enabled(true).expect("enabled");
        assert!(autostart.is_enabled().expect("state"));
    }

    #[test]
    fn unavailable_feature_reports_its_code() {
        let autostart = FakeAutostart::failing();

        let error = autostart.is_enabled().expect_err("unavailable");

        assert_eq!(
            error.code().as_str(),
            "platform_feature_unavailable",
            "интерфейс должен отличать недоступность от внутренней ошибки"
        );
    }
}
