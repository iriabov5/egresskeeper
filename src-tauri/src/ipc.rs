//! Типизированный контракт ошибок IPC.
//!
//! Ошибка пересекает границу процесса как структура с machine-readable `code`,
//! безопасным сообщением и — только там, где это полезно UI — структурированными
//! деталями. Детали содержат имя поля контракта и никогда не содержат значение,
//! введённое пользователем; всё остальное (пути, SQL, исходные ошибки) остаётся
//! в логах.

use egresskeeper_core::EgressError;
use serde::Serialize;

/// Структурированные детали ошибки.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IpcErrorDetails {
    /// Имя поля контракта, не прошедшего валидацию.
    pub field: String,
}

/// Ошибка, которую получает frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IpcError {
    /// Machine-readable код ошибки.
    pub code: &'static str,
    /// Сообщение, безопасное для показа пользователю.
    pub message: &'static str,
    /// Детали ошибки; отсутствуют, если UI они не нужны.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<IpcErrorDetails>,
}

impl IpcError {
    /// Преобразует ошибку ядра в IPC-ошибку и логирует детали.
    ///
    /// Логирование происходит именно здесь: это единственная точка, где ошибка
    /// покидает backend, поэтому исходная причина гарантированно попадает в лог,
    /// даже если клиент её не увидит.
    #[must_use]
    pub fn from_domain(error: &EgressError) -> Self {
        tracing::error!(
            code = error.code().as_str(),
            error = %error,
            "ipc request failed"
        );

        Self {
            code: error.code().as_str(),
            message: error.public_message(),
            details: error.invalid_field().map(|field| IpcErrorDetails {
                field: field.to_owned(),
            }),
        }
    }

    /// Преобразует результат операции ядра в результат IPC.
    ///
    /// Используется командами, чтобы не повторять маппинг ошибок в каждом
    /// обработчике.
    pub fn from_result<T>(result: Result<T, EgressError>) -> Result<T, Self> {
        result.map_err(|error| Self::from_domain(&error))
    }

    /// Создаёт ошибку внутреннего сбоя, у которой нет доменной причины.
    #[must_use]
    pub fn internal(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::from_domain(&EgressError::internal(source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_domain_code_to_ipc_code() {
        let error = EgressError::validation("host", "must not be empty");

        let ipc_error = IpcError::from_domain(&error);

        assert_eq!(ipc_error.code, "validation");
        assert_eq!(ipc_error.message, error.public_message());
    }

    #[test]
    fn validation_error_carries_the_contract_field() {
        let error = EgressError::validation("port_start", "must be between 1 and 65535");

        let ipc_error = IpcError::from_domain(&error);

        assert_eq!(
            ipc_error.details,
            Some(IpcErrorDetails {
                field: "port_start".to_owned()
            })
        );
    }

    #[test]
    fn validation_details_do_not_contain_user_input() {
        let user_value = "API.example.com:443/secret";
        let error = EgressError::validation("host", "must not contain a port");

        let ipc_error = IpcError::from_domain(&error);
        let serialized = serde_json::to_string(&ipc_error).expect("serializable");

        assert!(!serialized.contains(user_value));
        assert!(serialized.contains("host"));
    }

    #[test]
    fn errors_without_field_have_no_details() {
        for error in [
            EgressError::not_found("profile"),
            EgressError::storage_message("disk failed"),
            EgressError::internal(std::io::Error::other("boom")),
        ] {
            let ipc_error = IpcError::from_domain(&error);

            assert_eq!(ipc_error.details, None);
            assert_eq!(ipc_error.code, error.code().as_str());
        }
    }

    #[test]
    fn details_are_omitted_from_payload_when_absent() {
        let error = EgressError::not_found("rule");

        let json = serde_json::to_value(IpcError::from_domain(&error)).expect("serializable");

        assert_eq!(
            json,
            serde_json::json!({
                "code": "not_found",
                "message": "Запрошенный объект не найден."
            })
        );
    }

    #[test]
    fn storage_error_serializes_without_internal_details() {
        let error = EgressError::storage_message(
            "SELECT * FROM rules failed at /var/db/egresskeeper.sqlite3",
        );

        let json = serde_json::to_value(IpcError::from_domain(&error)).expect("serializable");

        assert_eq!(
            json,
            serde_json::json!({
                "code": "storage_unavailable",
                "message": "Не удалось обратиться к локальному хранилищу."
            })
        );
        assert!(!json.to_string().contains("SELECT"));
        assert!(!json.to_string().contains("/var/db"));
    }
}
