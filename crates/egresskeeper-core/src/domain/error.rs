//! Ошибки предметной области и machine-readable коды для границы IPC.
//!
//! Разделение намеренное: `Display` (через `thiserror`) содержит детали и
//! предназначен для логов, а [`EgressError::public_message`] возвращает
//! безопасный текст для пользователя. Наружу, через IPC, уходят только
//! [`ErrorCode`], безопасное сообщение и — для ошибок валидации — имя поля
//! контракта, которое не прошло проверку.

use serde::Serialize;

/// Machine-readable код ошибки, который пересекает границу IPC.
///
/// Коды стабильны и являются частью контракта: UI различает причины отказа по
/// коду, а не разбирает текст сообщения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Входные данные не прошли валидацию на границе приложения.
    Validation,
    /// Запрошенная сущность не найдена.
    NotFound,
    /// Локальное хранилище недоступно или операция не может быть выполнена.
    StorageUnavailable,
    /// Каталог состояния приложения недоступен или не может быть подготовлен.
    StateDirUnavailable,
    /// Порт для listener'а proxy занят или недоступен.
    PortUnavailable,
    /// Возможность недоступна на текущей платформе или в текущем окружении.
    PlatformFeatureUnavailable,
    /// Внутренняя ошибка, детали которой не раскрываются клиенту.
    Internal,
}

impl ErrorCode {
    /// Строковое представление кода для IPC-контракта и логов.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::NotFound => "not_found",
            Self::StorageUnavailable => "storage_unavailable",
            Self::StateDirUnavailable => "state_dir_unavailable",
            Self::PortUnavailable => "port_unavailable",
            Self::PlatformFeatureUnavailable => "platform_feature_unavailable",
            Self::Internal => "internal",
        }
    }
}

/// Ошибка уровня приложения.
#[derive(Debug, thiserror::Error)]
pub enum EgressError {
    /// Входные данные не прошли валидацию. Поле описывает контракт вызова, а не
    /// значение, введённое пользователем.
    #[error("field `{field}` is invalid: {reason}")]
    Validation {
        /// Имя поля контракта.
        field: &'static str,
        /// Причина отказа.
        reason: &'static str,
    },

    /// Сущность не найдена.
    #[error("`{entity}` was not found")]
    NotFound {
        /// Имя сущности: профиль, правило и т. п.
        entity: &'static str,
    },

    /// Операция с локальным хранилищем не удалась.
    #[error("storage failure: {source}")]
    Storage {
        /// Исходная ошибка хранилища.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Порт для listener'а proxy занят или недоступен.
    #[error("proxy port is unavailable: {source}")]
    PortUnavailable {
        /// Исходная ошибка операционной системы.
        #[source]
        source: std::io::Error,
    },

    /// Возможность недоступна на текущей платформе.
    #[error("platform feature `{feature}` is unavailable")]
    PlatformFeatureUnavailable {
        /// Имя возможности: трей, автозапуск и т. п.
        feature: &'static str,
    },

    /// Каталог состояния приложения не удалось подготовить.
    #[error("state directory is unavailable: {source}")]
    StateDirUnavailable {
        /// Исходная ошибка файловой системы.
        #[source]
        source: std::io::Error,
    },

    /// Непредвиденная внутренняя ошибка.
    #[error("internal failure: {source}")]
    Internal {
        /// Исходная ошибка.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl EgressError {
    /// Создаёт ошибку валидации поля контракта.
    #[must_use]
    pub const fn validation(field: &'static str, reason: &'static str) -> Self {
        Self::Validation { field, reason }
    }

    /// Создаёт ошибку отсутствующей сущности.
    #[must_use]
    pub const fn not_found(entity: &'static str) -> Self {
        Self::NotFound { entity }
    }

    /// Создаёт ошибку хранилища по исходной ошибке.
    #[must_use]
    pub fn storage(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Storage {
            source: Box::new(source),
        }
    }

    /// Создаёт ошибку хранилища по сообщению.
    ///
    /// Используется для случаев, когда исходная ошибка не несёт полезных деталей,
    /// например при чтении некорректных данных из базы.
    #[must_use]
    pub fn storage_message(message: impl Into<String>) -> Self {
        Self::storage(std::io::Error::other(message.into()))
    }

    /// Создаёт ошибку недоступной на платформе возможности.
    #[must_use]
    pub const fn platform_feature_unavailable(feature: &'static str) -> Self {
        Self::PlatformFeatureUnavailable { feature }
    }

    /// Создаёт внутреннюю ошибку по исходной причине.
    #[must_use]
    pub fn internal(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }

    /// Код ошибки для IPC-контракта.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Validation { .. } => ErrorCode::Validation,
            Self::NotFound { .. } => ErrorCode::NotFound,
            Self::Storage { .. } => ErrorCode::StorageUnavailable,
            Self::StateDirUnavailable { .. } => ErrorCode::StateDirUnavailable,
            Self::PortUnavailable { .. } => ErrorCode::PortUnavailable,
            Self::PlatformFeatureUnavailable { .. } => ErrorCode::PlatformFeatureUnavailable,
            Self::Internal { .. } => ErrorCode::Internal,
        }
    }

