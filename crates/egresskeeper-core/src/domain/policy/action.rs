//! Действие политики: разрешить или запретить соединение.

use serde::{Deserialize, Serialize};

use crate::domain::error::EgressError;

/// Действие, которое политика применяет к соединению.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Соединение разрешено.
    Allow,
    /// Соединение запрещено.
    Deny,
}

impl Default for Action {
    /// Действие по умолчанию — запрет: неизвестное направление не разрешается
    /// само по себе (fail-closed).
    fn default() -> Self {
        Self::Deny
    }
}

impl Action {
    /// Строковое представление для хранилища и IPC-контракта.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    /// Разбирает действие из строкового представления.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`], если значение не является
    /// допустимым действием.
    pub fn parse(input: &str) -> Result<Self, EgressError> {
        match input {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err(EgressError::validation(
                "action",
                "must be `allow` or `deny`",
            )),
        }
    }

    /// Возвращает `true`, если действие разрешает соединение.
    #[must_use]
    pub const fn allows(self) -> bool {
        matches!(self, Self::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;

    #[test]
    fn default_action_is_deny() {
        assert_eq!(Action::default(), Action::Deny);
    }

    #[test]
    fn parse_accepts_known_values() {
        assert_eq!(Action::parse("allow").expect("allow"), Action::Allow);
        assert_eq!(Action::parse("deny").expect("deny"), Action::Deny);
    }

    #[test]
    fn parse_rejects_unknown_values() {
        let error = Action::parse("maybe").expect_err("unknown action");

        assert_eq!(error.code(), ErrorCode::Validation);
        assert_eq!(error.invalid_field(), Some("action"));
    }

    #[test]
    fn serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&Action::Allow).expect("serialize"),
            "\"allow\""
        );
        assert_eq!(Action::as_str(Action::Deny), "deny");
    }
}
