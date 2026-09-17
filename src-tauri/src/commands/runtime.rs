//! Команды runtime-информации.

use egresskeeper_core::RuntimeOverview;

use crate::ipc::IpcError;
use crate::state::AppState;

/// Возвращает runtime-информацию приложения.
///
/// Команда намеренно асинхронная: она не должна блокировать главный поток окна,
/// даже когда backend занят инициализацией или фоновой работой.
///
/// # Errors
///
/// Возвращает [`IpcError`], если backend не смог инициализироваться.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_runtime_overview(
    state: tauri::State<'_, AppState>,
) -> Result<RuntimeOverview, IpcError> {
    let overview = state.overview()?;

    // Граница IPC — место, где видно, что UI действительно получил состояние.
    // Ошибка логируется при маппинге, поэтому здесь фиксируется только успех.
    tracing::debug!(command = "get_runtime_overview", "ipc command completed");

    Ok(overview)
}
