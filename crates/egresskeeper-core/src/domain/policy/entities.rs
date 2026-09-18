//! Сущности политики: профиль, правило и их идентификаторы.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::error::EgressError;
use crate::domain::policy::action::Action;
use crate::domain::policy::host::HostMatcher;
use crate::domain::policy::port::PortSpec;

/// Максимальная длина имени профиля.
const MAX_PROFILE_NAME_LENGTH: usize = 64;

/// Идентификатор профиля.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileId(String);

impl ProfileId {
    /// Создаёт новый идентификатор.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Разбирает идентификатор, пришедший из IPC.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `profile_id`, если значение
    /// не является UUID.
    pub fn parse(input: &str) -> Result<Self, EgressError> {
        parse_uuid(input, "profile_id").map(Self)
    }

    /// Восстанавливает идентификатор из хранилища.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища: некорректный идентификатор в базе означает
    /// повреждённые данные, а не ошибку пользователя.
    pub fn from_stored(input: &str) -> Result<Self, EgressError> {
        Uuid::parse_str(input)
            .map(|uuid| Self(uuid.to_string()))
            .map_err(|source| {
                EgressError::storage_message(format!("stored profile id is invalid: {source}"))
            })
    }

    /// Строковое представление.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProfileId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Идентификатор правила.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(String);

impl RuleId {
    /// Создаёт новый идентификатор.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Разбирает идентификатор, пришедший из IPC.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `rule_id`, если значение не
    /// является UUID.
    pub fn parse(input: &str) -> Result<Self, EgressError> {
        parse_uuid(input, "rule_id").map(Self)
    }

    /// Восстанавливает идентификатор из хранилища.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища при повреждённых данных.
    pub fn from_stored(input: &str) -> Result<Self, EgressError> {
        Uuid::parse_str(input)
            .map(|uuid| Self(uuid.to_string()))
            .map_err(|source| {
                EgressError::storage_message(format!("stored rule id is invalid: {source}"))
            })
    }

    /// Строковое представление.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RuleId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Профиль политики.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Идентификатор профиля.
    pub id: ProfileId,
    /// Имя профиля, уникальное среди профилей.
    pub name: String,
    /// Действие, применяемое, когда ни одно правило не совпало.
    pub default_action: Action,
    /// Момент создания в миллисекундах Unix epoch.
    pub created_at_unix_ms: i64,
    /// Момент последнего изменения в миллисекундах Unix epoch.
    pub updated_at_unix_ms: i64,
}

impl Profile {
    /// Проверяет и нормализует имя профиля.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `name`, если имя пустое,
    /// слишком длинное или содержит управляющие символы.
    pub fn validate_name(input: &str) -> Result<String, EgressError> {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return Err(EgressError::validation("name", "must not be empty"));
        }

        if trimmed.chars().count() > MAX_PROFILE_NAME_LENGTH {
            return Err(EgressError::validation("name", "is too long"));
        }

        if trimmed.chars().any(char::is_control) {
            return Err(EgressError::validation(
                "name",
                "must not contain control characters",
            ));
        }

        Ok(trimmed.to_owned())
    }

    /// Приводит имя к виду, по которому проверяется уникальность.
    ///
    /// Используется Unicode-приведение к нижнему регистру, а не `COLLATE NOCASE`
    /// в SQLite: последний работает только с ASCII, из-за чего имена вроде
    /// «Работа» и «работа» считались бы разными.
    #[must_use]
    pub fn fold_name(input: &str) -> String {
        input.trim().to_lowercase()
    }
}

/// Данные для создания профиля.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDraft {
    /// Проверенное имя профиля.
    pub name: String,
    /// Действие по умолчанию.
    pub default_action: Action,
}

/// Правило профиля.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Идентификатор правила.
    pub id: RuleId,
    /// Профиль, которому принадлежит правило.
    pub profile_id: ProfileId,
    /// Позиция в порядке профиля, начиная с нуля.
    pub position: u32,
    /// Действие правила.
    pub action: Action,
    /// Сопоставление host.
    pub host: HostMatcher,
    /// Ограничение порта.
    pub port: PortSpec,
}

/// Проверенные данные нового правила или изменения существующего.
///
/// Существование этого типа означает, что значения уже прошли валидацию домена.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDraft {
    /// Действие правила.
    pub action: Action,
    /// Сопоставление host.
    pub host: HostMatcher,
    /// Ограничение порта.
    pub port: PortSpec,
}

/// Разбирает UUID и приводит его к каноническому виду.
fn parse_uuid(input: &str, field: &'static str) -> Result<String, EgressError> {
    Uuid::parse_str(input.trim())
        .map(|uuid| uuid.to_string())
        .map_err(|_| EgressError::validation(field, "must be a uuid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;

    #[test]
    fn generated_ids_are_unique_and_parseable() {
        let first = ProfileId::new();
        let second = ProfileId::new();

        assert_ne!(first, second);
        assert_eq!(ProfileId::parse(first.as_str()).expect("round trip"), first);
    }

    #[test]
    fn parse_rejects_non_uuid_input() {
        let error = ProfileId::parse("not-a-uuid").expect_err("invalid id");

        assert_eq!(error.code(), ErrorCode::Validation);
        assert_eq!(error.invalid_field(), Some("profile_id"));

        let error = RuleId::parse("42").expect_err("invalid id");
        assert_eq!(error.invalid_field(), Some("rule_id"));
    }

    #[test]
    fn stored_id_failure_is_a_storage_error() {
        let error = ProfileId::from_stored("broken").expect_err("invalid stored id");

        assert_eq!(error.code(), ErrorCode::StorageUnavailable);
    }

    #[test]
    fn profile_name_is_trimmed_and_validated() {
        assert_eq!(
            Profile::validate_name("  Работа  ").expect("valid name"),
            "Работа"
        );

        let error = Profile::validate_name("   ").expect_err("empty name");
        assert_eq!(error.invalid_field(), Some("name"));
    }

    #[test]
    fn profile_name_rejects_control_characters() {
        let error = Profile::validate_name("Работа\u{7}").expect_err("control character");

        assert_eq!(error.code(), ErrorCode::Validation);
    }

    #[test]
    fn profile_name_folding_is_unicode_aware() {
        assert_eq!(Profile::fold_name("  Работа  "), "работа");
        assert_eq!(Profile::fold_name("РАБОТА"), "работа");
        assert_eq!(Profile::fold_name("Work"), "work");
    }

    #[test]
    fn profile_name_rejects_too_long_values() {
        let name = "a".repeat(MAX_PROFILE_NAME_LENGTH + 1);

        assert!(Profile::validate_name(&name).is_err());
    }
}
