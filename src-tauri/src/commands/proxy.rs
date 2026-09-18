//! Команды управления proxy.
//!
//! Команды читают и меняют конфигурацию listeners в хранилище и передают
//! намерение рантайму. Фактическое состояние всегда берётся из рантайма: команда
//! возвращает объединённое представление, поэтому UI не выводит состояние из
//! собственного намерения.

use crate::commands::run_blocking;
use crate::ipc::IpcError;
use crate::proxy_view::{ProxyStatusView, build_status};
use crate::state::AppState;

/// Возвращает состояние proxy.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой хранилища, если конфигурация недоступна.
#[tauri::command(rename_all = "snake_case")]
pub async fn proxy_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatusView, IpcError> {
    let state = state.inner().clone();

    run_blocking("proxy_status", move || {
        let listeners = IpcError::from_result(state.listeners()?.list())?;

        Ok(build_status(listeners, state.proxy()?))
    })
    .await
}

/// Создаёт listener.
///
/// Новый listener создаётся выключенным: включение — отдельное явное действие.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации порта или отсутствия профиля.
#[tauri::command(rename_all = "snake_case")]
pub async fn proxy_create_listener(
    state: tauri::State<'_, AppState>,
    port: u16,
    profile_id: String,
) -> Result<ProxyStatusView, IpcError> {
    let state = state.inner().clone();

    run_blocking("proxy_create_listener", move || {
        let service = state.listeners()?;

        IpcError::from_result(service.create(port, &profile_id))?;

        Ok(build_status(
            IpcError::from_result(service.list())?,
            state.proxy()?,
        ))
    })
    .await
}

/// Изменяет конфигурацию listener'а.
///
/// Изменение работающего listener'а отклоняется: сначала его нужно выключить.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой валидации, отсутствия сущности или
/// недоступности хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn proxy_update_listener(
    state: tauri::State<'_, AppState>,
    listener_id: String,
    port: u16,
    profile_id: String,
) -> Result<ProxyStatusView, IpcError> {
    let state = state.inner().clone();

    run_blocking("proxy_update_listener", move || {
        let service = state.listeners()?;

        IpcError::from_result(service.update(&listener_id, port, &profile_id))?;

        Ok(build_status(
            IpcError::from_result(service.list())?,
            state.proxy()?,
        ))
    })
    .await
}

/// Включает или выключает listener.
///
/// Признак включения — намерение пользователя; фактическое состояние придёт
/// событием изменения состояния или следующим запросом статуса.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой отсутствия listener'а или недоступности
/// хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn proxy_set_listener_enabled(
    state: tauri::State<'_, AppState>,
    listener_id: String,
    enabled: bool,
) -> Result<ProxyStatusView, IpcError> {
    let state = state.inner().clone();

    run_blocking("proxy_set_listener_enabled", move || {
        let service = state.listeners()?;

        IpcError::from_result(service.set_enabled(&listener_id, enabled))?;

        Ok(build_status(
            IpcError::from_result(service.list())?,
            state.proxy()?,
        ))
    })
    .await
}

/// Удаляет listener, останавливая его.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой отсутствия listener'а или недоступности
/// хранилища.
#[tauri::command(rename_all = "snake_case")]
pub async fn proxy_delete_listener(
    state: tauri::State<'_, AppState>,
    listener_id: String,
) -> Result<ProxyStatusView, IpcError> {
    let state = state.inner().clone();

    run_blocking("proxy_delete_listener", move || {
        let service = state.listeners()?;

        IpcError::from_result(service.delete(&listener_id))?;

        Ok(build_status(
            IpcError::from_result(service.list())?,
            state.proxy()?,
        ))
    })
    .await
}
