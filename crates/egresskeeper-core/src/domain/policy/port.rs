//! Ограничение порта назначения в правиле.

use serde::{Deserialize, Serialize};

use crate::domain::error::EgressError;

/// Ограничение порта назначения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortSpec {
    /// Любой допустимый порт.
    Any,
    /// Конкретный порт.
    Exactly {
        /// Номер порта.
        port: u16,
    },
    /// Диапазон портов включительно.
    Range {
        /// Начало диапазона.
        start: u16,
        /// Конец диапазона.
        end: u16,
    },
}

impl PortSpec {
    /// Ограничение «любой порт».
    #[must_use]
    pub const fn any() -> Self {
        Self::Any
    }

    /// Ограничение конкретным портом.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полем `port`, если порт равен нулю.
    pub fn exactly(port: u16) -> Result<Self, EgressError> {
        if port == 0 {
            return Err(EgressError::validation(
                "port",
                "must be between 1 and 65535",
            ));
        }

        Ok(Self::Exactly { port })
    }

    /// Ограничение диапазоном портов.
    ///
    /// # Errors
    ///
    /// Возвращает [`EgressError::Validation`] с полями `port_start` или
    /// `port_end`, если начало равно нулю или больше конца диапазона.
    pub fn range(start: u16, end: u16) -> Result<Self, EgressError> {
        if start == 0 {
            return Err(EgressError::validation(
                "port_start",
                "must be between 1 and 65535",
            ));
        }
        if start > end {
            return Err(EgressError::validation(
                "port_end",
                "must not be less than the range start",
            ));
        }

        Ok(Self::Range { start, end })
    }

    /// Проверяет, попадает ли порт в ограничение.
    #[must_use]
    pub const fn matches(self, port: u16) -> bool {
        match self {
            Self::Any => port > 0,
            Self::Exactly { port: expected } => port == expected,
            Self::Range { start, end } => port >= start && port <= end,
        }
    }

    /// Вид ограничения для хранилища и контракта.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Exactly { .. } => "exactly",
            Self::Range { .. } => "range",
        }
    }

    /// Границы ограничения: `(start, end)` для диапазона, `(port, port)` для
    /// конкретного порта и `(None, None)` для любого порта.
    #[must_use]
    pub const fn bounds(self) -> (Option<u16>, Option<u16>) {
        match self {
            Self::Any => (None, None),
            Self::Exactly { port } => (Some(port), None),
            Self::Range { start, end } => (Some(start), Some(end)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;

    #[test]
    fn any_matches_every_valid_port() {
        let spec = PortSpec::any();

        assert!(spec.matches(1));
        assert!(spec.matches(443));
        assert!(spec.matches(65_535));
        assert!(!spec.matches(0));
    }

    #[test]
    fn exactly_matches_single_port() {
        let spec = PortSpec::exactly(443).expect("port is valid");

        assert!(spec.matches(443));
        assert!(!spec.matches(8443));
    }

    #[test]
    fn exactly_rejects_zero() {
        let error = PortSpec::exactly(0).expect_err("zero port");

        assert_eq!(error.code(), ErrorCode::Validation);
        assert_eq!(error.invalid_field(), Some("port"));
    }

    #[test]
    fn range_matches_inclusive_bounds() {
        let spec = PortSpec::range(8000, 8100).expect("range is valid");

        for port in [8000, 8080, 8100] {
            assert!(spec.matches(port), "port {port} must match");
        }
        for port in [7999, 8101] {
            assert!(!spec.matches(port), "port {port} must not match");
        }
    }

    #[test]
    fn range_accepts_single_port_range() {
        let spec = PortSpec::range(443, 443).expect("range is valid");

        assert!(spec.matches(443));
        assert!(!spec.matches(444));
    }

    #[test]
    fn range_rejects_inverted_bounds() {
        let error = PortSpec::range(9000, 8000).expect_err("inverted range");

        assert_eq!(error.code(), ErrorCode::Validation);
        assert_eq!(error.invalid_field(), Some("port_end"));
    }

    #[test]
    fn range_rejects_zero_start() {
        let error = PortSpec::range(0, 80).expect_err("zero start");

        assert_eq!(error.invalid_field(), Some("port_start"));
    }

    #[test]
    fn serializes_with_kind_tag() {
        assert_eq!(
            serde_json::to_value(PortSpec::Any).expect("serialize"),
            serde_json::json!({ "kind": "any" })
        );
        assert_eq!(
            serde_json::to_value(PortSpec::exactly(443).expect("port")).expect("serialize"),
            serde_json::json!({ "kind": "exactly", "port": 443 })
        );
        assert_eq!(
            serde_json::to_value(PortSpec::range(8000, 8100).expect("range")).expect("serialize"),
            serde_json::json!({ "kind": "range", "start": 8000, "end": 8100 })
        );
    }
}
