//! Настройки приложения.
//!
//! Сервис владеет значениями по умолчанию и преобразованием между контрактом и
//! хранилищем. Хранилище знает только пары «ключ — значение», поэтому новая
//! настройка не требует миграции схемы.

use serde::{Deserialize, Serialize};

use crate::application::ports::SettingsRepository;
use crate::domain::error::EgressError;

/// Ключ настройки поведения при закрытии главного окна.
const CLOSE_BEHAVIOR_KEY: &str = "shell.close_behavior";

/// Поведение приложения при закрытии главного окна.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    /// Скрыть окно и продолжить работу: proxy продолжает принимать соединения.
    ///
    /// Значение по умолчанию: proxy задуман как постоянно работающий.
    #[default]
    HideToTray,
    /// Завершить приложение вместе с listeners.
    Quit,
}

impl CloseBehavior {
    /// Строковое представление для хранилища и контракта.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HideToTray => "hide_to_tray",
            Self::Quit => "quit",
        }
    }

    /// Разбирает сохранённое значение.
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "hide_to_tray" => Some(Self::HideToTray),
            "quit" => Some(Self::Quit),
            _ => None,
        }
    }
}

/// Настройки оболочки, которые меняет пользователь.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellSettings {
    /// Поведение при закрытии главного окна.
    pub close_behavior: CloseBehavior,
}

/// Use-cases настроек приложения.
#[derive(Debug)]
pub struct SettingsService<R: SettingsRepository> {
    repository: R,
}

impl<R: SettingsRepository> SettingsService<R> {
    /// Создаёт сервис поверх реализации порта.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }

    /// Возвращает настройки оболочки.
    ///
    /// Отсутствующая настройка означает значение по умолчанию. Испорченное
    /// значение (например, оставшееся от старой версии) также приводит к
    /// значению по умолчанию, но фиксируется в логе: настроение интерфейса не
    /// должно ломаться из-за одной записи в базе.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если настройки недоступны.
    pub fn shell_settings(&self) -> Result<ShellSettings, EgressError> {
        Ok(ShellSettings {
            close_behavior: self.close_behavior()?,
        })
    }

    /// Меняет поведение при закрытии главного окна.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища, если записать значение не удалось.
    pub fn set_close_behavior(
        &self,
        behavior: CloseBehavior,
    ) -> Result<ShellSettings, EgressError> {
        self.repository
            .set_setting(CLOSE_BEHAVIOR_KEY, behavior.as_str())?;

        self.shell_settings()
    }

    /// Читает поведение при закрытии окна.
    fn close_behavior(&self) -> Result<CloseBehavior, EgressError> {
        let Some(stored) = self.repository.get_setting(CLOSE_BEHAVIOR_KEY)? else {
            return Ok(CloseBehavior::default());
        };

        match CloseBehavior::parse(&stored) {
            Some(behavior) => Ok(behavior),
            None => {
                // Значение по умолчанию, а не ошибка: интерфейс должен
                // открываться, даже если в базе осталось значение от другой
                // версии, но факт фиксируется в логе.
                tracing::warn!(
                    value = %stored,
                    "stored close behavior is unknown; using the default"
                );

                Ok(CloseBehavior::default())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// Хранилище настроек в памяти.
    #[derive(Debug, Default)]
    struct InMemorySettings {
        values: Mutex<HashMap<String, String>>,
    }

    impl InMemorySettings {
        fn with_value(key: &str, value: &str) -> Self {
            let storage = Self::default();
            storage
                .values
                .lock()
                .expect("lock")
                .insert(key.to_owned(), value.to_owned());

            storage
        }
    }

    impl SettingsRepository for InMemorySettings {
        fn get_setting(&self, key: &str) -> Result<Option<String>, EgressError> {
            Ok(self.values.lock().expect("lock").get(key).cloned())
        }

        fn set_setting(&self, key: &str, value: &str) -> Result<(), EgressError> {
            self.values
                .lock()
                .expect("lock")
                .insert(key.to_owned(), value.to_owned());

            Ok(())
        }
    }

    #[test]
    fn missing_setting_yields_default() {
        let service = SettingsService::new(InMemorySettings::default());

        let settings = service.shell_settings().expect("settings");

        assert_eq!(settings.close_behavior, CloseBehavior::HideToTray);
        assert_eq!(settings.close_behavior, CloseBehavior::default());
    }

    #[test]
    fn setting_round_trips_through_storage() {
        let service = SettingsService::new(InMemorySettings::default());

        let saved = service
            .set_close_behavior(CloseBehavior::Quit)
            .expect("saved");

        assert_eq!(saved.close_behavior, CloseBehavior::Quit);
        assert_eq!(
            service.shell_settings().expect("settings").close_behavior,
            CloseBehavior::Quit
        );
    }

    #[test]
    fn unknown_stored_value_falls_back_to_default() {
        let service =
            SettingsService::new(InMemorySettings::with_value(CLOSE_BEHAVIOR_KEY, "explode"));

        let settings = service.shell_settings().expect("settings");

        assert_eq!(settings.close_behavior, CloseBehavior::default());
    }

    #[test]
    fn close_behavior_serializes_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&CloseBehavior::HideToTray).expect("serialize"),
            "\"hide_to_tray\""
        );
        assert_eq!(CloseBehavior::Quit.as_str(), "quit");
        assert_eq!(
            CloseBehavior::parse("hide_to_tray"),
            Some(CloseBehavior::HideToTray)
        );
    }
}
