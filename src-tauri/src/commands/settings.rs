//! Команды настроек приложения.

use egresskeeper_core::CloseBehavior;

use crate::commands::run_blocking;
use crate::ipc::IpcError;
use crate::settings_view::ShellSettingsView;
use crate::state::AppState;

/// Возвращает настройки приложения и состояние платформенных возможностей.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой хранилища, если настройки недоступны.
#[tauri::command(rename_all = "snake_case")]
pub async fn settings_get(
    state: tauri::State<'_, AppState>,
) -> Result<ShellSettingsView, IpcError> {
    let state = state.inner().clone();

    run_blocking("settings_get", move || state.shell_settings()).await
}

/// Меняет поведение при закрытии главного окна.
///
/// # Errors
///
/// Возвращает [`IpcError`] с ошибкой хранилища, если записать настройку не удалось.
#[tauri::command(rename_all = "snake_case")]
pub async fn settings_set_close_behavior(
    state: tauri::State<'_, AppState>,
    close_behavior: CloseBehavior,
) -> Result<ShellSettingsView, IpcError> {
    let state = state.inner().clone();

    run_blocking("settings_set_close_behavior", move || {
        state.set_close_behavior(close_behavior)
    })
    .await
}

/// Включает или выключает запуск при входе в систему.
///
/// # Errors
///
/// Возвращает [`IpcError`] с кодом недоступной возможности, если автозапуск
/// недоступен, или с ошибкой изменения.
#[tauri::command(rename_all = "snake_case")]
pub async fn settings_set_autostart(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<ShellSettingsView, IpcError> {
    let state = state.inner().clone();

    run_blocking("settings_set_autostart", move || {
        state.set_autostart(enabled)
    })
    .await
}
