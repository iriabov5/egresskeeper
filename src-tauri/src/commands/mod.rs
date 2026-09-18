//! Команды IPC.
//!
//! Каждая команда — тонкий адаптер: получить состояние, выполнить операцию,
//! преобразовать ошибку в типизированный ответ. Бизнес-логики здесь нет.
//!
//! Обращения к SQLite блокирующие, поэтому они выполняются в блокирующем пуле:
//! `system-architecture` требует изолировать блокирующие операции от
//! async-исполнителя.

pub mod policy;
pub mod proxy;
pub mod runtime;
pub mod settings;

pub use policy::{
    policy_add_rule, policy_create_profile, policy_delete_profile, policy_delete_rule,
    policy_evaluate, policy_list_profiles, policy_profile_detail, policy_rename_profile,
    policy_reorder_rules, policy_set_default_action, policy_update_rule,
};
pub use proxy::{
    proxy_create_listener, proxy_delete_listener, proxy_set_listener_enabled, proxy_status,
    proxy_update_listener,
};
pub use runtime::get_runtime_overview;
pub use settings::{settings_get, settings_set_autostart, settings_set_close_behavior};

use crate::ipc::IpcError;

/// Выполняет блокирующую операцию вне async-исполнителя.
///
/// Паника внутри блокирующей задачи не должна ронять приложение и не должна
/// превращаться в необработанную ошибку: она возвращается как типизированная
/// внутренняя ошибка. Имя команды попадает в лог, поэтому видно, какие запросы
/// действительно пришли из UI.
async fn run_blocking<T: Send + 'static>(
    command: &'static str,
    operation: impl FnOnce() -> Result<T, IpcError> + Send + 'static,
) -> Result<T, IpcError> {
    match tauri::async_runtime::spawn_blocking(operation).await {
        Ok(result) => {
            tracing::debug!(command, "blocking ipc command completed");
            result
        }
        Err(join_error) => {
            tracing::error!(error = %join_error, "blocking ipc task failed");
            Err(IpcError::internal(std::io::Error::other(
                "blocking task failed",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use egresskeeper_core::EgressError;

    use super::*;

    #[test]
    fn blocking_task_returns_its_value() {
        let value = tauri::async_runtime::block_on(run_blocking("test_command", || {
            Ok::<u32, IpcError>(42)
        }))
        .expect("task succeeds");

        assert_eq!(value, 42);
    }

    #[test]
    fn blocking_task_propagates_typed_error() {
        let error = tauri::async_runtime::block_on(run_blocking("test_command", || {
            Err::<u32, IpcError>(IpcError::from_domain(&EgressError::validation(
                "host",
                "must not be empty",
            )))
        }))
        .expect_err("task fails");

        assert_eq!(error.code, "validation");
        assert_eq!(
            error.details.map(|details| details.field),
            Some("host".to_owned())
        );
    }
}