    /// Имя поля контракта для ошибок валидации.
    ///
    /// Это единственная структурированная деталь, которая уходит в UI: имя поля
    /// не является секретом, а значение, введённое пользователем, наружу не
    /// передаётся.
    #[must_use]
    pub const fn invalid_field(&self) -> Option<&'static str> {
        match self {
            Self::Validation { field, .. } => Some(field),
            _ => None,
        }
    }

    /// Сообщение, безопасное для показа пользователю.
    ///
    /// Текст намеренно не содержит путей, имён файлов, SQL и деталей реализации:
    /// конкретика остаётся в логах, куда попадает `Display` этой ошибки.
    #[must_use]
    pub const fn public_message(&self) -> &'static str {
        match self {
            Self::Validation { .. } => "Запрос содержит недопустимые данные.",
            Self::NotFound { .. } => "Запрошенный объект не найден.",
            Self::Storage { .. } => "Не удалось обратиться к локальному хранилищу.",
            Self::StateDirUnavailable { .. } => {
                "Не удалось подготовить каталог состояния приложения."
            }
            Self::PortUnavailable { .. } => "Не удалось занять порт для proxy.",
            Self::PlatformFeatureUnavailable { .. } => {
                "Эта возможность недоступна на текущей платформе."
            }
            Self::Internal { .. } => "Внутренняя ошибка приложения.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_exposes_validation_code() {
        let error = EgressError::validation("app_version", "must not be empty");

        assert_eq!(error.code(), ErrorCode::Validation);
        assert_eq!(error.code().as_str(), "validation");
        assert_eq!(error.invalid_field(), Some("app_version"));
    }

    #[test]
    fn state_dir_error_exposes_state_dir_code() {
        let error = EgressError::StateDirUnavailable {
            source: std::io::Error::other("boom"),
        };

        assert_eq!(error.code(), ErrorCode::StateDirUnavailable);
        assert_eq!(error.code().as_str(), "state_dir_unavailable");
        assert_eq!(error.invalid_field(), None);
    }

    #[test]
    fn internal_error_exposes_internal_code() {
        let error = EgressError::Internal {
            source: Box::new(std::io::Error::other("boom")),
        };

        assert_eq!(error.code(), ErrorCode::Internal);
        assert_eq!(error.code().as_str(), "internal");
    }

    #[test]
    fn port_error_exposes_port_code() {
        let error = EgressError::PortUnavailable {
            source: std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use"),
        };

        assert_eq!(error.code(), ErrorCode::PortUnavailable);
        assert_eq!(error.code().as_str(), "port_unavailable");
        assert_eq!(error.invalid_field(), None);
    }

    #[test]
    fn platform_feature_error_exposes_its_code() {
        let error = EgressError::platform_feature_unavailable("autostart");

        assert_eq!(error.code(), ErrorCode::PlatformFeatureUnavailable);
        assert_eq!(error.code().as_str(), "platform_feature_unavailable");
        assert!(!error.public_message().contains("autostart"));
    }

    #[test]
    fn not_found_error_exposes_not_found_code() {
        let error = EgressError::not_found("profile");

        assert_eq!(error.code(), ErrorCode::NotFound);
        assert_eq!(error.code().as_str(), "not_found");
    }

    #[test]
    fn storage_error_exposes_storage_code() {
        let error = EgressError::storage_message("disk is on fire");

        assert_eq!(error.code(), ErrorCode::StorageUnavailable);
        assert_eq!(error.code().as_str(), "storage_unavailable");
    }

    #[test]
    fn public_message_does_not_leak_internal_details() {
        let secret_path = "/Users/secret-owner/private/state";
        let error = EgressError::StateDirUnavailable {
            source: std::io::Error::other(format!("failed to create {secret_path}")),
        };

        let message = error.public_message();

        assert!(!message.contains(secret_path));
        assert!(!message.contains("secret-owner"));
        assert!(error.to_string().contains(secret_path));
    }

    #[test]
    fn storage_public_message_does_not_leak_sql_or_paths() {
        let error = EgressError::storage_message(
            "SELECT * FROM rules WHERE profile_id = 'x' failed at /var/db/egresskeeper.sqlite3",
        );

        let message = error.public_message();

        assert!(!message.contains("SELECT"));
        assert!(!message.contains("/var/db"));
        assert!(message.contains("хранилищу"));
    }

    #[test]
    fn error_codes_are_serialized_in_snake_case() {
        let serialized = serde_json::to_string(&ErrorCode::StateDirUnavailable).expect("serialize");

        assert_eq!(serialized, "\"state_dir_unavailable\"");
    }
}
