//! Конфигурация listener'а proxy.
//!
//! Listener описывается портом прослушивания и профилем политики. Порт ограничен
//! непривилегированным диапазоном: приложение не должно требовать прав
//! администратора, а привязка выполняется только к loopback-адресу (см. дизайн
//! change'а и `proxy-runtime`).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::error::EgressError;
use crate::domain::policy::entities::ProfileId;

/// Минимальный допустимый порт прослушивания.
pub const MIN_PROXY_PORT: u16 = 1024;

/// Максимальный допустимый порт прослушивания.
pub const MAX_PROXY_PORT: u16 = 65_535;

/// Порт прослушивания, прошедший проверку.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProxyPort(u16);

impl ProxyPort {
    /// Проверяет и создаёт порт прослушивания.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `port`, если порт выходит
    /// за пределы непривилегированного диапазона.
    pub fn parse(port: u16) -> Result<Self, EgressError> {
        if !(MIN_PROXY_PORT..=MAX_PROXY_PORT).contains(&port) {
            return Err(EgressError::validation(
                "port",
                "must be between 1024 and 65535",
            ));
        }

        Ok(Self(port))
    }

    /// Восстанавливает порт из хранилища.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку хранилища: некорректный порт в базе означает
    /// повреждённые данные.
    pub fn from_stored(port: i64) -> Result<Self, EgressError> {
        u16::try_from(port)
            .ok()
            .and_then(|port| Self::parse(port).ok())
            .ok_or_else(|| EgressError::storage_message("stored listener has an invalid port"))
    }

    /// Числовое значение порта.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl std::fmt::Display for ProxyPort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Идентификатор listener'а.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ListenerId(String);

impl ListenerId {
    /// Создаёт новый идентификатор.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Разбирает идентификатор, пришедший из IPC.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `listener_id`, если
    /// значение не является UUID.
    pub fn parse(input: &str) -> Result<Self, EgressError> {
        Uuid::parse_str(input.trim())
            .map(|uuid| Self(uuid.to_string()))
            .map_err(|_| EgressError::validation("listener_id", "must be a uuid"))
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
                EgressError::storage_message(format!("stored listener id is invalid: {source}"))
            })
    }

    /// Строковое представление.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ListenerId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ListenerId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Listener proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listener {
    /// Идентификатор listener'а.
    pub id: ListenerId,
    /// Порт прослушивания.
    pub port: ProxyPort,
    /// Профиль политики, применяемый к соединениям.
    pub profile_id: ProfileId,
    /// Признак включения: намерение пользователя.
    ///
    /// Фактическое состояние (принимает ли listener соединения) живёт в рантайме
    /// proxy и может отличаться, например при занятом порте.
    pub enabled: bool,
    /// Момент создания в миллисекундах Unix epoch.
    pub created_at_unix_ms: i64,
    /// Момент последнего изменения в миллисекундах Unix epoch.
    pub updated_at_unix_ms: i64,
}

/// Проверенные данные listener'а для создания или изменения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerDraft {
    /// Порт прослушивания.
    pub port: ProxyPort,
    /// Профиль политики.
    pub profile_id: ProfileId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;

    #[test]
    fn port_accepts_unprivileged_range() {
        for port in [MIN_PROXY_PORT, 8787, MAX_PROXY_PORT] {
            assert_eq!(ProxyPort::parse(port).expect("port is valid").get(), port);
        }
    }

    #[test]
    fn port_rejects_privileged_and_zero_ports() {
        for port in [0, 80, 443, 1023] {
            let error = ProxyPort::parse(port).expect_err("port must be rejected");

            assert_eq!(error.code(), ErrorCode::Validation);
            assert_eq!(error.invalid_field(), Some("port"));
        }
    }

    #[test]
    fn stored_port_rejects_invalid_values() {
        assert!(ProxyPort::from_stored(8787).is_ok());
        assert_eq!(
            ProxyPort::from_stored(70_000)
                .expect_err("out of range")
                .code(),
            ErrorCode::StorageUnavailable
        );
        assert!(ProxyPort::from_stored(-1).is_err());
    }

    #[test]
    fn listener_id_round_trips_and_rejects_garbage() {
        let id = ListenerId::new();

        assert_eq!(ListenerId::parse(id.as_str()).expect("round trip"), id);
        assert_eq!(
            ListenerId::parse("not-a-uuid")
                .expect_err("invalid id")
                .invalid_field(),
            Some("listener_id")
        );
        assert_eq!(
            ListenerId::from_stored("broken")
                .expect_err("invalid stored id")
                .code(),
            ErrorCode::StorageUnavailable
        );
    }

    #[test]
    fn port_serializes_as_number() {
        assert_eq!(
            serde_json::to_value(ProxyPort::parse(8787).expect("port")).expect("serialize"),
            serde_json::json!(8787)
        );
    }
}
